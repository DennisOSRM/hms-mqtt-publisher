// TODO: support CA33 command to take over metrics consumption
// TODO: support publishing to S-Miles cloud, too

mod inverter;
mod mqtt;
mod protos;

use crate::inverter::Inverter;
use crate::mqtt::{MetricCollector, Mqtt};

use std::io::Write;
use std::thread;
use std::time::Duration;

use chrono::Local;
use clap::Parser;
use env_logger::Builder;
use log::{info, LevelFilter};
use protos::hoymiles::RealData;

#[derive(Parser)]
struct Cli {
    inverter_host: String,
    mqtt_broker_host: String,
    mqtt_username: Option<String>,
    mqtt_password: Option<String>,
}

static REQUEST_DELAY: u64 = 30_500;

fn main() {
    Builder::new()
        .format(|buf, record| {
            writeln!(
                buf,
                "{} [{}] - {}",
                Local::now().format("%Y-%m-%dT%H:%M:%S"),
                record.level(),
                record.args()
            )
        })
        .filter(None, LevelFilter::Info)
        .init();

    let cli = Cli::parse();

    // set up mqtt connection
    info!(
        "inverter: {}, mqtt broker {}",
        cli.inverter_host, cli.mqtt_broker_host
    );

    let mut inverter = Inverter::new(&cli.inverter_host);

    let mut mqtt = Mqtt::new(
        &cli.mqtt_broker_host,
        &cli.mqtt_username,
        &cli.mqtt_password,
    );

    loop {
        if let Some(r) = inverter.update_state() {
            mqtt.publish(&r);
        }

        // TODO: this has to move into the Inverter struct in an async implementation
        thread::sleep(Duration::from_millis(REQUEST_DELAY));
    }
}
