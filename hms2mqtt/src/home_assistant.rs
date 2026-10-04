use crate::home_assistant_config::DeviceConfig;
use crate::mqtt_wrapper::MqttWrapper;
use crate::{mqtt_config::MqttConfig, protos::hoymiles::RealData::HMSStateResponse};

use crate::home_assistant_config::SensorConfig;
use crate::metric_collector::MetricCollector;
use log::{debug, error};
use serde_json::json;

pub struct HomeAssistant<MQTT: MqttWrapper> {
    client: MQTT,
    device_id: Option<String>,
}

impl<MQTT: MqttWrapper> HomeAssistant<MQTT> {
    pub fn new(config: &MqttConfig) -> Self {
        let client = MQTT::new(config, "-ha");
        Self {
            client,
            device_id: config.device_id.clone(),
        }
    }

    fn publish_json(&mut self, topic: &str, payload: serde_json::Value) {
        debug!("Publishing to {topic} with payload {payload}");

        let payload = serde_json::to_string(&payload).unwrap();
        if let Err(e) =
            self.client
                .publish(topic, crate::mqtt_wrapper::QoS::AtMostOnce, true, payload)
        {
            error!("Failed to publish message: {e:?}");
        }
    }

    fn publish_configs(&mut self, config_topic: &str, sensor_configs: &Vec<SensorConfig>) {
        // configs let home assistant know what sensors are available and where to find them
        for sensor_config in sensor_configs {
            let config_topic = format!("{}/{}/config", config_topic, sensor_config.unique_id);
            let config_payload = serde_json::to_value(sensor_config).unwrap();
            self.publish_json(&config_topic, config_payload);
        }
    }

    fn publish_states(&mut self, hms_state: &HMSStateResponse, state_topic: &str) {
        // states contain the actual data
        let json_payload = hms_state.to_json_payload();
        self.publish_json(state_topic, json_payload);
    }
}

impl<MQTT: MqttWrapper> MetricCollector for HomeAssistant<MQTT> {
    fn publish(&mut self, hms_state: &HMSStateResponse) {
        let id = self
            .device_id
            .clone()
            .unwrap_or_else(|| hms_state.short_dtu_sn());
        let config_topic = format!("homeassistant/sensor/hms_{id}");
        let state_topic = format!("solar/hms_{id}/state");

        let device_config = hms_state.create_sensor_configs(&state_topic, &id);

        self.publish_configs(&config_topic, &device_config);
        self.publish_states(hms_state, &state_topic);
    }
}

/// `HMSStateResponse` is a struct that contains the data from the inverter.
///
/// Provide utility functions to extract data from the struct.
impl HMSStateResponse {
    fn get_model(&self) -> String {
        // TODO: figure out a way to properly identify the model
        "HMS-WiFi".to_string()
    }

    fn get_name(&self, id: &str) -> String {
        format!("Hoymiles {} {}", self.get_model(), id)
    }

    fn short_dtu_sn(&self) -> String {
        // first 8 characters; shorter serials are used as they are
        self.dtu_sn.chars().take(8).collect()
    }

    fn get_total_efficiency(&self) -> f32 {
        let total_module_power: f32 = self
            .port_state
            .iter()
            .map(|port| port.pv_power as f32)
            .sum();
        if total_module_power > 0.0 {
            self.pv_current_power as f32 / total_module_power * 100.0
        } else {
            0.0
        }
    }

