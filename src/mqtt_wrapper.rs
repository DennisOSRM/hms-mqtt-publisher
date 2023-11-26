use crate::mqtt_config::MqttConfig;

pub enum QoS {
    AtMostOnce,
    AtLeastOnce,
    ExactlyOnce,
}

#[derive(Debug)]
pub struct ClientError; // dummy error message

// TODO: add an implementation of the MqttWrapper for testing
// TODO: should this be renamed to MqttImplementation?
pub trait MqttWrapper {
    // This trait provides an interface that the decouples library code from an
    // implementation of the MQTT client. On library calling code, one needs to
    // wrap the MQTT implementation, i.e. the client, in a new type that in
    // turn implements this trait.

    fn subscribe(&mut self, topic: &str, qos: QoS) -> Result<(), ClientError>;

    fn publish<S, V>(
        &mut self,
        topic: S,
        qos: QoS,
        retain: bool,
        payload: V,
    ) -> Result<(), ClientError>
    where
        S: Into<String>,
        V: Into<Vec<u8>>;

    fn new(config: &MqttConfig) -> Self;
}
