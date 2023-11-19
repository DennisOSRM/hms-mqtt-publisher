
use serde::Serialize;


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
            // Rust compiler sets the CARGO_PKG_VERSION environment from the Cargo.toml . 
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
        let mut _device_class = None;
        if device_class != "" {
            _device_class = Some(device_class.to_string());
        }
        SensorConfig {
            unique_id: unique_id.to_string(), 
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
