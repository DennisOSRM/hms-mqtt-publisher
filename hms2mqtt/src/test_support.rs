// helpers shared by the unit tests
use crate::inverter::HEADER_LEN;
use crate::mqtt_config::MqttConfig;
use crate::mqtt_wrapper::{MqttWrapper, QoS};
use crate::protos::hoymiles::RealData::{HMSStateResponse, InverterState, PortState};
use crc16::{State, MODBUS};
use protobuf::Message;
use std::io::{Read, Write};

/// MQTT client that records published topics instead of sending them.
pub struct RecordingMqtt {
    pub published: Vec<(String, Vec<u8>)>,
    pub subscribed: Vec<String>,
    /// messages handed out by the next receive()
    pub incoming: Vec<(String, Vec<u8>)>,
    /// availability topic the output configured the client with
    pub availability_topic: Option<String>,
}

impl MqttWrapper for RecordingMqtt {
    fn subscribe(&mut self, topic: &str, _qos: QoS) -> anyhow::Result<()> {
        self.subscribed.push(topic.to_string());
        Ok(())
    }

    fn receive(&mut self) -> Vec<(String, Vec<u8>)> {
        std::mem::take(&mut self.incoming)
    }

    fn publish<S, V>(
        &mut self,
        topic: S,
        _qos: QoS,
        _retain: bool,
        payload: V,
    ) -> anyhow::Result<()>
    where
        S: Clone + Into<String>,
        V: Clone + Into<Vec<u8>>,
    {
        self.published.push((topic.into(), payload.into()));
        Ok(())
    }

    fn new(config: &MqttConfig, _suffix: &str) -> Self {
        Self {
            published: Vec::new(),
            subscribed: Vec::new(),
            incoming: Vec::new(),
            availability_topic: config.availability_topic.clone(),
        }
    }
}

pub fn test_config() -> MqttConfig {
    MqttConfig {
        host: "localhost".to_owned(),
        port: None,
        username: None,
        password: None,
        tls: None,
        client_id: None,
        device_id: None,
        availability_topic: None,
    }
}

/// A response like the DTU sends: `inverters` inverter entries and `ports` PV ports.
pub fn response(dtu_sn: &str, inverters: usize, ports: usize) -> HMSStateResponse {
    let mut response = HMSStateResponse::new();
    response.dtu_sn = dtu_sn.to_owned();
    for i in 0..inverters {
        let mut inverter = InverterState::new();
        inverter.port_id = 1;
        inverter.inv_id = 1000 + i as i64;
        inverter.link = 1;
        response.inverter_state.push(inverter);
    }
    for p in 0..ports {
        let mut port = PortState::new();
        port.pv_port = (p % 4) as i32 + 1;
        response.port_state.push(port);
    }
    response
}

/// Answer of the fake DTU to one request frame
pub type Reply = Box<dyn Fn(&[u8]) -> Vec<u8> + Send>;

/// Starts a fake DTU on localhost that answers `replies.len()` connections, one reply each.
/// Each reply closure gets the request frame and returns the bytes to send back.
/// Returns the port and a handle yielding the received request frames.
pub fn fake_dtu(replies: Vec<Reply>) -> (u16, std::thread::JoinHandle<Vec<Vec<u8>>>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for reply in replies {
            let (mut conn, _) = listener.accept().unwrap();
            let mut header = [0u8; HEADER_LEN];
            conn.read_exact(&mut header).unwrap();
            let len = u16::from_be_bytes([header[8], header[9]]) as usize;
            let mut request = header.to_vec();
            request.resize(len, 0);
            conn.read_exact(&mut request[HEADER_LEN..]).unwrap();
            conn.write_all(&reply(&request)).unwrap();
            requests.push(request);
        }
        requests
    });
    (port, handle)
}

/// A DTU reply frame with command `cmd` to `request`, echoing its sequence number, valid CRC
pub fn frame_reply(request: &[u8], cmd: u16, payload: &[u8]) -> Vec<u8> {
    let mut f = b"HM".to_vec();
    f.extend_from_slice(&cmd.to_be_bytes());
    f.extend_from_slice(&request[4..6]);
    f.extend_from_slice(&State::<MODBUS>::calculate(payload).to_be_bytes());
    f.extend_from_slice(&(HEADER_LEN as u16 + payload.len() as u16).to_be_bytes());
    f.extend_from_slice(payload);
    f
}

/// A real-time data reply (0xA211) to `request` carrying `response`
pub fn reply_to(request: &[u8], response: &HMSStateResponse) -> Vec<u8> {
    frame_reply(request, 0xa211, &response.write_to_bytes().unwrap())
}

/// The command of a request frame
pub fn request_command(request: &[u8]) -> u16 {
    u16::from_be_bytes([request[2], request[3]])
}
