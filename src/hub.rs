use std::sync::Arc;

use anyhow::Result;
use mavlink::{MavFrame, Message};
use tokio::sync::{broadcast, mpsc, RwLock};

use crate::{sinks::Sink, sources::Source};

pub struct Hub<M>
where
    M: Message,
{
    sinks: Arc<RwLock<Vec<Sink<M>>>>,
    sources: Arc<RwLock<Vec<Source<M>>>>,
    mpsc_recv: mpsc::Receiver<MavFrame<M>>,
    bcst_send: broadcast::Sender<MavFrame<M>>,
}

pub struct ProcessedFrame<M>
where
    M: Message,
{
    filter: Arc<RwLock<SinkFilter>>,
    frame: MavFrame<M>,
}
pub struct SinkFilter {
    component_ids: Vec<u8>,
    system_ids: Vec<u8>,
}
