use hms2mqtt::mqtt_config::MqttConfig;
use log::info;
use serde_derive::Deserialize;

/// Configuration from `config.toml`, overridden by environment variables.
///
/// | Variable           | Setting                                          |
/// |--------------------|--------------------------------------------------|
/// | `INVERTER_HOST`    | `inverter_host`                                  |
/// | `UPDATE_INTERVAL`  | `update_interval` (milliseconds)                 |
/// | `MQTT_BROKER_HOST` | `host` of the MQTT outputs                       |
/// | `MQTT_PORT`        | `port` of the MQTT outputs                       |
/// | `MQTT_USERNAME`    | `username` of the MQTT outputs                   |
/// | `MQTT_PASSWORD`    | `password` of the MQTT outputs                   |
/// | `MQTT_TLS`         | `tls` of the MQTT outputs (`true` or `false`)    |
///
/// The `MQTT_*` variables apply to every MQTT output in `config.toml`. Without any MQTT output
/// in the file, they configure both the Home Assistant and the simple MQTT output. Empty
/// variables and empty credentials count as not set.
#[derive(Debug, Default, Deserialize, PartialEq)]
pub struct Config {
    #[serde(default)]
    pub inverter_host: String,
    pub update_interval: Option<u64>,
    pub home_assistant: Option<MqttConfig>,
    pub simple_mqtt: Option<MqttConfig>,
}

const MQTT_VARIABLES: [&str; 5] = [
    "MQTT_BROKER_HOST",
    "MQTT_PORT",
    "MQTT_USERNAME",
    "MQTT_PASSWORD",
    "MQTT_TLS",
];

