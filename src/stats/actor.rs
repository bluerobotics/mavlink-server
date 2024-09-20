use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use anyhow::Result;
use tokio::sync::{mpsc, RwLock};
use tracing::*;

use crate::{
    hub::Hub,
    stats::{
        accumulated::{
            driver::AccumulatedDriversStats, messages::AccumulatedHubMessagesStats,
            AccumulatedStatsInner,
        },
        driver::{DriverStats, DriverStatsInner},
        messages::HubMessagesStats,
        DriversStats, StatsCommand, StatsInner,
    },
};

pub struct StatsActor {
    hub: Hub,
    start_time: Arc<RwLock<u64>>,
    update_period: Arc<RwLock<tokio::time::Duration>>,
    last_accumulated_drivers_stats: Arc<RwLock<Vec<(String, AccumulatedDriverStats)>>>,
    drivers_stats: Arc<RwLock<DriversStats>>,
    last_accumulated_hub_stats: Arc<RwLock<AccumulatedStatsInner>>,
    hub_stats: Arc<RwLock<StatsInner>>,
    last_accumulated_hub_messages_stats: Arc<RwLock<AccumulatedHubMessagesStats>>,
    hub_messages_stats: Arc<RwLock<HubMessagesStats>>,
}

impl StatsActor {
    pub async fn start(mut self, mut receiver: mpsc::Receiver<StatsCommand>) {
        let drivers_stats_task = tokio::spawn({
            let hub = self.hub.clone();
            let update_period = self.update_period.clone();
            let last_accumulated_drivers_stats = self.last_accumulated_drivers_stats.clone();
            let drivers_stats = self.drivers_stats.clone();
            let start_time = self.start_time.clone();

            async move {
                loop {
                    update_driver_stats(
                        &hub,
                        &last_accumulated_drivers_stats,
                        &drivers_stats,
                        &start_time,
                    )
                    .await;

                    tokio::time::sleep(*update_period.read().await).await;
                }
            }
        });

        let hub_stats_task = tokio::spawn({
            let hub = self.hub.clone();
            let update_period = self.update_period.clone();
            let last_accumulated_hub_stats = self.last_accumulated_hub_stats.clone();
            let hub_stats = self.hub_stats.clone();
            let start_time = self.start_time.clone();

            async move {
                loop {
                    update_hub_stats(&hub, &last_accumulated_hub_stats, &hub_stats, &start_time)
                        .await;

                    tokio::time::sleep(*update_period.read().await).await;
                }
            }
        });

        let hub_messages_stats_task = tokio::spawn({
            let hub = self.hub.clone();
            let update_period = self.update_period.clone();
            let last_accumulated_hub_messages_stats =
                self.last_accumulated_hub_messages_stats.clone();
            let hub_messages_stats = self.hub_messages_stats.clone();
            let start_time = self.start_time.clone();

            async move {
                loop {
                    update_hub_messages_stats(
                        &hub,
                        &last_accumulated_hub_messages_stats,
                        &hub_messages_stats,
                        &start_time,
                    )
                    .await;

                    tokio::time::sleep(*update_period.read().await).await;
                }
            }
        });

        while let Some(command) = receiver.recv().await {
            match command {
                StatsCommand::SetPeriod { period, response } => {
                    let result = self.set_period(period).await;
                    let _ = response.send(result);
                }
                StatsCommand::Reset { response } => {
                    let result = self.reset().await;
                    let _ = response.send(result);
                }
                StatsCommand::GetDriversStats { response } => {
                    let result = self.drivers_stats().await;
                    let _ = response.send(result);
                }
                StatsCommand::GetHubStats { response } => {
                    let result = self.hub_stats().await;
                    let _ = response.send(result);
                }
                StatsCommand::GetHubMessagesStats { response } => {
                    let result = self.hub_messages_stats().await;
                    let _ = response.send(result);
                }
            }
        }

        hub_messages_stats_task.abort();
        drivers_stats_task.abort();
        hub_stats_task.abort();
    }

