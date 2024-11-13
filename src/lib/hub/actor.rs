use std::{ops::Div, sync::Arc};

use anyhow::{anyhow, Context, Result};
use indexmap::IndexMap;
use tokio::sync::{broadcast, mpsc, RwLock};
use tracing::*;

use crate::{
    cli,
    drivers::{Driver, DriverInfo},
    hub::HubCommand,
    protocol::Protocol,
    stats::{
        accumulated::{
            driver::AccumulatedDriversStats, messages::AccumulatedHubMessagesStats,
            AccumulatedStatsInner,
        },
        driver::DriverUuid,
    },
};

#[allow(dead_code)]
pub struct HubActor {
    drivers: IndexMap<DriverUuid, Arc<dyn Driver>>,
    bcst_sender: broadcast::Sender<Arc<Protocol>>,
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
                HubCommand::RemoveDriver { uuid, response } => {
                    let result = self.remove_driver(uuid).await;
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
                HubCommand::GetHubMessagesStats { response } => {
                    let hub_messages_stats = self.get_hub_messages_stats().await;
                    let _ = response.send(hub_messages_stats);
                }
                HubCommand::ResetAllStats { response } => {
                    let _ = response.send(self.reset_all_stats().await);
                }
            }
        }
    }

    #[instrument(level = "debug")]
    pub fn new(
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
            drivers: IndexMap::new(),
            bcst_sender,
            component_id,
            system_id,
            heartbeat_task,
            hub_stats_task,
            hub_stats,
            hub_messages_stats,
        }
    }

    #[instrument(level = "debug", skip(self, driver))]
    async fn add_driver(&mut self, driver: Arc<dyn Driver>) -> Result<DriverUuid> {
        let uuid = *driver.uuid();
        if self.drivers.insert(uuid, driver.clone()).is_some() {
            return Err(anyhow!(
                "Failed addinng driver: uuid {uuid:?} is already present"
            ));
        }

        let hub_sender = self.bcst_sender.clone();

        tokio::spawn(async move { driver.run(hub_sender).await });

        Ok(uuid)
    }

    #[instrument(level = "debug", skip(self))]
    async fn remove_driver(&mut self, uuid: DriverUuid) -> Result<()> {
        self.drivers
            .swap_remove(&uuid)
            .context("Driver uuid {uuid:?} not found")?;
        Ok(())
    }

    #[instrument(level = "debug", skip(self))]
    async fn drivers(&self) -> IndexMap<DriverUuid, Box<dyn DriverInfo>> {
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

        let burst_size = 5;
        let mut burst_msgs_counter = 0;
        let mut do_burst = cli::send_initial_heartbeats();

        loop {
            let duration = if do_burst {
                if burst_msgs_counter == burst_size {
                    do_burst = false;
                }

                tokio::time::Duration::from_millis(100)
            } else {
                tokio::time::Duration::from_secs_f32(1f32.div(*frequency.read().await))
            };

            tokio::time::sleep(duration).await;

            if bcst_sender.receiver_count().eq(&0) {
                continue; // Don't try to send if the channel has no subscribers yet
            }

            let header = mavlink::MavHeader {
                system_id: *system_id.read().await,
                component_id: *component_id.read().await,
                ..Default::default()
            };

            let message = Arc::new(Protocol::from_mavlink_raw(header, &message, ""));

            if let Err(error) = bcst_sender.send(message) {
                error!("Failed to send HEARTBEAT message: {error}");
            }

            if do_burst && burst_msgs_counter < burst_size {
                burst_msgs_counter += 1;
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
            hub_stats.write().await.update(&message);

            hub_messages_stats.write().await.update(&message);
        }

        Ok(())
    }

    #[instrument(level = "debug", skip(self))]
    fn get_sender(&self) -> broadcast::Sender<Arc<Protocol>> {
        self.bcst_sender.clone()
    }

    #[instrument(level = "debug", skip(self))]
    async fn get_drivers_stats(&self) -> AccumulatedDriversStats {
        let mut drivers_stats = IndexMap::with_capacity(self.drivers.len());
        for (_id, driver) in self.drivers.iter() {
            let stats = driver.stats().await;
            let uuid = *driver.uuid();

            drivers_stats.insert(uuid, stats);
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
