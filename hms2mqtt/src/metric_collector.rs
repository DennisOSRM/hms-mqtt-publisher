use crate::command::Command;
use crate::protos::hoymiles::RealData::{HMSStateResponse, Warning};

pub trait MetricCollector {
    fn publish(&mut self, hms_state: &HMSStateResponse);

    /// Publishes the warnings the DTU reported; fetched less often than the real-time data.
    fn publish_warnings(&mut self, _hms_state: &HMSStateResponse, _warnings: &[Warning]) {}

    /// Commands received since the last call
    fn commands(&mut self) -> Vec<Command> {
        Vec::new()
    }
}
