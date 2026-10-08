use hms2mqtt::command::POWER_LIMIT_RANGE;
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
/// | `MQTT_CLIENT_ID`   | `client_id` of the MQTT outputs                  |
/// | `DEVICE_ID`        | `device_id`                                      |
/// | `PERFORMANCE_MODE` | `performance_mode` (`true` or `false`)           |
/// | `STARTUP_POWER_LIMIT` | `startup_power_limit` (percent, 2 to 100)     |
///
/// The `MQTT_*` variables apply to every MQTT output in `config.toml`. Without any MQTT output
/// in the file, they configure both the Home Assistant and the simple MQTT output. Empty
/// variables and empty credentials count as not set.
///
/// `device_id` names the inverter in MQTT topics and Home Assistant ids. Set different ids to
/// run one instance per inverter against the same broker. The MQTT client id defaults to one
/// derived from `device_id` or `inverter_host` (plus `-ha` / `-sm`), so that instances don't
/// disconnect each other. A `client_id` in `config.toml` is used as is; `MQTT_CLIENT_ID` too,
/// unless both outputs are enabled, then `-ha` and `-sm` are appended.
#[derive(Debug, Default, Deserialize, PartialEq)]
pub struct Config {
    #[serde(default)]
    pub inverter_host: String,
    pub update_interval: Option<u64>,
    pub device_id: Option<String>,
    /// Ask the DTU for its fast real-time data mode once at startup (command action 33)
    pub performance_mode: Option<bool>,
    /// Power limit in percent set once at startup
    pub startup_power_limit: Option<u32>,
    pub home_assistant: Option<MqttConfig>,
    pub simple_mqtt: Option<MqttConfig>,
}

const MQTT_VARIABLES: [&str; 6] = [
    "MQTT_BROKER_HOST",
    "MQTT_PORT",
    "MQTT_USERNAME",
    "MQTT_PASSWORD",
    "MQTT_TLS",
    "MQTT_CLIENT_ID",
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
        if let Some(mode) = var("PERFORMANCE_MODE") {
            info!("PERFORMANCE_MODE overrides performance_mode");
            config.performance_mode = Some(parse_bool("PERFORMANCE_MODE", &mode)?);
        }
        if let Some(limit) = var("STARTUP_POWER_LIMIT") {
            info!("STARTUP_POWER_LIMIT overrides startup_power_limit");
            config.startup_power_limit = Some(limit.trim().parse().map_err(|_| {
                format!("STARTUP_POWER_LIMIT must be a number of percent, got '{limit}'")
            })?);
        }
        if let Some(device_id) = var("DEVICE_ID") {
            info!("DEVICE_ID overrides device_id");
            for mqtt in [&mut config.home_assistant, &mut config.simple_mqtt]
                .into_iter()
                .flatten()
            {
                mqtt.device_id = Some(device_id.clone());
            }
            config.device_id = Some(device_id);
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
                .map(|tls| parse_bool("MQTT_TLS", &tls))
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
            if let Some(client_id) = var("MQTT_CLIENT_ID") {
                // used as is, unless both outputs connect: they need different client ids
                let both = config.home_assistant.is_some() && config.simple_mqtt.is_some();
                for (suffix, mqtt) in [
                    ("-ha", &mut config.home_assistant),
                    ("-sm", &mut config.simple_mqtt),
                ] {
                    if let Some(mqtt) = mqtt {
                        mqtt.client_id = Some(if both {
                            format!("{client_id}{suffix}")
                        } else {
                            client_id.clone()
                        });
                    }
                }
            }
            info!("MQTT_* environment variables override the MQTT settings");
        }

        config.device_id = config.device_id.take().filter(|id| !id.trim().is_empty());
        for (suffix, mqtt) in [
            ("-ha", &mut config.home_assistant),
            ("-sm", &mut config.simple_mqtt),
        ] {
            let Some(mqtt) = mqtt else { continue };
            // `username = ""` means no credentials, not an empty user name
            mqtt.username = mqtt.username.take().filter(|value| !value.is_empty());
            mqtt.password = mqtt.password.take().filter(|value| !value.is_empty());
            mqtt.client_id = mqtt.client_id.take().filter(|value| !value.is_empty());
            mqtt.device_id = mqtt
                .device_id
                .take()
                .filter(|id| !id.trim().is_empty())
                .or_else(|| config.device_id.clone());
            // derived from the output's effective device id, so each inverter gets its own
            if mqtt.client_id.is_none() {
                mqtt.client_id = Some(format!(
                    "hms-mqtt-publish-{}{suffix}",
                    client_id_part(mqtt.device_id.as_deref().unwrap_or(&config.inverter_host))
                ));
            }
        }

        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), String> {
        if let Some(limit) = self.startup_power_limit {
            if !POWER_LIMIT_RANGE.contains(&limit) {
                return Err(format!(
                    "startup_power_limit must be between {} and {} %, got {limit}",
                    POWER_LIMIT_RANGE.start(),
                    POWER_LIMIT_RANGE.end()
                ));
            }
        }
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
            if let Some(id) = mqtt.as_ref().and_then(|mqtt| mqtt.device_id.as_deref()) {
                if !is_valid_device_id(id) {
                    return Err(format!(
                        "invalid device_id '{id}' for [{name}]: use only letters, digits, '_' and '-'"
                    ));
                }
            }
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

fn parse_bool(name: &str, value: &str) -> Result<bool, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" => Ok(true),
        "false" | "0" | "no" => Ok(false),
        _ => Err(format!("{name} must be true or false, got '{value}'")),
    }
}

