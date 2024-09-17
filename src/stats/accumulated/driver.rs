use std::sync::Arc;

use crate::protocol::Protocol;

use super::AccumulatedStatsInner;

#[async_trait::async_trait]
pub trait AccumulatedDriverStatsProvider {
    async fn stats(&self) -> AccumulatedDriverStats;
    async fn reset_stats(&self);
}

#[derive(Default, Debug, Clone)]
pub struct AccumulatedDriverStats {
    pub input: Option<AccumulatedStatsInner>,
    pub output: Option<AccumulatedStatsInner>,
}

impl AccumulatedDriverStats {
    pub async fn update_input(&mut self, message: &Arc<Protocol>) {
        if let Some(stats) = self.input.as_mut() {
            stats.update(message).await;
        } else {
            self.input.replace(AccumulatedStatsInner::default());
        }
    }

    pub async fn update_output(&mut self, message: &Arc<Protocol>) {
        if let Some(stats) = self.output.as_mut() {
            stats.update(message).await;
        } else {
            self.output.replace(AccumulatedStatsInner::default());
        }
    }
}
