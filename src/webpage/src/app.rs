use std::collections::BTreeMap;

use chrono::prelude::*;
use eframe::egui::{CollapsingHeader, Context};
use egui_plot::{Line, Plot, PlotPoints};
use ewebsock::{connect, WsReceiver, WsSender};
use humantime::format_duration;
use url::Url;
use web_sys::window;

pub struct App {
    receiver: WsReceiver,
    sender: WsSender,
    vehicles: BTreeMap<u8, BTreeMap<u8, BTreeMap<String, MessageInfo>>>,
    search_query: String,
    collapse_all: bool,
    expand_all: bool,
}

#[derive(Clone)]
struct MessageInfo {
    last_sample_time: DateTime<chrono::Utc>,
    fields: BTreeMap<String, FieldInfo>,
}

#[derive(Clone)]
struct FieldInfo {
    history: Vec<(DateTime<Utc>, f64)>,
    latest_value: f64,
}

impl App {
    pub fn new() -> Self {
        let location = window().unwrap().location();
        let host = location.host().unwrap();
        let protocol = if location.protocol().unwrap() == "https:" {
            "wss:"
        } else {
            "ws:"
        };

        let url = format!("{}//{}/rest/ws", protocol, host);

        let (sender, receiver) = {
            let url = Url::parse(&url)
                .unwrap()
                .to_string();
            connect(url, ewebsock::Options::default()).expect("Can't connect")
        };

        Self {
            receiver,
            sender,
            vehicles: Default::default(),
            search_query: String::new(),
            collapse_all: false,
            expand_all: false,
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        let now = Utc::now();
        eframe::egui::CentralPanel::default().show(ctx, |ui| {
            // Heading and search bar with buttons
            ui.horizontal(|ui| {
                ui.heading("Search:");
                ui.add(
                    eframe::egui::TextEdit::singleline(&mut self.search_query)
                        .hint_text("Search..."),
                );
                if ui.button("Clear").clicked() {
                    self.search_query.clear();
                }
                if ui.button("Collapse All").clicked() {
                    self.collapse_all = true;
                    self.expand_all = false;
                }
                if ui.button("Expand All").clicked() {
                    self.expand_all = true;
                    self.collapse_all = false;
                }
            });

            // Process incoming messages
            while let Some(ewebsock::WsEvent::Message(ewebsock::WsMessage::Text(message))) = self.receiver.try_recv() {
                let Ok(message_json) = serde_json::from_str::<serde_json::Value>(&message)
                else {
                    continue;
                };
                let Some(system_id) = message_json["header"]["system_id"]
                    .as_u64()
                    .map(|n| n as u8)
                else {
                    continue;
                };
                let Some(component_id) = message_json["header"]["component_id"]
                    .as_u64()
                    .map(|n| n as u8)
                else {
                    continue;
                };
                let Some(message_name) = message_json["message"]["type"]
                    .as_str()
                    .map(|s| s.to_string())
                else {
                    continue;
                };
                self.vehicles
                    .entry(system_id)
                    .or_default()
                    .entry(component_id)
                    .or_default();
                let Some(vehicle) = self.vehicles.get_mut(&system_id) else {
                    continue;
                };
                let Some(messages) = vehicle.get_mut(&component_id) else {
                    continue;
                };
                let message_info =
                    messages.entry(message_name).or_insert_with(|| MessageInfo {
                        last_sample_time: now,
                        fields: Default::default(),
                    });
                message_info.last_sample_time = now;
                let Some(fields) = message_json["message"].as_object() else {
                    continue;
                };
                for (field_name, value) in fields {
                    if field_name == "type" {
                        continue;
                    }
                    let Some(num) = extract_number(value) else {
                        continue;
                    };
                    let field_info = message_info
                        .fields
                        .entry(field_name.clone())
                        .or_insert_with(|| FieldInfo {
                            history: Vec::new(),
                            latest_value: num,
                        });
                    field_info.latest_value = num;
                    field_info.history.push((now, num));
                    if field_info.history.len() > 1000 {
                        field_info.history.remove(0);
                    }
                }
            }

            let search_query = self.search_query.to_lowercase();

            // Vertical scroll area for messages
            eframe::egui::ScrollArea::vertical().show(ui, |ui| {
                for (system_id, components) in &self.vehicles {
                    let mut matching_components = BTreeMap::new();

                    for (component_id, messages) in components {
                        let mut matching_messages = BTreeMap::new();

                        for (name, message) in messages {
                            let name_lower = name.to_lowercase();
                            let message_matches = search_query.is_empty()
                                || name_lower.contains(&search_query);

                            let mut matching_fields = BTreeMap::new();

                            if message_matches {
                                matching_fields.extend(
                                    message.fields.iter().map(|(k, v)| (k.clone(), v.clone())),
                                );
                            } else {
                                for (field_name, field_info) in &message.fields {
                                    let field_name_lower = field_name.to_lowercase();
                                    if field_name_lower.contains(&search_query) {
                                        matching_fields
                                            .insert(field_name.clone(), field_info.clone());
                                    }
                                }
                            }

                            if message_matches || !matching_fields.is_empty() {
                                matching_messages.insert(
                                    name.clone(),
                                    (message.clone(), matching_fields, message_matches),
                                );
                            }
                        }

                        if !matching_messages.is_empty() {
                            matching_components.insert(*component_id, matching_messages);
                        }
                    }

                    if !matching_components.is_empty() {
                        let vehicle_id = ui.make_persistent_id(format!("vehicle_{}", system_id));
                        let vehicle_header = CollapsingHeader::new(format!("Vehicle {system_id}"))
                            .id_salt(vehicle_id)
                            .default_open(true);

                        vehicle_header.show(ui, |ui| {
                            for (component_id, messages) in matching_components {
                                let component_id_str = format!("component_{}_{}", system_id, component_id);
                                let component_id_hash = ui.make_persistent_id(&component_id_str);
                                let component_header = CollapsingHeader::new(format!(
                                    "Component {component_id}"
                                ))
                                .id_salt(component_id_hash)
                                .default_open(true);

                                component_header.show(ui, |ui| {
                                    for (
                                        name,
                                        (message, matching_fields, _message_matches),
                                    ) in messages
                                    {
                                        let message_id_str =
                                            format!("message_{}_{}_{}", system_id, component_id, name);
                                        let message_id_hash =
                                            ui.make_persistent_id(&message_id_str);
                                        let mut message_header = CollapsingHeader::new(name)
                                            .id_salt(message_id_hash);

                                        if self.expand_all {
                                            message_header = message_header.open(Some(true));
                                        } else if self.collapse_all {
                                            message_header = message_header.open(Some(false));
                                        } else if !search_query.is_empty() {
                                            message_header = message_header.open(Some(true));
                                        }

                                        message_header.show(ui, |ui| {
                                            for (field_name, field_info) in &matching_fields {
                                                let field_value_str = format!(
                                                    "{}: {}",
                                                    field_name, field_info.latest_value
                                                );
                                                let label = ui.label(field_value_str);

                                                if label.hovered() {
                                                    eframe::egui::show_tooltip(
                                                        ui.ctx(),
                                                        ui.layer_id(),
                                                        ui.id(),
                                                        |ui| {
                                                            let points: PlotPoints = field_info
                                                                .history
                                                                .iter()
                                                                .map(|(time, value)| {
                                                                    let timestamp = time
                                                                        .timestamp_millis()
                                                                        as f64;
                                                                    [timestamp, *value]
                                                                })
                                                                .collect();

                                                            let line = Line::new(points)
                                                                .name(field_name.clone());

                                                            Plot::new(field_name.clone())
                                                                .view_aspect(2.0)
                                                                .x_axis_formatter(|x, _range| {
                                                                    let datetime = DateTime::from_timestamp_millis(
                                                                        x.value as i64,
                                                                    );
                                                                    if let Some(dt) = datetime {
                                                                        dt.format("%H:%M:%S")
                                                                            .to_string()
                                                                    } else {
                                                                        "".to_string()
                                                                    }
                                                                })
                                                                .label_formatter(|name, value| {
                                                                    let datetime = DateTime::from_timestamp_millis(
                                                                        value.x as i64,
                                                                    );
                                                                    if let Some(dt) = datetime {
                                                                        format!(
                                                                            "{}: {:.2}\nTime: {}",
                                                                            name,
                                                                            value.y,
                                                                            dt.format("%H:%M:%S")
                                                                        )
                                                                    } else {
                                                                        format!(
                                                                            "{}: {:.2}",
                                                                            name, value.y
                                                                        )
                                                                    }
                                                                })
                                                                .show(ui, |plot_ui| {
                                                                    plot_ui.line(line);
                                                                });
                                                        },
                                                    );
                                                }
                                            }
                                            ui.label(
                                                format_duration(
                                                    (now - message.last_sample_time)
                                                        .to_std()
                                                        .unwrap(),
                                                )
                                                .to_string()
                                                    + " Ago",
                                            );
                                        });
                                    }
                                });
                            }
                        });
                    }
                }
            });

            // Reset collapse and expand flags
            if self.expand_all || self.collapse_all {
                self.expand_all = false;
                self.collapse_all = false;
            }

            ctx.request_repaint();
        });
    }
}

fn extract_number(value: &serde_json::Value) -> Option<f64> {
    if let Some(num) = value.as_f64() {
        Some(num)
    } else if let Some(num) = value.as_i64() {
        Some(num as f64)
    } else if let Some(num) = value.as_u64() {
        Some(num as f64)
    } else {
        None
    }
}
