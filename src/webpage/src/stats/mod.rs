use chrono::{DateTime, Utc};
use ringbuffer::RingBuffer;
use serde::Deserialize;

use crate::messages::FieldInfo;

pub mod drivers_stats;
pub mod hub_messages_stats;
pub mod hub_stats;
pub mod stats_frequency;

pub type SystemId = u8;
pub type ComponentId = u8;
pub type MessageId = u32;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct StatsInner<B, M, D> {
    pub last_message_time_us: u64,
    pub bytes: B,
    pub messages: M,
    pub delay_stats: D,
}

impl StatsInner<ByteStatsHistorical, MessageStatsHistorical, DelayStatsHistorical> {
    fn update(
        &mut self,
        now: DateTime<Utc>,
        sample_stats: StatsInner<ByteStatsSample, MessageStatsSample, DelayStatsSample>,
    ) {
        // Update ByteStats
        self.bytes
            .total_bytes
            .history
            .push((now, sample_stats.bytes.total_bytes));
        self.bytes
            .bytes_per_second
            .history
            .push((now, sample_stats.bytes.bytes_per_second));
        self.bytes
            .average_bytes_per_second
            .history
            .push((now, sample_stats.bytes.average_bytes_per_second));

        // Update MessageStats
        self.messages
            .total_messages
            .history
            .push((now, sample_stats.messages.total_messages));
        self.messages
            .messages_per_second
            .history
            .push((now, sample_stats.messages.messages_per_second));
        self.messages
            .average_messages_per_second
            .history
            .push((now, sample_stats.messages.average_messages_per_second));

        // Update DelayStats
        self.delay_stats
            .delay
            .history
            .push((now, sample_stats.delay_stats.delay));
        self.delay_stats
            .jitter
            .history
            .push((now, sample_stats.delay_stats.jitter));

        self.last_message_time_us = sample_stats.last_message_time_us;
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ByteStatsSample {
    pub total_bytes: u32,
    pub bytes_per_second: f64,
    pub average_bytes_per_second: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct MessageStatsSample {
    pub total_messages: u32,
    pub messages_per_second: f64,
    pub average_messages_per_second: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct DelayStatsSample {
    pub delay: f64,
    pub jitter: f64,
}

#[derive(Debug, Clone, Default)]
pub struct ByteStatsHistorical {
    pub total_bytes: FieldInfo<u32>,
    pub bytes_per_second: FieldInfo<f64>,
    pub average_bytes_per_second: FieldInfo<f64>,
}

#[derive(Debug, Clone, Default)]
pub struct MessageStatsHistorical {
    pub total_messages: FieldInfo<u32>,
    pub messages_per_second: FieldInfo<f64>,
    pub average_messages_per_second: FieldInfo<f64>,
}

#[derive(Debug, Clone, Default)]
pub struct DelayStatsHistorical {
    pub delay: FieldInfo<f64>,
    pub jitter: FieldInfo<f64>,
}