/// Characters allowed in device ids: they become MQTT topic levels and Home Assistant ids
fn is_valid_device_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Makes `value` usable in an MQTT client id. Characters that brokers may reject are replaced,
/// and a hash of the original value is appended in that case, so that e.g. `inverter.one` and
/// `inverter-one` still get different client ids.
fn client_id_part(value: &str) -> String {
    let safe: String = value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if safe == value {
        return safe;
    }
    // FNV-1a: stable across runs and platforms, unlike std's DefaultHasher
    let hash = value.bytes().fold(0x811c_9dc5_u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    });
    format!("{safe}-{hash:08x}")
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

    /// An output as loaded for inverter_host "hms" without further settings
    fn mqtt(host: &str) -> MqttConfig {
        MqttConfig {
            host: host.to_string(),
            client_id: Some("hms-mqtt-publish-hms-ha".to_string()),
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
            client_id: None,
            device_id: None,
            availability_topic: None,
        };
        assert_eq!(config.inverter_host, "192.168.4.182");
        assert_eq!(config.update_interval, Some(60_500));
        let with_client_id = |id: &str| MqttConfig {
            client_id: Some(id.to_string()),
            ..expected.clone()
        };
        assert_eq!(
            config.home_assistant,
            Some(with_client_id("hms-mqtt-publish-192-168-4-182-d5013b7d-ha"))
        );
        assert_eq!(
            config.simple_mqtt,
            Some(with_client_id("hms-mqtt-publish-192-168-4-182-d5013b7d-sm"))
        );
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
    fn default_client_ids_differ_per_inverter() {
        let client_id = |inverter: &str| {
            Config::load(
                None,
                env(&[("INVERTER_HOST", inverter), ("MQTT_BROKER_HOST", "broker")]),
            )
            .unwrap()
            .home_assistant
            .unwrap()
            .client_id
            .unwrap()
        };
        assert_eq!(
            client_id("192.168.4.182"),
            "hms-mqtt-publish-192-168-4-182-d5013b7d-ha"
        );
        assert_ne!(client_id("192.168.4.182"), client_id("192.168.4.183"));
        // replacing characters must not make different hosts equal
        assert_ne!(client_id("inverter.one"), client_id("inverter-one"));
        assert_eq!(
            client_id("inverter-one"),
            "hms-mqtt-publish-inverter-one-ha"
        );
    }

    #[test]
    fn device_id_applies_to_all_outputs_and_client_id() {
        let toml = "inverter_host = \"hms\"\ndevice_id = \"roof\"\n[home_assistant]\nhost = \"b\"\n[simple_mqtt]\nhost = \"b\"\n";
        let config = Config::load(Some(toml), env(&[])).unwrap();
        for mqtt in [config.home_assistant.unwrap(), config.simple_mqtt.unwrap()] {
            assert_eq!(mqtt.device_id.as_deref(), Some("roof"));
        }
        let config = Config::load(Some(toml), env(&[])).unwrap();
        assert_eq!(
            config.home_assistant.unwrap().client_id.as_deref(),
            Some("hms-mqtt-publish-roof-ha")
        );
        assert_eq!(
            config.simple_mqtt.unwrap().client_id.as_deref(),
            Some("hms-mqtt-publish-roof-sm")
        );

        let config = Config::load(
            Some(toml),
            env(&[("DEVICE_ID", "garage"), ("MQTT_CLIENT_ID", "custom")]),
        )
        .unwrap();
        let ha = config.home_assistant.unwrap();
        assert_eq!(ha.device_id.as_deref(), Some("garage"));
        // both outputs connect, so they need distinct client ids
        assert_eq!(ha.client_id.as_deref(), Some("custom-ha"));
        assert_eq!(
            config.simple_mqtt.unwrap().client_id.as_deref(),
            Some("custom-sm")
        );
    }

    #[test]
    fn configured_client_ids_are_used_as_is() {
        // MQTT_CLIENT_ID with a single output, e.g. for brokers with client id ACLs
        let toml = "inverter_host = \"hms\"\n[home_assistant]\nhost = \"b\"\n";
        let config = Config::load(Some(toml), env(&[("MQTT_CLIENT_ID", "allowed")])).unwrap();
        assert_eq!(
            config.home_assistant.unwrap().client_id.as_deref(),
            Some("allowed")
        );

        // client_id in config.toml, also with both outputs
        let toml = "inverter_host = \"hms\"\n[home_assistant]\nhost = \"b\"\nclient_id = \"one\"\n[simple_mqtt]\nhost = \"b\"\nclient_id = \"two\"\n";
        let config = Config::load(Some(toml), env(&[])).unwrap();
        assert_eq!(
            config.home_assistant.unwrap().client_id.as_deref(),
            Some("one")
        );
        assert_eq!(
            config.simple_mqtt.unwrap().client_id.as_deref(),
            Some("two")
        );
    }

    #[test]
    fn device_id_from_environment_overrides_per_output_values() {
        let toml =
            "inverter_host = \"hms\"\n[simple_mqtt]\nhost = \"b\"\ndevice_id = \"hms800wt2\"\n";
        let config = Config::load(Some(toml), env(&[("DEVICE_ID", "garage")])).unwrap();
        assert_eq!(
            config.simple_mqtt.unwrap().device_id.as_deref(),
            Some("garage")
        );
    }

    #[test]
    fn device_ids_must_be_usable_in_topics() {
        for id in ["a/b", "a+b", "a#b", "a b", "a\0b", "dach.süd"] {
            let err = Config::load(
                None,
                env(&[
                    ("INVERTER_HOST", "hms"),
                    ("MQTT_BROKER_HOST", "broker"),
                    ("DEVICE_ID", id),
                ]),
            )
            .unwrap_err();
            assert!(err.contains("invalid device_id"), "{id}: {err}");
        }
        let toml = "inverter_host = \"hms\"\n[simple_mqtt]\nhost = \"b\"\ndevice_id = \"x/y\"\n";
        assert!(Config::load(Some(toml), env(&[]))
            .unwrap_err()
            .contains("[simple_mqtt]"));
        assert!(Config::load(
            None,
            env(&[
                ("INVERTER_HOST", "hms"),
                ("MQTT_BROKER_HOST", "broker"),
                ("DEVICE_ID", "Roof_2-a"),
            ]),
        )
        .is_ok());
    }

    #[test]
    fn client_id_follows_the_device_id_of_its_output() {
        let toml = "inverter_host = \"hms\"\n[simple_mqtt]\nhost = \"b\"\ndevice_id = \"roof\"\n";
        let sm = Config::load(Some(toml), env(&[]))
            .unwrap()
            .simple_mqtt
            .unwrap();
        assert_eq!(sm.client_id.as_deref(), Some("hms-mqtt-publish-roof-sm"));
    }

    #[test]
    fn device_id_per_output_takes_precedence() {
        let toml = "inverter_host = \"hms\"\ndevice_id = \"roof\"\n[simple_mqtt]\nhost = \"b\"\ndevice_id = \"hms800wt2\"\nclient_id = \"old-client\"\n";
        let sm = Config::load(Some(toml), env(&[]))
            .unwrap()
            .simple_mqtt
            .unwrap();
        assert_eq!(sm.device_id.as_deref(), Some("hms800wt2"));
        assert_eq!(sm.client_id.as_deref(), Some("old-client"));
    }

    #[test]
    fn startup_options_from_toml_and_environment() {
        let base = [("INVERTER_HOST", "hms"), ("MQTT_BROKER_HOST", "broker")];
        let config = Config::load(None, env(&base)).unwrap();
        assert_eq!(
            (config.performance_mode, config.startup_power_limit),
            (None, None)
        );

        let toml = "inverter_host = \"hms\"\nperformance_mode = true\nstartup_power_limit = 80\n[simple_mqtt]\nhost = \"b\"\n";
        let config = Config::load(Some(toml), env(&[])).unwrap();
        assert_eq!(config.performance_mode, Some(true));
        assert_eq!(config.startup_power_limit, Some(80));

        let config = Config::load(
            Some(toml),
            env(&[("PERFORMANCE_MODE", "false"), ("STARTUP_POWER_LIMIT", "55")]),
        )
        .unwrap();
        assert_eq!(config.performance_mode, Some(false));
        assert_eq!(config.startup_power_limit, Some(55));
    }

    #[test]
    fn invalid_startup_options_are_reported() {
        let base = [("INVERTER_HOST", "hms"), ("MQTT_BROKER_HOST", "broker")];
        for (name, value) in [
            ("PERFORMANCE_MODE", "sometimes"),
            ("STARTUP_POWER_LIMIT", "1"),
            ("STARTUP_POWER_LIMIT", "101"),
            ("STARTUP_POWER_LIMIT", "half"),
        ] {
            let mut vars = base.to_vec();
            vars.push((name, value));
            let err = Config::load(None, env(&vars)).unwrap_err();
            assert!(err.to_lowercase().contains(&name.to_lowercase()), "{err}");
        }
    }

    #[test]
    fn repo_config_example_parses() {
        let contents = include_str!("../../../config.toml");
        let config = Config::load(Some(contents), env(&[])).unwrap();
        assert!(!config.inverter_host.is_empty());
    }
}
