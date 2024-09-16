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
        driver::{AccumulatedDriverStats, AccumulatedStatsInner},
        DriverStats, DriversStats, StatsCommand, StatsInner,
    },
};

pub struct StatsActor {
    hub: Hub,
    start_time: Arc<RwLock<u64>>,
    update_period: Arc<RwLock<tokio::time::Duration>>,
    last_accumulated_drivers_stats: Arc<RwLock<Vec<(String, AccumulatedDriverStats)>>>,
    drivers_stats: Arc<RwLock<DriversStats>>,
}

impl StatsActor {
    pub async fn start(mut self, mut receiver: mpsc::Receiver<StatsCommand>) {
        let drivers_stats_task = tokio::spawn({
            let hub = self.hub.clone();
            let update_period = Arc::clone(&self.update_period);
            let last_accumulated_drivers_stats = Arc::clone(&self.last_accumulated_drivers_stats);
            let drivers_stats = Arc::clone(&self.drivers_stats);
            let start_time = Arc::clone(&self.start_time);

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
            }
        }

        drivers_stats_task.abort();
    }

    #[instrument(level = "debug", skip(hub))]
    pub async fn new(hub: Hub, update_period: tokio::time::Duration) -> Self {
        let update_period = Arc::new(RwLock::new(update_period));
        let last_accumulated_drivers_stats = Arc::new(RwLock::new(Vec::new()));
        let drivers_stats = Arc::new(RwLock::new(Vec::new()));
        let start_time = Arc::new(RwLock::new(chrono::Utc::now().timestamp_micros() as u64));

        Self {
            hub,
            update_period,
            last_accumulated_drivers_stats,
            drivers_stats,
            start_time,
        }
    }

    #[instrument(level = "debug", skip(self))]
    pub async fn drivers_stats(&mut self) -> Result<DriversStats> {
        let drivers_stats = self.drivers_stats.read().await.clone();

        Ok(drivers_stats)
    }

    #[instrument(level = "debug", skip(self))]
    pub async fn set_period(&mut self, period: tokio::time::Duration) -> Result<()> {
        *self.update_period.write().await = period;

        Ok(())
    }

    #[instrument(level = "debug", skip(self))]
    pub async fn reset(&mut self) -> Result<()> {
        // note: hold the guards until the hub clear each driver stats to minimize weird states
        let mut driver_stats = self.drivers_stats.write().await;

        if let Err(error) = self.hub.reset_all_stats().await {
            error!("Failed resetting stats: {error:?}");
        }
        *self.start_time.write().await = chrono::Utc::now().timestamp_micros() as u64;
        self.last_accumulated_drivers_stats.write().await.clear();
        driver_stats.clear();

        Ok(())
    }
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
            let new_input_stats = calculate_driver_stats(
                last.as_ref().and_then(|l| l.input.clone()),
                current_stats.input.clone(),
                start_time,
            );
            let new_output_stats = calculate_driver_stats(
                last.as_ref().and_then(|l| l.output.clone()),
                current_stats.output.clone(),
                start_time,
            );

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

/// Function to calculate the driver stats for either input or output, with proper averages
#[instrument(level = "debug")]
fn calculate_driver_stats(
    last_stats: Option<AccumulatedStatsInner>,
    current_stats: Option<AccumulatedStatsInner>,
    start_time: u64,
) -> Option<StatsInner> {
    if let Some(current_stats) = current_stats {
        let time_diff = accumulated_driver_stats_time_diff(last_stats.as_ref(), &current_stats);
        let total_time = total_time_since_start(start_time, &current_stats);

        let diff_messages = current_stats.messages as u64
            - last_stats.as_ref().map_or(0, |stats| stats.messages as u64);
        let total_messages = current_stats.messages as u64;
        let messages_per_second = divide_safe(diff_messages as f64, time_diff);
        let average_messages_per_second = divide_safe(total_messages as f64, total_time);

        let diff_bytes =
            current_stats.bytes as u64 - last_stats.as_ref().map_or(0, |stats| stats.bytes as u64);
        let total_bytes = current_stats.bytes as u64;
        let bytes_per_second = divide_safe(diff_bytes as f64, time_diff);
        let average_bytes_per_second = divide_safe(total_bytes as f64, total_time);

        let delay = divide_safe(current_stats.delay as f64, current_stats.messages as f64);
        let last_delay = divide_safe(
            last_stats.as_ref().map_or(0f64, |stats| stats.delay as f64),
            last_stats
                .as_ref()
                .map_or(0f64, |stats| stats.messages as f64),
        );
        let jitter = (delay - last_delay).abs();

        Some(StatsInner {
            last_message_time: current_stats.last_update,
            total_bytes,
            bytes_per_second,
            average_bytes_per_second,
            total_messages,
            messages_per_second,
            average_messages_per_second,
            delay,
            jitter,
        })
    } else {
        None
    }
}

/// Function to calculate the total time since the start (in seconds)
#[instrument(level = "debug")]
fn total_time_since_start(start_time: u64, current_stats: &AccumulatedStatsInner) -> f64 {
    calculate_time_diff(start_time, current_stats.last_update)
}

/// Function to calculate the time difference (in seconds)
#[instrument(level = "debug")]
fn accumulated_driver_stats_time_diff(
    last_stats: Option<&AccumulatedStatsInner>,
    current_stats: &AccumulatedStatsInner,
) -> f64 {
    if let Some(last_stats) = last_stats {
        // Microseconds to seconds
        calculate_time_diff(last_stats.last_update, current_stats.last_update)
    } else {
        f64::INFINITY
    }
}

fn calculate_time_diff(last_time: u64, current_time: u64) -> f64 {
    (current_time as f64 - last_time as f64) / 1_000_000.0
}

#[instrument(level = "debug")]
fn divide_safe(numerator: f64, denominator: f64) -> f64 {
    if denominator > 0.0 {
        numerator / denominator
    } else {
        0.0
    }
}