    #[instrument(level = "debug", skip(hub))]
    pub async fn new(hub: Hub, update_period: tokio::time::Duration) -> Self {
        let update_period = Arc::new(RwLock::new(update_period));
        let last_accumulated_hub_stats = Arc::new(RwLock::new(AccumulatedStatsInner::default()));
        let hub_stats = Arc::new(RwLock::new(StatsInner::default()));
        let last_accumulated_drivers_stats = Arc::new(RwLock::new(Vec::new()));
        let drivers_stats = Arc::new(RwLock::new(Vec::new()));
        let last_accumulated_hub_messages_stats =
            Arc::new(RwLock::new(AccumulatedHubMessagesStats::default()));
        let hub_messages_stats = Arc::new(RwLock::new(HubMessagesStats::default()));
        let start_time = Arc::new(RwLock::new(chrono::Utc::now().timestamp_micros() as u64));

        Self {
            hub,
            start_time,
            update_period,
            last_accumulated_hub_stats,
            hub_stats,
            last_accumulated_drivers_stats,
            drivers_stats,
            last_accumulated_hub_messages_stats,
            hub_messages_stats,
        }
    }

    #[instrument(level = "debug", skip(self))]
    async fn hub_stats(&self) -> Result<StatsInner> {
        let hub_stats = self.hub_stats.read().await.clone();

        Ok(hub_stats)
    }

    #[instrument(level = "debug", skip(self))]
    async fn hub_messages_stats(&self) -> Result<HubMessagesStats> {
        let hub_messages_stats = self.hub_messages_stats.read().await.clone();

        Ok(hub_messages_stats)
    }

    #[instrument(level = "debug", skip(self))]
    async fn drivers_stats(&mut self) -> Result<DriversStats> {
        let drivers_stats = self.drivers_stats.read().await.clone();

        Ok(drivers_stats)
    }

    #[instrument(level = "debug", skip(self))]
    async fn set_period(&mut self, period: tokio::time::Duration) -> Result<()> {
        *self.update_period.write().await = period;

        Ok(())
    }

    #[instrument(level = "debug", skip(self))]
    async fn reset(&mut self) -> Result<()> {
        // note: hold the guards until the hub clear each driver stats to minimize weird states
        let mut hub_stats = self.hub_stats.write().await;
        let mut driver_stats = self.drivers_stats.write().await;
        let mut hub_messages_stats = self.hub_messages_stats.write().await;

        if let Err(error) = self.hub.reset_all_stats().await {
            error!("Failed resetting stats: {error:?}");
        }
        *self.start_time.write().await = chrono::Utc::now().timestamp_micros() as u64;

        self.last_accumulated_drivers_stats.write().await.clear();
        driver_stats.clear();

        *hub_messages_stats = HubMessagesStats::default();
        *self.last_accumulated_hub_messages_stats.write().await =
            AccumulatedHubMessagesStats::default();

        *hub_stats = StatsInner::default();
        *self.last_accumulated_hub_stats.write().await = AccumulatedStatsInner::default();

        Ok(())
    }
}

#[instrument(level = "debug", skip_all)]
async fn update_hub_messages_stats(
    hub: &Hub,
    last_accumulated_hub_messages_stats: &RwLock<AccumulatedHubMessagesStats>,
    hub_messages_stats: &RwLock<HubMessagesStats>,
    start_time: &RwLock<u64>,
) {
    let last_stats = last_accumulated_hub_messages_stats.read().await.clone();
    let current_stats = hub.hub_messages_stats().await.unwrap();
    let start_time = start_time.read().await.clone();

    let mut new_hub_messages_stats = HubMessagesStats::default();

    for (system_id, current_system_stats) in &current_stats.systems_messages_stats {
        for (component_id, current_component_stats) in
            &current_system_stats.components_messages_stats
        {
            for (message_id, current_message_stats) in &current_component_stats.messages_stats {
                let default_message_stats = AccumulatedStatsInner::default();

                let last_message_stats = last_stats
                    .systems_messages_stats
                    .get(system_id)
                    .and_then(|sys| sys.components_messages_stats.get(component_id))
                    .and_then(|comp| comp.messages_stats.get(message_id))
                    .unwrap_or(&default_message_stats);

                let new_stats = StatsInner::from_accumulated(
                    current_message_stats,
                    last_message_stats,
                    start_time,
                );

                new_hub_messages_stats
                    .systems_messages_stats
                    .entry(*system_id)
                    .or_default()
                    .components_messages_stats
                    .entry(*component_id)
                    .or_default()
                    .messages_stats
                    .insert(*message_id, new_stats);
            }
        }
    }

    trace!("{new_hub_messages_stats:#?}");

    *hub_messages_stats.write().await = new_hub_messages_stats;
    *last_accumulated_hub_messages_stats.write().await = current_stats;
}