    fn to_json_payload(&self) -> serde_json::Value {
        // when modifying this function, modify the sensor config in create_device_config accordingly
        let mut json = json!({
            "dtu_sn": self.dtu_sn,
            "pv_current_power": format!("{:.2}", self.pv_current_power as f32 * 0.1),
            "pv_daily_yield": self.pv_daily_yield,
            "efficiency": format!("{:.2}", self.get_total_efficiency())
        });

        // Convert each PortState to json
        for port in self.port_state.iter() {
            json[format!("pv_{}_vol", port.pv_port)] =
                format!("{:.2}", port.pv_vol as f32 * 0.1).into();
            json[format!("pv_{}_cur", port.pv_port)] =
                format!("{:.2}", port.pv_cur as f32 * 0.01).into();
            json[format!("pv_{}_power", port.pv_port)] =
                format!("{:.2}", port.pv_power as f32 * 0.1).into();
            json[format!("pv_{}_energy_total", port.pv_port)] = port.pv_energy_total.into();
            json[format!("pv_{}_daily_yield", port.pv_port)] = port.pv_daily_yield.into();
            json[format!("pv_{}_code", port.pv_port)] = port.code.into();
        }
        // Convert each InverterState to json (for a HMS-XXXW-2T, there is only one inverter)
        for inverter in self.inverter_state.iter() {
            json[format!("inv_{}_grid_voltage", inverter.port_id)] =
                format!("{:.2}", inverter.grid_voltage as f32 * 0.1).into();
            json[format!("inv_{}_grid_freq", inverter.port_id)] =
                format!("{:.2}", inverter.grid_freq as f32 * 0.01).into();
            json[format!("inv_{}_pv_current_power", inverter.port_id)] =
                format!("{:.2}", inverter.pv_current_power as f32 * 0.1).into();
            json[format!("inv_{}_temperature", inverter.port_id)] =
                format!("{:.2}", inverter.temperature as f32 * 0.1).into();
            json[format!("inv_{}_ac_current", inverter.port_id)] =
                format!("{:.2}", inverter.ac_current as f32 * 0.01).into();
            json[format!("inv_{}_reactive_power", inverter.port_id)] =
                format!("{:.2}", inverter.reactive_power as f32 * 0.1).into();
            json[format!("inv_{}_power_factor", inverter.port_id)] =
                format!("{:.3}", inverter.power_factor as f32 * 0.001).into();
            json[format!("inv_{}_power_limit", inverter.port_id)] =
                format!("{:.1}", inverter.power_limit as f32 * 0.1).into();
            json[format!("inv_{}_warning_count", inverter.port_id)] = inverter.warning_count.into();
            json[format!("inv_{}_mi_signal", inverter.port_id)] = inverter.mi_signal.into();
        }

        json
    }

