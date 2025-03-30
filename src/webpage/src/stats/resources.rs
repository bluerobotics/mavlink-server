use ringbuffer::RingBuffer;
use serde::Deserialize;

use crate::messages::FieldInfo;

#[derive(Debug, Default, Clone, Copy, Deserialize)]
pub struct ResourceUsage {
    pub run_time: u64,
    pub cpu_usage: f32,
    pub memory_usage_bytes: u64,
    pub total_memory_bytes: u64,
}

#[derive(Debug, Clone, Default)]
pub struct ResourceUsageHistorical {
    pub run_time: FieldInfo<f64>,
    pub cpu_usage: FieldInfo<f32>,
    pub memory_usage_mbytes: FieldInfo<f64>,
    pub total_memory_mbytes: FieldInfo<f64>,
}

impl ResourceUsageHistorical {
    pub fn update(&mut self, sample: ResourceUsage) {
        let now = chrono::Utc::now();

        self.run_time.history.push((now, sample.run_time as f64));
        self.cpu_usage.history.push((now, sample.cpu_usage));
        self.memory_usage_mbytes
            .history
            .push((now, sample.memory_usage_bytes as f64 / 1024. / 1024.));
        self.total_memory_mbytes
            .history
            .push((now, sample.total_memory_bytes as f64 / 1024. / 1024.));
    }
}
