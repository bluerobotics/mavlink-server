use std::{collections::BTreeMap, sync::Arc};

use chrono::prelude::*;
use eframe::egui::{CollapsingHeader, Context};
use egui::mutex::Mutex;
use egui_dock::{DockArea, DockState, NodeIndex, Style, TabViewer};
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

const MAVLINK_MESSAGES_WEBSOCKET_PATH: &str = "rest/ws";
const HUB_MESSAGES_STATS_WEBSOCKET_PATH: &str = "stats/messages/ws";
const HUB_STATS_WEBSOCKET_PATH: &str = "stats/hub/ws";
const DRIVERS_STATS_WEBSOCKET_PATH: &str = "stats/drivers/ws";

#[derive(Clone, PartialEq)]
enum Tab {
    MessagesInspector,
    HubStats,
    MessagesStats,
    DriversStats,
}

pub struct App {
    now: DateTime<Utc>,
    mavlink_receiver: WsReceiver,
    mavlink_sender: WsSender,
    hub_messages_stats_receiver: WsReceiver,
    hub_messages_stats_sender: WsSender,
    hub_stats_receiver: WsReceiver,
    hub_stats_sender: WsSender,
    drivers_stats_receiver: WsReceiver,
    drivers_stats_sender: WsSender,
    /// Realtime messages, grouped by Vehicle ID and Component ID
    vehicles_mavlink: VehiclesMessages,
    /// Hub messages statistics, grouped by Vehicle ID and Component ID
    hub_messages_stats: HubMessagesStatsHistorical,
    /// Hub statistics
    hub_stats: HubStatsHistorical,
    /// Driver statistics
    drivers_stats: DriversStatsHistorical,
    search_query: String,
    collapse_all: bool,
    expand_all: bool,
    stats_frequency: Arc<Mutex<f32>>,
    dock_state: Option<DockState<Tab>>,
}

