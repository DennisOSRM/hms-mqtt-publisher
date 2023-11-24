use std::{thread, time::Duration};

use crate::mqtt_schemas::DeviceConfig;
use crate::{mqtt_config::MqttConfig, protos::hoymiles::RealData::HMSStateResponse};

use crate::metric_collector::MetricCollector;
use crate::mqtt_schemas::SensorConfig;
use log::{debug, error};
use rumqttc::{Client, MqttOptions, QoS};
use serde_json::json;

pub struct Mqtt {
    client: Client,
}

impl Mqtt {
    pub fn new(config: &MqttConfig) -> Self {
        let this_host = hostname::get().unwrap().into_string().unwrap();
        let mut mqttoptions = MqttOptions::new(
            format!("hms-mqtt-publisher_{}", this_host),
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

    fn publish_json(&mut self, topic: &str, payload: serde_json::Value) {
        debug!("Publishing to {topic} with payload {payload}");

        let payload = serde_json::to_string(&payload).unwrap();
        if let Err(e) = self.client.publish(topic, QoS::AtMostOnce, true, payload) {
            error!("Failed to publish message: {e}");
        }
    }

    fn publish_configs(&mut self, config_topic: &str, sensor_configs: &Vec<SensorConfig>) {
        // configs let home assistant know what sensors are available and where to find them
        for sensor_config in sensor_configs {
            let config_topic = format!("{}/{}/config", config_topic, sensor_config.unique_id);
            let config_payload = serde_json::to_value(&sensor_config).unwrap();
            self.publish_json(&config_topic, config_payload);
        }
    }

    fn publish_states(&mut self, hms_state: &HMSStateResponse, state_topic: &str) {
        // states contain the actual data
        let json_payload = hms_state.to_json_payload();
        self.publish_json(state_topic, json_payload);
    }
}

impl MetricCollector for Mqtt {
    fn publish(&mut self, hms_state: &HMSStateResponse) {
        let config_topic = format!("homeassistant/sensor/hms_{}", hms_state.short_dtu_sn());
        let state_topic = format!("solar/hms_{}/state", hms_state.short_dtu_sn());

        let device_config = hms_state.create_sensor_configs(&state_topic);

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
        format!("HMS-WiFi")
    }

    fn get_name(&self) -> String {
        format!("Hoymiles {} {}", self.get_model(), self.short_dtu_sn())
    }

    fn short_dtu_sn(&self) -> String {
        self.dtu_sn[..8].to_string()
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
                format!("{:.2}", port.pv_cur as f32 * 0.1).into();
            json[format!("pv_{}_power", port.pv_port)] =
                format!("{:.2}", port.pv_power as f32 * 0.1).into();
            json[format!("pv_{}_energy_total", port.pv_port)] = port.pv_energy_total.into();
            json[format!("pv_{}_daily_yield", port.pv_port)] = port.pv_daily_yield.into();
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
        }

        json
    }

    fn create_sensor_configs(&self, state_topic: &str) -> Vec<SensorConfig> {
        let mut sensors = Vec::new();

        let device_config = DeviceConfig::new(
            self.get_name(),
            self.get_model(),
            Vec::from([format!("hms_{}", self.short_dtu_sn())]),
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
            ]);
        }
        sensors
    }
}
