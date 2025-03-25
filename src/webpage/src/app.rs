use std::{collections::BTreeMap, sync::Arc};

use chrono::prelude::*;
use eframe::egui::Context;
use egui::mutex::Mutex;
use egui_dock::{DockArea, DockState, NodeIndex, Style, TabViewer};
use egui_extras::TableBody;
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
    tabs::{control::ControlTab, helper::HelperTab},
    widgets::driver_stats::DriverStatsWidget,
    widgets::hub_stats::HubStatsWidget,
    widgets::message_inspector::MessageInspectorWidget,
    widgets::message_stats::MessageStatsWidget,
};

const MAVLINK_MESSAGES_WEBSOCKET_PATH: &str = "rest/ws";
const CONTROL_VEHICLES_WEBSOCKET_PATH: &str = "rest/vehicles/ws";
const CONTROL_PARAMETERS_WEBSOCKET_PATH: &str = "rest/vehicles/parameters/ws";
const HUB_MESSAGES_STATS_WEBSOCKET_PATH: &str = "stats/messages/ws";
const HUB_STATS_WEBSOCKET_PATH: &str = "stats/hub/ws";
const DRIVERS_STATS_WEBSOCKET_PATH: &str = "stats/drivers/ws";

enum Screens {
    Main,
    Helper,
    Control,
}

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
    vehicles_receiver: WsReceiver,
    /// We are not using this, but it's needed to keep the connection alive
    vehicles_sender: WsSender,
    hub_messages_stats_receiver: WsReceiver,
    hub_messages_stats_sender: WsSender,
    hub_stats_receiver: WsReceiver,
    hub_stats_sender: WsSender,
    drivers_stats_receiver: WsReceiver,
    drivers_stats_sender: WsSender,
    parameters_receiver: WsReceiver,
    parameters_sender: WsSender,
    /// Realtime messages, grouped by Vehicle ID and Component ID
    vehicles_mavlink: VehiclesMessages,
    vehicles: serde_json::Value,
    parameters: BTreeMap<String, serde_json::Value>,
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
    show_screen: Screens,
    control_tab: ControlTab,
    helper_tab: HelperTab,
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

        let (vehicles_sender, vehicles_receiver) =
            connect_websocket(CONTROL_VEHICLES_WEBSOCKET_PATH).unwrap();

        let (parameters_sender, parameters_receiver) =
            connect_websocket(CONTROL_PARAMETERS_WEBSOCKET_PATH).unwrap();

        let mut dock_state = DockState::new(vec![Tab::MessagesInspector]);

        let [left, right] =
            dock_state
                .main_surface_mut()
                .split_left(NodeIndex::root(), 0.3, vec![Tab::HubStats]);
        let [_, _] = dock_state
            .main_surface_mut()
            .split_right(left, 0.5, vec![Tab::MessagesStats]);
        let [_, _] =
            dock_state
                .main_surface_mut()
                .split_below(right, 0.25, vec![Tab::DriversStats]);

        Self {
            now: Utc::now(),
            mavlink_receiver,
            mavlink_sender,
            vehicles_receiver,
            vehicles_sender,
            hub_messages_stats_receiver,
            hub_messages_stats_sender,
            hub_stats_receiver,
            hub_stats_sender,
            drivers_stats_receiver,
            drivers_stats_sender,
            parameters_receiver,
            parameters_sender,
            vehicles_mavlink: Default::default(),
            vehicles: Default::default(),
            parameters: Default::default(),
            hub_messages_stats: Default::default(),
            hub_stats: Default::default(),
            drivers_stats: Default::default(),
            search_query: String::new(),
            collapse_all: false,
            expand_all: false,
            stats_frequency,
            dock_state: Some(dock_state),
            show_screen: Screens::Main,
            control_tab: Default::default(),
            helper_tab: Default::default(),
        }
    }
}

