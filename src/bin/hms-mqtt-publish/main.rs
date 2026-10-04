// TODO: support CA33 command to take over metrics consumption
// TODO: support publishing to S-Miles cloud, too

mod config;
mod logging;
mod rumqttc_wrapper;

use config::Config;
use hms2mqtt::home_assistant::HomeAssistant;
use hms2mqtt::inverter::Inverter;
use hms2mqtt::metric_collector::MetricCollector;
use hms2mqtt::simple_mqtt::SimpleMqtt;
use rumqttc_wrapper::RumqttcWrapper;
use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use log::{error, info};

static REQUEST_DELAY_DEFAULT: u64 = 30_500;

/// The DTU only serves fresh data about every 30 s, so shorter intervals fall back to the default.
fn update_interval(configured: Option<u64>) -> u64 {
    configured
        .filter(|&value| value > REQUEST_DELAY_DEFAULT)
        .unwrap_or(REQUEST_DELAY_DEFAULT)
}

/// config.toml from the current working dir, or next to the executable if the former doesn't exist
fn config_file() -> Option<PathBuf> {
    let candidates = [
        std::env::current_dir().ok(),
        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|dir| dir.to_path_buf())),
    ];
    candidates
        .into_iter()
        .flatten()
        .map(|dir| dir.join("config.toml"))
        .find(|path| path.exists())
}

fn main() {
    logging::init_logger();
    info!("Running revision: {}", env!("GIT_HASH"));

    // as PID 1 in a container the process gets no default signal handling, so docker stop
    // would have to kill it after a timeout
    if let Err(e) = ctrlc::set_handler(|| {
        info!("received termination signal, exiting");
        std::process::exit(0);
    }) {
        error!("could not install signal handler: {e}");
    }
    if std::env::args().len() > 1 {
        error!("Arguments passed. Tool is configured by config.toml and environment variables");
    }

    let contents = match config_file() {
        Some(path) => {
            info!("loading configuration from {}", path.display());
            match fs::read_to_string(&path) {
                Ok(contents) => Some(contents),
                Err(e) => {
                    error!("could not read {}: {e}", path.display());
                    std::process::exit(1);
                }
            }
        }
        None => {
            info!("no config.toml found, using environment variables only");
            None
        }
    };
    let config = match Config::load(contents.as_deref(), |name| std::env::var(name).ok()) {
        Ok(config) => config,
        Err(e) => {
            error!("{e}");
            std::process::exit(1);
        }
    };

    let interval = update_interval(config.update_interval);
    if interval != REQUEST_DELAY_DEFAULT {
        info!(
            "using non-default update interval of {:.2}s",
            (interval as f64 / 1000.)
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
        thread::sleep(Duration::from_millis(interval));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intervals_below_the_dtu_limit_use_the_default() {
        assert_eq!(update_interval(None), REQUEST_DELAY_DEFAULT);
        assert_eq!(update_interval(Some(0)), REQUEST_DELAY_DEFAULT);
        assert_eq!(update_interval(Some(10_000)), REQUEST_DELAY_DEFAULT);
        assert_eq!(
            update_interval(Some(REQUEST_DELAY_DEFAULT)),
            REQUEST_DELAY_DEFAULT
        );
        assert_eq!(update_interval(Some(60_000)), 60_000);
    }
}
