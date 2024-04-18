use std::{
    sync::{mpsc, Arc, Mutex},
    thread,
    time::Duration,
};

use hms2mqtt::{
    mqtt_config::MqttConfig,
    mqtt_wrapper::{self},
};
use log::warn;
use rumqttc::{
    tokio_rustls::{self, rustls::ClientConfig},
    Client, Event, MqttOptions, Packet, Transport,
};

pub struct RumqttcWrapper {
    client: Client,
    /// messages on subscribed topics, forwarded by the event loop thread
    incoming: mpsc::Receiver<(String, Vec<u8>)>,
    /// renewed after every (re)connect, as the broker may not keep them
    subscriptions: Arc<Mutex<Vec<(String, rumqttc::QoS)>>>,
}

fn match_qos(qos: mqtt_wrapper::QoS) -> rumqttc::QoS {
    match qos {
        mqtt_wrapper::QoS::AtMostOnce => rumqttc::QoS::AtMostOnce,
        mqtt_wrapper::QoS::AtLeastOnce => rumqttc::QoS::AtLeastOnce,
        mqtt_wrapper::QoS::ExactlyOnce => rumqttc::QoS::ExactlyOnce,
    }
}

/// The configured port, or the standard MQTT port for plain (1883) or TLS (8883) connections.
fn broker_port(config: &MqttConfig) -> u16 {
    config.port.unwrap_or(if config.tls.is_some_and(|tls| tls) {
        8883
    } else {
        1883
    })
}

/// TLS settings that trust the root certificates of the operating system.
fn tls_client_config() -> ClientConfig {
    // Use rustls-native-certs to load root certificates from the operating system.
    let mut roots = tokio_rustls::rustls::RootCertStore::empty();
    rustls_native_certs::load_native_certs()
        .expect("could not load platform certs")
        .into_iter()
        .for_each(|cert| {
            roots.add(cert).unwrap();
        });

    ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth()
}

impl mqtt_wrapper::MqttWrapper for RumqttcWrapper {
    fn subscribe(&mut self, topic: &str, qos: mqtt_wrapper::QoS) -> anyhow::Result<()> {
        self.subscriptions
            .lock()
            .unwrap()
            .push((topic.to_string(), match_qos(qos)));
        Ok(self.client.subscribe(topic, match_qos(qos))?)
    }

    fn receive(&mut self) -> Vec<(String, Vec<u8>)> {
        self.incoming.try_iter().collect()
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
        let use_tls = config.tls.is_some_and(|tls| tls);

        // configured client ids are final; the suffix only keeps the fallback distinct
        let client_id = config
            .client_id
            .clone()
            .unwrap_or_else(|| format!("hms-mqtt-publish{suffix}"));
        let status_topic = format!("{client_id}/status");
        let mut mqttoptions = MqttOptions::new(
            client_id,
            &config.host,
            broker_port(config),
        );
        mqttoptions.set_keep_alive(Duration::from_secs(5));
        if use_tls {
            mqttoptions.set_transport(Transport::tls_with_config(tls_client_config().into()));
        }

        //parse the mqtt authentication options
        if let Some((username, password)) = match (&config.username, &config.password) {
            (None, None) => None,
            (None, Some(_)) => None,
            (Some(username), None) => Some((username.clone(), "".into())),
            (Some(username), Some(password)) => Some((username.clone(), password.clone())),
        } {
            mqttoptions.set_credentials(username, password);
        }
        mqttoptions.set_last_will(rumqttc::LastWill::new(
            &status_topic,
            "offline",
            rumqttc::QoS::ExactlyOnce,
            true,
        ));

        let (client, mut connection) = Client::new(mqttoptions, 512);

        // Birth message
        let _ = client.publish(&status_topic, rumqttc::QoS::ExactlyOnce, true, "online");

        let (sender, incoming) = mpsc::channel();
        let subscriptions: Arc<Mutex<Vec<(String, rumqttc::QoS)>>> = Arc::default();
        let (event_client, event_subscriptions) = (client.clone(), subscriptions.clone());
        thread::spawn(move || {
            // keep polling the event loop to make sure outgoing messages get sent
            // the call to .iter() blocks and suspends the thread effectively by
            // calling .recv() under the hood. This implies that the loop terminates
            // once the client unsubs
            for event in connection.iter() {
                match event {
                    Ok(Event::Incoming(Packet::Publish(publish))) => {
                        let _ = sender.send((publish.topic.clone(), publish.payload.to_vec()));
                    }
                    Ok(Event::Incoming(Packet::ConnAck(_))) => {
                        for (topic, qos) in event_subscriptions.lock().unwrap().iter() {
                            if let Err(e) = event_client.try_subscribe(topic.clone(), *qos) {
                                warn!("could not renew subscription to {topic}: {e}");
                            }
                        }
                    }
                    _ => {}
                }
            }
        });
        Self {
            client,
            incoming,
            subscriptions,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(port: Option<u16>, tls: Option<bool>) -> MqttConfig {
        MqttConfig {
            host: "broker".to_owned(),
            port,
            username: None,
            password: None,
            tls,
            client_id: None,
            device_id: None,
        }
    }

    #[test]
    fn default_ports_follow_tls_setting() {
        assert_eq!(broker_port(&config(None, None)), 1883);
        assert_eq!(broker_port(&config(None, Some(false))), 1883);
        assert_eq!(broker_port(&config(None, Some(true))), 8883);
        assert_eq!(broker_port(&config(Some(1234), Some(true))), 1234);
    }

    #[test]
    fn tls_config_builds_with_the_compiled_crypto_provider() {
        // rustls >= 0.23 picks its crypto provider at runtime and panics here
        // if none or more than one is compiled in
        let config = tls_client_config();
        assert!(config.alpn_protocols.is_empty());
    }

    #[test]
    fn qos_levels_map_one_to_one() {
        assert_eq!(
            match_qos(mqtt_wrapper::QoS::AtMostOnce),
            rumqttc::QoS::AtMostOnce
        );
        assert_eq!(
            match_qos(mqtt_wrapper::QoS::AtLeastOnce),
            rumqttc::QoS::AtLeastOnce
        );
        assert_eq!(
            match_qos(mqtt_wrapper::QoS::ExactlyOnce),
            rumqttc::QoS::ExactlyOnce
        );
    }
}
