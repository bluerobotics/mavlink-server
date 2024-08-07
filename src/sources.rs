use std::sync::Arc;

use anyhow::Result;
use mavlink::{MavFrame, Message};
use tokio::{
    sync::{broadcast, mpsc, RwLock},
    task::JoinHandle,
};

pub struct Source<M>
where
    M: Message,
{
    // TODO: driver: UDP SENDER
    hub_send: mpsc::Sender<MavFrame<M>>,
    task_handler: JoinHandle<Result<()>>,
}
