use std::collections::BTreeMap;

use chrono::prelude::*;
use eframe::egui::{CollapsingHeader, Context};
use egui_plot::{Line, Plot, PlotPoints};
use ewebsock::{connect, WsReceiver, WsSender};
use humantime::format_duration;
use url::Url;
use web_sys::window;

use crate::{
    messages::{FieldInfo, MessageInfo, VehiclesMessages},
    stats::hub_messages_stats::HubMessagesStatsHistorical,
};

const MAVLINK_MESSAGES_WEBSOCKET_PATH: &str = "rest/ws";
const HUB_MESSAGES_STATS_WEBSOCKET_PATH: &str = "stats/messages/ws";

pub struct App {
    now: DateTime<Utc>,
    mavlink_receiver: WsReceiver,
    mavlink_sender: WsSender,
    hub_messages_stats_receiver: WsReceiver,
    hub_messages_stats_sender: WsSender,
    /// Realtime messages, grouped by Vehicle ID and Component ID
    vehicles_mavlink: VehiclesMessages,
    /// Hub messages statsistics, groupbed by Vehicle ID and Component ID
    hub_messages_stats: HubMessagesStatsHistorical,
    search_query: String,
    collapse_all: bool,
    expand_all: bool,
}

impl Default for App {
    fn default() -> Self {
        let (mavlink_sender, mavlink_receiver) =
            connect_websocket(MAVLINK_MESSAGES_WEBSOCKET_PATH).unwrap();

        let (hub_messages_stats_sender, hub_messages_stats_receiver) =
            connect_websocket(HUB_MESSAGES_STATS_WEBSOCKET_PATH).unwrap();

        Self {
            now: Utc::now(),
            mavlink_receiver,
            mavlink_sender,
            hub_messages_stats_receiver,
            hub_messages_stats_sender,
            vehicles_mavlink: Default::default(),
            hub_messages_stats: Default::default(),
            search_query: String::new(),
            collapse_all: false,
            expand_all: false,
        }
    }
}

