use serde_derive::Deserialize;

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct MqttConfig {
    pub host: String,
    pub port: Option<u16>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub tls: Option<bool>,
    /// MQTT client id; must be unique per broker connection
    pub client_id: Option<String>,
    /// Name of the inverter in topics and Home Assistant ids; defaults to the first 8 characters
    /// of the DTU serial number (Home Assistant) and "hms800wt2" (simple MQTT)
    pub device_id: Option<String>,
}
