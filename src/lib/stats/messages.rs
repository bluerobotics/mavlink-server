use indexmap::IndexMap;
use serde::Serialize;

use super::StatsInner;

pub type SystemId = u8;
pub type ComponentId = u8;
pub type MessageId = u32;

#[derive(Default, Clone, Debug, Serialize)]
pub struct HubMessagesStats {
    pub systems_messages_stats: IndexMap<SystemId, SystemMessagesStats>,
}

#[derive(Default, Clone, Debug, Serialize)]
pub struct SystemMessagesStats {
    pub components_messages_stats: IndexMap<ComponentId, ComponentMessageStats>,
}

#[derive(Default, Clone, Debug, Serialize)]
pub struct ComponentMessageStats {
    pub messages_stats: IndexMap<MessageId, StatsInner>,
}
