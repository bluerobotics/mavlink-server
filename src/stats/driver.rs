use std::sync::Arc;

use crate::protocol::Protocol;

#[async_trait::async_trait]
pub trait DriverStats {
    async fn stats(&self) -> DriverStatsInfo;
    async fn reset_stats(&self);
}

#[derive(Default, Debug, Clone)]
pub struct DriverStatsInfo {
    pub input: Option<DriverStatsInfoInner>,
    pub output: Option<DriverStatsInfoInner>,
}

impl DriverStatsInfo {
    pub async fn update_input(&mut self, message: Arc<Protocol>) {
        if let Some(stats) = self.input.as_mut() {
            stats.update(message).await;
        } else {
            self.input.replace(DriverStatsInfoInner::default());
        }
    }

    pub async fn update_output(&mut self, message: Arc<Protocol>) {
        if let Some(stats) = self.output.as_mut() {
            stats.update(message).await;
        } else {
            self.output.replace(DriverStatsInfoInner::default());
        }
    }
}

#[derive(Clone, Debug)]
pub struct DriverStatsInfoInner {
    pub last_update: u64,
    pub messages: usize,
    pub bytes: usize,
    pub delay: u64,
}

impl Default for DriverStatsInfoInner {
    fn default() -> Self {
        Self {
            last_update: chrono::Utc::now().timestamp_micros() as u64,
            messages: 0,
            bytes: 0,
            delay: 0,
        }
    }
}

impl DriverStatsInfoInner {
    pub async fn update(&mut self, message: Arc<Protocol>) {
        self.last_update = chrono::Utc::now().timestamp_micros() as u64;
        self.bytes += message.raw_bytes().len();
        self.messages += 1;
        self.delay += self.last_update - message.timestamp;
    }
}
