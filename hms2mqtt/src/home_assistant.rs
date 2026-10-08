use crate::home_assistant_config::DeviceConfig;
use crate::mqtt_wrapper::MqttWrapper;
use crate::{
    mqtt_config::MqttConfig,
    protos::hoymiles::RealData::{HMSStateResponse, Warning},
};

use crate::command::{power_limit_commands, Command, POWER_LIMIT_RANGE};
use crate::home_assistant_config::{NumberConfig, SensorConfig};
use crate::metric_collector::{warnings_json, MetricCollector};
use log::{debug, error};
use serde_json::json;

pub struct HomeAssistant<MQTT: MqttWrapper> {
    client: MQTT,
    device_id: Option<String>,
    /// command topic subscribed to, once the device id is known
    command_topic: Option<String>,
    /// "online" / "offline" of this publisher, referenced by every discovered entity
    availability_topic: String,
}

/// Availability topic of a Home Assistant output, e.g. `solar/hms_roof/availability`.
///
/// It has to be known before connecting, as it is the last will, so it can't use the DTU serial
/// number. Without a device id, the client id (unique per broker connection) keeps instances
/// apart.
pub fn availability_topic(config: &MqttConfig) -> String {
    match (&config.device_id, &config.client_id) {
        (Some(device_id), _) => format!("solar/hms_{device_id}/availability"),
        (None, Some(client_id)) => format!("solar/{client_id}/availability"),
        (None, None) => "solar/hms-mqtt-publish-ha/availability".to_string(),
    }
}

