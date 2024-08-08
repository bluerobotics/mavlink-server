use std::{collections::HashMap, sync::Arc};

use anyhow::{anyhow, Context, Result};
use mavlink::MAVLinkV2MessageRaw;
use tokio::sync::{broadcast, RwLock};
use tracing::*;

use crate::drivers::{Driver, DriverInfo};

pub struct Hub {
    drivers: Arc<RwLock<HashMap<u64, Arc<dyn Driver>>>>,
    bcst_sender: broadcast::Sender<MAVLinkV2MessageRaw>,
    last_id: Arc<RwLock<u64>>, // Use RwLock for read and write operations
}

impl Hub {
    #[instrument(level = "debug")]
    pub async fn new(buffer_size: usize) -> Self {
        let (bcst_sender, _) = broadcast::channel(buffer_size);

        Self {
            drivers: Arc::new(RwLock::new(HashMap::new())),
            bcst_sender,
            last_id: Arc::new(RwLock::new(0)),
        }
    }

    #[instrument(level = "debug", skip(self, driver))]
    pub async fn add_driver(&self, driver: Arc<dyn Driver>) -> Result<u64> {
        let mut last_id = self.last_id.write().await;
        let id = *last_id;
        *last_id += 1;

        let mut drivers = self.drivers.write().await;

        if drivers.insert(id, driver.clone()).is_some() {
            return Err(anyhow!(
                "Failed addinng driver: id {id:?} is already present"
            ));
        }

        let hub_sender = self.bcst_sender.clone();

        tokio::spawn(async move { driver.run(hub_sender).await });

        Ok(id)
    }

    #[instrument(level = "debug", skip(self))]
    pub async fn remove_driver(&self, id: u64) -> Result<()> {
        let mut drivers = self.drivers.write().await;
        drivers.remove(&id).context("Driver id {id:?} not found")?;
        Ok(())
    }

    #[instrument(level = "debug", skip(self))]
    pub async fn drivers(&self) -> HashMap<u64, DriverInfo> {
        let drivers = self.drivers.read().await;
        drivers
            .iter()
            .map(|(&id, driver)| (id, driver.info()))
            .collect()
    }

    #[instrument(level = "debug", skip(self))]
    pub fn get_sender(&self) -> broadcast::Sender<MAVLinkV2MessageRaw> {
        self.bcst_sender.clone()
    }
}
