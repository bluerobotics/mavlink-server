use chrono::{DateTime, Utc};
use eframe::egui::{self, CollapsingHeader};
use egui_extras::{Column, TableBuilder};

use crate::stats::hub_stats::HubStatsHistorical;

pub struct HubStatsWidget<'a> {
    pub now: DateTime<Utc>,
    pub hub_stats: &'a HubStatsHistorical,
}

impl<'a> HubStatsWidget<'a> {
    pub fn new(now: DateTime<Utc>, hub_stats: &'a HubStatsHistorical) -> Self {
        Self { now, hub_stats }
    }

    pub fn show(&self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .id_salt("scrollhub_stats")
            .auto_shrink(false)
            .show(ui, |ui| {
                let hub_stats = &self.hub_stats.stats;

                CollapsingHeader::new("Hub Stats")
                    .id_salt(ui.make_persistent_id("hub_stats"))
                    .default_open(true)
                    .show(ui, |ui| {
                        let salt = ui.make_persistent_id("hub_stats_table");
                        TableBuilder::new(ui)
                            .id_salt(salt)
                            .striped(true)
                            .column(Column::auto().at_least(100.))
                            .column(Column::remainder().at_least(150.))
                            .body(|mut body| {
                                crate::app::add_label_and_plot_all_stats(
                                    &mut body, self.now, hub_stats,
                                );
                            });
                    });
            });
    }
}
