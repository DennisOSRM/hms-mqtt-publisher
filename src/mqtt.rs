use std::{thread, time::Duration};

use crate::protos::hoymiles::RealData::{HMSStateResponse, InverterState};

use serde::Serialize;
use rumqttc::{Client, MqttOptions, QoS};
use serde_json::json;
use log::{error, debug};

pub trait MetricCollector {
    fn publish(&mut self, hms_state: &HMSStateResponse);
}

pub struct Mqtt {
    client: Client,
}


/// `HMSStateResponse` is a struct that contains the data from the inverter.
///
/// Provide utility functions to extract data from the struct.
impl HMSStateResponse {
    fn get_model(&self) -> String {
         // TODO: identify model from dtu_sn
        "HMS-800W-T2".to_string()
    }

    fn get_name(&self) -> String {
        format!("Hoymiles {} {}", self.get_model(), self.short_dtu_sn())
    }

    fn short_dtu_sn(&self) -> String {
        self.dtu_sn.iter().take(8).map(|v| v.to_string()).collect::<String>()
    }

    fn get_total_efficiency(&self) -> f32{
        let total_input_power: f32 = self.port_state.iter()
        .map(|port| port.pv_power as f32).sum();
    
        if total_input_power > 0.0 {
            return (self.pv_current_power as f32) / total_input_power * 100.0
        } else {
            return 0.0
        };
    }

    fn get_inverter(&self, pv_port: i32) -> InverterState {
        self.inverter_state.iter().find(
            |inv| inv.port_id == pv_port
        ).unwrap_or(&InverterState::default()).clone()
    }

    fn to_json_payload(&self) -> serde_json::Value {
        // when modifying this function, modify the sensor config in create_device_config accordingly
        let mut json = json!({
            "dtu_sn": self.dtu_sn.iter().map(|v| v.to_string()).collect::<String>(),
            "pv_current_power": format!("{:.2}", self.pv_current_power as f32 * 0.1),
            "pv_daily_yield": self.pv_daily_yield,
            "efficiency": self.get_total_efficiency(),
        });

        // Convert each PortState to json
        for port in self.port_state.iter() {
            json[format!("pv_{}_vol", port.pv_port)] = format!("{:.2}", port.pv_vol as f32 * 0.1).into();
            json[format!("pv_{}_cur", port.pv_port)] = format!("{:.2}", port.pv_cur as f32 * 0.1).into();
            json[format!("pv_{}_power", port.pv_port)] = format!("{:.2}", port.pv_power as f32 * 0.1).into();
            json[format!("pv_{}_energy_total", port.pv_port)] = port.pv_energy_total.into();
            json[format!("pv_{}_daily_yield", port.pv_port)] = port.pv_daily_yield.into();
            
            let inverter = &self.get_inverter(port.pv_port);

            json[format!("inv_{}_grid_voltage", port.pv_port)] = format!("{:.2}", inverter.grid_voltage as f32 * 0.1).into();
            json[format!("inv_{}_grid_freq", port.pv_port)] = format!("{:.2}", inverter.grid_freq as f32 * 0.1).into();
            json[format!("inv_{}_pv_current_power", port.pv_port)] = format!("{:.2}", inverter.pv_current_power as f32 * 0.1).into();
            json[format!("inv_{}_temperature", port.pv_port)] = format!("{:.2}", inverter.temperature as f32 * 0.1).into();

            // Efficiency: Requires data from both PortState and InverterState
            let efficiency = (port.pv_power as f32) / (inverter.pv_current_power as f32) * 100.0;
            json[format!("inv_{}_efficiency", port.pv_port)] = format!("{:.2}", efficiency).into();
        }

        json
    }
}

/// `DeviceConfig` is used to define the configuration for a Home Assistant device
/// in the MQTT discovery protocol and is used to group entities together.
/// 
#[derive(Serialize, Clone)]
pub struct DeviceConfig {
    name: String,
    model: String,
    identifiers: Vec<String>,
    manufacturer: String,
    sw_version: String,  // Software version of the application that supplies the discovered MQTT item.
}


