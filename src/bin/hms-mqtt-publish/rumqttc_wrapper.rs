use std::{thread, time::Duration};

use hms2mqtt::{
    mqtt_config::MqttConfig,
    mqtt_wrapper::{self},
};
use log::warn;
use rumqttc::{
    tokio_rustls::{self, rustls::ClientConfig},
    Client, MqttOptions,
    QoS::AtMostOnce,
    Transport,
};

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

/// The configured port, or the standard MQTT port for plain (1883) or TLS (8883) connections.
fn broker_port(config: &MqttConfig) -> u16 {
    config.port.unwrap_or(if config.tls.is_some_and(|tls| tls) {
        8883
    } else {
        1883
    })
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
        let use_tls = config.tls.is_some_and(|tls| tls);

        let mut mqttoptions = MqttOptions::new(
            "hms800wt2-mqtt-publisher".to_string() + suffix,
            &config.host,
            broker_port(config),
        );
        mqttoptions.set_keep_alive(Duration::from_secs(5));
        if use_tls {
            // Use rustls-native-certs to load root certificates from the operating system.
            let mut roots = tokio_rustls::rustls::RootCertStore::empty();
            rustls_native_certs::load_native_certs()
                .expect("could not load platform certs")
                .into_iter()
                .for_each(|cert| {
                    roots.add(cert).unwrap();
                });

            let client_config = ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth();

            mqttoptions.set_transport(Transport::tls_with_config(client_config.into()));
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

        let (client, mut connection) = Client::new(mqttoptions, 512);

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
