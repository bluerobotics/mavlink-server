use anyhow::Result;
use mavlink::MAVLinkV2MessageRaw;
use tokio::sync::broadcast;
use tracing::*;

use super::{Driver, DriverInfo};

pub struct FakeSink;
pub struct FakeSource;

#[async_trait::async_trait]
impl Driver for FakeSink {
    async fn run(&self, hub_sender: broadcast::Sender<MAVLinkV2MessageRaw>) -> Result<()> {
        let mut hub_receiver = hub_sender.subscribe();

        while let Ok(message) = hub_receiver.recv().await {
            // TODO: Filter

            debug!("Message received: {message:?}")
        }

        Ok(())
    }

    fn info(&self) -> DriverInfo {
        DriverInfo {
            name: "FakeSink".to_string(),
        }
    }
}

#[async_trait::async_trait]
impl Driver for FakeSource {
    async fn run(&self, hub_sender: broadcast::Sender<MAVLinkV2MessageRaw>) -> Result<()> {
        loop {
            let message = MAVLinkV2MessageRaw::default();

            debug!("Fake message created: {message:?}");

            hub_sender.send(message)?;

            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
    }

    fn info(&self) -> DriverInfo {
        DriverInfo {
            name: "FakeSource".to_string(),
        }
    }
}