impl Default for App {
    fn default() -> Self {
        let (mavlink_sender, mavlink_receiver) =
            connect_websocket(MAVLINK_MESSAGES_WEBSOCKET_PATH).unwrap();

        let stats_frequency = Arc::new(Mutex::new(1.));
        crate::stats::stats_frequency::stats_frequency(&stats_frequency);

        let (hub_messages_stats_sender, hub_messages_stats_receiver) =
            connect_websocket(HUB_MESSAGES_STATS_WEBSOCKET_PATH).unwrap();

        let (hub_stats_sender, hub_stats_receiver) =
            connect_websocket(HUB_STATS_WEBSOCKET_PATH).unwrap();

        let (drivers_stats_sender, drivers_stats_receiver) =
            connect_websocket(DRIVERS_STATS_WEBSOCKET_PATH).unwrap();

        let mut dock_state = DockState::new(vec![Tab::MessagesInspector]);

        let [left, right] =
            dock_state
                .main_surface_mut()
                .split_left(NodeIndex::root(), 0.3, vec![Tab::HubStats]);
        let [_, _] = dock_state
            .main_surface_mut()
            .split_below(left, 0.7, vec![Tab::MessagesStats]);
        let [_, _] = dock_state
            .main_surface_mut()
            .split_below(right, 0.5, vec![Tab::DriversStats]);

        Self {
            now: Utc::now(),
            mavlink_receiver,
            mavlink_sender,
            hub_messages_stats_receiver,
            hub_messages_stats_sender,
            hub_stats_receiver,
            hub_stats_sender,
            drivers_stats_receiver,
            drivers_stats_sender,
            vehicles_mavlink: Default::default(),
            hub_messages_stats: Default::default(),
            hub_stats: Default::default(),
            drivers_stats: Default::default(),
            search_query: String::new(),
            collapse_all: false,
            expand_all: false,
            stats_frequency,
            dock_state: Some(dock_state),
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

    fn process_mavlink_websocket(&mut self) {
        use ewebsock::{WsEvent, WsMessage};

        loop {
            match self.mavlink_receiver.try_recv() {
                Some(WsEvent::Message(WsMessage::Text(message))) => {
                    self.deal_with_mavlink_message(message)
                }
                Some(WsEvent::Closed) => {
                    log::error!("MAVLink WebSocket closed");
                    (self.mavlink_sender, self.mavlink_receiver) =
                        connect_websocket(MAVLINK_MESSAGES_WEBSOCKET_PATH).unwrap();

                    break;
                }
                Some(WsEvent::Error(message)) => {
                    log::error!("MAVLink WebSocket error: {message}");
                    (self.mavlink_sender, self.mavlink_receiver) =
                        connect_websocket(MAVLINK_MESSAGES_WEBSOCKET_PATH).unwrap();

                    break;
                }
                Some(WsEvent::Opened) => {
                    log::info!("MAVLink WebSocket opened");
                }
                something @ Some(_) => {
                    log::trace!("MAVLink WebSocket got an unexpected event: {something:#?}");
                }
                None => break,
            }
        }
    }

    fn process_hub_messages_stats_websocket(&mut self) {
        use ewebsock::{WsEvent, WsMessage};

        loop {
            match self.hub_messages_stats_receiver.try_recv() {
                Some(WsEvent::Message(WsMessage::Text(message))) => {
                    self.deal_with_hub_messages_stats_message(message)
                }
                Some(WsEvent::Closed) => {
                    log::error!("Hub Messages Stats WebSocket closed");
                    (
                        self.hub_messages_stats_sender,
                        self.hub_messages_stats_receiver,
                    ) = connect_websocket(HUB_MESSAGES_STATS_WEBSOCKET_PATH).unwrap();

                    break;
                }
                Some(WsEvent::Error(message)) => {
                    log::error!("Hub Messages Stats WebSocket error: {message}");
                    (
                        self.hub_messages_stats_sender,
                        self.hub_messages_stats_receiver,
                    ) = connect_websocket(HUB_MESSAGES_STATS_WEBSOCKET_PATH).unwrap();

                    break;
                }
                Some(WsEvent::Opened) => {
                    log::info!("Hub Messages Stats WebSocket opened");
                }
                something @ Some(_) => {
                    log::trace!(
                        "Hub Messages Stats WebSocket got an unexpected event: {something:#?}"
                    );
                }
                None => break,
            }
        }
    }

    fn deal_with_hub_messages_stats_message(&mut self, message: String) {
        let hub_messages_stats_sample =
            match serde_json::from_str::<HubMessagesStatsSample>(&message) {
                Ok(stats) => stats,
                Err(error) => {
                    log::error!(
                    "Failed to parse Hub Messages Stats message: {error:?}. Message: {message:#?}"
                );
                    return;
                }
            };

        self.hub_messages_stats.update(hub_messages_stats_sample)
    }

    fn process_hub_stats_websocket(&mut self) {
        use ewebsock::{WsEvent, WsMessage};

        loop {
            match self.hub_stats_receiver.try_recv() {
                Some(WsEvent::Message(WsMessage::Text(message))) => {
                    self.deal_with_hub_stats_message(message)
                }
                Some(WsEvent::Closed) => {
                    log::error!("Hub Stats WebSocket closed");
                    (self.hub_stats_sender, self.hub_stats_receiver) =
                        connect_websocket(HUB_STATS_WEBSOCKET_PATH).unwrap();

                    break;
                }
                Some(WsEvent::Error(message)) => {
                    log::error!("Hub Stats WebSocket error: {message}");
                    (self.hub_stats_sender, self.hub_stats_receiver) =
                        connect_websocket(HUB_STATS_WEBSOCKET_PATH).unwrap();

                    break;
                }
                Some(WsEvent::Opened) => {
                    log::info!("Hub Stats WebSocket opened");
                }
                something @ Some(_) => {
                    log::trace!("Hub Stats WebSocket got an unexpected event: {something:#?}");
                }
                None => break,
            }
        }
    }

    fn deal_with_hub_stats_message(&mut self, message: String) {
        let hub_stats_sample = match serde_json::from_str::<HubStatsSample>(&message) {
            Ok(stats) => stats,
            Err(error) => {
                log::error!("Failed to parse Hub Stats message: {error:?}. Message: {message:#?}");
                return;
            }
        };

        self.hub_stats.update(hub_stats_sample)
    }

    fn process_drivers_stats_websocket(&mut self) {
        use ewebsock::{WsEvent, WsMessage};

        loop {
            match self.drivers_stats_receiver.try_recv() {
                Some(WsEvent::Message(WsMessage::Text(message))) => {
                    self.deal_with_drivers_stats_message(message)
                }
                Some(WsEvent::Closed) => {
                    log::error!("Drivers Stats WebSocket closed");
                    (self.drivers_stats_sender, self.drivers_stats_receiver) =
                        connect_websocket(DRIVERS_STATS_WEBSOCKET_PATH).unwrap();

                    break;
                }
                Some(WsEvent::Error(message)) => {
                    log::error!("Drivers Stats WebSocket error: {message}");
                    (self.drivers_stats_sender, self.drivers_stats_receiver) =
                        connect_websocket(DRIVERS_STATS_WEBSOCKET_PATH).unwrap();

                    break;
                }
                Some(WsEvent::Opened) => {
                    log::info!("Drivers Stats WebSocket opened");
                }
                something @ Some(_) => {
                    log::trace!("Drivers Stats WebSocket got an unexpected event: {something:#?}");
                }
                None => break,
            }
        }
    }

    fn deal_with_drivers_stats_message(&mut self, message: String) {
        let drivers_stats_sample = match serde_json::from_str::<DriversStatsSample>(&message) {
            Ok(stats) => stats,
            Err(error) => {
                log::error!(
                    "Failed to parse Drivers Stats message: {error:?}. Message: {message:#?}"
                );
                return;
            }
        };

        self.drivers_stats.update(drivers_stats_sample)
    }

    fn create_messages_ui(&self, ui: &mut eframe::egui::Ui) {
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
                    let _ = CollapsingHeader::new(format!("Vehicle {system_id}"))
                        .id_salt(ui.make_persistent_id(format!("vehicle_{system_id}")))
                        .default_open(true)
                        .show(ui, |ui| {
                            for (component_id, messages) in matching_components {
                                let _ = CollapsingHeader::new(format!("Component {component_id}"))
                                    .id_salt(ui.make_persistent_id(format!(
                                        "component_{system_id}_{component_id}"
                                    )))
                                    .default_open(true)
                                    .show(ui, |ui| {
                                        for (name, (message, matching_fields, _message_matches)) in
                                            messages
                                        {
                                            let mut message_header = CollapsingHeader::new(&name)
                                                .default_open(true)
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
                                                    .column(Column::remainder().at_least(150.))
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

    fn create_hub_messages_stats_ui(&self, ui: &mut eframe::egui::Ui) {
        eframe::egui::ScrollArea::vertical()
            .id_salt("scroll_messages_stats")
            .show(ui, |ui| {
                for (system_id, components) in &self.hub_messages_stats.systems_messages_stats {
                    let _ = CollapsingHeader::new(format!("Vehicle ID: {system_id}"))
                        .id_salt(ui.make_persistent_id(format!("vehicle_{system_id}")))
                        .default_open(true)
                        .show(ui, |ui| {
                            for (component_id, messages) in &components.components_messages_stats {
                                let _ =
                                    CollapsingHeader::new(format!("Component ID: {component_id}"))
                                        .id_salt(ui.make_persistent_id(format!(
                                            "component_{system_id}_{component_id}"
                                        )))
                                        .default_open(true)
                                        .show(ui, |ui| {
                                            for (message_id, message_stats) in
                                                &messages.messages_stats
                                            {
                                                let _ = CollapsingHeader::new(format!(
                                                    "Message ID: {message_id}"
                                                ))
                                                .id_salt(ui.make_persistent_id(format!(
                                                "message_{system_id}_{component_id}_{message_id}"
                                            )))
                                                .default_open(true)
                                                .show(ui, |ui: &mut egui::Ui| {
                                                    TableBuilder::new(ui)
                                                        .id_salt("hub_stats_table")
                                                        .striped(true)
                                                        .column(Column::auto().at_least(100.))
                                                        .column(Column::remainder().at_least(150.))
                                                        .body(|mut body| {
                                                            add_label_and_plot_all_stats(
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

    fn create_hub_stats_ui(&self, ui: &mut eframe::egui::Ui) {
        eframe::egui::ScrollArea::vertical()
            .id_salt("scrollhub_stats")
            .auto_shrink(false)
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
                            .column(Column::remainder().at_least(150.))
                            .body(|mut body| {
                                add_label_and_plot_all_stats(&mut body, self.now, hub_stats);
                            });
                    });
            });
    }

    fn create_drivers_stats_ui(&self, ui: &mut eframe::egui::Ui) {
        eframe::egui::ScrollArea::vertical()
            .id_salt("scrolldrivers_stats")
            .show(ui, |ui| {
                let drivers_stats = &self.drivers_stats.drivers_stats;

                for (driver_uuid, driver_stats) in drivers_stats {
                    let driver_name = &driver_stats.name;
                    let driver_type = &driver_stats.driver_type;

                    let drivers_stats_id_str = format!("driver_stats_{driver_uuid}");
                    let drivers_stats_id_hash = ui.make_persistent_id(&drivers_stats_id_str);

                    let _ = CollapsingHeader::new(format!("Driver Stats: {driver_name}"))
                        .id_salt(drivers_stats_id_hash)
                        .default_open(true)
                        .show(ui, |ui| {
                            let stats = &driver_stats.stats;

                            TableBuilder::new(ui)
                                .id_salt(format!("driver_stats_info_table_{driver_uuid}"))
                                .striped(true)
                                .column(Column::auto().at_least(120.))
                                .column(Column::remainder().at_least(150.))
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

                            let _ = CollapsingHeader::new("Input")
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
                                            .column(Column::remainder().at_least(150.))
                                            .body(|mut body| {
                                                add_label_and_plot_all_stats(
                                                    &mut body,
                                                    self.now,
                                                    input_stats,
                                                );
                                            });
                                    }
                                });

                            let _ = CollapsingHeader::new("Output")
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
                                            .column(Column::remainder().at_least(150.))
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
    add_row_with_graph(
        body,
        &message_stats.messages.average_messages_per_second,
        "Avg Messages/s",
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
    add_row_with_graph(body, &message_stats.delay_stats.jitter, "Jitter [s]");

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
            let label = ui.label(field_name);

            if label.hovered() {
                show_stats_tooltip(ui, field_info, field_name);
            };
        });
        row.col(|ui| {
            let label = ui.label(
                field_info
                    .history
                    .back()
                    .map(|(_time, value)| value.to_string())
                    .unwrap_or("?".to_string()),
            );

            if label.hovered() {
                show_stats_tooltip(ui, field_info, field_name);
            };
        });
    });
}

//https://cdn-useast1.kapwing.com/static/templates/our-meme-template-full-9bbb8a21.webp
struct OurTabViewer<'a> {
    app: &'a mut App,
}

impl<'a> TabViewer for OurTabViewer<'a> {
    type Tab = Tab;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        match tab {
            Tab::MessagesInspector => "Messages Inspector".into(),
            Tab::HubStats => "Hub Stats".into(),
            Tab::MessagesStats => "Messages Stats".into(),
            Tab::DriversStats => "Drivers Stats".into(),
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        match tab {
            Tab::MessagesInspector => {
                ui.horizontal_top(|ui| {
                    ui.label("Search:");
                    ui.add(
                        eframe::egui::TextEdit::singleline(&mut self.app.search_query)
                            .hint_text("Search...")
                            .desired_width(100.),
                    );
                    if ui.button("Clear").clicked() {
                        self.app.search_query.clear();
                    }
                    if ui.button("Collapse All").clicked() {
                        self.app.collapse_all = true;
                        self.app.expand_all = false;
                    }
                    if ui.button("Expand All").clicked() {
                        self.app.expand_all = true;
                        self.app.collapse_all = false;
                    }
                });

                self.app.create_messages_ui(ui);

                // Reset collapse and expand flags
                if self.app.expand_all || self.app.collapse_all {
                    self.app.expand_all = false;
                    self.app.collapse_all = false;
                }
            }
            Tab::HubStats => {
                self.app.create_hub_stats_ui(ui);
            }
            Tab::MessagesStats => {
                self.app.create_hub_messages_stats_ui(ui);
            }
            Tab::DriversStats => {
                self.app.create_drivers_stats_ui(ui);
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.now = Utc::now();
        self.process_mavlink_websocket();
        self.process_hub_messages_stats_websocket();
        self.process_hub_stats_websocket();
        self.process_drivers_stats_websocket();

        self.top_bar(ctx);

        egui::SidePanel::left("left_menu")
            .show_separator_line(true)
            .min_width(150.)
            .show(ctx, |ui| {
                ui.vertical(|ui| {
                    let mut stats_frequency = self.stats_frequency.lock().to_owned();
                    ui.label("Stats Frequency");
                    if ui
                        .add(
                            egui::Slider::new(&mut stats_frequency, 0.1..=10.)
                                .suffix("Hz")
                                .fixed_decimals(1)
                                .step_by(0.1)
                                .logarithmic(true)
                                .trailing_fill(true),
                        )
                        .drag_stopped()
                    {
                        crate::stats::stats_frequency::set_stats_frequency(
                            &self.stats_frequency.clone(),
                            stats_frequency,
                        );
                    }
                });
            });

        if let Some(mut dock_state) = self.dock_state.take() {
            DockArea::new(&mut dock_state)
                .style(Style::from_egui(ctx.style().as_ref()))
                .show(ctx, &mut OurTabViewer { app: self });
            self.dock_state = Some(dock_state);
        }

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
