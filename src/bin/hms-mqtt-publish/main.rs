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
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use log::{error, info};

#[derive(Debug, Deserialize)]
struct Config {
    inverter_host: String,
    update_interval: Option<u64>,
    smiles_cooperation: Option<bool>,
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

    if config.smiles_cooperation.is_some_and(|value| value) {
        info!("S-Miles cloud cooperative mode enabled");
    } else {
        info!("S-Miles cloud cooperative mode disabled");
    }

    loop {
        // Do not query the inverter when the S-Miles cloud is about to update
        if config.smiles_cooperation.is_some_and(|value| value) {
            let now = SystemTime::now();
            let duration_since_epoch = now.duration_since(UNIX_EPOCH).unwrap();
            let seconds_since_epoch = duration_since_epoch.as_secs();

            let seconds_in_current_minute = seconds_since_epoch % 60;
            let minutes_since_epoch = seconds_since_epoch / 60;
            let minutes_in_current_hour = minutes_since_epoch % 60;

            // This is the time at which the S-Miles update seems to take place
            // Adding some extra time before and after, in which we dont publish
            if minutes_in_current_hour % 15 == 14 {
                thread::sleep(Duration::from_millis(
                    (15 + 60 - seconds_in_current_minute) * 1000,
                ));
            }
        }

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
