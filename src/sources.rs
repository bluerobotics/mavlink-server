use anyhow::Result;
use mavlink::MAVLinkV2MessageRaw;
use tokio::{sync::mpsc, task::JoinHandle};
use tracing::*;