impl Config {
    /// Builds the configuration from the contents of `config.toml` (if there is one) and the
    /// environment, given as a lookup function so that tests don't depend on the process
    /// environment.
    pub fn load(toml: Option<&str>, env: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let mut config: Config = match toml {
            Some(contents) => {
                toml::from_str(contents).map_err(|e| format!("config.toml is invalid: {e}"))?
            }
            None => Config::default(),
        };

        // empty variables are treated as not set, as container setups often pass them empty
        let var = |name: &str| env(name).filter(|value| !value.trim().is_empty());

        if let Some(host) = var("INVERTER_HOST") {
            info!("INVERTER_HOST overrides inverter_host");
            config.inverter_host = host;
        }
        if let Some(interval) = var("UPDATE_INTERVAL") {
            info!("UPDATE_INTERVAL overrides update_interval");
            config.update_interval = Some(interval.trim().parse().map_err(|_| {
                format!("UPDATE_INTERVAL must be a number of milliseconds, got '{interval}'")
            })?);
        }

        if MQTT_VARIABLES.iter().any(|name| var(name).is_some()) {
            let port = var("MQTT_PORT")
                .map(|port| {
                    port.trim()
                        .parse::<u16>()
                        .map_err(|_| format!("MQTT_PORT must be a port number, got '{port}'"))
                })
                .transpose()?;
            let tls = var("MQTT_TLS")
                .map(|tls| match tls.trim().to_ascii_lowercase().as_str() {
                    "true" | "1" | "yes" => Ok(true),
                    "false" | "0" | "no" => Ok(false),
                    _ => Err(format!("MQTT_TLS must be true or false, got '{tls}'")),
                })
                .transpose()?;

            if config.home_assistant.is_none() && config.simple_mqtt.is_none() {
                config.home_assistant = Some(MqttConfig::default());
                config.simple_mqtt = Some(MqttConfig::default());
            }
            for mqtt in [&mut config.home_assistant, &mut config.simple_mqtt]
                .into_iter()
                .flatten()
            {
                if let Some(host) = var("MQTT_BROKER_HOST") {
                    mqtt.host = host;
                }
                if port.is_some() {
                    mqtt.port = port;
                }
                if let Some(username) = var("MQTT_USERNAME") {
                    mqtt.username = Some(username);
                }
                if let Some(password) = var("MQTT_PASSWORD") {
                    mqtt.password = Some(password);
                }
                if tls.is_some() {
                    mqtt.tls = tls;
                }
            }
            info!("MQTT_* environment variables override the MQTT settings");
        }

        // `username = ""` means no credentials, not an empty user name
        for mqtt in [&mut config.home_assistant, &mut config.simple_mqtt]
            .into_iter()
            .flatten()
        {
            mqtt.username = mqtt.username.take().filter(|value| !value.is_empty());
            mqtt.password = mqtt.password.take().filter(|value| !value.is_empty());
        }

        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), String> {
        if self.inverter_host.trim().is_empty() {
            return Err(
                "no inverter configured: set inverter_host in config.toml or INVERTER_HOST"
                    .to_string(),
            );
        }
        if self.home_assistant.is_none() && self.simple_mqtt.is_none() {
            return Err(
                "no output configured: add [home_assistant] or [simple_mqtt] to config.toml, \
                 or set MQTT_BROKER_HOST"
                    .to_string(),
            );
        }
        for (name, mqtt) in [
            ("home_assistant", &self.home_assistant),
            ("simple_mqtt", &self.simple_mqtt),
        ] {
            if mqtt
                .as_ref()
                .is_some_and(|mqtt| mqtt.host.trim().is_empty())
            {
                return Err(format!(
                    "the MQTT broker of [{name}] has no host: set host in config.toml or MQTT_BROKER_HOST"
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let vars: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |name| vars.get(name).cloned()
    }

    fn mqtt(host: &str) -> MqttConfig {
        MqttConfig {
            host: host.to_string(),
            ..MqttConfig::default()
        }
    }

    const FULL_TOML: &str = r#"
        inverter_host = "hms.local"
        update_interval = 60000

        [home_assistant]
        host = "broker"
        port = 1883

        [simple_mqtt]
        host = "broker2"
        username = "user"
        password = "secret"
        tls = true
    "#;

    #[test]
    fn toml_only() {
        let config = Config::load(Some(FULL_TOML), env(&[])).unwrap();
        assert_eq!(config.inverter_host, "hms.local");
        assert_eq!(config.update_interval, Some(60_000));
        let ha = config.home_assistant.unwrap();
        assert_eq!((ha.host.as_str(), ha.port), ("broker", Some(1883)));
        let sm = config.simple_mqtt.unwrap();
        assert_eq!(sm.username.as_deref(), Some("user"));
        assert_eq!(sm.tls, Some(true));
    }

    #[test]
    fn environment_only_configures_both_outputs() {
        let config = Config::load(
            None,
            env(&[
                ("INVERTER_HOST", "192.168.4.182"),
                ("UPDATE_INTERVAL", "60500"),
                ("MQTT_BROKER_HOST", "broker"),
                ("MQTT_PORT", "8883"),
                ("MQTT_USERNAME", "user"),
                ("MQTT_PASSWORD", "pass"),
                ("MQTT_TLS", "true"),
            ]),
        )
        .unwrap();
        let expected = MqttConfig {
            host: "broker".to_string(),
            port: Some(8883),
            username: Some("user".to_string()),
            password: Some("pass".to_string()),
            tls: Some(true),
        };
        assert_eq!(config.inverter_host, "192.168.4.182");
        assert_eq!(config.update_interval, Some(60_500));
        assert_eq!(config.home_assistant, Some(expected.clone()));
        assert_eq!(config.simple_mqtt, Some(expected));
    }

    #[test]
    fn environment_overrides_toml_per_field() {
        let config = Config::load(
            Some(FULL_TOML),
            env(&[("INVERTER_HOST", "10.0.0.2"), ("MQTT_PASSWORD", "new")]),
        )
        .unwrap();
        assert_eq!(config.inverter_host, "10.0.0.2");
        assert_eq!(config.update_interval, Some(60_000));
        let ha = config.home_assistant.unwrap();
        assert_eq!(ha.host, "broker");
        assert_eq!(ha.password.as_deref(), Some("new"));
        let sm = config.simple_mqtt.unwrap();
        assert_eq!(sm.host, "broker2");
        assert_eq!(sm.username.as_deref(), Some("user"));
        assert_eq!(sm.password.as_deref(), Some("new"));
    }

    #[test]
    fn mqtt_variables_only_touch_configured_outputs() {
        let toml = "inverter_host = \"hms\"\n[simple_mqtt]\nhost = \"broker\"\n";
        let config = Config::load(Some(toml), env(&[("MQTT_PORT", "1884")])).unwrap();
        assert!(config.home_assistant.is_none());
        assert_eq!(config.simple_mqtt.unwrap().port, Some(1884));
    }

    #[test]
    fn empty_variables_and_credentials_count_as_unset() {
        let config = Config::load(
            None,
            env(&[
                ("INVERTER_HOST", "hms"),
                ("MQTT_BROKER_HOST", "broker"),
                ("MQTT_PORT", ""),
                ("MQTT_USERNAME", ""),
                ("MQTT_PASSWORD", ""),
                ("UPDATE_INTERVAL", ""),
            ]),
        )
        .unwrap();
        assert_eq!(config.update_interval, None);
        assert_eq!(config.home_assistant, Some(mqtt("broker")));

        // config files written by older container scripts contain empty credentials
        let toml = "inverter_host = \"hms\"\n[home_assistant]\nhost = \"b\"\nusername = \"\"\npassword = \"\"\n";
        let config = Config::load(Some(toml), env(&[])).unwrap();
        assert_eq!(config.home_assistant, Some(mqtt("b")));
    }

    #[test]
    fn invalid_values_are_reported() {
        let base = [("INVERTER_HOST", "hms"), ("MQTT_BROKER_HOST", "broker")];
        for (name, value) in [
            ("MQTT_PORT", "70000"),
            ("MQTT_PORT", "abc"),
            ("UPDATE_INTERVAL", "1m"),
            ("MQTT_TLS", "maybe"),
        ] {
            let mut vars = base.to_vec();
            vars.push((name, value));
            let err = Config::load(None, env(&vars)).unwrap_err();
            assert!(err.contains(name), "{err}");
        }
        assert!(Config::load(Some("inverter_host = "), env(&[]))
            .unwrap_err()
            .contains("config.toml is invalid"));
    }

    #[test]
    fn missing_settings_are_reported() {
        let err = Config::load(None, env(&[])).unwrap_err();
        assert!(err.contains("INVERTER_HOST"), "{err}");

        let err = Config::load(None, env(&[("INVERTER_HOST", "hms")])).unwrap_err();
        assert!(err.contains("no output configured"), "{err}");

        let err = Config::load(
            None,
            env(&[("INVERTER_HOST", "hms"), ("MQTT_PORT", "1883")]),
        )
        .unwrap_err();
        assert!(err.contains("has no host"), "{err}");
    }

    #[test]
    fn repo_config_example_parses() {
        let contents = include_str!("../../../config.toml");
        let config = Config::load(Some(contents), env(&[])).unwrap();
        assert!(!config.inverter_host.is_empty());
    }
}
