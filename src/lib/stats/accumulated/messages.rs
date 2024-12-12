use std::sync::Arc;

use indexmap::IndexMap;
use serde::Serialize;

use super::AccumulatedStatsInner;
use crate::{
    protocol::Protocol,
    stats::messages::{ComponentId, MessageId, SystemId},
};

#[derive(Default, Clone, Debug, Serialize)]
pub struct AccumulatedHubMessagesStats {
    pub systems_messages_stats: IndexMap<SystemId, AccumulatedSystemMessagesStats>,
}

#[derive(Default, Clone, Debug, Serialize)]
pub struct AccumulatedSystemMessagesStats {
    pub components_messages_stats: IndexMap<ComponentId, AccumulatedComponentMessageStats>,
}

#[derive(Default, Clone, Debug, Serialize)]
pub struct AccumulatedComponentMessageStats {
    pub messages_stats: IndexMap<MessageId, AccumulatedStatsInner>,
}

impl AccumulatedHubMessagesStats {
    pub fn update(&mut self, message: &Arc<Protocol>) {
        self.systems_messages_stats
            .entry(*message.system_id())
            .or_default()
            .components_messages_stats
            .entry(*message.component_id())
            .or_default()
            .messages_stats
            .entry(message.message_id())
            .and_modify(|accumulated_stats| accumulated_stats.update(message))
            .or_default();
    }
}