    fn create_sensor_configs(&self, state_topic: &str, id: &str) -> Vec<SensorConfig> {
        let mut sensors = Vec::new();

        let device_config = DeviceConfig::new(
            self.get_name(id),
            self.get_model(),
            Vec::from([format!("hms_{id}")]),
        );

        // Sensors for the whole inverter
        sensors.extend([
            SensorConfig::string(state_topic, &device_config, "DTU Serial Number", "dtu_sn"),
            SensorConfig::power(
                state_topic,
                &device_config,
                "Total Power",
                "pv_current_power",
            ),
            SensorConfig::energy(
                state_topic,
                &device_config,
                "Total Daily Yield",
                "pv_daily_yield",
            ),
            SensorConfig::efficiency(state_topic, &device_config, "Efficiency", "efficiency"),
        ]);

        // Sensors for each pv string
        for port in &self.port_state {
            let idx = port.pv_port;
            sensors.extend([
                SensorConfig::power(
                    state_topic,
                    &device_config,
                    &format!("PV {} Power", idx),
                    &format!("pv_{}_power", idx),
                ),
                SensorConfig::voltage(
                    state_topic,
                    &device_config,
                    &format!("PV {} Voltage", idx),
                    &format!("pv_{}_vol", idx),
                ),
                SensorConfig::current(
                    state_topic,
                    &device_config,
                    &format!("PV {} Current", idx),
                    &format!("pv_{}_cur", idx),
                ),
                SensorConfig::energy(
                    state_topic,
                    &device_config,
                    &format!("PV {} Daily Yield", idx),
                    &format!("pv_{}_daily_yield", idx),
                ),
                SensorConfig::energy(
                    state_topic,
                    &device_config,
                    &format!("PV {} Energy Total", idx),
                    &format!("pv_{}_energy_total", idx),
                ),
                SensorConfig::diagnostic_value(
                    state_topic,
                    &device_config,
                    &format!("PV {} Status Code", idx),
                    &format!("pv_{}_code", idx),
                ),
            ]);
        }
        for inverter in &self.inverter_state {
            let idx = inverter.port_id;
            sensors.extend([
                SensorConfig::power(
                    state_topic,
                    &device_config,
                    &format!("Inverter {} Power", idx),
                    &format!("inv_{}_pv_current_power", idx),
                ),
                SensorConfig::temperature(
                    state_topic,
                    &device_config,
                    &format!("Inverter {} Temperature", idx),
                    &format!("inv_{}_temperature", idx),
                ),
                SensorConfig::voltage(
                    state_topic,
                    &device_config,
                    &format!("Inverter {} Grid Voltage", idx),
                    &format!("inv_{}_grid_voltage", idx),
                ),
                SensorConfig::frequency(
                    state_topic,
                    &device_config,
                    &format!("Inverter {} Grid Frequency", idx),
                    &format!("inv_{}_grid_freq", idx),
                ),
                SensorConfig::current(
                    state_topic,
                    &device_config,
                    &format!("Inverter {} AC Current", idx),
                    &format!("inv_{}_ac_current", idx),
                ),
                SensorConfig::reactive_power(
                    state_topic,
                    &device_config,
                    &format!("Inverter {} Reactive Power", idx),
                    &format!("inv_{}_reactive_power", idx),
                ),
                SensorConfig::power_factor(
                    state_topic,
                    &device_config,
                    &format!("Inverter {} Power Factor", idx),
                    &format!("inv_{}_power_factor", idx),
                ),
                SensorConfig::percentage(
                    state_topic,
                    &device_config,
                    &format!("Inverter {} Power Limit", idx),
                    &format!("inv_{}_power_limit", idx),
                ),
                SensorConfig::diagnostic_value(
                    state_topic,
                    &device_config,
                    &format!("Inverter {} Warning Count", idx),
                    &format!("inv_{}_warning_count", idx),
                ),
                SensorConfig::diagnostic_value(
                    state_topic,
                    &device_config,
                    &format!("Inverter {} Signal", idx),
                    &format!("inv_{}_mi_signal", idx),
                ),
            ]);
        }
        sensors
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{response, test_config, RecordingMqtt};

    #[test]
    fn short_serial_does_not_panic() {
        for sn in ["", "4143", "414312345678"] {
            let mut ha = HomeAssistant::<RecordingMqtt>::new(&test_config());
            ha.publish(&response(sn, 1, 2));
            assert!(!ha.client.published.is_empty());
        }
        assert_eq!(response("414312345678", 1, 2).short_dtu_sn(), "41431234");
        assert_eq!(response("4143", 1, 2).short_dtu_sn(), "4143");
    }

    fn sample() -> HMSStateResponse {
        let mut r = response("414312345678", 1, 2);
        r.pv_current_power = 5500; // 550.0 W
        r.pv_daily_yield = 1434;
        let inv = &mut r.inverter_state[0];
        inv.grid_voltage = 2310;
        inv.grid_freq = 5003;
        inv.pv_current_power = 5500;
        inv.temperature = 504;
        inv.ac_current = 238;
        inv.power_factor = 999;
        inv.power_limit = 1000;
        inv.warning_count = 4;
        r.port_state[0].pv_vol = 350;
        r.port_state[0].pv_cur = 785;
        r.port_state[0].pv_power = 2750;
        r.port_state[0].pv_daily_yield = 552;
        r.port_state[1].pv_power = 3058;
        r
    }

    #[test]
    fn payload_values_are_scaled() {
        let json = sample().to_json_payload();
        assert_eq!(json["pv_current_power"], "550.00");
        assert_eq!(json["pv_daily_yield"], 1434);
        assert_eq!(json["pv_1_vol"], "35.00");
        assert_eq!(json["pv_1_cur"], "7.85");
        assert_eq!(json["pv_1_power"], "275.00");
        assert_eq!(json["pv_1_daily_yield"], 552);
        assert_eq!(json["inv_1_grid_voltage"], "231.00");
        assert_eq!(json["inv_1_grid_freq"], "50.03");
        assert_eq!(json["inv_1_temperature"], "50.40");
        assert_eq!(json["inv_1_ac_current"], "2.38");
        assert_eq!(json["inv_1_power_factor"], "0.999");
        assert_eq!(json["inv_1_power_limit"], "100.0");
        assert_eq!(json["inv_1_warning_count"], 4);
    }

    #[test]
    fn efficiency_is_ac_over_dc_power_and_safe_at_night() {
        // 5500 / (2750 + 3058) = 94.697 %
        assert_eq!(sample().to_json_payload()["efficiency"], "94.70");
        assert_eq!(
            response("414312345678", 1, 2).to_json_payload()["efficiency"],
            "0.00"
        );
    }

    #[test]
    fn every_discovered_sensor_has_a_value_in_the_state_payload() {
        for (inverters, ports) in [(1, 1), (1, 2), (1, 4), (0, 2)] {
            let r = response("414312345678", inverters, ports);
            let json = r.to_json_payload();
            let configs = r.create_sensor_configs("solar/hms_41431234/state", "41431234");
            assert!(!configs.is_empty());
            for config in configs {
                let config = serde_json::to_value(&config).unwrap();
                let template = config["value_template"].as_str().unwrap();
                let key = template
                    .trim_start_matches("{{ value_json.")
                    .trim_end_matches(" }}");
                assert!(
                    !json[key].is_null(),
                    "sensor {} reads missing key {key}",
                    config["unique_id"]
                );
                assert_eq!(config["state_topic"], "solar/hms_41431234/state");
            }
        }
    }

    #[test]
    fn publishes_discovery_under_the_short_serial() {
        let mut ha = HomeAssistant::<RecordingMqtt>::new(&test_config());
        ha.publish(&sample());
        let topics: Vec<&str> = ha
            .client
            .published
            .iter()
            .map(|(t, _)| t.as_str())
            .collect();
        assert!(topics.contains(&"solar/hms_41431234/state"));
        assert!(
            topics
                .iter()
                .any(|t| t.starts_with("homeassistant/sensor/hms_41431234/")
                    && t.ends_with("/config"))
        );
    }

    #[test]
    fn device_id_names_topics_and_ids() {
        let mut config = test_config();
        config.device_id = Some("roof".to_string());
        let mut ha = HomeAssistant::<RecordingMqtt>::new(&config);
        ha.publish(&response("414312345678", 1, 2));
        let topics: Vec<&str> = ha
            .client
            .published
            .iter()
            .map(|(t, _)| t.as_str())
            .collect();
        assert!(topics.contains(&"solar/hms_roof/state"));
        assert!(topics
            .iter()
            .all(|t| !t.starts_with("homeassistant/")
                || t.starts_with("homeassistant/sensor/hms_roof/")));
        let (_, payload) = ha
            .client
            .published
            .iter()
            .find(|(t, _)| t.starts_with("homeassistant/"))
            .unwrap();
        let config: serde_json::Value = serde_json::from_slice(payload).unwrap();
        assert!(config["unique_id"]
            .as_str()
            .unwrap()
            .starts_with("hms_roof_"));
        assert_eq!(config["device"]["name"], "Hoymiles HMS-WiFi roof");
    }

    #[test]
    fn inverters_with_the_same_serial_prefix_get_distinct_ids() {
        let topics = |device_id: &str| {
            let mut config = test_config();
            config.device_id = Some(device_id.to_string());
            let mut ha = HomeAssistant::<RecordingMqtt>::new(&config);
            ha.publish(&response("414312345678", 1, 2));
            ha.client
                .published
                .iter()
                .map(|(t, _)| t.clone())
                .collect::<Vec<_>>()
        };
        let (a, b) = (topics("12345678"), topics("12349999"));
        assert!(a.iter().all(|t| !b.contains(t)));
    }

    #[test]
    fn any_number_of_ports_and_inverters() {
        for (inverters, ports) in [(0, 0), (1, 1), (1, 4), (2, 8)] {
            let mut ha = HomeAssistant::<RecordingMqtt>::new(&test_config());
            ha.publish(&response("414312345678", inverters, ports));
        }
    }
}
