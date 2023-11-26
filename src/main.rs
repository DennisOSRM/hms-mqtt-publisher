// TODO: support CA33 command to take over metrics consumption
// TODO: support publishing to S-Miles cloud, too

mod home_assistant;
mod home_assistant_config;
mod inverter;
mod logging;
mod metric_collector;
mod mqtt_config;
mod protos;
mod simple_mqtt;

use crate::home_assistant::HomeAssistant;
use crate::inverter::Inverter;
use crate::logging::init_logger;
use crate::metric_collector::MetricCollector;
use crate::simple_mqtt::SimpleMqtt;

use mqtt_config::MqttConfig;
use serde_derive::Deserialize;
use std::fs;
use std::thread;
use std::time::Duration;

use log::{error, info};
use protos::hoymiles::RealData;

#[derive(Debug, Deserialize)]
struct Config {
    inverter_host: String,
    home_assistant: Option<MqttConfig>,
    simple_mqtt: Option<MqttConfig>,
}

static REQUEST_DELAY: u64 = 30_500;

fn main() {
    init_logger();

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
        output_channels.push(Box::new(HomeAssistant::new(&config)));
    }

    if let Some(config) = config.simple_mqtt {
        info!("Publishing to simple MQTT broker");
        output_channels.push(Box::new(SimpleMqtt::new(&config)));
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
