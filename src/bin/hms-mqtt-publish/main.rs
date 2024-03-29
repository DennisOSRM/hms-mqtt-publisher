// TODO: support CA33 command to take over metrics consumption
// TODO: support publishing to S-Miles cloud, too

mod logging;
mod rumqttc_wrapper;

use hms2mqtt::home_assistant::HomeAssistant;
use hms2mqtt::inverter::Inverter;
use hms2mqtt::metric_collector::MetricCollector;
use hms2mqtt::mqtt_config;
use hms2mqtt::simple_mqtt::SimpleMqtt;
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
    update_interval: Option<u64>,
    home_assistant: Option<MqttConfig>,
    simple_mqtt: Option<MqttConfig>,
}

static REQUEST_DELAY_DEFAULT: u64 = 30_500;

fn main() {
    logging::init_logger();
    info!("Running revision: {}", env!("GIT_HASH"));
    if std::env::args().len() > 1 {
        error!("Arguments passed. Tool is configured by config.toml in its path");
    }

    // load configuration from current working dir, or relative to executable if former location fails
    let mut path = std::env::current_dir().expect("can't retrieve current dir");
    path.push("config.toml");
    if !path.exists() {
        info!(
            "{} does not exist. Trying relative path",
            path.to_str().expect("Cannot retrieve path")
        );
        path = std::env::current_exe().expect("Unable to get current executable path");
        path.pop();
        path.push("config.toml");
    }
    info!(
        "loading configuration from {}",
        path.to_str().expect("Cannot retrieve path")
    );
    let contents = fs::read_to_string(path).expect("Could not read config.toml");
    let config: Config = toml::from_str(&contents).expect("toml config unparsable");

    if config
        .update_interval
        .is_some_and(|value| value > REQUEST_DELAY_DEFAULT)
    {
        info!(
            "using non-default update interval of {:.2}s",
            (config.update_interval.unwrap() as f64 / 1000.)
        )
    } else {
        info!(
            "using default update interval of {:.2}s",
            (REQUEST_DELAY_DEFAULT as f64 / 1000.)
        )
    }

    info!("inverter host: {}", config.inverter_host);
    let mut inverter = Inverter::new(&config.inverter_host);

    let mut output_channels: Vec<Box<dyn MetricCollector>> = Vec::new();
    if let Some(config) = config.home_assistant {
        info!("Publishing to Home Assistant");
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

        // TODO: the sleep has to move into the Inverter struct in an async implementation
        if config
            .update_interval
            .is_some_and(|value| value > REQUEST_DELAY_DEFAULT)
        {
            thread::sleep(Duration::from_millis(config.update_interval.unwrap()));
        } else {
            thread::sleep(Duration::from_millis(REQUEST_DELAY_DEFAULT));
        }
    }
}
