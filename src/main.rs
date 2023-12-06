// TODO: support CA33 command to take over metrics consumption
// TODO: support publishing to S-Miles cloud, too

mod config;
mod logging;
mod rumqttc_wrapper;

use hms2mqtt::home_assistant::HomeAssistant;
use hms2mqtt::inverter::Inverter;
use hms2mqtt::metric_collector::MetricCollector;
use hms2mqtt::simple_mqtt::SimpleMqtt;
use log::{error, info};
use rumqttc_wrapper::RumqttcWrapper;

use std::env;

use std::thread;
use std::time::Duration;

use crate::config::Config;

static REQUEST_DELAY: u64 = 30_500;

fn main() {
    logging::init_logger();
    info!("Running revision: {}", env!("GIT_HASH"));
    if std::env::args().len() > 1 {
        error!("Arguments passed. Tool is configured by config.toml in its path");
    }

    let config = Config::load();
    if !config.is_valid() {
        error!("configuration is invalid: {config:?}");
        return;
    }

    println!("{config:#?}");

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
