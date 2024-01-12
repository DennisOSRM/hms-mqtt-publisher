use std::{thread, time::Duration};

use hms2mqtt::{
    mqtt_config::MqttConfig,
    mqtt_wrapper::{self},
};
use log::warn;
use rumqttc::{Client, MqttOptions, QoS::AtMostOnce};

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
        S: Clone + Into<String>,
        V: Clone + Into<Vec<u8>>,
    {
        // try publishing up to three times
        if self
            .client
            .try_publish(topic.clone(), match_qos(qos), retain, payload.clone())
            .is_ok()
        {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
        if self
            .client
            .try_publish(topic.clone(), match_qos(qos), retain, payload.clone())
            .is_ok()
        {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
        Ok(self
            .client
            .try_publish(topic, match_qos(qos), retain, payload)?)
    }

    fn new(config: &MqttConfig, suffix: &str) -> Self {
        let mut mqttoptions = MqttOptions::new(
            "hms800wt2-mqtt-publisher".to_string() + suffix,
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

        let (mut client, mut connection) = Client::new(mqttoptions, 512);

        thread::spawn(move || {
            // keep polling the event loop to make sure outgoing messages get sent
            // the call to .iter() blocks and suspends the thread effectively by
            // calling .recv() under the hood. This implies that the loop terminates
            // once the client unsubs
            for _ in connection.iter() {}
        });
        if let Err(e) = client.subscribe("hms800wt2", AtMostOnce) {
            warn!("subscription to base topic failed: {e}");
        }
        Self { client }
    }
}
