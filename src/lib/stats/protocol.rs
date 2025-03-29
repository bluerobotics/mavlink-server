use anyhow::Result;
use tokio::sync::{mpsc, oneshot};

use super::messages::HubMessagesStats;
use crate::stats::{resources::ResourceUsage, DriversStats, StatsInner};

pub enum StatsCommand {
    SetPeriod {
        period: tokio::time::Duration,
        response: oneshot::Sender<Result<std::time::Duration>>,
    },
    GetPeriod {
        response: oneshot::Sender<Result<std::time::Duration>>,
    },
    Reset {
        response: oneshot::Sender<Result<()>>,
    },
    GetResources {
        response: oneshot::Sender<Result<ResourceUsage>>,
    },
    GetResourcesStream {
        response: oneshot::Sender<Result<mpsc::Receiver<ResourceUsage>>>,
    },
    GetDriversStats {
        response: oneshot::Sender<Result<DriversStats>>,
    },
    GetDriversStatsStream {
        response: oneshot::Sender<Result<mpsc::Receiver<DriversStats>>>,
    },
    GetHubStats {
        response: oneshot::Sender<Result<StatsInner>>,
    },
    GetHubStatsStream {
        response: oneshot::Sender<Result<mpsc::Receiver<StatsInner>>>,
    },
    GetHubMessagesStats {
        response: oneshot::Sender<Result<HubMessagesStats>>,
    },
    GetHubMessagesStatsStream {
        response: oneshot::Sender<Result<mpsc::Receiver<HubMessagesStats>>>,
    },
}
