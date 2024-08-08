use std::{
    collections::HashMap,
    sync::{atomic::AtomicU64, Arc},
};

use anyhow::{anyhow, Context, Result};
use mavlink::MAVLinkV2MessageRaw;
use tokio::{
    sync::{broadcast, mpsc, RwLock},
    task::JoinHandle,
};
use tracing::*;

use crate::{
    sinks::{Sink, SinkInfo},
    sources::{Source, SourceInfo},
};

pub struct Hub {
    sinks: Arc<RwLock<HashMap<u64, Sink>>>,
    sources: Arc<RwLock<HashMap<u64, Source>>>,
    mpsc_sender: mpsc::Sender<MAVLinkV2MessageRaw>,
    bcst_sender: broadcast::Sender<Arc<WrappedMessage>>,
    receiver_task: JoinHandle<Result<()>>,
    last_id: Arc<RwLock<AtomicU64>>,
}

#[derive(Debug, Clone)]
pub struct WrappedMessage {
    pub filter: Arc<RwLock<Option<SinkFilter>>>,
    pub message: Arc<MAVLinkV2MessageRaw>,
}

#[derive(Debug, Clone)]
pub struct SinkFilter {
    component_ids: Vec<u8>,
    system_ids: Vec<u8>,
}

impl Hub {
    #[instrument(level = "debug")]
    pub async fn new(buffer_size: usize) -> Self {
        let (mpsc_sender, mpsc_receiver) = mpsc::channel::<MAVLinkV2MessageRaw>(buffer_size);
        let (bcst_sender, bcst_receiver) = broadcast::channel::<Arc<WrappedMessage>>(buffer_size);

        let bcst_sender2 = bcst_sender.clone();

        let receiver_task =
            tokio::spawn(async move { Self::recv_task(mpsc_receiver, bcst_sender2).await });

        Self {
            sinks: Default::default(),
            sources: Default::default(),
            mpsc_sender,
            bcst_sender,
            receiver_task,
            last_id: Default::default(),
        }
    }

    #[instrument(level = "debug", skip(self, sink))]
    pub async fn add_sink(&self, sink: Sink) -> Result<()> {
        let (key, _) = &self.last_id.write().await.get_mut().overflowing_add(1);

        if self.sinks.write().await.insert(*key, sink).is_some() {
            return Err(anyhow!("Failed inserting sink: Sink is already present"));
        }

        Ok(())
    }

    #[instrument(level = "debug", skip(self))]
    pub async fn remove_sink(&self, id: &u64) -> Result<()> {
        self.sinks
            .write()
            .await
            .remove(id)
            .context("Sink not found")?;

        Ok(())
    }

    #[instrument(level = "debug", skip(self, source))]
    pub async fn add_source(&self, source: Source) -> Result<()> {
        let (key, _) = &self.last_id.write().await.get_mut().overflowing_add(1);

        if self.sources.write().await.insert(*key, source).is_some() {
            return Err(anyhow!(
                "Failed inserting source: Source is already present"
            ));
        }

        Ok(())
    }

    #[instrument(level = "debug", skip(self))]
    pub async fn remove_source(&self, id: &u64) -> Result<()> {
        self.sources
            .write()
            .await
            .remove(id)
            .context("Source not found")?;

        Ok(())
    }

    #[instrument(level = "debug", skip(self))]
    pub async fn sinks(&self) -> Vec<SinkInfo> {
        self.sinks
            .read()
            .await
            .values()
            .map(|sink| sink.info.clone())
            .collect()
    }

    #[instrument(level = "debug", skip(self))]
    pub async fn sources(&self) -> Vec<SourceInfo> {
        self.sources
            .read()
            .await
            .values()
            .map(|source| source.info.clone())
            .collect()
    }

    #[instrument(level = "debug")]
    async fn recv_task(
        mut receiver: mpsc::Receiver<MAVLinkV2MessageRaw>,
        sender: broadcast::Sender<Arc<WrappedMessage>>,
    ) -> Result<()> {
        debug!("Task started");

        while let Some(frame) = receiver.recv().await {
            let wrapped_frame = Arc::new(WrappedMessage {
                filter: Default::default(),
                message: Arc::new(frame),
            });
            if let Err(err) = sender.send(wrapped_frame) {
                error!("Failing sending message: {err:}");
            }
        }

        debug!("Task terminated");

        Ok(())
    }

    #[instrument(level = "debug", skip(self))]
    pub fn get_sender(&self) -> mpsc::Sender<MAVLinkV2MessageRaw> {
        self.mpsc_sender.clone()
    }

    #[instrument(level = "debug", skip(self))]
    pub fn get_receiver(&self) -> broadcast::Receiver<Arc<WrappedMessage>> {
        self.bcst_sender.subscribe()
    }
}
