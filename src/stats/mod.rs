pub mod accumulated;
mod actor;
mod messages;
mod protocol;

use std::sync::{Arc, Mutex};

use accumulated::AccumulatedStatsInner;
use anyhow::Result;
use tokio::sync::{mpsc, oneshot};

use actor::StatsActor;
use protocol::StatsCommand;

use crate::hub::Hub;

pub type DriversStats = Vec<(String, DriverStats)>;

#[derive(Debug, Clone)]
pub struct DriverStats {
    pub input: Option<StatsInner>,
    pub output: Option<StatsInner>,
}

#[derive(Debug, Clone, Default)]
pub struct StatsInner {
    pub last_message_time_us: u64,

    pub total_bytes: u64,
    pub bytes_per_second: f64,
    pub average_bytes_per_second: f64,

    pub total_messages: u64,
    pub messages_per_second: f64,
    pub average_messages_per_second: f64,

    pub delay: f64,
    pub jitter: f64,
}

impl StatsInner {
    pub fn from_accumulated(
        current_stats: &AccumulatedStatsInner,
        last_stats: &AccumulatedStatsInner,
        start_time: u64,
    ) -> Self {
        let time_diff =
            calculate_time_diff_us(last_stats.last_update_us, current_stats.last_update_us);
        let total_time = calculate_time_diff_us(start_time, current_stats.last_update_us);

        let diff_messages = current_stats.messages - last_stats.messages;
        let total_messages = current_stats.messages;
        let messages_per_second = divide_safe(diff_messages as f64, time_diff);
        let average_messages_per_second = divide_safe(total_messages as f64, total_time);

        let diff_bytes = current_stats.bytes - last_stats.bytes;
        let total_bytes = current_stats.bytes;
        let bytes_per_second = divide_safe(diff_bytes as f64, time_diff);
        let average_bytes_per_second = divide_safe(total_bytes as f64, total_time);

        let delay = divide_safe(current_stats.delay as f64, current_stats.messages as f64);
        let last_delay = divide_safe(last_stats.delay as f64, last_stats.messages as f64);
        let jitter = (delay - last_delay).abs();

        Self {
            last_message_time_us: current_stats.last_update_us,
            total_bytes,
            bytes_per_second,
            average_bytes_per_second,
            total_messages,
            messages_per_second,
            average_messages_per_second,
            delay,
            jitter,
        }
    }
}

#[derive(Clone)]
pub struct Stats {
    sender: mpsc::Sender<StatsCommand>,
    task: Arc<Mutex<tokio::task::JoinHandle<()>>>,
}

impl Stats {
    pub async fn new(hub: Hub, update_period: tokio::time::Duration) -> Self {
        let (sender, receiver) = mpsc::channel(32);
        let actor = StatsActor::new(hub, update_period).await;
        let task = Arc::new(Mutex::new(tokio::spawn(actor.start(receiver))));
        Self { sender, task }
    }

    pub async fn driver_stats(&mut self) -> Result<DriversStats> {
        let (response_tx, response_rx) = oneshot::channel();
        self.sender
            .send(StatsCommand::GetDriversStats {
                response: response_tx,
            })
            .await?;
        response_rx.await?
    }

    pub async fn hub_stats(&mut self) -> Result<StatsInner> {
        let (response_tx, response_rx) = oneshot::channel();
        self.sender
            .send(StatsCommand::GetHubStats {
                response: response_tx,
            })
            .await?;
        response_rx.await?
    }

    pub async fn set_period(&mut self, period: tokio::time::Duration) -> Result<()> {
        let (response_tx, response_rx) = oneshot::channel();
        self.sender
            .send(StatsCommand::SetPeriod {
                period,
                response: response_tx,
            })
            .await?;
        response_rx.await?
    }

    pub async fn reset(&mut self) -> Result<()> {
        let (response_tx, response_rx) = oneshot::channel();
        self.sender
            .send(StatsCommand::Reset {
                response: response_tx,
            })
            .await?;
        response_rx.await?
    }
}

fn calculate_time_diff_us(last_micros: u64, current_micros: u64) -> f64 {
    (current_micros as f64 - last_micros as f64) / 1_000_000.0
}

fn divide_safe(numerator: f64, denominator: f64) -> f64 {
    if denominator > 0.0 {
        numerator / denominator
    } else {
        0.0
    }
}
