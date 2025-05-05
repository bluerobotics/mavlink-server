use std::sync::Arc;

use anyhow::Result;
use indexmap::IndexMap;
use tokio::sync::{mpsc, Mutex, Notify, RwLock};
use tracing::*;

use crate::{
    hub,
    stats::{
        accumulated::{
            driver::AccumulatedDriversStats, messages::AccumulatedHubMessagesStats,
            AccumulatedStatsInner,
        },
        driver::{DriverStats, DriverStatsInner},
        messages::HubMessagesStats,
        resources::{self, ResourceUsage},
        DriversStats, StatsCommand, StatsInner,
    },
};

pub struct StatsActor {
    start_time: Arc<RwLock<u64>>,
    update_period: Arc<RwLock<tokio::time::Duration>>,
    last_accumulated_drivers_stats: Arc<Mutex<AccumulatedDriversStats>>,
    drivers_stats: Arc<RwLock<DriversStats>>,
    drivers_stats_notify: Arc<Notify>,
    last_accumulated_hub_stats: Arc<Mutex<AccumulatedStatsInner>>,
    hub_stats: Arc<RwLock<StatsInner>>,
    hub_stats_notify: Arc<Notify>,
    last_accumulated_hub_messages_stats: Arc<Mutex<AccumulatedHubMessagesStats>>,
    hub_messages_stats: Arc<RwLock<HubMessagesStats>>,
    hub_messages_stats_notify: Arc<Notify>,
    resources: Arc<RwLock<ResourceUsage>>,
    resources_notify: Arc<Notify>,
}

