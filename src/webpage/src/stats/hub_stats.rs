use serde::Deserialize;

use super::{
    ByteStatsHistorical, ByteStatsSample, DelayStatsHistorical, DelayStatsSample,
    MessageStatsHistorical, MessageStatsSample, StatsInner,
};

pub type HubStatsSample = HubStats<ByteStatsSample, MessageStatsSample, DelayStatsSample>;
pub type HubStatsHistorical =
    HubStats<ByteStatsHistorical, MessageStatsHistorical, DelayStatsHistorical>;

#[derive(Default, Clone, Debug, Deserialize)]
pub struct HubStats<B, M, D> {
    #[serde(flatten)]
    pub stats: StatsInner<B, M, D>,
}

impl HubStatsHistorical {
    pub fn update(&mut self, sample: HubStatsSample) {
        let now = chrono::Utc::now();

        self.stats.update(now, sample.stats);
    }
}
