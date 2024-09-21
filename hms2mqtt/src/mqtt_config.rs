use serde_derive::Deserialize;

#[derive(Debug, Deserialize)]
pub struct MqttConfig {
    pub host: String,
    pub port: Option<u16>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub client_id: Option<String>,
    pub tls: Option<bool>,
}
