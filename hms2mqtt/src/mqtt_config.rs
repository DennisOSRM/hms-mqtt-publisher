use serde_derive::Deserialize;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct MqttConfig {
    pub host: String,
    pub port: Option<u16>,
    pub username: Option<String>,
    pub password: Option<String>,
}

impl MqttConfig {
    pub fn is_valid(&self) -> bool {
        !self.host.is_empty()
    }
}

mod tests {
    #[test]
    fn valid_config() {
        use super::MqttConfig;

        let mqtt_config = MqttConfig {
            host: "lala".into(),
            port: None,
            username: None,
            password: None,
        };
        assert!(mqtt_config.is_valid());
    }

    #[test]
    fn invalid_config() {
        use super::MqttConfig;

        let mqtt_config = MqttConfig {
            host: "".into(),
            port: None,
            username: None,
            password: None,
        };
        assert!(!mqtt_config.is_valid());
    }

    #[test]
    fn default_invalid_config() {
        use super::MqttConfig;

        let mqtt_config = MqttConfig::default();
        assert!(!mqtt_config.is_valid());
    }
}
