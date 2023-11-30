use std::{thread, time::Duration};

use crate::{
    metric_collector::MetricCollector, mqtt_config::MqttConfig,
    protos::hoymiles::RealData::HMSStateResponse,
};

use log::{debug, warn};
use rumqttc::{Client, MqttOptions, QoS};

pub struct SimpleMqtt {
    client: Client,
}

impl SimpleMqtt {
    pub fn new(config: &MqttConfig) -> Self {
        let mut mqttoptions = MqttOptions::new(
            "hms800w2t-mqtt-publisher",
            &config.host,
            config.port.unwrap_or(1883),
        );
        mqttoptions.set_keep_alive(Duration::from_secs(5));

        //parse the mqtt authentication options
        if let Some((username, password)) = match (&config.username, &config.password) {
            (None, None) => None,
            (None, Some(_)) => None,
            (Some(username), None) => Some((username.clone(), "".into())),
            (Some(username), Some(password)) => Some((username.clone(), password.clone())),
        } {
            mqttoptions.set_credentials(username, password);
        }

        let (client, mut connection) = Client::new(mqttoptions, 10);

        thread::spawn(move || {
            // keep polling the event loop to make sure outgoing messages get sent
            for _ in connection.iter() {}
        });

        Self { client }
    }
}

impl MetricCollector for SimpleMqtt {
    fn publish(&mut self, hms_state: &HMSStateResponse) {
        debug!("{hms_state}");

        // self.client.subscribe("hms800wt2", QoS::AtMostOnce).unwrap();

        let pv_current_power = hms_state.pv_current_power as f32 / 10.;
        let pv_daily_yield = hms_state.pv_daily_yield;
        let pv_grid_voltage = hms_state.inverter_state[0].grid_voltage as f32 / 10.;
        let pv_grid_freq = hms_state.inverter_state[0].grid_freq as f32 / 100.;
        let pv_inv_temperature = hms_state.inverter_state[0].temperature as f32 / 10.;
        let pv_port1_voltage = hms_state.port_state[0].pv_vol as f32 / 10.;
        let pv_port1_curr = hms_state.port_state[0].pv_cur as f32 / 100.;
        let pv_port1_power = hms_state.port_state[0].pv_power as f32 / 10.;
        let pv_port1_energy = hms_state.port_state[0].pv_energy_total as f32 / 10.;
        let pv_port1_daily_yield = hms_state.port_state[0].pv_daily_yield as f32 / 10.;
        let pv_port2_voltage = hms_state.port_state[1].pv_vol as f32 / 10.;
        let pv_port2_curr = hms_state.port_state[1].pv_cur as f32 / 100.;
        let pv_port2_power = hms_state.port_state[1].pv_power as f32 / 10.;
        let pv_port2_energy = hms_state.port_state[1].pv_energy_total as f32 / 10.;
        let pv_port2_daily_yield = hms_state.port_state[1].pv_daily_yield as f32 / 10.;

        // TODO: this section bears a lot of repetition. Investigate if there's a more idiomatic way to get the same result, perhaps using a macro
        let topic_payload_pairs = [
            ("hms800wt2/pv_current_power", pv_current_power.to_string()),
            ("hms800wt2/pv_daily_yield", pv_daily_yield.to_string()),
            ("hms800wt2/pv_current_power", pv_current_power.to_string()),
            ("hms800wt2/pv_daily_yield", pv_daily_yield.to_string()),
            ("hms800wt2/pv_grid_voltage", pv_grid_voltage.to_string()),
            ("hms800wt2/pv_grid_freq", pv_grid_freq.to_string()),
            (
                "hms800wt2/pv_inv_temperature",
                pv_inv_temperature.to_string(),
            ),
            ("hms800wt2/pv_port1_voltage", pv_port1_voltage.to_string()),
            ("hms800wt2/pv_port1_curr", pv_port1_curr.to_string()),
            ("hms800wt2/pv_port1_power", pv_port1_power.to_string()),
            ("hms800wt2/pv_port1_energy", pv_port1_energy.to_string()),
            (
                "hms800wt2/pv_port1_daily_yield",
                pv_port1_daily_yield.to_string(),
            ),
            ("hms800wt2/pv_port2_voltage", pv_port2_voltage.to_string()),
            ("hms800wt2/pv_port2_curr", pv_port2_curr.to_string()),
            ("hms800wt2/pv_port2_power", pv_port2_power.to_string()),
            ("hms800wt2/pv_port2_energy", pv_port2_energy.to_string()),
            (
                "hms800wt2/pv_port2_daily_yield",
                pv_port2_daily_yield.to_string(),
            ),
        ];

        topic_payload_pairs
            .into_iter()
            .for_each(|(topic, payload)| {
                if let Err(e) = self.client.publish(topic, QoS::AtMostOnce, true, payload) {
                    warn!("mqtt error: {e}")
                }
            });
    }
}
