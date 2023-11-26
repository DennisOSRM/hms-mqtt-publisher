use std::time::Duration;

use hms_mqtt_publish::{
    mqtt_config::MqttConfig,
    mqtt_wrapper::{self},
};
use rumqttc::{Client, Connection, MqttOptions};

pub struct RumqttcWrapper {
    client: Client,
    // TODO: check if connection is needed by EspMqtt implementation
    //       if no, check if it can be removed from here
    //       I.e. does this run without an ever-spinning thread to poll the event loop?
    //       If the connection can't be dropped then implement the following
    //       1. spawn background thread
    //       2. add a channel to the background thread
    //       3. implement Drop trait and shutdown bg thread
    connection: Connection,
}

fn match_qos(qos: mqtt_wrapper::QoS) -> rumqttc::QoS {
    match qos {
        mqtt_wrapper::QoS::AtMostOnce => rumqttc::QoS::AtMostOnce,
        mqtt_wrapper::QoS::AtLeastOnce => rumqttc::QoS::AtLeastOnce,
        mqtt_wrapper::QoS::ExactlyOnce => rumqttc::QoS::ExactlyOnce,
    }
}

impl mqtt_wrapper::MqttWrapper for RumqttcWrapper {
    fn subscribe(
        &mut self,
        topic: &str,
        qos: mqtt_wrapper::QoS,
    ) -> Result<(), mqtt_wrapper::ClientError> {
        if let Ok(result) = self.client.subscribe(topic, match_qos(qos)) {
            return Ok(result);
        }
        // TODO: log or convert the error
        Err(mqtt_wrapper::ClientError)
    }

    fn publish<S, V>(
        &mut self,
        topic: S,
        qos: mqtt_wrapper::QoS,
        retain: bool,
        payload: V,
    ) -> Result<(), mqtt_wrapper::ClientError>
    where
        S: Into<String>,
        V: Into<Vec<u8>>,
    {
        if let Ok(result) = self.client.publish(topic, match_qos(qos), retain, payload) {
            return Ok(result);
        }
        // TODO: log or convert the error
        Err(mqtt_wrapper::ClientError)
    }

    fn new(config: &MqttConfig) -> Self {
        let mut mqttoptions = MqttOptions::new(
            "hms800wt2-mqtt-publisher",
            &config.host,
            config.port.unwrap_or(1883),
        );
        mqttoptions.set_keep_alive(Duration::from_secs(5));

        //parse the mqtt authentication options
        if let Some((username, password)) = match (&config.username, &config.password) {
            (None, None) => None,
            (None, Some(_)) => None,
            (Some(username), None) => Some((username.clone(), "".into())),
            (Some(username), Some(password)) => Some((username.clone(), password.clone())),
        } {
            mqttoptions.set_credentials(username, password);
        }

        let (client, connection) = Client::new(mqttoptions, 10);
        Self { client, connection }
    }
}
