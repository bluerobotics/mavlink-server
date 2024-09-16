use std::{collections::HashMap, sync::Arc};

use anyhow::Result;
use tokio::sync::{broadcast, oneshot};

use crate::{
    drivers::{Driver, DriverInfo},
    protocol::Protocol,
    stats::driver::AccumulatedDriverStats,
};

pub enum HubCommand {
    AddDriver {
        driver: Arc<dyn Driver>,
        response: oneshot::Sender<Result<u64>>,
    },
    RemoveDriver {
        id: u64,
        response: oneshot::Sender<Result<()>>,
    },
    GetDrivers {
        response: oneshot::Sender<HashMap<u64, Box<dyn DriverInfo>>>,
    },
    GetSender {
        response: oneshot::Sender<broadcast::Sender<Arc<Protocol>>>,
    },
    GetDriversStats {
        response: oneshot::Sender<Vec<(String, AccumulatedDriverStats)>>,
    },
    ResetAllStats {
        response: oneshot::Sender<Result<()>>,
    },
}