impl App {
    fn top_bar(&mut self, ctx: &Context) {
        eframe::egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            eframe::egui::menu::bar(ui, |ui| {
                if ui.button("MAVLink Server").clicked() {
                    self.show_screen = Screens::Main;
                }
                ui.add_space(16.0);
                if ui.button("Helper").clicked() {
                    self.show_screen = Screens::Helper;
                }
                ui.add_space(16.0);
                if ui.button("Control").clicked() {
                    self.show_screen = Screens::Control;
                }

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

    fn show_main_screen(&mut self, ctx: &Context) {
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
    }

    fn show_helper_screen(&mut self, ctx: &Context) {
        self.helper_tab.show(ctx);
    }

    fn show_control_screen(&mut self, ctx: &Context) {
        self.control_tab.show(ctx, &self.vehicles, &self.parameters);
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
        let Some(message_id) = message_json["header"]["message_id"]
            .as_u64()
            .map(|n| n as u32)
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
        let message_info = messages.entry(message_id).or_insert_with(|| MessageInfo {
            name: message_name,
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

    fn process_vehicles_websocket(&mut self) {
        use ewebsock::{WsEvent, WsMessage};

        loop {
            match self.vehicles_receiver.try_recv() {
                Some(WsEvent::Message(WsMessage::Text(message))) => {
                    self.deal_with_vehicles_message(message)
                }
                Some(WsEvent::Closed) => {
                    log::error!("Vehicles WebSocket closed");
                    (self.vehicles_sender, self.vehicles_receiver) =
                        connect_websocket(CONTROL_VEHICLES_WEBSOCKET_PATH).unwrap();

                    break;
                }
                Some(WsEvent::Error(message)) => {
                    log::error!("Vehicles WebSocket error: {message}");
                    (self.vehicles_sender, self.vehicles_receiver) =
                        connect_websocket(CONTROL_VEHICLES_WEBSOCKET_PATH).unwrap();

                    break;
                }
                Some(WsEvent::Opened) => {
                    log::info!("Vehicles WebSocket opened");
                }
                something @ Some(_) => {
                    log::trace!("Vehicles WebSocket got an unexpected event: {something:#?}");
                }
                None => break,
            }
        }
    }

    fn process_parameters_websocket(&mut self) {
        use ewebsock::{WsEvent, WsMessage};

        loop {
            match self.parameters_receiver.try_recv() {
                Some(WsEvent::Message(WsMessage::Text(message))) => {
                    self.deal_with_parameters_message(message)
                }
                Some(WsEvent::Closed) => {
                    log::error!("Parameters WebSocket closed");
                    (self.parameters_sender, self.parameters_receiver) =
                        connect_websocket(CONTROL_VEHICLES_WEBSOCKET_PATH).unwrap();

                    break;
                }
                Some(WsEvent::Error(message)) => {
                    log::error!("Parameters WebSocket error: {message}");
                    (self.parameters_sender, self.parameters_receiver) =
                        connect_websocket(CONTROL_PARAMETERS_WEBSOCKET_PATH).unwrap();

                    break;
                }
                Some(WsEvent::Opened) => {
                    log::info!("Parameters WebSocket opened");
                }
                something @ Some(_) => {
                    log::trace!("Parameters WebSocket got an unexpected event: {something:#?}");
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

    fn deal_with_vehicles_message(&mut self, message: String) {
        let Ok(message_json) = serde_json::from_str::<serde_json::Value>(&message) else {
            log::error!("Failed to parse Vehicles message: {message}");
            return;
        };
        self.vehicles = message_json;
    }

    fn deal_with_parameters_message(&mut self, message: String) {
        let Ok(message_json) = serde_json::from_str::<serde_json::Value>(&message) else {
            log::error!("Failed to parse Vehicles message: {message}");
            return;
        };

        // We only deal with a single vehicle for now
        if !message_json.as_object().unwrap().contains_key("1") {
            return;
        }
        let parameter = message_json["1"].as_object().unwrap();
        for (key, value) in parameter {
            self.parameters.insert(key.to_string(), value.clone());
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
        MessageInspectorWidget::new(
            self.now,
            &self.vehicles_mavlink,
            &self.search_query,
            self.collapse_all,
            self.expand_all,
        )
        .show(ui);
    }

    fn create_hub_messages_stats_ui(&self, ui: &mut eframe::egui::Ui) {
        MessageStatsWidget::new(self.now, &self.hub_messages_stats).show(ui);
    }

    fn create_hub_stats_ui(&self, ui: &mut eframe::egui::Ui) {
        HubStatsWidget::new(self.now, &self.hub_stats).show(ui);
    }

    fn create_drivers_stats_ui(&self, ui: &mut eframe::egui::Ui) {
        DriverStatsWidget::new(self.now, &self.drivers_stats).show(ui);
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

pub fn add_label_and_plot_all_stats(
    body: &mut TableBody<'_>,
    now: DateTime<Utc>,
    message_stats: &StatsInner<ByteStatsHistorical, MessageStatsHistorical, DelayStatsHistorical>,
) {
    // Messages stats
    add_row_with_graph(
        body,
        &message_stats.messages.total_messages,
        "Messages [Total]",
    );
    add_row_with_graph(
        body,
        &message_stats.messages.messages_per_second,
        "Inst. Messages [Msg/s]",
    );
    add_row_with_graph(
        body,
        &message_stats.messages.average_messages_per_second,
        "Messages [M/s]",
    );

    // Bytes stats
    add_row_with_graph(body, &message_stats.bytes.total_bytes, "Bytes [Total]");
    add_row_with_graph(
        body,
        &message_stats.bytes.bytes_per_second,
        "Inst. Bytes [B/s]",
    );
    add_row_with_graph(
        body,
        &message_stats.bytes.average_bytes_per_second,
        "Avg. Bytes [B/s]",
    );

    // Delay stats
    add_row_with_graph(body, &message_stats.delay_stats.delay, "Delay [us]");
    add_row_with_graph(body, &message_stats.delay_stats.jitter, "Int. Jitter [s]");

    add_last_update_row(body, now, message_stats.last_message_time_us as i64);
}

pub fn add_last_update_row(
    body: &mut TableBody<'_>,
    now: DateTime<Utc>,
    last_message_time_us: i64,
) {
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

pub fn add_row_with_graph<T>(body: &mut TableBody<'_>, field_info: &FieldInfo<T>, field_name: &str)
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
        self.process_vehicles_websocket();
        self.process_parameters_websocket();
        self.process_hub_messages_stats_websocket();
        self.process_hub_stats_websocket();
        self.process_drivers_stats_websocket();

        self.top_bar(ctx);

        match self.show_screen {
            Screens::Main => self.show_main_screen(ctx),
            Screens::Helper => self.show_helper_screen(ctx),
            Screens::Control => self.show_control_screen(ctx),
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
        let points: PlotPoints<'_> = field_info
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
