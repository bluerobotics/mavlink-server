mod actor;
mod protocol;

use std::sync::{Arc, Mutex};

use actor::HubActor;
use anyhow::{Result, anyhow};
use indexmap::IndexMap;
use lazy_static::lazy_static;
use protocol::HubCommand;
use tokio::sync::{RwLock, broadcast, mpsc, oneshot};

use crate::{
    cli,
    drivers::{Driver, DriverInfo},
    protocol::Protocol,
    stats::{
        accumulated::{
            AccumulatedStatsInner, driver::AccumulatedDriversStats,
            messages::AccumulatedHubMessagesStats,
        },
        driver::DriverUuid,
    },
};

lazy_static! {
    static ref HUB: Hub = Hub::new(
        10000,
        Arc::new(RwLock::new(cli::mavlink_system_id())),
        Arc::new(RwLock::new(cli::mavlink_component_id())),
        Arc::new(RwLock::new(cli::mavlink_heartbeat_frequency())),
    );
}

lazy_static! {
    static ref NAMES_MAP: Arc<Mutex<IndexMap<String, u32>>> = Default::default();
}

struct Hub {
    sender: mpsc::Sender<HubCommand>,
    _task: Arc<Mutex<tokio::task::JoinHandle<()>>>,
}

impl Hub {
    fn new(
        buffer_size: usize,
        component_id: Arc<RwLock<u8>>,
        system_id: Arc<RwLock<u8>>,
        frequency: Arc<RwLock<f32>>,
    ) -> Self {
        let (sender, receiver) = mpsc::channel(32);
        let hub = HubActor::new(buffer_size, component_id, system_id, frequency);
        let _task = Arc::new(Mutex::new(tokio::spawn(hub.start(receiver))));

        Self { sender, _task }
    }
}

pub async fn add_driver(driver: Arc<dyn Driver>) -> Result<DriverUuid> {
    let (response_tx, response_rx) = oneshot::channel();
    HUB.sender
        .send(HubCommand::AddDriver {
            driver,
            response: response_tx,
        })
        .await?;
    response_rx.await?
}

pub async fn remove_driver(uuid: DriverUuid) -> Result<()> {
    let (response_tx, response_rx) = oneshot::channel();
    HUB.sender
        .send(HubCommand::RemoveDriver {
            uuid,
            response: response_tx,
        })
        .await?;
    response_rx.await?
}

pub async fn drivers() -> Result<IndexMap<DriverUuid, Box<dyn DriverInfo>>> {
    let (response_tx, response_rx) = oneshot::channel();
    HUB.sender
        .send(HubCommand::GetDrivers {
            response: response_tx,
        })
        .await?;
    let res = response_rx.await?;
    Ok(res)
}

pub async fn sender() -> Result<broadcast::Sender<Arc<Protocol>>> {
    let (response_tx, response_rx) = oneshot::channel();
    HUB.sender
        .send(HubCommand::GetSender {
            response: response_tx,
        })
        .await?;
    let res = response_rx.await?;
    Ok(res)
}

pub async fn drivers_stats() -> Result<AccumulatedDriversStats> {
    let (response_tx, response_rx) = oneshot::channel();
    HUB.sender
        .send(HubCommand::GetDriversStats {
            response: response_tx,
        })
        .await?;
    let res = response_rx.await?;
    Ok(res)
}

pub async fn hub_stats() -> Result<AccumulatedStatsInner> {
    let (response_tx, response_rx) = oneshot::channel();
    HUB.sender
        .send(HubCommand::GetHubStats {
            response: response_tx,
        })
        .await?;
    let res = response_rx.await?;
    Ok(res)
}

pub async fn hub_messages_stats() -> Result<AccumulatedHubMessagesStats> {
    let (response_tx, response_rx) = oneshot::channel();
    HUB.sender
        .send(HubCommand::GetHubMessagesStats {
            response: response_tx,
        })
        .await?;
    let res = response_rx.await?;
    Ok(res)
}

pub async fn reset_all_stats() -> Result<()> {
    let (response_tx, response_rx) = oneshot::channel();
    HUB.sender
        .send(HubCommand::ResetAllStats {
            response: response_tx,
        })
        .await?;
    response_rx.await?
}

pub fn generate_new_default_name(prefix: &str) -> Result<String> {
    let mut generated_names = NAMES_MAP.lock().unwrap();

    let num = generated_names
        .entry(prefix.to_owned())
        .and_modify(|n| {
            if *n == u32::MAX {
                // still panic-free; the Err below will be returned
            } else {
                *n = n.saturating_add(1);
            }
        })
        .or_insert(0);

    if *num == u32::MAX {
        return Err(anyhow!(
            "No indexes are left for the given prefix {prefix:?}. Current index: {num:?}."
        ));
    }

    Ok(format!("{prefix}{num}"))
}
