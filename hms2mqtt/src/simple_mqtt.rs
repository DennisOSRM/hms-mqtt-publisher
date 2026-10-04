use crate::{
    metric_collector::MetricCollector,
    mqtt_config::MqttConfig,
    mqtt_wrapper::{MqttWrapper, QoS},
    protos::hoymiles::RealData::HMSStateResponse,
};

use chrono::prelude::DateTime;
use chrono::Local;
use log::{debug, warn};
use std::time::{Duration, UNIX_EPOCH};

pub struct SimpleMqtt<MQTT: MqttWrapper> {
    client: MQTT,
    topic_prefix: String,
}

impl<MQTT: MqttWrapper> SimpleMqtt<MQTT> {
    pub fn new(config: &MqttConfig) -> Self {
        let client = MQTT::new(config, "-sm");
        let topic_prefix = config
            .device_id
            .clone()
            .unwrap_or_else(|| "hms800wt2".to_string());
        Self {
            client,
            topic_prefix,
        }
    }
}

impl<MQTT: MqttWrapper> MetricCollector for SimpleMqtt<MQTT> {
    fn publish(&mut self, hms_state: &HMSStateResponse) {
        debug!("{hms_state}");

        let d = UNIX_EPOCH + Duration::from_secs(hms_state.time as u64);
        let datetime = DateTime::<Local>::from(d);
        let inverter_local_time = datetime.format("%Y-%m-%d %H:%M:%S.%f").to_string();

        let pv_current_power = hms_state.pv_current_power as f32 / 10.;
        let pv_daily_yield = hms_state.pv_daily_yield;

        let prefix = &self.topic_prefix;
        let mut topic_payload_pairs = vec![
            (format!("{prefix}/inverter_local_time"), inverter_local_time),
            (
                format!("{prefix}/pv_current_power"),
                pv_current_power.to_string(),
            ),
            (
                format!("{prefix}/pv_daily_yield"),
                pv_daily_yield.to_string(),
            ),
        ];

        // Inverter-level values come from the first inverter, if the DTU reported one
        if let Some(inverter) = hms_state.inverter_state.first() {
            topic_payload_pairs.extend(
                [
                    (
                        "pv_grid_voltage",
                        (inverter.grid_voltage as f32 / 10.).to_string(),
                    ),
                    (
                        "pv_grid_freq",
                        (inverter.grid_freq as f32 / 100.).to_string(),
                    ),
                    (
                        "pv_inv_temperature",
                        (inverter.temperature as f32 / 10.).to_string(),
                    ),
                    (
                        "pv_inv_ac_current",
                        (inverter.ac_current as f32 / 100.).to_string(),
                    ),
                    (
                        "pv_inv_reactive_power",
                        (inverter.reactive_power as f32 / 10.).to_string(),
                    ),
                    (
                        "pv_inv_power_factor",
                        (inverter.power_factor as f32 / 1000.).to_string(),
                    ),
                    (
                        "pv_inv_power_limit",
                        (inverter.power_limit as f32 / 10.).to_string(),
                    ),
                    ("pv_inv_warning_count", inverter.warning_count.to_string()),
                    ("pv_inv_mi_signal", inverter.mi_signal.to_string()),
                ]
                .map(|(name, payload)| (format!("{prefix}/{name}"), payload)),
            );
        } else {
            warn!("response contains no inverter state");
        }

        // One set of topics per reported PV port (1 to 4 depending on the model), numbered from 1
        for (index, port) in hms_state.port_state.iter().enumerate() {
            let n = index + 1;
            topic_payload_pairs.extend(
                [
                    ("voltage", (port.pv_vol as f32 / 10.).to_string()),
                    ("curr", (port.pv_cur as f32 / 100.).to_string()),
                    ("power", (port.pv_power as f32 / 10.).to_string()),
                    ("energy", (port.pv_energy_total as f32).to_string()),
                    ("daily_yield", (port.pv_daily_yield as f32).to_string()),
                    ("code", port.code.to_string()),
                ]
                .map(|(name, payload)| (format!("{prefix}/pv_port{n}_{name}"), payload)),
            );
        }

        topic_payload_pairs
            .into_iter()
            .for_each(|(topic, payload)| {
                if let Err(e) = self.client.publish(topic, QoS::AtMostOnce, true, payload) {
                    warn!("mqtt error: {e:?}")
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{response, test_config, RecordingMqtt};

    fn published_topics(inverters: usize, ports: usize) -> Vec<String> {
        let mut sm = SimpleMqtt::<RecordingMqtt>::new(&test_config());
        sm.publish(&response("414312345678", inverters, ports));
        sm.client.published.iter().map(|(t, _)| t.clone()).collect()
    }

    #[test]
    fn publishes_every_port_of_any_model() {
        for ports in 1..=4 {
            let topics = published_topics(1, ports);
            for n in 1..=ports {
                assert!(topics.contains(&format!("hms800wt2/pv_port{n}_power")));
            }
            assert!(!topics.contains(&format!("hms800wt2/pv_port{}_power", ports + 1)));
        }
    }

    #[test]
    fn missing_inverter_or_ports_do_not_panic() {
        let topics = published_topics(0, 0);
        assert!(topics.contains(&"hms800wt2/pv_current_power".to_string()));
        assert!(!topics.contains(&"hms800wt2/pv_grid_voltage".to_string()));
    }

    #[test]
    fn values_are_scaled() {
        let mut r = response("414312345678", 1, 2);
        r.pv_current_power = 5500;
        r.inverter_state[0].grid_voltage = 2310;
        r.inverter_state[0].grid_freq = 5003;
        r.inverter_state[0].power_factor = 999;
        r.inverter_state[0].power_limit = 1000;
        r.port_state[1].pv_cur = 904;
        let mut sm = SimpleMqtt::<RecordingMqtt>::new(&test_config());
        sm.publish(&r);
        let value = |topic: &str| {
            let (_, payload) = sm
                .client
                .published
                .iter()
                .find(|(t, _)| t == topic)
                .unwrap_or_else(|| panic!("{topic} not published"));
            String::from_utf8(payload.clone()).unwrap()
        };
        assert_eq!(value("hms800wt2/pv_current_power"), "550");
        assert_eq!(value("hms800wt2/pv_grid_voltage"), "231");
        assert_eq!(value("hms800wt2/pv_grid_freq"), "50.03");
        assert_eq!(value("hms800wt2/pv_inv_power_factor"), "0.999");
        assert_eq!(value("hms800wt2/pv_inv_power_limit"), "100");
        assert_eq!(value("hms800wt2/pv_port2_curr"), "9.04");
    }

    #[test]
    fn device_id_replaces_the_topic_prefix() {
        let mut config = test_config();
        config.device_id = Some("roof".to_string());
        let mut sm = SimpleMqtt::<RecordingMqtt>::new(&config);
        sm.publish(&response("414312345678", 1, 2));
        let topics: Vec<&str> = sm
            .client
            .published
            .iter()
            .map(|(t, _)| t.as_str())
            .collect();
        assert!(topics.contains(&"roof/pv_current_power"));
        assert!(topics.contains(&"roof/pv_port2_power"));
        assert!(topics.iter().all(|t| t.starts_with("roof/")));
    }

    #[test]
    fn totals_are_published_once() {
        let topics = published_topics(1, 2);
        let count = topics
            .iter()
            .filter(|t| *t == "hms800wt2/pv_current_power")
            .count();
        assert_eq!(count, 1);
    }
}