#[instrument(level = "debug", skip_all)]
async fn update_hub_stats(
    hub: &Hub,
    last_accumulated_hub_stats: &Arc<RwLock<AccumulatedStatsInner>>,
    hub_stats: &Arc<RwLock<StatsInner>>,
    start_time: &Arc<RwLock<u64>>,
) {
    let last_stats = last_accumulated_hub_stats.read().await.clone();
    let current_stats = hub.hub_stats().await.unwrap();
    let start_time = start_time.read().await.clone();

    let new_hub_stats = StatsInner::from_accumulated(&current_stats, &last_stats, start_time);

    trace!("{new_hub_stats:#?}");

    *hub_stats.write().await = new_hub_stats;
    *last_accumulated_hub_stats.write().await = current_stats;
}

#[instrument(level = "debug", skip_all)]
async fn update_driver_stats(
    hub: &Hub,
    last_accumulated_drivers_stats: &Arc<RwLock<Vec<(String, AccumulatedDriverStats)>>>,
    driver_stats: &Arc<RwLock<DriversStats>>,
    start_time: &Arc<RwLock<u64>>,
) {
    let last_stats = last_accumulated_drivers_stats.read().await.clone();
    let current_stats = hub.drivers_stats().await.unwrap();

    let last_map: HashMap<_, _> = last_stats.into_iter().collect();
    let current_map: HashMap<_, _> = current_stats
        .iter()
        .map(|(name, raw)| (name.clone(), raw.clone()))
        .collect();

    let merged_keys: HashSet<String> = last_map.keys().chain(current_map.keys()).cloned().collect();

    let merged_stats: Vec<(
        String,
        (
            Option<AccumulatedDriverStats>,
            Option<AccumulatedDriverStats>,
        ),
    )> = merged_keys
        .into_iter()
        .map(|name| {
            let last = last_map.get(&name).cloned();
            let current = current_map.get(&name).cloned();
            (name, (last, current))
        })
        .collect();

    let mut new_driver_stats = Vec::new();

    let start_time = start_time.read().await.clone();

    for (name, (last, current)) in merged_stats {
        if let Some(current_stats) = current {
            let new_input_stats = if let Some(current_input_stats) = &current_stats.input {
                let default_input = AccumulatedStatsInner::default();

                let last_input_stats = if let Some(last_stats) = &last {
                    last_stats.input.as_ref().unwrap_or(&default_input)
                } else {
                    &default_input
                };

                Some(StatsInner::from_accumulated(
                    current_input_stats,
                    last_input_stats,
                    start_time,
                ))
            } else {
                None
            };

            let new_output_stats = if let Some(current_output_stats) = &current_stats.output {
                let default_output = AccumulatedStatsInner::default();

                let last_output_stats = if let Some(last_stats) = &last {
                    last_stats.output.as_ref().unwrap_or(&default_output)
                } else {
                    &default_output
                };

                Some(StatsInner::from_accumulated(
                    current_output_stats,
                    last_output_stats,
                    start_time,
                ))
            } else {
                None
            };

            new_driver_stats.push((
                name,
                DriverStats {
                    input: new_input_stats,
                    output: new_output_stats,
                },
            ));
        }
    }

    trace!("{new_driver_stats:#?}");

    *driver_stats.write().await = new_driver_stats;
    *last_accumulated_drivers_stats.write().await = current_stats;
}
