pub mod accumulated;
mod actor;
mod protocol;

use std::sync::{Arc, Mutex};

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
