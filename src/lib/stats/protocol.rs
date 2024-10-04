use anyhow::Result;
use tokio::sync::oneshot;

use crate::stats::{DriversStats, StatsInner};

use super::messages::HubMessagesStats;

pub enum StatsCommand {
    SetPeriod {
        period: tokio::time::Duration,
        response: oneshot::Sender<Result<()>>,
    },
    Reset {
        response: oneshot::Sender<Result<()>>,
    },
    GetDriversStats {
        response: oneshot::Sender<Result<DriversStats>>,
    },
    GetHubStats {
        response: oneshot::Sender<Result<StatsInner>>,
    },
    GetHubMessagesStats {
        response: oneshot::Sender<Result<HubMessagesStats>>,
    },
}