impl DeviceConfig {
    pub fn new(name: String, model: String, identifiers: Vec<String>) -> Self {
        Self {
            name,
            model,
            identifiers,
            manufacturer: "Hoymiles".to_string(),
            /// Rust compiler sets the CARGO_PKG_VERSION environment from the Cargo.toml . 
            sw_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

/// `SensorConfig` is used to define the configuration for a Home Assistant sensor entity
/// in the MQTT discovery protocol.
/// 
/// More information about the MQTT discovery protocol can be found here:
/// https://www.home-assistant.io/docs/mqtt/discovery/
/// 
/// More information about the Home assistant sensor entities can be found here:
/// https://developers.home-assistant.io/docs/core/entity/sensor/
///
#[derive(Serialize)]
pub struct SensorConfig {
    unique_id: String, //  A globally unique identifier for the sensor.
    name: String,  // The name of the sensor.
    state_topic: String, // The MQTT topic where sensor readings will be published.
    unit_of_measurement: String,  // The unit of measurement of the sensor.
    value_template: String, // A template to extract a value from the mqtt message.
    device: DeviceConfig,  // The device that the sensor belongs to, used to group entities together.
    // exclude field if they are empty
    #[serde(skip_serializing_if = "Option::is_none")]
    device_class: Option<String>,  // The type/class of the sensor, e.g. energy, power, temperature, etc.
}


impl SensorConfig {
    pub fn new_sensor(
        state_topic: &str, device_config: &DeviceConfig, unique_id: &str, name: &str, 
        device_class: &str, unit_of_measurement: &str
    ) -> Self {
        let value_template = format!("{{{{ value_json.{} }}}}", unique_id);
        let unique_id = format!("hms_{}_{}", device_config.identifiers[0], unique_id);
        let mut _device_class = None;
        if device_class != "" {
            _device_class = Some(device_class.to_string());
        }
        SensorConfig {
            unique_id, 
            name: name.to_string(),
            state_topic: state_topic.to_string(),
            unit_of_measurement: unit_of_measurement.to_string(),
            device_class: _device_class,
            value_template,
            device: device_config.clone()
        }
    }

    pub fn string(state_topic: &str, device_config: &DeviceConfig,  name: &str, key: &str) -> Self {
        Self::new_sensor(state_topic, &device_config, key, name, "", "")
    }

    pub fn power(state_topic: &str, device_config: &DeviceConfig,  name: &str, key: &str) -> Self {
        Self::new_sensor(state_topic, &device_config, key, name, "power", "W")
    }

    pub fn energy(state_topic: &str, device_config: &DeviceConfig,  name: &str, key: &str) -> Self {
        Self::new_sensor(state_topic, &device_config, key, name, "energy", "Wh")
    }

    pub fn voltage(state_topic: &str, device_config: &DeviceConfig,  name: &str, key: &str) -> Self {
        Self::new_sensor(state_topic, &device_config, key, name, "voltage", "V")
    }

    pub fn current(state_topic: &str, device_config: &DeviceConfig,  name: &str, key: &str) -> Self {
        Self::new_sensor(state_topic, &device_config, key, name, "current", "A")
    }

    pub fn temperature(state_topic: &str, device_config: &DeviceConfig,  name: &str, key: &str) -> Self {
        Self::new_sensor(state_topic, &device_config, key, name, "temperature", "°C")
    }

    pub fn efficiency(state_topic: &str, device_config: &DeviceConfig,  name: &str, key: &str) -> Self {
        Self::new_sensor(state_topic, &device_config, key, name, "", "%")
    }

}


fn create_sensor_configs(hms_state: &HMSStateResponse, state_topic: &str) -> Vec<SensorConfig> {
    let mut sensors = Vec::new();

    let device_config = DeviceConfig::new(
        hms_state.get_name(),
        hms_state.get_model(),
        Vec::from([format!("{}_{}", hms_state.get_model(), hms_state.short_dtu_sn())]),
    );

    // Sensors for the whole inverter
    sensors.extend([
        SensorConfig::string(state_topic, &device_config, "DTU Serial Number", "dtu_sn"),
        SensorConfig::power(state_topic, &device_config, "Total Power", "pv_current_power"),
        SensorConfig::energy(state_topic, &device_config, "Total Daily Yield", "pv_daily_yield"),
        SensorConfig::energy(state_topic, &device_config, "Efficiency", "efficiency")
    ]);

    // Sensors for each pv string
    for port in &hms_state.port_state {
        let idx = port.pv_port;
        sensors.extend([
            SensorConfig::power(state_topic, &device_config, &format!("PV {} Power", idx), &format!("pv_{}_power", idx)),
            SensorConfig::voltage(state_topic, &device_config, &format!("PV {} Voltage", idx), &format!("pv_{}_vol", idx)),
            SensorConfig::current(state_topic, &device_config, &format!("PV {} Current", idx), &format!("pv_{}_cur", idx)),
            SensorConfig::energy(state_topic, &device_config, &format!("PV {} Daily Yield", idx), &format!("pv_{}_daily_yield", idx)),
            SensorConfig::energy(state_topic, &device_config, &format!("PV {} Energy Total", idx), &format!("pv_{}_energy_total", idx)),

            SensorConfig::efficiency(state_topic, &device_config, &format!("Inverter {} Efficiency", idx), &format!("inv_{}_efficiency", idx)),
            SensorConfig::power(state_topic, &device_config, &format!("Inverter {} Power", idx), &format!("inv_{}_pv_current_power", idx)),
            SensorConfig::temperature(state_topic, &device_config, &format!("Inverter {} Temperature", idx), &format!("inv_{}_temperature", idx))
        ]);
    }
    sensors
}


impl Mqtt {
    pub fn new(
        host: &str,
        username: &Option<String>,
        password: &Option<String>,
        port: u16,
    ) -> Self {
        let this_host = hostname::get().unwrap().into_string().unwrap();
        let mut mqttoptions = MqttOptions::new(format!("hms-mqtt-publisher_{}", this_host), host, port);
        mqttoptions.set_keep_alive(Duration::from_secs(5));

        //parse the mqtt authentication options
        if let Some((username, password)) = match (username, password) {
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
            self.publish_json(&config_topic,  config_payload);
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

        let device_config = create_sensor_configs(hms_state, &state_topic);

        self.publish_configs(&config_topic, &device_config);
        self.publish_states(hms_state,  &state_topic);
    }
}
