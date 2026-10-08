use crate::command::Command;
use crate::protos::hoymiles::RealData::{HMSStateResponse, Warning};

/// The warnings as a JSON list, as published by the outputs
pub(crate) fn warnings_json(warnings: &[Warning]) -> serde_json::Value {
    warnings
        .iter()
        .map(|w| {
            serde_json::json!({
                "inverter": w.inv_id,
                "code": w.code,
                "count": w.count,
                "start": w.start_time,
                "end": w.end_time,
            })
        })
        .collect()
}

pub trait MetricCollector {
    fn publish(&mut self, hms_state: &HMSStateResponse);

    /// Publishes the warnings the DTU reported; fetched less often than the real-time data.
    fn publish_warnings(&mut self, _hms_state: &HMSStateResponse, _warnings: &[Warning]) {}

    /// Commands received since the last call
    fn commands(&mut self) -> Vec<Command> {
        Vec::new()
    }
}
