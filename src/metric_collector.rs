use crate::protos::hoymiles::RealData::HMSStateResponse;

pub trait MetricCollector {
    fn publish(&mut self, hms_state: &HMSStateResponse);
}