impl<MQTT: MqttWrapper> HomeAssistant<MQTT> {
    pub fn new(config: &MqttConfig) -> Self {
        let availability_topic = availability_topic(config);
        let client = MQTT::new(
            &MqttConfig {
                availability_topic: Some(availability_topic.clone()),
                ..config.clone()
            },
            "-ha",
        );
        Self {
            client,
            device_id: config.device_id.clone(),
            command_topic: None,
            availability_topic,
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

impl<MQTT: MqttWrapper> HomeAssistant<MQTT> {
    fn id(&self, hms_state: &HMSStateResponse) -> String {
        self.device_id
            .clone()
            .unwrap_or_else(|| hms_state.short_dtu_sn())
    }
}

impl<MQTT: MqttWrapper> MetricCollector for HomeAssistant<MQTT> {
    fn publish_warnings(&mut self, hms_state: &HMSStateResponse, warnings: &[Warning]) {
        let topic = format!("solar/hms_{}/warnings", self.id(hms_state));
        self.publish_json(
            &topic,
            json!({ "warnings_count": warnings.len(), "warnings": warnings_json(warnings) }),
        );
    }

    fn commands(&mut self) -> Vec<Command> {
        match &self.command_topic {
            Some(topic) => power_limit_commands(self.client.receive(), topic),
            None => Vec::new(),
        }
    }

    fn publish(&mut self, hms_state: &HMSStateResponse) {
        let id = self.id(hms_state);
        let command_topic = format!("solar/hms_{id}/power_limit/set");
        if self.command_topic.as_ref() != Some(&command_topic) {
            match self
                .client
                .subscribe(&command_topic, crate::mqtt_wrapper::QoS::AtLeastOnce)
            {
                Ok(()) => self.command_topic = Some(command_topic.clone()),
                Err(e) => error!("could not subscribe to {command_topic}: {e:?}"),
            }
        }
        let config_topic = format!("homeassistant/sensor/hms_{id}");
        let state_topic = format!("solar/hms_{id}/state");

        let availability = self.availability_topic.clone();
        let device_config: Vec<SensorConfig> = hms_state
            .create_sensor_configs(&state_topic, &id)
            .into_iter()
            .map(|sensor| sensor.with_availability(&availability))
            .collect();

        self.publish_configs(&config_topic, &device_config);
        if let Some(number) = hms_state
            .create_power_limit_config(&state_topic, &command_topic, &id)
            .map(|number| number.with_availability(&availability))
        {
            let topic = format!("homeassistant/number/hms_{id}/{}/config", number.unique_id);
            self.publish_json(&topic, serde_json::to_value(number).unwrap());
        }
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
        // when modifying this function, modify create_sensor_configs accordingly
        let mut json = json!({
            "dtu_sn": self.dtu_sn,
            "pv_current_power": format!("{:.2}", self.pv_current_power as f32 * 0.1),
            "pv_daily_yield": self.pv_daily_yield,
            "pv_energy_total": self.port_state.iter().map(|port| port.pv_energy_total as i64).sum::<i64>(),
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
        // Convert each InverterState to json (HMS-XXXXW-xT models report one inverter)
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
            // the DTU reports 0 until a limit has been set, which isn't a 0 % limit
            if inverter.power_limit > 0 {
                json[format!("inv_{}_power_limit", inverter.port_id)] =
                    format!("{:.1}", inverter.power_limit as f32 * 0.1).into();
            }
            json[format!("inv_{}_warning_count", inverter.port_id)] = inverter.warning_count.into();
            json[format!("inv_{}_mi_signal", inverter.port_id)] = inverter.mi_signal.into();
        }
        // Three-phase inverters (e.g. HMT series), numbered from 1
        for (index, inverter) in self.three_phase_inverter_state.iter().enumerate() {
            let key = |name: &str| format!("inv3_{}_{name}", index + 1);
            json[key("power")] = format!("{:.2}", inverter.power as f32 * 0.1).into();
            json[key("grid_freq")] = format!("{:.2}", inverter.grid_freq as f32 * 0.01).into();
            json[key("temperature")] = format!("{:.2}", inverter.temperature as f32 * 0.1).into();
            for (phase, voltage, current) in [
                ("l1", inverter.voltage_a, inverter.current_a),
                ("l2", inverter.voltage_b, inverter.current_b),
                ("l3", inverter.voltage_c, inverter.current_c),
            ] {
                json[key(&format!("voltage_{phase}"))] =
                    format!("{:.2}", voltage as f32 * 0.1).into();
                json[key(&format!("current_{phase}"))] =
                    format!("{:.2}", current as f32 * 0.01).into();
            }
            json[key("reactive_power")] =
                format!("{:.2}", inverter.reactive_power as f32 * 0.1).into();
            json[key("power_factor")] =
                format!("{:.3}", inverter.power_factor as f32 * 0.001).into();
            if inverter.power_limit > 0 {
                json[key("power_limit")] =
                    format!("{:.1}", inverter.power_limit as f32 * 0.1).into();
            }
            json[key("warning_count")] = inverter.warning_count.into();
            json[key("mi_signal")] = inverter.mi_signal.into();
        }

        json
    }

    /// Number entity to set the power limit of all inverters, showing the current limit of the
    /// first inverter
    fn create_power_limit_config(
        &self,
        state_topic: &str,
        command_topic: &str,
        id: &str,
    ) -> Option<NumberConfig> {
        let key = if let Some(inverter) = self.inverter_state.first() {
            format!("inv_{}_power_limit", inverter.port_id)
        } else if !self.three_phase_inverter_state.is_empty() {
            "inv3_1_power_limit".to_string()
        } else {
            return None;
        };
        let device_config = DeviceConfig::new(
            self.get_name(id),
            self.get_model(),
            Vec::from([format!("hms_{id}")]),
        );
        Some(NumberConfig::power_limit(
            state_topic,
            command_topic,
            &device_config,
            &key,
            POWER_LIMIT_RANGE,
        ))
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
            SensorConfig::energy(
                state_topic,
                &device_config,
                "Total Energy",
                "pv_energy_total",
            ),
            // fetched every few minutes and published to its own topic
            SensorConfig::diagnostic_value(
                &format!("solar/hms_{id}/warnings"),
                &device_config,
                "Warnings",
                "warnings_count",
            ),
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
            if inverter.power_limit > 0 {
                sensors.push(SensorConfig::percentage(
                    state_topic,
                    &device_config,
                    &format!("Inverter {} Power Limit", idx),
                    &format!("inv_{}_power_limit", idx),
                ));
            }
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
        for (index, inverter) in self.three_phase_inverter_state.iter().enumerate() {
            let n = index + 1;
            let name = |what: &str| format!("Three-Phase Inverter {n} {what}");
            let key = |what: &str| format!("inv3_{n}_{what}");
            let (topic, device) = (state_topic, &device_config);
            if inverter.power_limit > 0 {
                sensors.push(SensorConfig::percentage(
                    topic,
                    device,
                    &name("Power Limit"),
                    &key("power_limit"),
                ));
            }
            sensors.extend([
                SensorConfig::power(topic, device, &name("Power"), &key("power")),
                SensorConfig::temperature(topic, device, &name("Temperature"), &key("temperature")),
                SensorConfig::frequency(topic, device, &name("Grid Frequency"), &key("grid_freq")),
                SensorConfig::reactive_power(
                    topic,
                    device,
                    &name("Reactive Power"),
                    &key("reactive_power"),
                ),
                SensorConfig::power_factor(
                    topic,
                    device,
                    &name("Power Factor"),
                    &key("power_factor"),
                ),
                SensorConfig::diagnostic_value(
                    topic,
                    device,
                    &name("Warning Count"),
                    &key("warning_count"),
                ),
                SensorConfig::diagnostic_value(topic, device, &name("Signal"), &key("mi_signal")),
            ]);
            for phase in ["L1", "L2", "L3"] {
                let lower = phase.to_lowercase();
                sensors.extend([
                    SensorConfig::voltage(
                        topic,
                        device,
                        &name(&format!("Grid Voltage {phase}")),
                        &key(&format!("voltage_{lower}")),
                    ),
                    SensorConfig::current(
                        topic,
                        device,
                        &name(&format!("Current {phase}")),
                        &key(&format!("current_{lower}")),
                    ),
                ]);
            }
        }
        sensors
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protos::hoymiles::RealData::ThreePhaseInverterState;
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
    fn unset_power_limit_is_not_published() {
        let r = response("414312345678", 1, 2);
        assert!(r.to_json_payload()["inv_1_power_limit"].is_null());
        let configs = r.create_sensor_configs("solar/hms_41431234/state", "41431234");
        assert!(configs
            .iter()
            .all(|c| !serde_json::to_value(c).unwrap()["value_template"]
                .as_str()
                .unwrap()
                .contains("power_limit")));
        // a limit set through the app is published, sensor included
        let s = sample();
        let configs = s.create_sensor_configs("solar/hms_41431234/state", "41431234");
        assert!(configs
            .iter()
            .any(|c| serde_json::to_value(c).unwrap()["value_template"]
                .as_str()
                .unwrap()
                .contains("inv_1_power_limit")));
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
                if config["state_topic"] != "solar/hms_41431234/state" {
                    assert_eq!(config["state_topic"], "solar/hms_41431234/warnings");
                    continue;
                }
                let template = config["value_template"].as_str().unwrap();
                let key = template
                    .trim_start_matches("{{ value_json.")
                    .trim_end_matches(" }}");
                assert!(
                    !json[key].is_null(),
                    "sensor {} reads missing key {key}",
                    config["unique_id"]
                );
            }
        }
    }

    #[test]
    fn availability_topic_is_distinct_per_device_and_client() {
        let topic = |device_id: Option<&str>, client_id: Option<&str>| {
            let mut config = test_config();
            config.device_id = device_id.map(str::to_string);
            config.client_id = client_id.map(str::to_string);
            HomeAssistant::<RecordingMqtt>::new(&config)
                .client
                .availability_topic
                .unwrap()
        };
        assert_eq!(
            topic(Some("roof"), Some("client")),
            "solar/hms_roof/availability"
        );
        assert_eq!(
            topic(None, Some("hms-mqtt-publish-hms-ha")),
            "solar/hms-mqtt-publish-hms-ha/availability"
        );
        assert_eq!(topic(None, None), "solar/hms-mqtt-publish-ha/availability");
        assert_ne!(topic(Some("roof"), None), topic(Some("garage"), None));
    }

    #[test]
    fn every_discovered_entity_references_the_availability_topic() {
        let mut config = test_config();
        config.device_id = Some("roof".to_string());
        let mut ha = HomeAssistant::<RecordingMqtt>::new(&config);
        ha.publish(&response("414312345678", 1, 2));
        let discovery: Vec<serde_json::Value> = ha
            .client
            .published
            .iter()
            .filter(|(t, _)| t.starts_with("homeassistant/"))
            .map(|(_, payload)| serde_json::from_slice(payload).unwrap())
            .collect();
        assert!(discovery
            .iter()
            .any(|config| config["command_topic"].is_string()));
        for config in discovery {
            assert_eq!(
                config["availability_topic"], "solar/hms_roof/availability",
                "{}",
                config["unique_id"]
            );
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
        assert!(topics.iter().all(|t| !t.starts_with("homeassistant/")
            || t.starts_with("homeassistant/sensor/hms_roof/")
            || t.starts_with("homeassistant/number/hms_roof/")));
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
    fn three_phase_inverters_are_published() {
        let mut r = response("414312345678", 0, 4);
        let mut inverter = ThreePhaseInverterState::new();
        inverter.link = 1;
        inverter.power = 15_000;
        inverter.voltage_b = 2305;
        inverter.current_c = 712;
        inverter.grid_freq = 4998;
        r.three_phase_inverter_state.push(inverter);
        let json = r.to_json_payload();
        assert_eq!(json["inv3_1_power"], "1500.00");
        assert_eq!(json["inv3_1_voltage_l2"], "230.50");
        assert_eq!(json["inv3_1_current_l3"], "7.12");
        assert_eq!(json["inv3_1_grid_freq"], "49.98");

        let configs = r.create_sensor_configs("solar/hms_41431234/state", "41431234");
        let keys: Vec<String> = configs
            .iter()
            .map(|c| serde_json::to_value(c).unwrap()["value_template"].to_string())
            .collect();
        for key in ["inv3_1_power", "inv3_1_voltage_l1", "inv3_1_current_l3"] {
            assert!(keys.iter().any(|k| k.contains(key)), "{key}");
        }
        // every discovered sensor of the state topic reads a key that exists in the payload
        for config in configs {
            let config = serde_json::to_value(&config).unwrap();
            if config["state_topic"] != "solar/hms_41431234/state" {
                continue;
            }
            let template = config["value_template"].as_str().unwrap();
            let key = template
                .trim_start_matches("{{ value_json.")
                .trim_end_matches(" }}");
            assert!(!json[key].is_null(), "missing {key}");
        }
    }

    #[test]
    fn three_phase_power_limit_has_a_sensor_and_a_number_entity() {
        let mut r = response("414312345678", 0, 4);
        let mut inverter = ThreePhaseInverterState::new();
        inverter.link = 1;
        inverter.power_limit = 755;
        r.three_phase_inverter_state.push(inverter);
        assert_eq!(r.to_json_payload()["inv3_1_power_limit"], "75.5");

        let sensors = r.create_sensor_configs("solar/hms_41431234/state", "41431234");
        assert!(sensors
            .iter()
            .any(|c| c.unique_id == "hms_41431234_inv3_1_power_limit"));
        let number = r
            .create_power_limit_config("solar/hms_41431234/state", "set", "41431234")
            .expect("number entity");
        let number = serde_json::to_value(number).unwrap();
        assert!(number["value_template"]
            .as_str()
            .unwrap()
            .contains("inv3_1_power_limit"));
    }

    #[test]
    fn total_energy_is_the_sum_of_the_ports() {
        let mut r = response("414312345678", 1, 2);
        r.port_state[0].pv_energy_total = 922_613;
        r.port_state[1].pv_energy_total = 995_242;
        assert_eq!(r.to_json_payload()["pv_energy_total"], 1_917_855);
    }

    #[test]
    fn warnings_are_published_to_their_own_topic() {
        let mut ha = HomeAssistant::<RecordingMqtt>::new(&test_config());
        let mut warning = Warning::new();
        warning.inv_id = 22069994788405;
        warning.code = 141;
        warning.count = 2;
        warning.start_time = 1_790_000_000;
        ha.publish_warnings(&response("414312345678", 1, 2), &[warning]);
        let (topic, payload) = &ha.client.published[0];
        assert_eq!(topic, "solar/hms_41431234/warnings");
        let json: serde_json::Value = serde_json::from_slice(payload).unwrap();
        assert_eq!(json["warnings_count"], 1);
        assert_eq!(json["warnings"][0]["code"], 141);
        assert_eq!(json["warnings"][0]["inverter"], 22069994788405_i64);
        assert_eq!(json["warnings"][0]["end"], 0);
    }

    #[test]
    fn power_limit_number_entity_and_commands() {
        let mut ha = HomeAssistant::<RecordingMqtt>::new(&test_config());
        assert!(ha.commands().is_empty());
        ha.publish(&response("414312345678", 1, 2));
        assert_eq!(ha.client.subscribed, ["solar/hms_41431234/power_limit/set"]);

        let (_, payload) = ha
            .client
            .published
            .iter()
            .find(|(t, _)| t.starts_with("homeassistant/number/hms_41431234/"))
            .expect("number entity");
        let number: serde_json::Value = serde_json::from_slice(payload).unwrap();
        assert_eq!(
            number["command_topic"],
            "solar/hms_41431234/power_limit/set"
        );
        assert_eq!(number["state_topic"], "solar/hms_41431234/state");
        assert_eq!(
            (number["min"].as_u64(), number["max"].as_u64()),
            (Some(2), Some(100))
        );
        assert!(number["value_template"]
            .as_str()
            .unwrap()
            .contains("inv_1_power_limit"));

        // a second reading doesn't subscribe again
        ha.publish(&response("414312345678", 1, 2));
        assert_eq!(ha.client.subscribed.len(), 1);

        ha.client.incoming = vec![
            (
                "solar/hms_41431234/power_limit/set".to_string(),
                b"60".to_vec(),
            ),
            (
                "solar/hms_41431234/power_limit/set".to_string(),
                b"500".to_vec(),
            ),
            ("other/topic".to_string(), b"30".to_vec()),
        ];
        assert_eq!(ha.commands(), [Command::SetPowerLimit(60)]);
    }

    #[test]
    fn no_power_limit_entity_without_inverter() {
        let mut ha = HomeAssistant::<RecordingMqtt>::new(&test_config());
        ha.publish(&response("414312345678", 0, 2));
        assert!(ha
            .client
            .published
            .iter()
            .all(|(t, _)| !t.starts_with("homeassistant/number/")));
    }

    #[test]
    fn any_number_of_ports_and_inverters() {
        for (inverters, ports) in [(0, 0), (1, 1), (1, 4), (2, 8)] {
            let mut ha = HomeAssistant::<RecordingMqtt>::new(&test_config());
            ha.publish(&response("414312345678", inverters, ports));
        }
    }
}
