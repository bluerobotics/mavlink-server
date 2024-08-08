use std::{
    collections::HashMap,
    sync::{atomic::AtomicU64, Arc, Mutex},
};

use anyhow::{anyhow, Context, Result};
use mavlink::MAVLinkV2MessageRaw;
use tokio::sync::{broadcast, RwLock};
use tracing::*;

use crate::drivers::{Driver, DriverInfo};

pub struct Hub {
    drivers: Arc<RwLock<HashMap<u64, Arc<dyn Driver>>>>,
    bcst_sender: broadcast::Sender<MAVLinkV2MessageRaw>,
    last_id: Arc<Mutex<AtomicU64>>,
}

#[derive(Debug, Clone)]
pub struct SinkFilter {
    component_ids: Vec<u8>,
    system_ids: Vec<u8>,
}

impl Hub {
    #[instrument(level = "debug")]
    pub async fn new(buffer_size: usize) -> Self {
        let (bcst_sender, _) = broadcast::channel::<MAVLinkV2MessageRaw>(buffer_size);

        Self {
            drivers: Default::default(),
            bcst_sender,
            last_id: Default::default(),
        }
    }

    #[instrument(level = "debug", skip(self, sink))]
    pub async fn add_driver(&self, sink: Arc<dyn Driver>) -> Result<u64> {
        let id = self
            .last_id
            .lock()
            .unwrap()
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        let cloned_sink = sink.clone();

        if self.drivers.write().await.insert(id, cloned_sink).is_some() {
            return Err(anyhow!(
                "Failed addinng driver: id {id:?} is already present"
            ));
        }

        let hub_sender = self.bcst_sender.clone();

        tokio::spawn(async move { sink.run(hub_sender).await });

        Ok(id)
    }

    #[instrument(level = "debug", skip(self))]
    pub async fn remove_driver(&self, id: &u64) -> Result<()> {
        self.drivers
            .write()
            .await
            .remove(id)
            .context("Driver id {id:?} not found")?;

        Ok(())
    }

    #[instrument(level = "debug", skip(self))]
    pub async fn drivers(&self) -> HashMap<u64, DriverInfo> {
        self.drivers
            .read()
            .await
            .iter()
            .map(|(&id, driver)| (id, driver.info()))
            .collect()
    }

    #[instrument(level = "debug", skip(self))]
    pub fn get_sender(&self) -> broadcast::Sender<MAVLinkV2MessageRaw> {
        self.bcst_sender.clone()
    }
}
