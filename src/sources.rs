use anyhow::Result;
use mavlink::MAVLinkV2MessageRaw;
use tokio::{sync::mpsc, task::JoinHandle};
use tracing::*;

pub struct Source {
    // TODO: driver: UDP SENDER
    task: JoinHandle<Result<()>>,
    pub info: SourceInfo,
}

#[derive(Debug, Clone)]
pub enum SourceInfo {
    FakeSource,
}

impl Source {
    pub fn new(info: SourceInfo, hub_sender: mpsc::Sender<MAVLinkV2MessageRaw>) -> Self {
        let task = tokio::spawn(async move {
            match info {
                SourceInfo::FakeSource => fake_source_driver_task(hub_sender).await,
            }
        });

        Self { task, info }
    }
}

pub async fn fake_source_driver_task(hub_sender: mpsc::Sender<MAVLinkV2MessageRaw>) -> Result<()> {
    loop {
        let message = MAVLinkV2MessageRaw::default();

        debug!("Fake message created: {message:?}");

        hub_sender.send(message).await?;

        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
}
