// helpers shared by the unit tests
use crate::mqtt_config::MqttConfig;
use crate::mqtt_wrapper::{MqttWrapper, QoS};
use crate::protos::hoymiles::RealData::{HMSStateResponse, InverterState, PortState};

/// MQTT client that records published topics instead of sending them.
pub struct RecordingMqtt {
    pub published: Vec<(String, Vec<u8>)>,
}

impl MqttWrapper for RecordingMqtt {
    fn subscribe(&mut self, _topic: &str, _qos: QoS) -> anyhow::Result<()> {
        Ok(())
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

    fn new(_config: &MqttConfig, _suffix: &str) -> Self {
        Self {
            published: Vec::new(),
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
