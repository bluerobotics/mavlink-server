use std::sync::Arc;

use indexmap::IndexMap;

use super::StatsInner;

pub type DriverUuid = uuid::Uuid;

pub type DriversStats = IndexMap<DriverUuid, DriverStats>;

#[derive(Debug, Clone)]
pub struct DriverStats {
    pub name: Arc<String>,
    pub driver_type: &'static str,
    pub stats: DriverStatsInner,
}

#[derive(Debug, Clone)]
pub struct DriverStatsInner {
    pub input: Option<StatsInner>,
    pub output: Option<StatsInner>,
}
