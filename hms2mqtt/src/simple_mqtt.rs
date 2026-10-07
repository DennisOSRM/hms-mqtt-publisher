use crate::{
    command::{parse_power_limit, Command},
    metric_collector::MetricCollector,
    mqtt_config::MqttConfig,
    mqtt_wrapper::{MqttWrapper, QoS},
    protos::hoymiles::RealData::{HMSStateResponse, Warning},
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
        let mut simple_mqtt = Self {
            client,
            topic_prefix,
        };
        let command_topic = simple_mqtt.command_topic();
        if let Err(e) = simple_mqtt
            .client
            .subscribe(&command_topic, QoS::AtLeastOnce)
        {
            warn!("could not subscribe to {command_topic}: {e:?}");
        }
        simple_mqtt
    }
}

impl<MQTT: MqttWrapper> SimpleMqtt<MQTT> {
    /// Topic to set the power limit in percent, e.g. `hms800wt2/power_limit/set`
    fn command_topic(&self) -> String {
        format!("{}/power_limit/set", self.topic_prefix)
    }
}

impl<MQTT: MqttWrapper> MetricCollector for SimpleMqtt<MQTT> {
    fn commands(&mut self) -> Vec<Command> {
        let command_topic = self.command_topic();
        self.client
            .receive()
            .into_iter()
            .filter(|(topic, _)| *topic == command_topic)
            .filter_map(|(_, payload)| match parse_power_limit(&payload) {
                Ok(command) => Some(command),
                Err(e) => {
                    warn!("ignoring command on {command_topic}: {e}");
                    None
                }
            })
            .collect()
    }