impl App {
    fn top_bar(&mut self, ctx: &Context) {
        eframe::egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            eframe::egui::menu::bar(ui, |ui| {
                ui.label("MAVLink Server");
                ui.add_space(16.0);

                ui.with_layout(
                    eframe::egui::Layout::right_to_left(eframe::egui::Align::RIGHT),
                    |ui| {
                        eframe::egui::widgets::global_theme_preference_switch(ui);
                        ui.separator();
                    },
                );
            });
        });
    }

    fn deal_with_mavlink_message(&mut self, message: String) {
        let Ok(message_json) = serde_json::from_str::<serde_json::Value>(&message) else {
            return;
        };
        let Some(system_id) = message_json["header"]["system_id"]
            .as_u64()
            .map(|n| n as u8)
        else {
            return;
        };
        let Some(component_id) = message_json["header"]["component_id"]
            .as_u64()
            .map(|n| n as u8)
        else {
            return;
        };
        let Some(message_name) = message_json["message"]["type"]
            .as_str()
            .map(|s| s.to_string())
        else {
            return;
        };
        self.vehicles_mavlink
            .entry(system_id)
            .or_default()
            .entry(component_id)
            .or_default();
        let Some(vehicle) = self.vehicles_mavlink.get_mut(&system_id) else {
            return;
        };
        let Some(messages) = vehicle.get_mut(&component_id) else {
            return;
        };
        let message_info = messages.entry(message_name).or_insert_with(|| MessageInfo {
            last_sample_time: self.now,
            fields: Default::default(),
        });
        message_info.last_sample_time = self.now;
        let Some(fields) = message_json["message"].as_object() else {
            return;
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
            field_info.history.push((self.now, num));
            if field_info.history.len() > 1000 {
                field_info.history.remove(0);
            }
        }
    }

    fn process_mavlink_websocket(&mut self) {
        loop {
            match self.mavlink_receiver.try_recv() {
                Some(ewebsock::WsEvent::Message(ewebsock::WsMessage::Text(message))) => {
                    self.deal_with_mavlink_message(message)
                }
                Some(ewebsock::WsEvent::Closed) => {
                    log::error!("MAVLink WebSocket closed");
                    (self.mavlink_sender, self.mavlink_receiver) =
                        connect_websocket(MAVLINK_MESSAGES_WEBSOCKET_PATH).unwrap();
                    break;
                }
                Some(ewebsock::WsEvent::Error(message)) => {
                    log::error!("MAVLink WebSocket error: {}", message);
                    break;
                }
                Some(ewebsock::WsEvent::Opened) => {
                    log::info!("MAVLink WebSocket opened");
                }
                something @ Some(_) => {
                    log::warn!("Unexpected event: {:#?}", something);
                }
                None => break,
            }
        }
    }

    fn process_stats_websocket(&mut self) {
        while let Some(ewebsock::WsEvent::Message(ewebsock::WsMessage::Text(message))) =
            self.stats_receiver.try_recv()
        {
            // Parse the JSON message
            let root_json = match serde_json::from_str::<serde_json::Value>(&message) {
                Ok(json) => json,
                Err(e) => {
                    log::error!("Failed to parse JSON: {e}");
                    continue;
                }
            };

            // Extract "systems_messages_stats"
            let systems_messages_stats = match root_json.get("systems_messages_stats") {
                Some(value) => match value.as_object() {
                    Some(obj) => obj,
                    None => {
                        log::error!("'systems_messages_stats' is not an object");
                        continue;
                    }
                },
                None => {
                    log::error!("'systems_messages_stats' key not found");
                    continue;
                }
            };

            for (system_id_str, system_stats) in systems_messages_stats {
                let system_id = match system_id_str.parse::<u8>() {
                    Ok(id) => id,
                    Err(_) => {
                        log::error!("Invalid system_id: {system_id_str}");
                        continue;
                    }
                };

                let components_messages_stats = match system_stats.get("components_messages_stats")
                {
                    Some(value) => match value.as_object() {
                        Some(obj) => obj,
                        None => {
                            log::error!("'components_messages_stats' is not an object");
                            continue;
                        }
                    },
                    None => {
                        log::error!("'components_messages_stats' key not found");
                        continue;
                    }
                };

                for (component_id_str, component_stats) in components_messages_stats {
                    let component_id = match component_id_str.parse::<u8>() {
                        Ok(id) => id,
                        Err(_) => {
                            log::error!("Invalid component_id: {component_id_str}");
                            continue;
                        }
                    };

                    let messages_stats = match component_stats.get("messages_stats") {
                        Some(value) => match value.as_object() {
                            Some(obj) => obj,
                            None => {
                                log::error!("'messages_stats' is not an object");
                                continue;
                            }
                        },
                        None => {
                            log::error!("'messages_stats' key not found");
                            continue;
                        }
                    };

                    for (message_id_str, message_stats_json) in messages_stats {
                        let message_id = match message_id_str.parse::<u16>() {
                            Ok(id) => id,
                            Err(_) => {
                                log::error!("Invalid message_id: {message_id_str}");
                                continue;
                            }
                        };

                        let message_stats = parse_message_stats(message_stats_json);
                        self.messages_stats
                            .entry(system_id)
                            .or_default()
                            .entry(component_id)
                            .or_default()
                            .entry(message_id) // Now using u16
                            .and_modify(|existing| {
                                existing.update_with(&message_stats);
                            })
                            .or_insert(message_stats);
                    }
                }
            }
        }
    }

    fn create_messages_ui(&mut self, ui: &mut eframe::egui::Ui) {
        let search_query = self.search_query.to_lowercase();
        eframe::egui::ScrollArea::vertical().show(ui, |ui| {
            for (system_id, components) in &self.vehicles_mavlink {
                let mut matching_components = BTreeMap::new();

                for (component_id, messages) in components {
                    let mut matching_messages = BTreeMap::new();

                    for (name, message) in messages {
                        let name_lower = name.to_lowercase();
                        let message_matches =
                            search_query.is_empty() || name_lower.contains(&search_query);

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
                    let vehicle_id = ui.make_persistent_id(format!("vehicle_{system_id}"));
                    let vehicle_header = CollapsingHeader::new(format!("Vehicle {system_id}"))
                        .id_salt(vehicle_id)
                        .default_open(true);

                    vehicle_header.show(ui, |ui| {
                        for (component_id, messages) in matching_components {
                            let component_id_str = format!("component_{system_id}_{component_id}");
                            let component_id_hash = ui.make_persistent_id(&component_id_str);
                            let component_header =
                                CollapsingHeader::new(format!("Component {component_id}"))
                                    .id_salt(component_id_hash)
                                    .default_open(true);

                            component_header.show(ui, |ui| {
                                for (name, (message, matching_fields, _message_matches)) in messages
                                {
                                    let message_id_str =
                                        format!("message_{system_id}_{component_id}_{name}");
                                    let message_id_hash = ui.make_persistent_id(&message_id_str);
                                    let mut message_header =
                                        CollapsingHeader::new(name).id_salt(message_id_hash);

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
                                                "{field_name}: {}",
                                                field_info.latest_value
                                            );
                                            let label = ui.label(field_value_str);

                                            if label.hovered() {
                                                show_stats_tooltip(ui, field_info, field_name);
                                            }
                                        }
                                        ui.label(
                                            format_duration(
                                                (self.now - message.last_sample_time)
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
    }

    fn create_messages_stats_ui(&mut self, ui: &mut eframe::egui::Ui) {
        eframe::egui::ScrollArea::vertical()
            .id_salt("scroll_messages_stats")
            .show(ui, |ui| {
                for (system_id, components) in &self.messages_stats {
                    let vehicle_id = ui.make_persistent_id(format!("vehicle_{system_id}"));
                    let vehicle_header = CollapsingHeader::new(format!("Vehicle {system_id}"))
                        .id_salt(vehicle_id)
                        .default_open(true);
                    vehicle_header.show(ui, |ui| {
                        for (component_id, messages) in components {
                            let component_id_str = format!("component_{system_id}_{component_id}");
                            let component_id_hash = ui.make_persistent_id(&component_id_str);
                            let component_header =
                                CollapsingHeader::new(format!("Component {component_id}"))
                                    .id_salt(component_id_hash)
                                    .default_open(true);

                            component_header.show(ui, |ui| {
                                for (message_id, message_stats) in messages {
                                    let message_id_str =
                                        format!("message_{system_id}_{component_id}_{message_id}");
                                    let message_id_hash = ui.make_persistent_id(&message_id_str);
                                    let mut message_header =
                                        CollapsingHeader::new(format!("Message ID: {message_id}"))
                                            .id_salt(message_id_hash)
                                            .default_open(true);

                                    if self.expand_all {
                                        message_header = message_header.open(Some(true));
                                    } else if self.collapse_all {
                                        message_header = message_header.open(Some(false));
                                    }

                                    message_header.show(ui, |ui| {
                                        let label = ui
                                            .label(format!("Messages: {}", message_stats.messages));
                                        if label.hovered() {
                                            show_stats_tooltip(
                                                ui,
                                                &message_stats.fields["messages"],
                                                "Messages",
                                            );
                                        }
                                        let label =
                                            ui.label(format!("Bytes: {}", message_stats.bytes));
                                        if label.hovered() {
                                            show_stats_tooltip(
                                                ui,
                                                &message_stats.fields["bytes"],
                                                "Bytes",
                                            );
                                        }
                                        let label =
                                            ui.label(format!("Delay: {}", message_stats.delay));
                                        if label.hovered() {
                                            show_stats_tooltip(
                                                ui,
                                                &message_stats.fields["delay"],
                                                "Delay",
                                            );
                                        }

                                        if let Some(last_message) = &message_stats.last_message {
                                            ui.label(format!("Origin: {}", last_message.origin));
                                            ui.label(format!(
                                                "Timestamp: {}",
                                                last_message.timestamp
                                            ));
                                        }

                                        ui.label(
                                            format_duration(
                                                (self.now - message_stats.last_update_us)
                                                    .to_std()
                                                    .unwrap_or(std::time::Duration::from_secs(0)),
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
            });
    }
}

fn get_protocol() -> (String, String) {
    let location = window().unwrap().location();
    let host = location.host().unwrap();
    let protocol = if location.protocol().unwrap() == "https:" {
        "wss:"
    } else {
        "ws:"
    };
    (host, protocol.to_string())
}

fn connect_websocket(path: &str) -> Result<(WsSender, WsReceiver), String> {
    let (host, protocol) = get_protocol();

    let url = format!("{protocol}//{host}/{path}");

    let url = Url::parse(&url).unwrap();
    connect(url, ewebsock::Options::default())
}

impl eframe::App for App {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.now = Utc::now();
        self.process_mavlink_websocket();
        self.process_stats_websocket();

        self.top_bar(ctx);

        eframe::egui::SidePanel::left("left_panel").show(ctx, |ui| {
            ui.heading("Messages");
            ui.horizontal(|ui| {
                ui.label("Search:");
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

            self.create_messages_ui(ui);
            //self.create_messages_stats_ui(ui);
            // Reset collapse and expand flags
            if self.expand_all || self.collapse_all {
                self.expand_all = false;
                self.collapse_all = false;
            }
        });

        eframe::egui::CentralPanel::default().show(ctx, |ui| {
            self.create_messages_stats_ui(ui);
        });

        ctx.request_repaint();
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

fn show_stats_tooltip(ui: &mut eframe::egui::Ui, field_info: &FieldInfo, field_name: &str) {
    eframe::egui::show_tooltip(ui.ctx(), ui.layer_id(), ui.id(), |ui| {
        let points: PlotPoints = field_info
            .history
            .iter()
            .map(|(time, value)| {
                let timestamp = time.timestamp_millis() as f64;
                [timestamp, *value]
            })
            .collect();

        let line = Line::new(points).name(field_name.clone());

        Plot::new(field_name.clone())
            .view_aspect(2.0)
            .x_axis_formatter(|x, _range| {
                let datetime = DateTime::from_timestamp_millis(x.value as i64);
                if let Some(dt) = datetime {
                    dt.format("%H:%M:%S").to_string()
                } else {
                    "".to_string()
                }
            })
            .label_formatter(|name, value| {
                let datetime = DateTime::from_timestamp_millis(value.x as i64);
                if let Some(dt) = datetime {
                    format!("{name}: {:.2}\nTime: {}", value.y, dt.format("%H:%M:%S"))
                } else {
                    format!("{name}: {:.2}", value.y)
                }
            })
            .show(ui, |plot_ui| {
                plot_ui.line(line);
            });
    });
}
