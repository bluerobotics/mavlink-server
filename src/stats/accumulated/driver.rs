use std::sync::Arc;

use indexmap::IndexMap;
use serde::Serialize;

use crate::{drivers::DriverInfo, protocol::Protocol, stats::driver::DriverUuid};

use super::AccumulatedStatsInner;

pub type AccumulatedDriversStats = IndexMap<DriverUuid, AccumulatedDriverStats>;

#[async_trait::async_trait]
pub trait AccumulatedDriverStatsProvider {
    async fn stats(&self) -> AccumulatedDriverStats;
    async fn reset_stats(&self);
}

#[derive(Debug, Clone, Serialize)]
pub struct AccumulatedDriverStats {
    pub name: Arc<String>,
    pub driver_type: &'static str,
    pub stats: AccumulatedDriverStatsInner,
}

impl AccumulatedDriverStats {
    pub fn new(name: Arc<String>, info: &dyn DriverInfo) -> Self {
        Self {
            name,
            driver_type: info.name(),
            stats: AccumulatedDriverStatsInner::default(),
        }
    }
}

#[derive(Default, Debug, Clone, Serialize)]
pub struct AccumulatedDriverStatsInner {
    pub input: Option<AccumulatedStatsInner>,
    pub output: Option<AccumulatedStatsInner>,
}

impl AccumulatedDriverStatsInner {
    pub fn update_input(&mut self, message: &Arc<Protocol>) {
        if let Some(stats) = self.input.as_mut() {
            stats.update(message);
        } else {
            self.input.replace(AccumulatedStatsInner::default());
        }
    }

    pub fn update_output(&mut self, message: &Arc<Protocol>) {
        if let Some(stats) = self.output.as_mut() {
            stats.update(message);
        } else {
            self.output.replace(AccumulatedStatsInner::default());
        }
    }
}
