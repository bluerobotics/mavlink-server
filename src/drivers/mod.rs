pub mod fake;

use anyhow::Result;
use mavlink::MAVLinkV2MessageRaw;
use tokio::sync::broadcast;
use tracing::*;

#[async_trait::async_trait]
pub trait Driver: Send + Sync {
    async fn run(&self, hub_sender: broadcast::Sender<MAVLinkV2MessageRaw>) -> Result<()>;
    fn info(&self) -> DriverInfo;
}

#[derive(Debug, Clone)]
pub struct DriverInfo {
    name: String,
}
