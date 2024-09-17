use std::{collections::HashMap, ops::Div, sync::Arc};

use anyhow::{anyhow, Context, Result};
use mavlink::MAVLinkV2MessageRaw;
use tokio::sync::{broadcast, mpsc, RwLock};
use tracing::*;

use crate::{
    drivers::{Driver, DriverInfo},
    hub::HubCommand,
    protocol::Protocol,
    stats::accumulated::{
        driver::AccumulatedDriverStats, messages::AccumulatedHubMessagesStats,
        AccumulatedStatsInner,
    },
};

pub struct HubActor {
    drivers: HashMap<u64, Arc<dyn Driver>>,
    bcst_sender: broadcast::Sender<Arc<Protocol>>,
    last_driver_id: Arc<RwLock<u64>>,
    component_id: Arc<RwLock<u8>>,
    system_id: Arc<RwLock<u8>>,
    heartbeat_task: tokio::task::JoinHandle<Result<()>>,
    hub_stats_task: tokio::task::JoinHandle<Result<()>>,
    hub_stats: Arc<RwLock<AccumulatedStatsInner>>,
    hub_messages_stats: Arc<RwLock<AccumulatedHubMessagesStats>>,
}

impl HubActor {
    pub async fn start(mut self, mut receiver: mpsc::Receiver<HubCommand>) {
        while let Some(command) = receiver.recv().await {
            match command {
                HubCommand::AddDriver { driver, response } => {
                    let result = self.add_driver(driver).await;
                    let _ = response.send(result);
                }
                HubCommand::RemoveDriver { id, response } => {
                    let result = self.remove_driver(id).await;
                    let _ = response.send(result);
                }
                HubCommand::GetDrivers { response } => {
                    let drivers = self.drivers().await;
                    let _ = response.send(drivers);
                }
                HubCommand::GetSender { response } => {
                    let _ = response.send(self.bcst_sender.clone());
                }
                HubCommand::GetDriversStats { response } => {
                    let drivers_stats = self.get_drivers_stats().await;
                    let _ = response.send(drivers_stats);
                }
                HubCommand::GetHubStats { response } => {
                    let hub_stats = self.get_hub_stats().await;
                    let _ = response.send(hub_stats);
                }
                HubCommand::ResetAllStats { response } => {
                    let _ = response.send(self.reset_all_stats().await);
                }
            }
        }
    }

    #[instrument(level = "debug")]
    pub async fn new(
        buffer_size: usize,
        component_id: Arc<RwLock<u8>>,
        system_id: Arc<RwLock<u8>>,
        frequency: Arc<RwLock<f32>>,
    ) -> Self {
        let (bcst_sender, _) = broadcast::channel(buffer_size);

        let heartbeat_task = tokio::spawn({
            let bcst_sender = bcst_sender.clone();
            let component_id = component_id.clone();
            let system_id = system_id.clone();
            let frequency = frequency.clone();

            Self::heartbeat_task(bcst_sender, component_id, system_id, frequency)
        });

        let hub_stats = Arc::new(RwLock::new(AccumulatedStatsInner::default()));
        let hub_messages_stats = Arc::new(RwLock::new(AccumulatedHubMessagesStats::default()));
        let hub_stats_task = tokio::spawn({
            let bcst_sender = bcst_sender.clone();
            let hub_stats = hub_stats.clone();
            let hub_messages_stats = hub_messages_stats.clone();

            Self::stats_task(bcst_sender, hub_stats, hub_messages_stats)
        });

        Self {
            drivers: HashMap::new(),
            bcst_sender,
            last_driver_id: Arc::new(RwLock::new(0)),
            component_id,
            system_id,
            heartbeat_task,
            hub_stats_task,
            hub_stats,
            hub_messages_stats,
        }
    }

    #[instrument(level = "debug", skip(self, driver))]
    async fn add_driver(&mut self, driver: Arc<dyn Driver>) -> Result<u64> {
        let mut last_id = self.last_driver_id.write().await;
        let id = *last_id;
        *last_id += 1;

        if self.drivers.insert(id, driver.clone()).is_some() {
            return Err(anyhow!(
                "Failed addinng driver: id {id:?} is already present"
            ));
        }

        let hub_sender = self.bcst_sender.clone();

        tokio::spawn(async move { driver.run(hub_sender).await });

        Ok(id)
    }

