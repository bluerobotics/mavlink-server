use std::sync::Arc;

use anyhow::Result;
use mavlink::{MavFrame, Message};
use tokio::{
    sync::{broadcast, mpsc, RwLock},
    task::JoinHandle,
};

pub struct Sink<M>
where
    M: Message,
{
    // TODO: driver: UDP RECEIVER
    hub_recv: broadcast::Receiver<MavFrame<M>>,
    task_handler: JoinHandle<Result<()>>,
}
