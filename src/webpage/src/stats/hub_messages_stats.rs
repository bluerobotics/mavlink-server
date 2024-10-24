use indexmap::IndexMap;
use serde::Deserialize;

use super::{
    ByteStatsHistorical, ByteStatsSample, ComponentId, DelayStatsHistorical, DelayStatsSample,
    MessageId, MessageStatsHistorical, MessageStatsSample, StatsInner, SystemId,
};

pub type HubMessagesStatsSample =
    HubMessagesStats<ByteStatsSample, MessageStatsSample, DelayStatsSample>;
pub type HubMessagesStatsHistorical =
    HubMessagesStats<ByteStatsHistorical, MessageStatsHistorical, DelayStatsHistorical>;

#[derive(Default, Clone, Debug, Deserialize)]
pub struct HubMessagesStats<B, M, D> {
    pub systems_messages_stats: IndexMap<SystemId, SystemMessagesStats<B, M, D>>,
}

#[derive(Default, Clone, Debug, Deserialize)]
pub struct SystemMessagesStats<B, M, D> {
    pub components_messages_stats: IndexMap<ComponentId, ComponentMessageStats<B, M, D>>,
}

#[derive(Default, Clone, Debug, Deserialize)]
pub struct ComponentMessageStats<B, M, D> {
    pub messages_stats: IndexMap<MessageId, StatsInner<B, M, D>>,
}

impl HubMessagesStatsHistorical {
    pub fn update(&mut self, sample: HubMessagesStatsSample) {
        let now = chrono::Utc::now();

        for (system_id, sample_system_stats) in sample.systems_messages_stats {
            // System stats
            let system_stats = self.systems_messages_stats.entry(system_id).or_default();

            for (component_id, sample_component_stats) in
                sample_system_stats.components_messages_stats
            {
                // Component stats
                let component_stats = system_stats
                    .components_messages_stats
                    .entry(component_id)
                    .or_default();

                for (message_id, sample_message_stats) in sample_component_stats.messages_stats {
                    // Message stats
                    let message_stats = component_stats
                        .messages_stats
                        .entry(message_id)
                        .or_default();

                    message_stats.update(now, sample_message_stats);
                }
            }
        }
    }
}
