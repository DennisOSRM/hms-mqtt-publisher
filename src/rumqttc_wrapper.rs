use std::{thread, time::Duration};

use hms_mqtt_publish::{
    mqtt_config::MqttConfig,
    mqtt_wrapper::{self},
};
use rumqttc::{Client, MqttOptions};

pub struct RumqttcWrapper {
    client: Client,
}

fn match_qos(qos: mqtt_wrapper::QoS) -> rumqttc::QoS {
    match qos {
        mqtt_wrapper::QoS::AtMostOnce => rumqttc::QoS::AtMostOnce,
        mqtt_wrapper::QoS::AtLeastOnce => rumqttc::QoS::AtLeastOnce,
        mqtt_wrapper::QoS::ExactlyOnce => rumqttc::QoS::ExactlyOnce,
    }
}

impl mqtt_wrapper::MqttWrapper for RumqttcWrapper {
    fn subscribe(&mut self, topic: &str, qos: mqtt_wrapper::QoS) -> anyhow::Result<()> {
        Ok(self.client.subscribe(topic, match_qos(qos))?)
    }

    fn publish<S, V>(
        &mut self,
        topic: S,
        qos: mqtt_wrapper::QoS,
        retain: bool,
        payload: V,
    ) -> anyhow::Result<()>
    where
        S: Into<String>,
        V: Into<Vec<u8>>,
    {
        Ok(self
            .client
            .try_publish(topic, match_qos(qos), retain, payload)?)
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

        let (client, mut connection) = Client::new(mqttoptions, 10);

        thread::spawn(move || {
            // keep polling the event loop to make sure outgoing messages get sent
            // the call to .iter() blocks and suspends the thread effectively by
            // calling .recv() under the hood. This implies that the loop terminates
            // once the client unsubs
            for _ in connection.iter() {}
        });

        Self { client }
    }
}
