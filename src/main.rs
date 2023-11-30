// TODO: support CA33 command to take over metrics consumption
// TODO: support publishing to S-Miles cloud, too

mod logging;
mod rumqttc_wrapper;

use hms_mqtt_publish::home_assistant::HomeAssistant;
use hms_mqtt_publish::inverter::Inverter;
use hms_mqtt_publish::metric_collector::MetricCollector;
use hms_mqtt_publish::mqtt_config;
use hms_mqtt_publish::simple_mqtt::SimpleMqtt;
use mqtt_config::MqttConfig;
use rumqttc_wrapper::RumqttcWrapper;
use serde_derive::Deserialize;
use std::fs;
use std::thread;
use std::time::Duration;

use log::{error, info};

#[derive(Debug, Deserialize)]
struct Config {
    inverter_host: String,
    home_assistant: Option<MqttConfig>,
    simple_mqtt: Option<MqttConfig>,
}

static REQUEST_DELAY: u64 = 30_500;

fn main() {
    logging::init_logger();

    if std::env::args().len() > 1 {
        error!("Arguments passed. Tool is configured by config.toml in its path");
    }

    let filename = "config.toml";
    let contents = fs::read_to_string(filename).expect("Could not read config.toml");
    let config: Config = toml::from_str(&contents).expect("toml config unparsable");

    info!("inverter host: {}", config.inverter_host);

    let mut inverter = Inverter::new(&config.inverter_host);

    let mut output_channels: Vec<Box<dyn MetricCollector>> = Vec::new();
    if let Some(config) = config.home_assistant {
        info!("Publishing to Home Assistent");
        output_channels.push(Box::new(HomeAssistant::<RumqttcWrapper>::new(&config)));
    }

    if let Some(config) = config.simple_mqtt {
        info!("Publishing to simple MQTT broker");
        output_channels.push(Box::new(SimpleMqtt::<RumqttcWrapper>::new(&config)));
    }

    loop {
        if let Some(r) = inverter.update_state() {
            output_channels.iter_mut().for_each(|channel| {
                channel.publish(&r);
            })
        }

        // TODO: this has to move into the Inverter struct in an async implementation
        thread::sleep(Duration::from_millis(REQUEST_DELAY));
    }
}
