use crate::mqtt_config::MqttConfig;

/// Payload of the availability topic while the publisher is connected
pub const ONLINE: &str = "online";
/// Payload of the availability topic once the publisher is gone, sent as its last will
pub const OFFLINE: &str = "offline";

#[derive(Clone, Copy)]
pub enum QoS {
    AtMostOnce,
    AtLeastOnce,
    ExactlyOnce,
}

// TODO: add an implementation of the MqttWrapper for testing
// TODO: should this be renamed to MqttImplementation?
pub trait MqttWrapper {
    // This trait provides an interface that the decouples library code from an
    // implementation of the MQTT client. On library calling code, one needs to
    // wrap the MQTT implementation, i.e. the client, in a new type that in
    // turn implements this trait.

    fn subscribe(&mut self, topic: &str, qos: QoS) -> anyhow::Result<()>;

    fn publish<S, V>(&mut self, topic: S, qos: QoS, retain: bool, payload: V) -> anyhow::Result<()>
    where
        S: Clone + Into<String>,
        V: Clone + Into<Vec<u8>>;

    fn new(config: &MqttConfig, suffix: &str) -> Self;

    /// Messages received on subscribed topics since the last call, as (topic, payload)
    fn receive(&mut self) -> Vec<(String, Vec<u8>)> {
        Vec::new()
    }
}
