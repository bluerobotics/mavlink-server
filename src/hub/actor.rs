use std::{collections::HashMap, ops::Div, sync::Arc};

use anyhow::{anyhow, Context, Result};
use mavlink::MAVLinkV2MessageRaw;
use tokio::sync::{broadcast, mpsc, RwLock};
use tracing::*;

use crate::{
    drivers::{Driver, DriverInfo},
    protocol::Protocol,
    stats::driver::DriverStatsInfo,
};

use super::protocol::HubCommand;

pub struct HubActor {
    drivers: Arc<RwLock<HashMap<u64, Arc<dyn Driver>>>>,
    bcst_sender: broadcast::Sender<Arc<Protocol>>,
    last_driver_id: Arc<RwLock<u64>>,
    component_id: Arc<RwLock<u8>>,
    system_id: Arc<RwLock<u8>>,
    heartbeat_task: tokio::task::JoinHandle<Result<()>>,
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
                HubCommand::GetStats { response } => {
                    let stats = self.get_stats().await;
                    let _ = response.send(stats);
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

        let bcst_sender_cloned = bcst_sender.clone();
        let component_id_cloned = component_id.clone();
        let system_id_cloned = system_id.clone();
        let frequency_cloned = frequency.clone();
        let heartbeat_task = tokio::spawn(async move {
            Self::heartbeat_task(
                bcst_sender_cloned,
                component_id_cloned,
                system_id_cloned,
                frequency_cloned,
            )
            .await
        });

        Self {
            drivers: Arc::new(RwLock::new(HashMap::new())),
            bcst_sender,
            last_driver_id: Arc::new(RwLock::new(0)),
            component_id,
            system_id,
            heartbeat_task,
        }
    }

    #[instrument(level = "debug", skip(self, driver))]
    pub async fn add_driver(&self, driver: Arc<dyn Driver>) -> Result<u64> {
        let mut last_id = self.last_driver_id.write().await;
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
    pub async fn drivers(&self) -> HashMap<u64, Box<dyn DriverInfo>> {
        let drivers = self.drivers.read().await;
        drivers
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

    #[instrument(level = "debug", skip(self))]
    pub fn get_sender(&self) -> broadcast::Sender<Arc<Protocol>> {
        self.bcst_sender.clone()
    }

    #[instrument(level = "debug", skip(self))]
    pub async fn get_stats(&self) -> Vec<(String, DriverStatsInfo)> {
        let drivers = self.drivers.read().await;

        let mut drivers_stats = Vec::with_capacity(drivers.len());
        for (_id, driver) in drivers.iter() {
            let stats = driver.stats().await;
            let info = driver.info();
            let name = info.name().to_owned();

            drivers_stats.push((name, stats));
        }

        drivers_stats
    }

    #[instrument(level = "debug", skip(self))]
    pub async fn reset_all_stats(&self) -> Result<()> {
        let drivers = self.drivers.write().await;

        for (_id, driver) in drivers.iter() {
            driver.reset_stats().await;
        }

        Ok(())
    }
}