use std::sync::Arc;

use anyhow::Result;
use indexmap::IndexMap;
use tokio::sync::{broadcast, oneshot};

use crate::{
    drivers::{Driver, DriverInfo},
    protocol::Protocol,
    stats::{
        accumulated::{
            driver::AccumulatedDriversStats, messages::AccumulatedHubMessagesStats,
            AccumulatedStatsInner,
        },
        driver::DriverUuid,
    },
};

pub enum HubCommand {
    AddDriver {
        driver: Arc<dyn Driver>,
        response: oneshot::Sender<Result<DriverUuid>>,
    },
    RemoveDriver {
        uuid: DriverUuid,
        response: oneshot::Sender<Result<()>>,
    },
    GetDrivers {
        response: oneshot::Sender<IndexMap<DriverUuid, Box<dyn DriverInfo>>>,
    },
    GetSender {
        response: oneshot::Sender<broadcast::Sender<Arc<Protocol>>>,
    },
    GetHubStats {
        response: oneshot::Sender<AccumulatedStatsInner>,
    },
    GetHubMessagesStats {
        response: oneshot::Sender<AccumulatedHubMessagesStats>,
    },
    GetDriversStats {
        response: oneshot::Sender<AccumulatedDriversStats>,
    },
    ResetAllStats {
        response: oneshot::Sender<Result<()>>,
    },
}
