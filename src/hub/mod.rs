mod actor;
mod protocol;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use anyhow::Result;
use tokio::sync::{broadcast, mpsc, oneshot, RwLock};

use crate::{
    drivers::{Driver, DriverInfo},
    protocol::Protocol,
    stats::driver::{AccumulatedDriverStats, AccumulatedStatsInner},
};

use actor::HubActor;
use protocol::HubCommand;

#[derive(Clone)]
pub struct Hub {
    sender: mpsc::Sender<HubCommand>,
    _task: Arc<Mutex<tokio::task::JoinHandle<()>>>,
}

impl Hub {
    pub async fn new(
        buffer_size: usize,
        component_id: Arc<RwLock<u8>>,
        system_id: Arc<RwLock<u8>>,
        frequency: Arc<RwLock<f32>>,
    ) -> Self {
        let (sender, receiver) = mpsc::channel(32);
        let hub = HubActor::new(buffer_size, component_id, system_id, frequency).await;
        let _task = Arc::new(Mutex::new(tokio::spawn(hub.start(receiver))));
        Self { sender, _task }
    }

    pub async fn add_driver(&self, driver: Arc<dyn Driver>) -> Result<u64> {
        let (response_tx, response_rx) = oneshot::channel();
        self.sender
            .send(HubCommand::AddDriver {
                driver,
                response: response_tx,
            })
            .await?;
        response_rx.await?
    }

    pub async fn remove_driver(&self, id: u64) -> Result<()> {
        let (response_tx, response_rx) = oneshot::channel();
        self.sender
            .send(HubCommand::RemoveDriver {
                id,
                response: response_tx,
            })
            .await?;
        response_rx.await?
    }

    pub async fn drivers(&self) -> Result<HashMap<u64, Box<dyn DriverInfo>>> {
        let (response_tx, response_rx) = oneshot::channel();
        self.sender
            .send(HubCommand::GetDrivers {
                response: response_tx,
            })
            .await?;
        let res = response_rx.await?;
        Ok(res)
    }

    pub async fn sender(&self) -> Result<broadcast::Sender<Arc<Protocol>>> {
        let (response_tx, response_rx) = oneshot::channel();
        self.sender
            .send(HubCommand::GetSender {
                response: response_tx,
            })
            .await?;
        let res = response_rx.await?;
        Ok(res)
    }

    pub async fn drivers_stats(&self) -> Result<Vec<(String, AccumulatedDriverStats)>> {
        let (response_tx, response_rx) = oneshot::channel();
        self.sender
            .send(HubCommand::GetDriversStats {
                response: response_tx,
            })
            .await?;
        let res = response_rx.await?;
        Ok(res)
    }

    pub async fn hub_stats(&self) -> Result<AccumulatedStatsInner> {
        let (response_tx, response_rx) = oneshot::channel();
        self.sender
            .send(HubCommand::GetHubStats {
                response: response_tx,
            })
            .await?;
        let res = response_rx.await?;
        Ok(res)
    }

    pub async fn reset_all_stats(&self) -> Result<()> {
        let (response_tx, response_rx) = oneshot::channel();
        self.sender
            .send(HubCommand::ResetAllStats {
                response: response_tx,
            })
            .await?;
        response_rx.await?
    }
}
