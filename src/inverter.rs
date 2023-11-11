use crate::protos::hoymiles::RealData::HMSStateResponse;
use crate::RealData::RealDataResDTO;
use clap::ValueEnum;
use crc16::*;
use log::{debug, info};
use protobuf::Message;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

static INVERTER_PORT: u16 = 10081;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum NetworkState {
    Unknown,
    Online,
    Offline,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Mode {
    Cooperative,
    Exclusive,
}

pub struct Inverter<'a> {
    host: &'a str,
    state: NetworkState,
    sequence: u16,
    _mode: Mode,
}

impl<'a> Inverter<'a> {
    pub fn new(host: &'a str, mode: Mode) -> Self {
        info!("Inverter communication mode: {mode:?}");
        Self {
            host,
            state: NetworkState::Unknown,
            sequence: 0_u16,
            _mode: mode,
        }
    }

    fn set_state(&mut self, new_state: NetworkState) {
        if self.state != new_state {
            self.state = new_state;
            info!("Inverter is {new_state:?}");
        }
    }

    pub fn update_state(&mut self) -> Option<HMSStateResponse> {
        self.sequence = self.sequence.wrapping_add(1);

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
        message.extend_from_slice(&self.sequence.to_be_bytes());
        message.extend_from_slice(&crc16.to_be_bytes());
        message.extend_from_slice(&len.to_be_bytes());
        message.extend_from_slice(&request_as_bytes);

        let ip = self.host.parse().expect("Unable to parse socket address");
        let address = SocketAddr::new(IpAddr::V4(ip), INVERTER_PORT);
        let stream = TcpStream::connect_timeout(&address, Duration::from_millis(500));
        if let Err(e) = stream {
            debug!("{e}");
            self.set_state(NetworkState::Offline);
            return None;
        }

        let mut stream = stream.unwrap();
        if let Err(e) = stream.write(&message) {
            debug!(r#"{e}"#);
            self.set_state(NetworkState::Offline);
            return None;
        }

        let mut buf = [0u8; 1024];
        let read = stream.read(&mut buf);

        if let Err(e) = read {
            debug!("{e}");
            self.set_state(NetworkState::Offline);
            return None;
        }
        let read_length = read.unwrap();
        let parsed = HMSStateResponse::parse_from_bytes(&buf[10..read_length]);

        if let Err(e) = parsed {
            debug!("{e}");
            self.set_state(NetworkState::Offline);
            return None;
        }
        debug_assert!(parsed.is_ok());

        let response = parsed.unwrap();
        self.set_state(NetworkState::Online);
        Some(response)
    }
}