impl StatsActor {
    pub async fn start(mut self, mut receiver: mpsc::Receiver<StatsCommand>) {
        let drivers_stats_task = tokio::spawn({
            let update_period = self.update_period.clone();
            let last_accumulated_drivers_stats = self.last_accumulated_drivers_stats.clone();
            let drivers_stats = self.drivers_stats.clone();
            let drivers_stats_notify = self.drivers_stats_notify.clone();
            let start_time = self.start_time.clone();

            async move {
                loop {
                    update_driver_stats(
                        &last_accumulated_drivers_stats,
                        &drivers_stats,
                        &start_time,
                    )
                    .await;

                    drivers_stats_notify.notify_waiters();

                    tokio::time::sleep(*update_period.read().await).await;
                }
            }
        });

        let hub_stats_task = tokio::spawn({
            let update_period = self.update_period.clone();
            let last_accumulated_hub_stats = self.last_accumulated_hub_stats.clone();
            let hub_stats = self.hub_stats.clone();
            let hub_stats_notify = self.hub_stats_notify.clone();
            let start_time = self.start_time.clone();

            async move {
                loop {
                    update_hub_stats(&last_accumulated_hub_stats, &hub_stats, &start_time).await;

                    hub_stats_notify.notify_waiters();

                    tokio::time::sleep(*update_period.read().await).await;
                }
            }
        });

        let hub_messages_stats_task = tokio::spawn({
            let update_period = self.update_period.clone();
            let last_accumulated_hub_messages_stats =
                self.last_accumulated_hub_messages_stats.clone();
            let hub_messages_stats = self.hub_messages_stats.clone();
            let hub_messages_stats_notify = self.hub_messages_stats_notify.clone();
            let start_time = self.start_time.clone();

            async move {
                loop {
                    update_hub_messages_stats(
                        &last_accumulated_hub_messages_stats,
                        &hub_messages_stats,
                        &start_time,
                    )
                    .await;

                    hub_messages_stats_notify.notify_waiters();

                    tokio::time::sleep(*update_period.read().await).await;
                }
            }
        });

        let resources_task = tokio::spawn({
            let update_period = self.update_period.clone();
            let resources = self.resources.clone();
            let resources_notify = self.resources_notify.clone();

            async move {
                loop {
                    let resource_usage = resources::usage();
                    *resources.write().await =
                        resource_usage.expect("Failed to get resource usage");
                    resources_notify.notify_waiters();
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
                StatsCommand::GetPeriod { response } => {
                    let result = self.period().await;
                    let _ = response.send(result);
                }
                StatsCommand::Reset { response } => {
                    let result: std::result::Result<(), anyhow::Error> = self.reset().await;
                    let _ = response.send(result);
                }
                StatsCommand::GetResources { response } => {
                    let result = self.resources().await;
                    let _ = response.send(result);
                }
                StatsCommand::GetResourcesStream { response } => {
                    let result = self.resources_stream().await;
                    let _ = response.send(result);
                }
                StatsCommand::GetDriversStats { response } => {
                    let result = self.drivers_stats().await;
                    let _ = response.send(result);
                }
                StatsCommand::GetDriversStatsStream { response } => {
                    let result = self.drivers_stats_stream().await;
                    let _ = response.send(result);
                }
                StatsCommand::GetHubStats { response } => {
                    let result = self.hub_stats().await;
                    let _ = response.send(result);
                }
                StatsCommand::GetHubStatsStream { response } => {
                    let result = self.hub_stats_stream().await;
                    let _ = response.send(result);
                }
                StatsCommand::GetHubMessagesStats { response } => {
                    let result = self.hub_messages_stats().await;
                    let _ = response.send(result);
                }
                StatsCommand::GetHubMessagesStatsStream { response } => {
                    let result = self.hub_messages_stats_stream().await;
                    let _ = response.send(result);
                }
            }
        }

        hub_messages_stats_task.abort();
        drivers_stats_task.abort();
        hub_stats_task.abort();
        resources_task.abort();
    }

    #[instrument(level = "debug")]
    pub fn new(update_period: tokio::time::Duration) -> Self {
        let update_period = Arc::new(RwLock::new(update_period));
        let last_accumulated_hub_stats = Arc::new(Mutex::new(AccumulatedStatsInner::default()));
        let hub_stats = Arc::new(RwLock::new(StatsInner::default()));
        let hub_stats_notify = Arc::new(Notify::new());
        let last_accumulated_drivers_stats =
            Arc::new(Mutex::new(AccumulatedDriversStats::default()));
        let drivers_stats = Arc::new(RwLock::new(DriversStats::default()));
        let drivers_stats_notify = Arc::new(Notify::new());
        let last_accumulated_hub_messages_stats =
            Arc::new(Mutex::new(AccumulatedHubMessagesStats::default()));
        let hub_messages_stats = Arc::new(RwLock::new(HubMessagesStats::default()));
        let hub_messages_stats_notify = Arc::new(Notify::new());
        let start_time = Arc::new(RwLock::new(chrono::Utc::now().timestamp_micros() as u64));
        let resources = Arc::new(RwLock::new(ResourceUsage::default()));
        let resources_notify = Arc::new(Notify::new());

        Self {
            start_time,
            update_period,
            last_accumulated_hub_stats,
            hub_stats,
            hub_stats_notify,
            last_accumulated_drivers_stats,
            drivers_stats,
            drivers_stats_notify,
            last_accumulated_hub_messages_stats,
            hub_messages_stats,
            hub_messages_stats_notify,
            resources,
            resources_notify,
        }
    }

    #[instrument(level = "debug", skip(self))]
    async fn hub_stats(&self) -> Result<StatsInner> {
        let hub_stats = self.hub_stats.read().await.clone();

        Ok(hub_stats)
    }

    #[instrument(level = "debug", skip(self))]
    async fn hub_stats_stream(&self) -> Result<mpsc::Receiver<StatsInner>> {
        let (sender, receiver) = mpsc::channel(100);

        let hub_stats = self.hub_stats.clone();
        let hub_stats_notify = self.hub_stats_notify.clone();

        tokio::spawn(async move {
            loop {
                hub_stats_notify.notified().await;

                if let Err(error) = sender.send(hub_stats.read().await.clone()).await {
                    trace!("Finishing Hub Stats stream: {error:?}");
                    break;
                }
            }
        });

        Ok(receiver)
    }

    #[instrument(level = "debug", skip(self))]
    async fn hub_messages_stats(&self) -> Result<HubMessagesStats> {
        let hub_messages_stats = self.hub_messages_stats.read().await.clone();

        Ok(hub_messages_stats)
    }

    #[instrument(level = "debug", skip(self))]
    async fn hub_messages_stats_stream(&self) -> Result<mpsc::Receiver<HubMessagesStats>> {
        let (sender, receiver) = mpsc::channel(100);

        let hub_messages_stats = self.hub_messages_stats.clone();
        let hub_messages_stats_notify = self.hub_messages_stats_notify.clone();

        tokio::spawn(async move {
            loop {
                hub_messages_stats_notify.notified().await;

                if let Err(error) = sender.send(hub_messages_stats.read().await.clone()).await {
                    trace!("Finishing Hub Messages Stats stream: {error:?}");
                    break;
                }
            }
        });

        Ok(receiver)
    }

    #[instrument(level = "debug", skip(self))]
    async fn resources(&mut self) -> Result<ResourceUsage> {
        let resources = self.resources.read().await.clone();

        Ok(resources)
    }

    #[instrument(level = "debug", skip(self))]
    async fn resources_stream(&self) -> Result<mpsc::Receiver<ResourceUsage>> {
        let (sender, receiver) = mpsc::channel(100);

        let resources = self.resources.clone();
        let resources_notify = self.resources_notify.clone();

        tokio::spawn(async move {
            loop {
                resources_notify.notified().await;

                if let Err(error) = sender.send(*resources.read().await).await {
                    trace!("Finishing Resources stream: {error:?}");
                    break;
                }
            }
        });

        Ok(receiver)
    }

    #[instrument(level = "debug", skip(self))]
    async fn drivers_stats(&mut self) -> Result<DriversStats> {
        let drivers_stats = self.drivers_stats.read().await.clone();

        Ok(drivers_stats)
    }

    #[instrument(level = "debug", skip(self))]
    async fn drivers_stats_stream(&self) -> Result<mpsc::Receiver<DriversStats>> {
        let (sender, receiver) = mpsc::channel(100);

        let drivers_stats = self.drivers_stats.clone();
        let drivers_stats_notify = self.drivers_stats_notify.clone();

        tokio::spawn(async move {
            loop {
                drivers_stats_notify.notified().await;

                if let Err(error) = sender.send(drivers_stats.read().await.clone()).await {
                    trace!("Finishing Drivers Stats stream: {error:?}");
                    break;
                }
            }
        });

        Ok(receiver)
    }

    #[instrument(level = "debug", skip(self))]
    async fn set_period(&mut self, period: tokio::time::Duration) -> Result<tokio::time::Duration> {
        let period = tokio::time::Duration::from_secs_f32(period.as_secs_f32().clamp(0.1, 10.));
        *self.update_period.write().await = period;

        Ok(*self.update_period.read().await)
    }

    #[instrument(level = "debug", skip(self))]
    async fn period(&mut self) -> Result<tokio::time::Duration> {
        Ok(*self.update_period.read().await)
    }

    #[instrument(level = "debug", skip(self))]
    async fn reset(&mut self) -> Result<()> {
        // note: hold the guards until the hub clear each driver stats to minimize weird states
        let mut hub_stats = self.hub_stats.write().await;
        let mut driver_stats = self.drivers_stats.write().await;
        let mut hub_messages_stats = self.hub_messages_stats.write().await;

        if let Err(error) = hub::reset_all_stats().await {
            error!("Failed resetting stats: {error:?}");
        }
        *self.start_time.write().await = chrono::Utc::now().timestamp_micros() as u64;

        self.last_accumulated_drivers_stats.lock().await.clear();
        driver_stats.clear();

        *hub_messages_stats = HubMessagesStats::default();
        *self.last_accumulated_hub_messages_stats.lock().await =
            AccumulatedHubMessagesStats::default();

        *hub_stats = StatsInner::default();
        *self.last_accumulated_hub_stats.lock().await = AccumulatedStatsInner::default();

        Ok(())
    }
}

async fn update_hub_messages_stats(
    last_accumulated_hub_messages_stats: &Mutex<AccumulatedHubMessagesStats>,
    hub_messages_stats: &RwLock<HubMessagesStats>,
    start_time: &RwLock<u64>,
) {
    let mut last_stats = last_accumulated_hub_messages_stats.lock().await;
    let current_stats = hub::hub_messages_stats().await.unwrap();
    let start_time = *start_time.read().await;

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
    *last_stats = current_stats;
}

async fn update_hub_stats(
    last_accumulated_hub_stats: &Arc<Mutex<AccumulatedStatsInner>>,
    hub_stats: &Arc<RwLock<StatsInner>>,
    start_time: &Arc<RwLock<u64>>,
) {
    let mut last_stats = last_accumulated_hub_stats.lock().await;
    let current_stats = hub::hub_stats().await.unwrap();
    let start_time = *start_time.read().await;

    let new_hub_stats = StatsInner::from_accumulated(&current_stats, &last_stats, start_time);

    trace!("{new_hub_stats:#?}");

    *hub_stats.write().await = new_hub_stats;
    *last_stats = current_stats;
}

async fn update_driver_stats(
    last_accumulated_drivers_stats: &Arc<Mutex<AccumulatedDriversStats>>,
    driver_stats: &Arc<RwLock<DriversStats>>,
    start_time: &Arc<RwLock<u64>>,
) {
    let mut last_map = last_accumulated_drivers_stats.lock().await;
    let current_map = hub::drivers_stats().await.unwrap();
    let start_time = *start_time.read().await;

    let mut merged_stats = IndexMap::with_capacity(last_map.len().max(current_map.len()));

    for (uuid, last_struct) in last_map.iter() {
        merged_stats.insert(*uuid, (Some(last_struct), None));
    }

    for (uuid, current_struct) in &current_map {
        merged_stats
            .entry(*uuid)
            .and_modify(|e| e.1 = Some(current_struct))
            .or_insert((None, Some(current_struct)));
    }

    let mut new_map = IndexMap::with_capacity(merged_stats.len());

    let default_input = AccumulatedStatsInner::default();
    let default_output = AccumulatedStatsInner::default();

    for (uuid, (last, current)) in merged_stats {
        if let Some(current_stats) = current {
            let new_input_stats = if let Some(current_input_stats) = &current_stats.stats.input {
                let last_input_stats = last
                    .and_then(|l| l.stats.input.as_ref())
                    .unwrap_or(&default_input);

                Some(StatsInner::from_accumulated(
                    current_input_stats,
                    last_input_stats,
                    start_time,
                ))
            } else {
                None
            };

            let new_output_stats = if let Some(current_output_stats) = &current_stats.stats.output {
                let last_output_stats = last
                    .and_then(|l| l.stats.output.as_ref())
                    .unwrap_or(&default_output);

                Some(StatsInner::from_accumulated(
                    current_output_stats,
                    last_output_stats,
                    start_time,
                ))
            } else {
                None
            };

            new_map.insert(
                uuid,
                DriverStats {
                    name: current_stats.name.clone(),
                    driver_type: current_stats.driver_type,
                    stats: DriverStatsInner {
                        input: new_input_stats,
                        output: new_output_stats,
                    },
                },
            );
        }
    }

    trace!("{new_map:#?}");

    *driver_stats.write().await = new_map;
    *last_map = current_map;
}
