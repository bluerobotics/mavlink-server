use std::collections::BTreeMap;

use chrono::prelude::*;
use eframe::egui::{CollapsingHeader, Context};
use egui::Ui;
use egui_extras::{Column, TableBody, TableBuilder};
use egui_plot::{Line, Plot, PlotPoints};
use ewebsock::{connect, WsReceiver, WsSender};
use humantime::format_duration;
use ringbuffer::RingBuffer;
use url::Url;
use web_sys::window;

use crate::{
    messages::{FieldInfo, MessageInfo, VehiclesMessages},
    stats::{
        drivers_stats::{DriversStatsHistorical, DriversStatsSample},
        hub_messages_stats::{HubMessagesStatsHistorical, HubMessagesStatsSample},
        hub_stats::{HubStatsHistorical, HubStatsSample},
        ByteStatsHistorical, DelayStatsHistorical, MessageStatsHistorical, StatsInner,
    },
};

pub struct App {
    now: DateTime<Utc>,
    mavlink_receiver: WsReceiver,
    _mavlink_sender: WsSender,
    hub_messages_stats_receiver: WsReceiver,
    _hub_messages_stats_sender: WsSender,
    hub_stats_receiver: WsReceiver,
    _hub_stats_sender: WsSender,
    drivers_stats_receiver: WsReceiver,
    _drivers_stats_sender: WsSender,
    /// Realtime messages, grouped by Vehicle ID and Component ID
    vehicles_mavlink: VehiclesMessages,
    /// Hub messages statsistics, groupbed by Vehicle ID and Component ID
    hub_messages_stats: HubMessagesStatsHistorical,
    /// Hub statistics
    hub_stats: HubStatsHistorical,
    /// Driver statistics
    drivers_stats: DriversStatsHistorical,
    search_query: String,
    collapse_all: bool,
    expand_all: bool,
}

