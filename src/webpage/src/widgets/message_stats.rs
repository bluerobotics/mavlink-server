use chrono::{DateTime, Utc};
use eframe::egui::{self, CollapsingHeader};
use egui_extras::{Column, TableBuilder};

use crate::{components_names, stats::hub_messages_stats::HubMessagesStatsHistorical};

pub struct MessageStatsWidget<'a> {
    pub now: DateTime<Utc>,
    pub hub_messages_stats: &'a HubMessagesStatsHistorical,
}

impl<'a> MessageStatsWidget<'a> {
    pub fn new(now: DateTime<Utc>, hub_messages_stats: &'a HubMessagesStatsHistorical) -> Self {
        Self {
            now,
            hub_messages_stats,
        }
    }

    pub fn show(&self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .id_salt(ui.make_persistent_id("scroll_messages_stats"))
            .show(ui, |ui| {
                for (system_id, components) in &self.hub_messages_stats.systems_messages_stats {
                    CollapsingHeader::new(format!("Vehicle ID: {system_id}"))
                        .id_salt(ui.make_persistent_id(format!("vehicle_{system_id}")))
                        .default_open(true)
                        .show(ui, |ui| {
                            for (component_id, messages) in &components.components_messages_stats {
                                let component_name = components_names::get_component_name(*component_id);
                                CollapsingHeader::new(format!("Component ID: {component_id} ({component_name})"))
                                    .id_salt(ui.make_persistent_id(format!(
                                        "component_{system_id}_{component_id}"
                                    )))
                                    .default_open(true)
                                    .show(ui, |ui| {
                                        for (message_id, message_stats) in &messages.messages_stats {
                                            CollapsingHeader::new(format!(
                                                "Message ID: {message_id}"
                                            ))
                                            .id_salt(ui.make_persistent_id(format!(
                                                "message_{system_id}_{component_id}_{message_id}"
                                            )))
                                            .default_open(true)
                                            .show(ui, |ui: &mut egui::Ui| {
                                                let id_salt = ui.make_persistent_id(format!(
                                                    "messages_table_{system_id}_{component_id}_{message_id}"
                                                ));
                                                TableBuilder::new(ui)
                                                    .id_salt(id_salt)
                                                    .striped(true)
                                                    .column(Column::auto().at_least(100.))
                                                    .column(Column::remainder().at_least(150.))
                                                    .body(|mut body| {
                                                        crate::app::add_label_and_plot_all_stats(
                                                            &mut body,
                                                            self.now,
                                                            message_stats,
                                                        );
                                                    });
                                            });
                                        }
                                    });
                            }
                        });
                }
            });
    }
}
