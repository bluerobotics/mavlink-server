use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use eframe::egui::{self, CollapsingHeader};
use egui_extras::{Column, TableBuilder};

use crate::{components_names, messages::VehiclesMessages};

pub struct MessageInspectorWidget<'a> {
    pub now: DateTime<Utc>,
    pub vehicles_mavlink: &'a VehiclesMessages,
    pub search_query: &'a str,
    pub collapse_all: bool,
    pub expand_all: bool,
}

impl<'a> MessageInspectorWidget<'a> {
    pub fn new(
        now: DateTime<Utc>,
        vehicles_mavlink: &'a VehiclesMessages,
        search_query: &'a str,
        collapse_all: bool,
        expand_all: bool,
    ) -> Self {
        Self {
            now,
            vehicles_mavlink,
            search_query,
            collapse_all,
            expand_all,
        }
    }

    pub fn show(&self, ui: &mut egui::Ui) {
        let search_query = self.search_query.to_lowercase();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (system_id, components) in self.vehicles_mavlink {
                let mut matching_components = BTreeMap::new();

                for (component_id, messages) in components {
                    let mut matching_messages = BTreeMap::new();

                    for (message_id, message) in messages {
                        let name = &message.name;
                        let name_lower = name.to_lowercase();
                        let message_matches = search_query.is_empty()
                            || name_lower.contains(&search_query)
                            || message_id.to_string().contains(&search_query);

                        let mut matching_fields = BTreeMap::new();

                        if message_matches {
                            matching_fields
                                .extend(message.fields.iter().map(|(k, v)| (k.clone(), v.clone())));
                        } else {
                            for (field_name, field_info) in &message.fields {
                                let field_name_lower = field_name.to_lowercase();
                                if field_name_lower.contains(&search_query) {
                                    matching_fields.insert(field_name.clone(), field_info.clone());
                                }
                            }
                        }

                        if message_matches || !matching_fields.is_empty() {
                            matching_messages.insert(
                                message_id,
                                (message.clone(), matching_fields, message_matches),
                            );
                        }
                    }

                    if !matching_messages.is_empty() {
                        matching_components.insert(*component_id, matching_messages);
                    }
                }

                if !matching_components.is_empty() {
                    CollapsingHeader::new(format!("Vehicle ID: {system_id}"))
                        .id_salt(ui.make_persistent_id(format!("vehicle_{system_id}")))
                        .default_open(true)
                        .show(ui, |ui| {
                            for (component_id, messages) in matching_components {
                                let component_name = components_names::get_component_name(component_id);
                                CollapsingHeader::new(format!(
                                    "Component ID: {component_id} ({component_name})"
                                ))
                                .id_salt(ui.make_persistent_id(format!(
                                    "component_{system_id}_{component_id}"
                                )))
                                .default_open(true)
                                .show(ui, |ui| {
                                    for (message_id, (message, matching_fields, _message_matches)) in messages {
                                        let name = message.name;
                                        let mut message_header = CollapsingHeader::new(
                                            format!("Message ID: {message_id}\t({name})",),
                                        )
                                        .default_open(true)
                                        .id_salt(ui.make_persistent_id(format!(
                                            "message_{system_id}_{component_id}_{message_id}"
                                        )));

                                        if self.expand_all {
                                            message_header = message_header.open(Some(true));
                                        } else if self.collapse_all {
                                            message_header = message_header.open(Some(false));
                                        } else if !search_query.is_empty() {
                                            message_header = message_header.open(Some(true));
                                        }

                                        message_header.show(ui, |ui| {
                                            let id_salt = ui.make_persistent_id(format!(
                                                "messages_table_{system_id}_{component_id}_{message_id}"
                                            ));
                                            TableBuilder::new(ui)
                                                .id_salt(id_salt)
                                                .striped(true)
                                                .column(Column::auto().at_least(100.))
                                                .column(Column::remainder().at_least(150.))
                                                .body(|mut body| {
                                                    for (field_name, field_info) in &matching_fields {
                                                        crate::app::add_row_with_graph(
                                                            &mut body,
                                                            field_info,
                                                            field_name,
                                                        );
                                                    }

                                                    crate::app::add_last_update_row(
                                                        &mut body,
                                                        self.now,
                                                        message.last_sample_time.timestamp_micros(),
                                                    );
                                                });
                                        });
                                    }
                                });
                            }
                        });
                }
            }
        });
    }
}