impl Default for App {
    fn default() -> Self {
        let location = window().unwrap().location();
        let host = location.host().unwrap();
        let protocol = if location.protocol().unwrap() == "https:" {
            "wss:"
        } else {
            "ws:"
        };

        let url = format!("{protocol}//{host}/rest/ws");
        let (_mavlink_sender, mavlink_receiver) = {
            let url = Url::parse(&url).unwrap().to_string();
            connect(url, ewebsock::Options::default()).expect("Can't connect")
        };

        let url = format!("{protocol}//{host}/stats/messages/ws");
        let (_hub_messages_stats_sender, hub_messages_stats_receiver) = {
            let url = Url::parse(&url).unwrap().to_string();
            connect(url, ewebsock::Options::default()).expect("Can't connect")
        };

        let url = format!("{protocol}//{host}/stats/hub/ws");
        let (_hub_stats_sender, hub_stats_receiver) = {
            let url = Url::parse(&url).unwrap().to_string();
            connect(url, ewebsock::Options::default()).expect("Can't connect")
        };

        let url = format!("{protocol}//{host}/stats/drivers/ws");
        let (_drivers_stats_sender, drivers_stats_receiver) = {
            let url = Url::parse(&url).unwrap().to_string();
            connect(url, ewebsock::Options::default()).expect("Can't connect")
        };

        Self {
            now: Utc::now(),
            mavlink_receiver,
            _mavlink_sender,
            hub_messages_stats_receiver,
            _hub_messages_stats_sender,
            hub_stats_receiver,
            _hub_stats_sender,
            drivers_stats_receiver,
            _drivers_stats_sender,
            vehicles_mavlink: Default::default(),
            hub_messages_stats: Default::default(),
            hub_stats: Default::default(),
            drivers_stats: Default::default(),
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

    fn process_mavlink_websocket(&mut self) {
        while let Some(ewebsock::WsEvent::Message(ewebsock::WsMessage::Text(message))) =
            self.mavlink_receiver.try_recv()
        {
            let Ok(message_json) = serde_json::from_str::<serde_json::Value>(&message) else {
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
            self.vehicles_mavlink
                .entry(system_id)
                .or_default()
                .entry(component_id)
                .or_default();
            let Some(vehicle) = self.vehicles_mavlink.get_mut(&system_id) else {
                continue;
            };
            let Some(messages) = vehicle.get_mut(&component_id) else {
                continue;
            };
            let message_info = messages.entry(message_name).or_insert_with(|| MessageInfo {
                last_sample_time: self.now,
                fields: Default::default(),
            });
            message_info.last_sample_time = self.now;
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
                let new_entry = (self.now, num);
                message_info
                    .fields
                    .entry(field_name.clone())
                    .and_modify(|field| {
                        field.history.push(new_entry);
                    })
                    .or_insert_with(|| {
                        let mut field_info = FieldInfo::default();
                        field_info.history.push(new_entry);
                        field_info
                    });
            }
        }
    }

    fn process_hub_messages_stats_websocket(&mut self) {
        while let Some(ewebsock::WsEvent::Message(ewebsock::WsMessage::Text(message))) =
            self.hub_messages_stats_receiver.try_recv()
        {
            // Parse the JSON message
            let hub_messages_stats_sample = match serde_json::from_str::<HubMessagesStatsSample>(
                &message,
            ) {
                Ok(stats) => stats,
                Err(error) => {
                    log::error!(
                        "Failed to parse Hub Messages Stats message: {error:?}. Message: {message:#?}"
                    );
                    continue;
                }
            };

            self.hub_messages_stats.update(hub_messages_stats_sample)
        }
    }

    fn process_hub_stats_websocket(&mut self) {
        while let Some(ewebsock::WsEvent::Message(ewebsock::WsMessage::Text(message))) =
            self.hub_stats_receiver.try_recv()
        {
            // Parse the JSON message
            let hub_stats_sample = match serde_json::from_str::<HubStatsSample>(&message) {
                Ok(stats) => stats,
                Err(error) => {
                    log::error!(
                        "Failed to parse Hub Stats message: {error:?}. Message: {message:#?}"
                    );
                    continue;
                }
            };

            self.hub_stats.update(hub_stats_sample)
        }
    }

    fn process_drivers_stats_websocket(&mut self) {
        while let Some(ewebsock::WsEvent::Message(ewebsock::WsMessage::Text(message))) =
            self.drivers_stats_receiver.try_recv()
        {
            // Parse the JSON message
            let drivers_stats_sample = match serde_json::from_str::<DriversStatsSample>(&message) {
                Ok(stats) => stats,
                Err(error) => {
                    log::error!(
                        "Failed to parse Drivers Stats message: {error:?}. Message: {message:#?}"
                    );
                    continue;
                }
            };

            self.drivers_stats.update(drivers_stats_sample)
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
                    CollapsingHeader::new(format!("Vehicle {system_id}"))
                        .id_salt(ui.make_persistent_id(format!("vehicle_{system_id}")))
                        .default_open(true)
                        .show(ui, |ui| {
                            for (component_id, messages) in matching_components {
                                CollapsingHeader::new(format!("Component {component_id}"))
                                    .id_salt(ui.make_persistent_id(format!(
                                        "component_{system_id}_{component_id}"
                                    )))
                                    .default_open(true)
                                    .show(ui, |ui| {
                                        for (name, (message, matching_fields, _message_matches)) in
                                            messages
                                        {
                                            let mut message_header = CollapsingHeader::new(&name)
                                                .id_salt(ui.make_persistent_id(format!(
                                                    "message_{system_id}_{component_id}_{name}"
                                                )));

                                            if self.expand_all {
                                                message_header = message_header.open(Some(true));
                                            } else if self.collapse_all {
                                                message_header = message_header.open(Some(false));
                                            } else if !search_query.is_empty() {
                                                message_header = message_header.open(Some(true));
                                            }

                                            message_header.show(ui, |ui| {
                                                TableBuilder::new(ui)
                                                    .id_salt("hub_stats_table")
                                                    .striped(true)
                                                    .column(Column::auto().at_least(100.))
                                                    .column(Column::remainder())
                                                    .body(|mut body| {
                                                        for (field_name, field_info) in
                                                            &matching_fields
                                                        {
                                                            add_row_with_graph(
                                                                &mut body, field_info, field_name,
                                                            );
                                                        }

                                                        add_last_update_row(
                                                            &mut body,
                                                            self.now,
                                                            message
                                                                .last_sample_time
                                                                .timestamp_micros(),
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

    fn create_hub_messages_stats_ui(&mut self, ui: &mut eframe::egui::Ui) {
        eframe::egui::ScrollArea::vertical()
            .id_salt("scroll_messages_stats")
            .show(ui, |ui| {
                for (system_id, components) in &self.hub_messages_stats.systems_messages_stats {
                    CollapsingHeader::new(format!("Vehicle ID: {system_id}"))
                        .id_salt(ui.make_persistent_id(format!("vehicle_{system_id}")))
                        .default_open(true)
                        .show(ui, |ui| {
                            for (component_id, messages) in &components.components_messages_stats {
                                CollapsingHeader::new(format!("Component ID: {component_id}"))
                                    .id_salt(ui.make_persistent_id(format!(
                                        "component_{system_id}_{component_id}"
                                    )))
                                    .default_open(true)
                                    .show(ui, |ui| {
                                        for (message_id, message_stats) in &messages.messages_stats
                                        {
                                            CollapsingHeader::new(format!(
                                                "Message ID: {message_id}"
                                            ))
                                            .id_salt(ui.make_persistent_id(format!(
                                                "message_{system_id}_{component_id}_{message_id}"
                                            )))
                                            .default_open(true)
                                            .show(
                                                ui,
                                                |ui: &mut Ui| {
                                                    TableBuilder::new(ui)
                                                        .id_salt("hub_stats_table")
                                                        .striped(true)
                                                        .column(Column::auto().at_least(100.))
                                                        .column(Column::remainder())
                                                        .body(|mut body| {
                                                            add_label_and_plot_all_stats(
                                                                &mut body,
                                                                self.now,
                                                                message_stats,
                                                            );
                                                        });
                                                },
                                            );
                                        }
                                    });
                            }
                        });
                }
            });
    }

    fn create_hub_stats_ui(&mut self, ui: &mut eframe::egui::Ui) {
        eframe::egui::ScrollArea::vertical()
            .id_salt("scroll_hub_stats")
            .show(ui, |ui| {
                let hub_stats = &self.hub_stats.stats;

                CollapsingHeader::new("Hub Stats")
                    .id_salt(ui.make_persistent_id("hub_stats"))
                    .default_open(true)
                    .show(ui, |ui| {
                        TableBuilder::new(ui)
                            .id_salt("hub_stats_table")
                            .striped(true)
                            .column(Column::auto().at_least(100.))
                            .column(Column::remainder())
                            .body(|mut body| {
                                add_label_and_plot_all_stats(&mut body, self.now, hub_stats);
                            });
                    });
            });
    }

    fn create_drivers_stats_ui(&mut self, ui: &mut eframe::egui::Ui) {
        eframe::egui::ScrollArea::vertical()
            .id_salt("scroll_drivers_stats")
            .show(ui, |ui| {
                let drivers_stats = &self.drivers_stats.drivers_stats;

                for (driver_uuid, driver_stats) in drivers_stats {
                    let driver_name = &driver_stats.name;
                    let driver_type = &driver_stats.driver_type;

                    let drivers_stats_id_str = format!("driver_stats_{driver_uuid}");
                    let drivers_stats_id_hash = ui.make_persistent_id(&drivers_stats_id_str);

                    CollapsingHeader::new(format!("Driver Stats: {driver_name}"))
                        .id_salt(drivers_stats_id_hash)
                        .default_open(true)
                        .show(ui, |ui| {
                            let stats = &driver_stats.stats;

                            TableBuilder::new(ui)
                                .id_salt(format!("driver_stats_info_table_{driver_uuid}"))
                                .striped(true)
                                .column(Column::auto().at_least(120.))
                                .column(Column::remainder())
                                .body(|mut body| {
                                    body.row(15., |mut row| {
                                        row.col(|ui| {
                                            ui.label("Name");
                                        });
                                        row.col(|ui| {
                                            ui.label(driver_name);
                                        });
                                    });

                                    body.row(15., |mut row| {
                                        row.col(|ui| {
                                            ui.label("Type");
                                        });
                                        row.col(|ui| {
                                            ui.label(driver_type);
                                        });
                                    });

                                    body.row(15., |mut row| {
                                        row.col(|ui| {
                                            ui.label("UUID");
                                        });
                                        row.col(|ui| {
                                            ui.label(driver_uuid.to_string());
                                        });
                                    });
                                });

                            CollapsingHeader::new("Input")
                                .id_salt(ui.make_persistent_id(format!(
                                    "driver_stats_input_{driver_uuid}"
                                )))
                                .default_open(true)
                                .show(ui, |ui| {
                                    if let Some(input_stats) = &stats.input {
                                        TableBuilder::new(ui)
                                            .id_salt(format!(
                                                "driver_stats_input_table_{driver_uuid}"
                                            ))
                                            .striped(true)
                                            .column(Column::auto().at_least(100.))
                                            .column(Column::remainder())
                                            .body(|mut body| {
                                                add_label_and_plot_all_stats(
                                                    &mut body,
                                                    self.now,
                                                    input_stats,
                                                );
                                            });
                                    }
                                });

                            CollapsingHeader::new("Output")
                                .id_salt(ui.make_persistent_id(format!(
                                    "driver_stats_output_{driver_uuid}"
                                )))
                                .default_open(true)
                                .show(ui, |ui| {
                                    if let Some(output_stats) = &stats.output {
                                        TableBuilder::new(ui)
                                            .id_salt(format!(
                                                "driver_stats_output_table_{driver_uuid}"
                                            ))
                                            .striped(true)
                                            .column(Column::auto().at_least(100.))
                                            .column(Column::remainder())
                                            .body(|mut body| {
                                                add_label_and_plot_all_stats(
                                                    &mut body,
                                                    self.now,
                                                    output_stats,
                                                );
                                            });
                                    }
                                });
                        });
                }
            });
    }
}

fn add_label_and_plot_all_stats(
    body: &mut TableBody<'_>,
    now: DateTime<Utc>,
    message_stats: &StatsInner<ByteStatsHistorical, MessageStatsHistorical, DelayStatsHistorical>,
) {
    // Messages stats
    add_row_with_graph(body, &message_stats.messages.total_messages, "Messages");
    add_row_with_graph(
        body,
        &message_stats.messages.messages_per_second,
        "Messages/s",
    );

    // Bytes stats
    add_row_with_graph(body, &message_stats.bytes.total_bytes, "Bytes");
    add_row_with_graph(body, &message_stats.bytes.bytes_per_second, "Bytes/s");
    add_row_with_graph(
        body,
        &message_stats.bytes.average_bytes_per_second,
        "Avg Bytes",
    );

    // Delay stats
    add_row_with_graph(body, &message_stats.delay_stats.delay, "Delay [us]");
    add_row_with_graph(body, &message_stats.delay_stats.jitter, "Jitter [us]");

    add_last_update_row(body, now, message_stats.last_message_time_us as i64);
}

fn add_last_update_row(body: &mut TableBody<'_>, now: DateTime<Utc>, last_message_time_us: i64) {
    body.row(15., |mut row| {
        row.col(|ui| {
            ui.label("Last Update");
        });
        row.col(|ui| {
            ui.label(
                format_duration(
                    (now - chrono::DateTime::from_timestamp_micros(last_message_time_us)
                        .unwrap_or_default())
                    .to_std()
                    .unwrap_or_default(),
                )
                .to_string()
                    + " Ago",
            );
        });
    });
}

fn add_row_with_graph<T>(body: &mut TableBody<'_>, field_info: &FieldInfo<T>, field_name: &str)
where
    f64: std::convert::From<T>,
    T: Copy + std::fmt::Display + std::fmt::Debug + Default,
{
    body.row(15., |mut row| {
        row.col(|ui| {
            ui.label(field_name);
        });
        row.col(|ui| {
            let label = ui.label(
                field_info
                    .history
                    .back()
                    .map(|(_time, value)| format!("{value:.2}"))
                    .unwrap_or("?".to_string()),
            );

            if label.hovered() {
                show_stats_tooltip(ui, field_info, field_name);
            };
        });
    });
}

impl eframe::App for App {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.now = Utc::now();
        self.process_mavlink_websocket();
        self.process_hub_messages_stats_websocket();
        self.process_hub_stats_websocket();
        self.process_drivers_stats_websocket();

        self.top_bar(ctx);

        eframe::egui::Window::new("Messages").show(ctx, |ui| {
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

            // Reset collapse and expand flags
            if self.expand_all || self.collapse_all {
                self.expand_all = false;
                self.collapse_all = false;
            }
        });

        eframe::egui::Window::new("Messages Statistics").show(ctx, |ui| {
            self.create_hub_messages_stats_ui(ui);
        });

        eframe::egui::Window::new("Hub Statistics").show(ctx, |ui| {
            self.create_hub_stats_ui(ui);
        });

        eframe::egui::Window::new("Drivers Statistics").show(ctx, |ui| {
            self.create_drivers_stats_ui(ui);
        });

        ctx.request_repaint();
    }
}

fn extract_number(value: &serde_json::Value) -> Option<f64> {
    if let Some(num) = value.as_f64() {
        Some(num)
    } else if let Some(num) = value.as_i64() {
        Some(num as f64)
    } else {
        value.as_u64().map(|num| num as f64)
    }
}

fn show_stats_tooltip<T>(ui: &mut eframe::egui::Ui, field_info: &FieldInfo<T>, field_name: &str)
where
    f64: std::convert::From<T>,
    T: Copy + std::fmt::Debug,
{
    eframe::egui::show_tooltip(ui.ctx(), ui.layer_id(), ui.id(), |ui| {
        let points: PlotPoints = field_info
            .history
            .iter()
            .map(|(time, value)| {
                let timestamp = time.timestamp_millis() as f64;
                [timestamp, f64::from(*value)]
            })
            .collect();

        let line = Line::new(points).name(field_name);

        Plot::new(field_name)
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
