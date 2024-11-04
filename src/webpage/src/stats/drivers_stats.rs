use indexmap::IndexMap;
use serde::Deserialize;

use super::{
    ByteStatsHistorical, ByteStatsSample, DelayStatsHistorical, DelayStatsSample,
    MessageStatsHistorical, MessageStatsSample, StatsInner,
};

pub type DriverUuid = uuid::Uuid;

pub type DriversStatsSample = DriversStats<ByteStatsSample, MessageStatsSample, DelayStatsSample>;
pub type DriversStatsHistorical =
    DriversStats<ByteStatsHistorical, MessageStatsHistorical, DelayStatsHistorical>;

#[derive(Default, Clone, Debug, Deserialize)]
pub struct DriversStats<B, M, D> {
    #[serde(flatten)]
    pub drivers_stats: IndexMap<DriverUuid, DriverStats<B, M, D>>,
}

#[derive(Default, Debug, Clone, Deserialize)]
pub struct DriverStats<B, M, D> {
    pub name: String,
    pub driver_type: String,
    pub stats: DriverStatsInner<B, M, D>,
}

#[derive(Default, Debug, Clone, Deserialize)]
pub struct DriverStatsInner<B, M, D> {
    pub input: Option<StatsInner<B, M, D>>,
    pub output: Option<StatsInner<B, M, D>>,
}

impl DriversStatsHistorical {
    pub fn update(&mut self, sample: DriversStatsSample) {
        let now = chrono::Utc::now();

        for (driver_uuid, sample_driver_stats) in sample.drivers_stats {
            let driver_stats = &mut self
                .drivers_stats
                .entry(driver_uuid)
                .or_insert(DriverStats {
                    name: sample_driver_stats.name,
                    driver_type: sample_driver_stats.driver_type,
                    stats: Default::default(),
                });

            if let Some(sample_input_stats) = sample_driver_stats.stats.input {
                driver_stats
                    .stats
                    .input
                    .get_or_insert(Default::default())
                    .update(now, sample_input_stats)
            }

            if let Some(sample_output_stats) = sample_driver_stats.stats.output {
                driver_stats
                    .stats
                    .output
                    .get_or_insert(Default::default())
                    .update(now, sample_output_stats);
            }
        }
    }
}
