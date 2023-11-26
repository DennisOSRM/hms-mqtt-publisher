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
    sw_version: String, // Software version of the application that supplies the discovered MQTT item.
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
    pub unique_id: String,  //  A globally unique identifier for the sensor.
    name: String,           // The name of the sensor.
    state_topic: String,    // The MQTT topic where sensor readings will be published.
    value_template: String, // A template to extract a value from the mqtt message.
    device: DeviceConfig, // The device that the sensor belongs to, used to group entities together.
    // exclude optional if they are not provided
    #[serde(skip_serializing_if = "Option::is_none")]
    unit_of_measurement: Option<String>, // The unit of measurement of the sensor.
    #[serde(skip_serializing_if = "Option::is_none")]
    device_class: Option<String>, // The type/class of the sensor, e.g. energy, power, temperature, etc.
    #[serde(skip_serializing_if = "Option::is_none")]
    state_class: Option<String>, // The type/class of the state, e.g. measurement, total_increasing, etc.
}

impl SensorConfig {
    pub fn new_sensor(
        state_topic: &str,
        device_config: &DeviceConfig,
        unique_id: &str,
        name: &str,
        device_class: Option<String>,
        unit_of_measurement: Option<String>,
        state_class: Option<String>,
    ) -> Self {
        let value_template = format!("{{{{ value_json.{} }}}}", unique_id);
        let unique_id = format!("{}_{}", device_config.identifiers[0], unique_id);
        SensorConfig {
            unique_id,
            name: name.to_string(),
            state_topic: state_topic.to_string(),
            unit_of_measurement,
            device_class,
            value_template,
            device: device_config.clone(),
            state_class,
        }
    }

    pub fn string(state_topic: &str, device_config: &DeviceConfig, name: &str, key: &str) -> Self {
        Self::new_sensor(state_topic, device_config, key, name, None, None, None)
    }

    pub fn power(state_topic: &str, device_config: &DeviceConfig, name: &str, key: &str) -> Self {
        Self::new_sensor(
            state_topic,
            device_config,
            key,
            name,
            Some("power".to_string()),
            Some("W".to_string()),
            Some("measurement".to_string()),
        )
    }

    pub fn energy(state_topic: &str, device_config: &DeviceConfig, name: &str, key: &str) -> Self {
        Self::new_sensor(
            state_topic,
            device_config,
            key,
            name,
            Some("energy".to_string()),
            Some("Wh".to_string()),
            Some("total_increasing".to_string()),
        )
    }

    pub fn voltage(state_topic: &str, device_config: &DeviceConfig, name: &str, key: &str) -> Self {
        Self::new_sensor(
            state_topic,
            device_config,
            key,
            name,
            Some("voltage".to_string()),
            Some("V".to_string()),
            Some("measurement".to_string()),
        )
    }

    pub fn current(state_topic: &str, device_config: &DeviceConfig, name: &str, key: &str) -> Self {
        Self::new_sensor(
            state_topic,
            device_config,
            key,
            name,
            Some("current".to_string()),
            Some("A".to_string()),
            Some("measurement".to_string()),
        )
    }

    pub fn temperature(
        state_topic: &str,
        device_config: &DeviceConfig,
        name: &str,
        key: &str,
    ) -> Self {
        Self::new_sensor(
            state_topic,
            device_config,
            key,
            name,
            Some("temperature".to_string()),
            Some("°C".to_string()),
            Some("measurement".to_string()),
        )
    }

    pub fn efficiency(
        state_topic: &str,
        device_config: &DeviceConfig,
        name: &str,
        key: &str,
    ) -> Self {
        Self::new_sensor(
            state_topic,
            device_config,
            key,
            name,
            None,
            Some("%".to_string()),
            Some("measurement".to_string()),
        )
    }

    pub fn frequency(
        state_topic: &str,
        device_config: &DeviceConfig,
        name: &str,
        key: &str,
    ) -> Self {
        Self::new_sensor(
            state_topic,
            device_config,
            key,
            name,
            Some("frequency".to_string()),
            Some("Hz".to_string()),
            Some("measurement".to_string()),
        )
    }
}
