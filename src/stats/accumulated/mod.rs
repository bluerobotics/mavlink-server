pub mod driver;
pub mod messages;

use std::sync::Arc;

use crate::protocol::Protocol;

#[derive(Clone, Debug)]
pub struct AccumulatedStatsInner {
    pub last_update_us: u64,
    pub messages: u64,
    pub bytes: u64,
    pub delay: u64,
}

impl Default for AccumulatedStatsInner {
    fn default() -> Self {
        Self {
            last_update_us: chrono::Utc::now().timestamp_micros() as u64,
            messages: 0,
            bytes: 0,
            delay: 0,
        }
    }
}

impl AccumulatedStatsInner {
    pub fn update(&mut self, message: &Arc<Protocol>) {
        self.last_update_us = chrono::Utc::now().timestamp_micros() as u64;
        self.bytes = self.bytes.wrapping_add(message.raw_bytes().len() as u64);
        self.messages = self.messages.wrapping_add(1);
        self.delay = self
            .delay
            .wrapping_add(self.last_update_us - message.timestamp);
    }
}