    #[instrument(level = "debug", skip(self))]
    async fn remove_driver(&mut self, id: u64) -> Result<()> {
        self.drivers
            .remove(&id)
            .context("Driver id {id:?} not found")?;
        Ok(())
    }

    #[instrument(level = "debug", skip(self))]
    async fn drivers(&self) -> HashMap<u64, Box<dyn DriverInfo>> {
        self.drivers
            .iter()
            .map(|(&id, driver)| (id, driver.info()))
            .collect()
    }

    async fn heartbeat_task(
        bcst_sender: broadcast::Sender<Arc<Protocol>>,
        system_id: Arc<RwLock<u8>>,
        component_id: Arc<RwLock<u8>>,
        frequency: Arc<RwLock<f32>>,
    ) -> Result<()> {
        let message =
            mavlink::ardupilotmega::MavMessage::HEARTBEAT(mavlink::ardupilotmega::HEARTBEAT_DATA {
                custom_mode: 0,
                mavtype: mavlink::ardupilotmega::MavType::MAV_TYPE_ONBOARD_CONTROLLER, // or MAV_TYPE_ONBOARD_GENERIC
                autopilot: mavlink::ardupilotmega::MavAutopilot::MAV_AUTOPILOT_INVALID, // or MAV_AUTOPILOT_GENERIC?
                base_mode: mavlink::ardupilotmega::MavModeFlag::empty(),
                system_status: mavlink::ardupilotmega::MavState::MAV_STATE_STANDBY,
                mavlink_version: 0x3,
            });

        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs_f32(
                1f32.div(*frequency.read().await),
            ))
            .await;

            if bcst_sender.receiver_count().eq(&0) {
                continue; // Don't try to send if the channel has no subscribers yet
            }

            let header = mavlink::MavHeader {
                system_id: *system_id.read().await,
                component_id: *component_id.read().await,
                ..Default::default()
            };

            let mut message_raw = Protocol::new("", MAVLinkV2MessageRaw::new());
            message_raw.serialize_message(header, &message);

            if let Err(error) = bcst_sender.send(Arc::new(message_raw)) {
                error!("Failed to send HEARTBEAT message: {error}");
            }
        }
    }

    async fn stats_task(
        bcst_sender: broadcast::Sender<Arc<Protocol>>,
        hub_stats: Arc<RwLock<AccumulatedStatsInner>>,
        hub_messages_stats: Arc<RwLock<AccumulatedHubMessagesStats>>,
    ) -> Result<()> {
        let mut bsct_receiver = bcst_sender.subscribe();

        while let Ok(message) = bsct_receiver.recv().await {
            hub_stats.write().await.update(&message).await;

            hub_messages_stats.write().await.update(&message).await;
        }

        Ok(())
    }

    #[instrument(level = "debug", skip(self))]
    fn get_sender(&self) -> broadcast::Sender<Arc<Protocol>> {
        self.bcst_sender.clone()
    }

    #[instrument(level = "debug", skip(self))]
    async fn get_drivers_stats(&self) -> Vec<(String, AccumulatedDriverStats)> {
        let mut drivers_stats = Vec::with_capacity(self.drivers.len());
        for (_id, driver) in self.drivers.iter() {
            let stats = driver.stats().await;
            let info = driver.info();
            let name = info.name().to_owned();

            drivers_stats.push((name, stats));
        }

        drivers_stats
    }

    #[instrument(level = "debug", skip(self))]
    async fn get_hub_stats(&self) -> AccumulatedStatsInner {
        self.hub_stats.read().await.clone()
    }

    #[instrument(level = "debug", skip(self))]
    async fn get_hub_messages_stats(&self) -> AccumulatedHubMessagesStats {
        self.hub_messages_stats.read().await.clone()
    }

    #[instrument(level = "debug", skip(self))]
    async fn reset_all_stats(&mut self) -> Result<()> {
        for (_id, driver) in self.drivers.iter() {
            driver.reset_stats().await;
        }

        *self.hub_stats.write().await = AccumulatedStatsInner::default();

        *self.hub_messages_stats.write().await = AccumulatedHubMessagesStats::default();

        Ok(())
    }
}
