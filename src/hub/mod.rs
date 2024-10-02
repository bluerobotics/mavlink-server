mod actor;
mod protocol;

use std::sync::{Arc, Mutex};

use anyhow::Result;
use async_once::AsyncOnce;
use indexmap::IndexMap;
use lazy_static::lazy_static;
use tokio::sync::{broadcast, mpsc, oneshot, RwLock};

use crate::{
    drivers::{Driver, DriverInfo},
    protocol::Protocol,
    stats::{
        accumulated::{
            driver::AccumulatedDriversStats, messages::AccumulatedHubMessagesStats,
            AccumulatedStatsInner,
        },
        driver::DriverUuid,
    },
};

use actor::HubActor;
use protocol::HubCommand;

lazy_static! {
    static ref HUB: AsyncOnce<Hub> = AsyncOnce::new(async {
        Hub::new(
            10000,
            Arc::new(RwLock::new(
                mavlink::ardupilotmega::MavComponent::MAV_COMP_ID_ONBOARD_COMPUTER as u8,
            )),
            Arc::new(RwLock::new(1)),
            Arc::new(RwLock::new(1.)),
        )
        .await
    });
}

struct Hub {
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
}

pub async fn add_driver(driver: Arc<dyn Driver>) -> Result<DriverUuid> {
    let (response_tx, response_rx) = oneshot::channel();
    HUB.get()
        .await
        .sender
        .send(HubCommand::AddDriver {
            driver,
            response: response_tx,
        })
        .await?;
    response_rx.await?
}

pub async fn remove_driver(uuid: DriverUuid) -> Result<()> {
    let (response_tx, response_rx) = oneshot::channel();
    HUB.get()
        .await
        .sender
        .send(HubCommand::RemoveDriver {
            uuid,
            response: response_tx,
        })
        .await?;
    response_rx.await?
}

pub async fn drivers() -> Result<IndexMap<DriverUuid, Box<dyn DriverInfo>>> {
    let (response_tx, response_rx) = oneshot::channel();
    HUB.get()
        .await
        .sender
        .send(HubCommand::GetDrivers {
            response: response_tx,
        })
        .await?;
    let res = response_rx.await?;
    Ok(res)
}

pub async fn sender() -> Result<broadcast::Sender<Arc<Protocol>>> {
    let (response_tx, response_rx) = oneshot::channel();
    HUB.get()
        .await
        .sender
        .send(HubCommand::GetSender {
            response: response_tx,
        })
        .await?;
    let res = response_rx.await?;
    Ok(res)
}

pub async fn drivers_stats() -> Result<AccumulatedDriversStats> {
    let (response_tx, response_rx) = oneshot::channel();
    HUB.get()
        .await
        .sender
        .send(HubCommand::GetDriversStats {
            response: response_tx,
        })
        .await?;
    let res = response_rx.await?;
    Ok(res)
}

pub async fn hub_stats() -> Result<AccumulatedStatsInner> {
    let (response_tx, response_rx) = oneshot::channel();
    HUB.get()
        .await
        .sender
        .send(HubCommand::GetHubStats {
            response: response_tx,
        })
        .await?;
    let res = response_rx.await?;
    Ok(res)
}

pub async fn hub_messages_stats() -> Result<AccumulatedHubMessagesStats> {
    let (response_tx, response_rx) = oneshot::channel();
    HUB.get()
        .await
        .sender
        .send(HubCommand::GetHubMessagesStats {
            response: response_tx,
        })
        .await?;
    let res = response_rx.await?;
    Ok(res)
}

pub async fn reset_all_stats() -> Result<()> {
    let (response_tx, response_rx) = oneshot::channel();
    HUB.get()
        .await
        .sender
        .send(HubCommand::ResetAllStats {
            response: response_tx,
        })
        .await?;
    response_rx.await?
}
