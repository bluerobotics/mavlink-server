use std::sync::Arc;

use anyhow::Result;
use mavlink::MAVLinkV2MessageRaw;
use tokio::{sync::broadcast, task::JoinHandle};
use tracing::*;

use crate::hub::WrappedMessage;

pub struct Sink {
    task: JoinHandle<Result<()>>,
    pub info: SinkInfo,
}

#[derive(Debug, Clone)]
pub enum SinkInfo {
    FakeSink,
}

impl Sink {
    pub fn new(info: SinkInfo, hub_receiver: broadcast::Receiver<Arc<WrappedMessage>>) -> Self {
        let task = tokio::spawn(async move {
            match info {
                SinkInfo::FakeSink => fake_sink_driver_task(hub_receiver).await,
            }
        });

        Self { task, info }
    }
}

pub async fn fake_sink_driver_task(
    mut hub_receiver: broadcast::Receiver<Arc<WrappedMessage>>,
) -> Result<()> {
    while let Ok(wrapped_message) = hub_receiver.recv().await {
        // TODO: Filter

        let message = wrapped_message.message.clone();

        debug!("Message received: {message:?}")
    }

    Ok(())
}
