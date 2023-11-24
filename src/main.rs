// TODO: support CA33 command to take over metrics consumption
// TODO: support publishing to S-Miles cloud, too

mod inverter;
mod mqtt;
mod protos;
mod logging;
mod mqtt_schemas;

use crate::inverter::Inverter;
use crate::mqtt::{MetricCollector, Mqtt};
use crate::logging::init_logger;

use std::thread;
use std::time::Duration;

use clap::Parser;
use inverter::Mode;
use log::info;
use protos::hoymiles::RealData;

#[derive(Parser)]
struct Cli {
    inverter_host: String,
    mqtt_broker_host: String,
    mqtt_username: Option<String>,
    mqtt_password: Option<String>,
    #[clap(default_value = "1883")]
    mqtt_broker_port: u16,
    #[clap(value_enum, default_value = "cooperative")]
    inverter_mode: Mode,
}

static REQUEST_DELAY: u64 = 30_500;

fn main() {
    init_logger();
    let cli = Cli::parse();

    // set up mqtt connection
    info!(
        "inverter: {}, mqtt broker {}",
        cli.inverter_host, cli.mqtt_broker_host
    );

    let mut inverter = Inverter::new(&cli.inverter_host, cli.inverter_mode);

    let mut mqtt = Mqtt::new(
        &cli.mqtt_broker_host,
        &cli.mqtt_username,
        &cli.mqtt_password,
        cli.mqtt_broker_port,
    );

    loop {
        if let Some(r) = inverter.update_state() {
            mqtt.publish(&r);
        }

        // TODO: this has to move into the Inverter struct in an async implementation
        thread::sleep(Duration::from_millis(REQUEST_DELAY));
    }
}
