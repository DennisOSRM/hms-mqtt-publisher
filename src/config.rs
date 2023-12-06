use std::{env, fs};

use hms2mqtt::mqtt_config::MqttConfig;
use log::warn;
use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Config {
    pub inverter_host: String,
    pub home_assistant: Option<MqttConfig>,
    pub simple_mqtt: Option<MqttConfig>,
}

impl Config {
    pub fn is_valid(&self) -> bool {
        !self.inverter_host.is_empty()
            && (self.home_assistant.as_ref().is_some_and(|x| x.is_valid())
                || self.simple_mqtt.as_ref().is_some_and(|x| x.is_valid()))
    }

    pub fn load() -> Config {
        // parse config from TOML file if present
        let filename = "config.toml";
        let contents = match fs::read_to_string(filename) {
            Ok(contents) => contents,
            Err(e) => {
                warn!("Could not read config.toml: {e}");
                "".into()
            }
        };
        let mut config = match toml::from_str::<Config>(&contents) {
            Ok(config) => config,
            Err(e) => {
                warn!("toml config unparsable: {e}");
                Config::default()
            }
        };

        // overwrite config if environment variables are set
        // $INVERTER_HOST
        if let Ok(inverter_host) = env::var("INVERTER_HOST") {
            config.inverter_host = inverter_host;
        }
        // $MQTT_BROKER_HOST
        let mut mqtt_config_overwritten = false;
        if let Ok(host) = env::var("MQTT_BROKER_HOST") {
            if config.home_assistant.is_none() {
                config.home_assistant = Some(MqttConfig::default())
            }
            config
                .home_assistant
                .get_or_insert(MqttConfig::default())
                .host = host;
            mqtt_config_overwritten = true;
        }
        // $MQTT_USERNAME (optional)
        if let Ok(username) = env::var("MQTT_USERNAME") {
            config
                .home_assistant
                .get_or_insert(MqttConfig::default())
                .username = Some(username);
            mqtt_config_overwritten = true;
        }
        // $MQTT_PASSWORD (optional)
        if let Ok(password) = env::var("MQTT_PASSWORD") {
            config
                .home_assistant
                .get_or_insert(MqttConfig::default())
                .password = Some(password);
            mqtt_config_overwritten = true;
        }
        // $MQTT_PORT (optional)
        if let Ok(port) = env::var("MQTT_PORT") {
            config
                .home_assistant
                .get_or_insert(MqttConfig::default())
                .port = Some(port.parse().unwrap_or(1883));
            mqtt_config_overwritten = true;
        }
        if mqtt_config_overwritten {
            config.simple_mqtt = config.home_assistant.clone();
        }
        config
    }
}
