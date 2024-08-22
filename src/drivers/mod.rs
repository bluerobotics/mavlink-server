pub mod fake;
pub mod file;
pub mod tcp;
pub mod udp;

use crate::protocol::Protocol;
use anyhow::Result;
use tokio::sync::broadcast;

#[async_trait::async_trait]
pub trait Driver: Send + Sync {
    async fn run(&self, hub_sender: broadcast::Sender<Protocol>) -> Result<()>;
    fn info(&self) -> DriverInfo;
}

#[derive(Debug, Clone)]
pub struct DriverInfo {
    name: String,
}
