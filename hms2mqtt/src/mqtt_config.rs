use serde_derive::Deserialize;

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct MqttConfig {
    pub host: String,
    pub port: Option<u16>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub tls: Option<bool>,
}
