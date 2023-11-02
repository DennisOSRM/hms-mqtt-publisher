// TODO: support CA33 command to take over metrics consumption
// TODO: support publishing to S-Miles cloud, too

mod protos;

use crate::protos::hoymiles::RealData::HMSStateResponse;
use crate::RealData::RealDataResDTO;

use std::error::Error;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;
use std::{fmt, thread};
use std::{
    io::{Read, Write},
    net::TcpStream,
};

use chrono::Local;
use clap::Parser;
use crc16::*;
use env_logger::Builder;
use log::{debug, info, LevelFilter, warn};
use protobuf::Message;
use protos::hoymiles::RealData;
use rumqttc::{Client, MqttOptions, QoS};

#[derive(Parser)]
struct Cli {
    inverter_host: String,
    mqtt_broker_host: String,
    mqtt_username: Option<String>,
    mqtt_password: Option<String>,
}

static INVERTER_PORT: u16 = 10081;
static REQUEST_DELAY: u64 = 30_500;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum InverterState {
    Online,
    Offline,
}

#[derive(Debug)]
enum ErrorState {
    NetworkRead,
    Offline,
    ParseResponse,
    Unknown,
}

impl Error for ErrorState {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        None
    }

    fn description(&self) -> &str {
        "description() is deprecated; use Display"
    }

    fn cause(&self) -> Option<&dyn Error> {
        self.source()
    }
}

impl fmt::Display for ErrorState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let message = match self {
            ErrorState::NetworkRead => "network read failed",
            ErrorState::Offline => "host not reachable",
            ErrorState::ParseResponse => "response not parseable",
            ErrorState::Unknown => "unknown",
        };
        write!(f, "ErrorState: {message}")
    }
}

fn get_inverter_state(sequence: u16, host: &str) -> Result<HMSStateResponse, ErrorState> {
    let /*mut*/ request = RealDataResDTO::default();
    // let date = Local::now();
    // let time_string = date.format("%Y-%m-%d %H:%M:%S").to_string();
    // request.ymd_hms = time_string;
    // request.cp = 23 + sequence as i32;
    // request.offset = 0;
    // request.time = epoch();
    let header = b"\x48\x4d\xa3\x03";
    let request_as_bytes = request.write_to_bytes().expect("serialize to bytes");
    let crc16 = State::<MODBUS>::calculate(&request_as_bytes);
    let len = request_as_bytes.len() as u16 + 10u16;

    // compose request message
    let mut message = Vec::new();
    message.extend_from_slice(header);
    message.extend_from_slice(&sequence.to_be_bytes());
    message.extend_from_slice(&crc16.to_be_bytes());
    message.extend_from_slice(&len.to_be_bytes());
    message.extend_from_slice(&request_as_bytes);

    let ip = host.parse().expect("Unable to parse socket address");
    let address = SocketAddr::new(IpAddr::V4(ip), INVERTER_PORT);
    let stream = TcpStream::connect_timeout(&address, Duration::from_millis(500));
    if let Err(e) = stream {
        debug!("{e}");
        return Err(ErrorState::Offline);
    }

    let mut stream = stream.unwrap();
    if let Err(e) = stream.write(&message) {
        debug!(r#"{e}"#);
        return Err(ErrorState::Unknown);
    }

    let mut buf = [0u8; 1024];
    let read = stream.read(&mut buf);

    if let Err(e) = read {
        debug!("{e}");
        return Err(ErrorState::NetworkRead);
    }
    let read_length = read.unwrap();
    let parsed = HMSStateResponse::parse_from_bytes(&buf[10..read_length]);

    if let Err(e) = parsed {
        debug!("{e}");
        return Err(ErrorState::ParseResponse);
    }

    let response = parsed.unwrap();
    Ok(response)
}

fn send_to_mqtt(hms_state: &HMSStateResponse, client: &mut Client) {
    debug!("{hms_state}");

    let pv_current_power = hms_state.pv_current_power as f32 / 10.;
    let pv_daily_yield = hms_state.pv_daily_yield;

    client
        .subscribe("hms800wt2/pv_current_power", QoS::AtMostOnce)
        .unwrap();
    match client.publish(
        "hms800wt2/pv_current_power",
        QoS::AtMostOnce,
        true,
        pv_current_power.to_string(),
    ) {
        Ok(_) => {}
        Err(e) => warn!("mqtt error: {e}"),
    }
    match client.publish(
        "hms800wt2/pv_daily_yield",
        QoS::AtMostOnce,
        true,
        pv_daily_yield.to_string(),
    ) {
        Ok(_) => {}
        Err(e) => warn!("mqtt error: {e}"),
    }
}

fn main() {
    Builder::new()
        .format(|buf, record| {
            writeln!(
                buf,
                "{} [{}] - {}",
                Local::now().format("%Y-%m-%dT%H:%M:%S"),
                record.level(),
                record.args()
            )
        })
        .filter(None, LevelFilter::Info)
        .init();

    let cli = Cli::parse();

    // set up mqtt connection
    info!("inverter: {}, mqtt broker {}", cli.inverter_host, cli.mqtt_broker_host);
    let mut mqttoptions = MqttOptions::new("hms800wt2-mqtt-publisher", cli.mqtt_broker_host, 1883);
    mqttoptions.set_keep_alive(Duration::from_secs(5));

    //parse the mqtt authentication options
    if let Some((username, password)) = match (cli.mqtt_username, cli.mqtt_password) {
        (None, None) => None,
        (None, Some(_)) => None,
        (Some(username), None) => Some((username.clone(), "".into())),
        (Some(username), Some(password)) => Some((username.clone(), password.clone())),
    } {
        mqttoptions.set_credentials(username, password);
    }

    let (mut client, mut connection) = Client::new(mqttoptions, 10);
    thread::spawn(move || {
        // keep polling the event loop to make sure outgoing messages get sent
        for _ in connection.iter() {}
    });

    let mut sequence = 0u16;
    let mut current_state = InverterState::Offline;
    loop {
        sequence = sequence.wrapping_add(1);
        // factor out a function that returns a (Response, State);
        let new_state = match get_inverter_state(sequence, &cli.inverter_host) {
            Ok(r) => {
                debug!("{r}");
                send_to_mqtt(&r, &mut client);
                InverterState::Online
            }
            Err(e) => {
                info!("error: {e}");
                InverterState::Offline
            }
        };

        if current_state != new_state {
            current_state = new_state;
            info!("Inverter is {current_state:?}");
        }

        thread::sleep(Duration::from_millis(REQUEST_DELAY));
    }
}