    fn publish_warnings(&mut self, _hms_state: &HMSStateResponse, warnings: &[Warning]) {
        let prefix = &self.topic_prefix;
        let list: Vec<serde_json::Value> = warnings
            .iter()
            .map(|w| {
                serde_json::json!({
                    "inverter": w.inv_id,
                    "code": w.code,
                    "count": w.count,
                    "start": w.start_time,
                    "end": w.end_time,
                })
            })
            .collect();
        for (topic, payload) in [
            (
                format!("{prefix}/warnings_count"),
                warnings.len().to_string(),
            ),
            (
                format!("{prefix}/warnings"),
                serde_json::Value::from(list).to_string(),
            ),
        ] {
            if let Err(e) = self.client.publish(topic, QoS::AtMostOnce, true, payload) {
                warn!("mqtt error: {e:?}")
            }
        }
    }

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
            (
                format!("{prefix}/pv_energy_total"),
                hms_state
                    .port_state
                    .iter()
                    .map(|port| port.pv_energy_total as i64)
                    .sum::<i64>()
                    .to_string(),
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
                .into_iter()
                // the DTU reports 0 until a limit has been set, which isn't a 0 % limit
                .filter(|(name, _)| *name != "pv_inv_power_limit" || inverter.power_limit > 0)
                .map(|(name, payload)| (format!("{prefix}/{name}"), payload)),
            );
        } else if let Some(inverter) = hms_state.three_phase_inverter_state.first() {
            // three-phase inverters publish the shared values under the same topics as
            // single-phase ones, plus the voltage and current of each phase
            topic_payload_pairs.extend(
                [
                    (
                        "pv_grid_freq".to_string(),
                        (inverter.grid_freq as f32 / 100.).to_string(),
                    ),
                    (
                        "pv_inv_temperature".to_string(),
                        (inverter.temperature as f32 / 10.).to_string(),
                    ),
                    (
                        "pv_inv_reactive_power".to_string(),
                        (inverter.reactive_power as f32 / 10.).to_string(),
                    ),
                    (
                        "pv_inv_power_factor".to_string(),
                        (inverter.power_factor as f32 / 1000.).to_string(),
                    ),
                    (
                        "pv_inv_power_limit".to_string(),
                        (inverter.power_limit as f32 / 10.).to_string(),
                    ),
                    (
                        "pv_inv_warning_count".to_string(),
                        inverter.warning_count.to_string(),
                    ),
                    (
                        "pv_inv_mi_signal".to_string(),
                        inverter.mi_signal.to_string(),
                    ),
                ]
                .into_iter()
                .chain(
                    [
                        ("l1", inverter.voltage_a, inverter.current_a),
                        ("l2", inverter.voltage_b, inverter.current_b),
                        ("l3", inverter.voltage_c, inverter.current_c),
                    ]
                    .into_iter()
                    .flat_map(|(phase, voltage, current)| {
                        [
                            (
                                format!("pv_grid_voltage_{phase}"),
                                (voltage as f32 / 10.).to_string(),
                            ),
                            (
                                format!("pv_inv_ac_current_{phase}"),
                                (current as f32 / 100.).to_string(),
                            ),
                        ]
                    }),
                )
                .filter(|(name, _)| name != "pv_inv_power_limit" || inverter.power_limit > 0)
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
    fn three_phase_inverter_topics() {
        let mut r = response("414312345678", 0, 4);
        let mut inverter = crate::protos::hoymiles::RealData::ThreePhaseInverterState::new();
        inverter.voltage_c = 2312;
        inverter.current_a = 512;
        inverter.grid_freq = 5001;
        r.three_phase_inverter_state.push(inverter);
        let mut sm = SimpleMqtt::<RecordingMqtt>::new(&test_config());
        sm.publish(&r);
        let value = |topic: &str| {
            sm.client
                .published
                .iter()
                .find(|(t, _)| t == topic)
                .map(|(_, p)| String::from_utf8(p.clone()).unwrap())
        };
        assert_eq!(
            value("hms800wt2/pv_grid_voltage_l3").as_deref(),
            Some("231.2")
        );
        assert_eq!(
            value("hms800wt2/pv_inv_ac_current_l1").as_deref(),
            Some("5.12")
        );
        assert_eq!(value("hms800wt2/pv_grid_freq").as_deref(), Some("50.01"));
        assert_eq!(value("hms800wt2/pv_port4_power").as_deref(), Some("0"));
        assert!(value("hms800wt2/pv_grid_voltage").is_none());
    }

    #[test]
    fn total_energy_and_warnings() {
        let mut r = response("414312345678", 1, 2);
        r.port_state[0].pv_energy_total = 1000;
        r.port_state[1].pv_energy_total = 234;
        let mut sm = SimpleMqtt::<RecordingMqtt>::new(&test_config());
        sm.publish(&r);
        let mut warning = Warning::new();
        warning.code = 141;
        sm.publish_warnings(&r, &[warning]);
        let value = |topic: &str| {
            sm.client
                .published
                .iter()
                .find(|(t, _)| t == topic)
                .map(|(_, p)| String::from_utf8(p.clone()).unwrap())
        };
        assert_eq!(value("hms800wt2/pv_energy_total").as_deref(), Some("1234"));
        assert_eq!(value("hms800wt2/warnings_count").as_deref(), Some("1"));
        let list: serde_json::Value =
            serde_json::from_str(&value("hms800wt2/warnings").unwrap()).unwrap();
        assert_eq!(list[0]["code"], 141);
    }

    #[test]
    fn unset_power_limit_is_not_published() {
        let topics = published_topics(1, 2);
        assert!(!topics.contains(&"hms800wt2/pv_inv_power_limit".to_string()));
    }

    #[test]
    fn power_limit_commands() {
        let mut config = test_config();
        config.device_id = Some("roof".to_string());
        let mut sm = SimpleMqtt::<RecordingMqtt>::new(&config);
        assert_eq!(sm.client.subscribed, ["roof/power_limit/set"]);
        sm.client.incoming = vec![
            ("roof/power_limit/set".to_string(), b"40.4".to_vec()),
            ("roof/power_limit/set".to_string(), b"abc".to_vec()),
        ];
        assert_eq!(sm.commands(), [Command::SetPowerLimit(40)]);
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
