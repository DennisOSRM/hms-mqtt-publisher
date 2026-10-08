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
/// https://www.home-assistant.io/integrations/mqtt/#mqtt-discovery
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
    #[serde(skip_serializing_if = "Option::is_none")]
    entity_category: Option<String>, // e.g. diagnostic
    #[serde(skip_serializing_if = "Option::is_none")]
    availability_topic: Option<String>, // "online" / "offline", unavailable when offline
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
            entity_category: None,
            availability_topic: None,
        }
    }

    /// Shows the sensor as unavailable while `topic` reads "offline"
    pub fn with_availability(mut self, topic: Option<&str>) -> Self {
        self.availability_topic = topic.map(str::to_string);
        self
    }

    fn diagnostic(mut self) -> Self {
        self.entity_category = Some("diagnostic".to_string());
        self
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

    pub fn reactive_power(
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
            Some("reactive_power".to_string()),
            Some("var".to_string()),
            Some("measurement".to_string()),
        )
    }

    pub fn power_factor(
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
            Some("power_factor".to_string()),
            None,
            Some("measurement".to_string()),
        )
    }

    pub fn percentage(
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

    pub fn diagnostic_value(
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
            None,
            Some("measurement".to_string()),
        )
        .diagnostic()
    }
}

/// Home Assistant MQTT number entity, used for settings that can be changed
///
/// https://www.home-assistant.io/integrations/number.mqtt/
#[derive(Serialize)]
pub struct NumberConfig {
    pub unique_id: String,
    name: String,
    command_topic: String,
    state_topic: String,
    value_template: String,
    device: DeviceConfig,
    min: u32,
    max: u32,
    step: u32,
    unit_of_measurement: String,
    mode: String,
    entity_category: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    availability_topic: Option<String>,
}

impl NumberConfig {
    /// Power limit in percent; `key` is the state payload field with the current limit
    pub fn power_limit(
        state_topic: &str,
        command_topic: &str,
        device_config: &DeviceConfig,
        key: &str,
        range: std::ops::RangeInclusive<u32>,
    ) -> Self {
        NumberConfig {
            unique_id: format!("{}_power_limit_set", device_config.identifiers[0]),
            name: "Power Limit".to_string(),
            command_topic: command_topic.to_string(),
            state_topic: state_topic.to_string(),
            // the limit is only in the payload once one has been set
            value_template: format!(
                "{{% if value_json.{key} is defined %}}{{{{ value_json.{key} }}}}{{% endif %}}"
            ),
            device: device_config.clone(),
            min: *range.start(),
            max: *range.end(),
            step: 1,
            unit_of_measurement: "%".to_string(),
            mode: "box".to_string(),
            entity_category: "config".to_string(),
            availability_topic: None,
        }
    }

    /// Shows the number as unavailable while `topic` reads "offline"
    pub fn with_availability(mut self, topic: Option<&str>) -> Self {
        self.availability_topic = topic.map(str::to_string);
        self
    }
}
