use std::sync::Arc;

use eframe::egui::{self, Context};
use egui::mutex::Mutex;
use ehttp::{Request, Response};

const MAVFTP_AVAILABLE: &str = "v1/mavftp/available";

#[derive(Debug, Clone, Default)]
struct FtpTarget {
    system_id: u8,
    component_id: u8,
    description: String,
}

#[derive(Debug, Clone)]
struct LogEntryUi {
    id: u16,
    time_utc: u32,
    size: u32,
}

#[derive(Debug, Clone)]
enum FetchState<T> {
    Idle,
    Loading,
    Done(T),
    Error(String),
}

impl<T> Default for FetchState<T> {
    fn default() -> Self {
        Self::Idle
    }
}

#[derive(Debug, Clone)]
struct ActiveLogDownload {
    download_id: String,
    filename: String,
    bytes_downloaded: u64,
    total_bytes: u64,
    status: String,
    error: Option<String>,
}

pub struct VehicleLogsTab {
    targets: Arc<Mutex<FetchState<Vec<FtpTarget>>>>,
    auto_select_first: Arc<Mutex<bool>>,
    selected_target: Option<FtpTarget>,
    logs: Arc<Mutex<FetchState<Vec<LogEntryUi>>>>,
    status_message: Arc<Mutex<Option<String>>>,
    active_download: Arc<Mutex<Option<ActiveLogDownload>>>,
    erase_confirm: bool,
    needs_log_fetch: bool,
}

impl Default for VehicleLogsTab {
    fn default() -> Self {
        Self {
            targets: Arc::new(Mutex::new(FetchState::Idle)),
            auto_select_first: Arc::new(Mutex::new(false)),
            selected_target: None,
            logs: Arc::new(Mutex::new(FetchState::Idle)),
            status_message: Arc::new(Mutex::new(None)),
            active_download: Arc::new(Mutex::new(None)),
            erase_confirm: false,
            needs_log_fetch: false,
        }
    }
}

impl VehicleLogsTab {
    fn fetch_targets(&self) {
        *self.targets.lock() = FetchState::Loading;
        let targets = self.targets.clone();
        let auto_select = self.auto_select_first.clone();
        ehttp::fetch(Request::get(format!("/{MAVFTP_AVAILABLE}")), move |res| {
            let mut t = targets.lock();
            match res {
                Ok(Response { bytes, .. }) => {
                    let text = String::from_utf8_lossy(&bytes);
                    match serde_json::from_str::<serde_json::Value>(&text) {
                        Ok(json) => {
                            let parsed: Vec<FtpTarget> = json
                                .as_array()
                                .unwrap_or(&vec![])
                                .iter()
                                .filter_map(|v| {
                                    Some(FtpTarget {
                                        system_id: v["system_id"].as_u64()? as u8,
                                        component_id: v["component_id"].as_u64()? as u8,
                                        description: v["description"]
                                            .as_str()
                                            .unwrap_or("")
                                            .to_string(),
                                    })
                                })
                                .collect();
                            *t = FetchState::Done(parsed);
                            *auto_select.lock() = true;
                        }
                        Err(e) => *t = FetchState::Error(format!("Parse error: {e}")),
                    }
                }
                Err(e) => *t = FetchState::Error(e),
            }
        });
    }

    fn fetch_logs(&self) {
        let Some(target) = &self.selected_target else {
            return;
        };
        *self.logs.lock() = FetchState::Loading;
        let logs = self.logs.clone();
        let url = format!(
            "/v1/vehicle_logs/{}/{}/list",
            target.system_id, target.component_id
        );
        ehttp::fetch(Request::get(url), move |res| {
            let mut l = logs.lock();
            match res {
                Ok(Response { bytes, status, .. }) => {
                    let text = String::from_utf8_lossy(&bytes);
                    if status != 200 {
                        *l = FetchState::Error(text.to_string());
                        return;
                    }
                    match serde_json::from_str::<serde_json::Value>(&text) {
                        Ok(json) => {
                            let mut parsed: Vec<LogEntryUi> = json
                                .as_array()
                                .unwrap_or(&vec![])
                                .iter()
                                .filter_map(|v| {
                                    Some(LogEntryUi {
                                        id: v["id"].as_u64()? as u16,
                                        time_utc: v["time_utc"].as_u64()? as u32,
                                        size: v["size"].as_u64()? as u32,
                                    })
                                })
                                .collect();
                            parsed.sort_by(|a, b| b.id.cmp(&a.id));
                            *l = FetchState::Done(parsed);
                        }
                        Err(err) => *l = FetchState::Error(format!("Parse error: {err}")),
                    }
                }
                Err(err) => *l = FetchState::Error(err),
            }
        });
    }

    fn start_download(&self, log_id: u16, file_size: u32) {
        let Some(target) = &self.selected_target else {
            return;
        };

        if self.active_download.lock().is_some() {
            *self.status_message.lock() = Some("A download is already in progress".to_string());
            return;
        }

        let url = format!(
            "/v1/vehicle_logs/{}/{}/download/start?id={}&size={}",
            target.system_id, target.component_id, log_id, file_size
        );
        let active_dl = self.active_download.clone();
        let status_msg = self.status_message.clone();

        let mut request = Request::get(url);
        request.method = "POST".to_string();

        ehttp::fetch(request, move |res| match res {
            Ok(Response { bytes, status, .. }) => {
                if status == 200 {
                    let text = String::from_utf8_lossy(&bytes);
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                        if let Some(id) = json["download_id"].as_str() {
                            *active_dl.lock() = Some(ActiveLogDownload {
                                download_id: id.to_string(),
                                filename: format!("log_{log_id}.bin"),
                                bytes_downloaded: 0,
                                total_bytes: file_size as u64,
                                status: "downloading".to_string(),
                                error: None,
                            });
                            return;
                        }
                    }
                    *status_msg.lock() = Some("Failed to parse download response".to_string());
                } else {
                    let text = String::from_utf8_lossy(&bytes);
                    *status_msg.lock() = Some(format!("Download start failed: {text}"));
                }
            }
            Err(e) => {
                *status_msg.lock() = Some(format!("Download start error: {e}"));
            }
        });
    }

    fn poll_download_progress(&self) {
        let dl = self.active_download.lock().clone();
        let Some(dl) = dl else {
            return;
        };
        if dl.status != "downloading" {
            return;
        }

        let url = format!("/v1/vehicle_logs/download/{}/progress", dl.download_id);
        let active_dl = self.active_download.clone();

        ehttp::fetch(Request::get(url), move |res| {
            if let Ok(Response { bytes, status, .. }) = res {
                if status == 200 {
                    let text = String::from_utf8_lossy(&bytes);
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                        let mut guard = active_dl.lock();
                        if let Some(ref mut dl) = *guard {
                            dl.bytes_downloaded = json["bytes_downloaded"].as_u64().unwrap_or(0);
                            dl.total_bytes = json["total_bytes"].as_u64().unwrap_or(0);
                            if let Some(s) = json["status"].as_str() {
                                dl.status = s.to_string();
                            }
                            dl.error = json["error"].as_str().map(|s| s.to_string());
                        }
                    }
                }
            }
        });
    }

    fn fetch_completed_download(&self) {
        let dl = self.active_download.lock().clone();
        let Some(dl) = dl else {
            return;
        };

        let url = format!("/v1/vehicle_logs/download/{}/data", dl.download_id);
        let active_dl = self.active_download.clone();
        let status_msg = self.status_message.clone();

        ehttp::fetch(Request::get(url), move |res| match res {
            Ok(Response { bytes, status, .. }) => {
                if status == 200 {
                    super::mavftp::trigger_browser_download(&dl.filename, &bytes);
                    *status_msg.lock() = Some(format!(
                        "Downloaded: {} ({})",
                        dl.filename,
                        format_size(bytes.len() as u64)
                    ));
                } else {
                    *status_msg.lock() = Some("Failed to fetch download data".to_string());
                }
                *active_dl.lock() = None;
            }
            Err(e) => {
                *status_msg.lock() = Some(format!("Fetch error: {e}"));
                *active_dl.lock() = None;
            }
        });
    }

    fn erase_logs(&self) {
        let Some(target) = &self.selected_target else {
            return;
        };
        let url = format!(
            "/v1/vehicle_logs/{}/{}/erase",
            target.system_id, target.component_id
        );
        let status_msg = self.status_message.clone();
        let logs = self.logs.clone();

        let mut request = Request::get(url);
        request.method = "POST".to_string();

        ehttp::fetch(request, move |res| match res {
            Ok(Response { status, bytes, .. }) => {
                if status == 200 {
                    *status_msg.lock() = Some("All logs erased".to_string());
                    *logs.lock() = FetchState::Done(Vec::new());
                } else {
                    let text = String::from_utf8_lossy(&bytes);
                    *status_msg.lock() = Some(format!("Erase failed: {text}"));
                }
            }
            Err(e) => {
                *status_msg.lock() = Some(format!("Erase error: {e}"));
            }
        });
    }

    fn render_download_bar(&self, ui: &mut egui::Ui) {
        let dl = self.active_download.lock().clone();
        let Some(dl) = dl else {
            return;
        };

        ui.separator();

        match dl.status.as_str() {
            "downloading" => {
                ui.label(format!("Downloading: {}", dl.filename));

                let fraction = if dl.total_bytes > 0 {
                    dl.bytes_downloaded as f32 / dl.total_bytes as f32
                } else {
                    0.0
                };
                let progress_text = format!(
                    "{} / {} ({:.1}%)",
                    format_size(dl.bytes_downloaded),
                    format_size(dl.total_bytes),
                    fraction * 100.0
                );
                ui.add(egui::ProgressBar::new(fraction).text(progress_text));

                self.poll_download_progress();
            }
            "complete" => {
                ui.horizontal(|ui| {
                    ui.label(format!(
                        "Download complete: {} ({})",
                        dl.filename,
                        format_size(dl.total_bytes)
                    ));
                });
                ui.add(egui::ProgressBar::new(1.0).text("Saving..."));
                self.fetch_completed_download();
            }
            "error" => {
                let err_msg = dl.error.as_deref().unwrap_or("Unknown error");
                ui.colored_label(
                    egui::Color32::RED,
                    format!("Download failed: {} - {}", dl.filename, err_msg),
                );
                if ui.button("Dismiss").clicked() {
                    *self.active_download.lock() = None;
                }
            }
            _ => {}
        }
    }

    pub fn show(&mut self, ctx: &Context) {
        {
            let targets = self.targets.lock();
            if matches!(*targets, FetchState::Idle) {
                drop(targets);
                self.fetch_targets();
            }
        }

        if *self.auto_select_first.lock() {
            *self.auto_select_first.lock() = false;
            if let FetchState::Done(ref targets) = *self.targets.lock() {
                if let Some(first) = targets.first() {
                    self.selected_target = Some(first.clone());
                    self.needs_log_fetch = true;
                }
            }
        }

        if self.needs_log_fetch && self.selected_target.is_some() {
            self.needs_log_fetch = false;
            self.fetch_logs();
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Target:");

                let targets_state = self.targets.lock().clone();
                match targets_state {
                    FetchState::Loading => {
                        ui.spinner();
                        ui.label("Loading targets...");
                    }
                    FetchState::Error(ref e) => {
                        ui.colored_label(egui::Color32::RED, format!("Error: {e}"));
                        if ui.button("Retry").clicked() {
                            self.fetch_targets();
                        }
                    }
                    FetchState::Done(ref targets) => {
                        if targets.is_empty() {
                            ui.label("No FTP-capable targets found");
                            if ui.button("⟳").on_hover_text("Refresh targets").clicked() {
                                self.fetch_targets();
                            }
                        } else {
                            let current_label = self
                                .selected_target
                                .as_ref()
                                .map(|t| {
                                    format!(
                                        "{}:{} - {}",
                                        t.system_id, t.component_id, t.description
                                    )
                                })
                                .unwrap_or_else(|| "Select target...".to_string());

                            egui::ComboBox::from_id_salt("log_target_selector")
                                .selected_text(&current_label)
                                .show_ui(ui, |ui| {
                                    for target in targets {
                                        let label = format!(
                                            "{}:{} - {}",
                                            target.system_id,
                                            target.component_id,
                                            target.description
                                        );
                                        if ui
                                            .selectable_label(
                                                self.selected_target
                                                    .as_ref()
                                                    .map(|t| {
                                                        t.system_id == target.system_id
                                                            && t.component_id == target.component_id
                                                    })
                                                    .unwrap_or(false),
                                                &label,
                                            )
                                            .clicked()
                                        {
                                            self.selected_target = Some(target.clone());
                                            self.needs_log_fetch = true;
                                        }
                                    }
                                });

                            if ui.button("⟳").on_hover_text("Refresh targets").clicked() {
                                self.fetch_targets();
                            }
                        }
                    }
                    FetchState::Idle => {}
                }
            });

            if self.selected_target.is_none() {
                return;
            }

            ui.separator();

            ui.horizontal(|ui| {
                if ui.button("⟳").on_hover_text("Refresh log list").clicked() {
                    self.fetch_logs();
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button(egui::RichText::new("Erase All Logs").color(egui::Color32::RED))
                        .clicked()
                    {
                        self.erase_confirm = true;
                    }
                });
            });

            let status_msg = self.status_message.lock().clone();
            if let Some(msg) = status_msg {
                ui.horizontal(|ui| {
                    ui.label(&msg);
                    if ui.button("x").clicked() {
                        *self.status_message.lock() = None;
                    }
                });
            }

            self.render_download_bar(ui);

            ui.separator();

            let logs_state = self.logs.lock().clone();
            let has_active_download = self.active_download.lock().is_some();

            match logs_state {
                FetchState::Idle => {}
                FetchState::Loading => {
                    ui.spinner();
                    ui.label("Loading logs...");
                }
                FetchState::Error(ref e) => {
                    ui.colored_label(egui::Color32::RED, format!("Error: {e}"));
                }
                FetchState::Done(ref logs) => {
                    if logs.is_empty() {
                        ui.label("No logs found on the vehicle.");
                    } else {
                        let id_salt = ui.make_persistent_id("vehicle_logs_table");
                        egui_extras::TableBuilder::new(ui)
                            .id_salt(id_salt)
                            .striped(true)
                            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                            .column(egui_extras::Column::auto().at_least(50.0))
                            .column(egui_extras::Column::remainder().at_least(200.0))
                            .column(egui_extras::Column::auto().at_least(100.0))
                            .column(egui_extras::Column::auto().at_least(80.0))
                            .header(22.0, |mut header| {
                                header.col(|ui| {
                                    ui.strong("ID");
                                });
                                header.col(|ui| {
                                    ui.strong("Date/Time");
                                });
                                header.col(|ui| {
                                    ui.strong("Size");
                                });
                                header.col(|ui| {
                                    ui.strong("Actions");
                                });
                            })
                            .body(|body| {
                                body.rows(22.0, logs.len(), |mut row| {
                                    let entry = &logs[row.index()];

                                    row.col(|ui| {
                                        ui.label(entry.id.to_string());
                                    });

                                    row.col(|ui| {
                                        ui.label(format_utc_timestamp(entry.time_utc));
                                    });

                                    row.col(|ui| {
                                        ui.label(format_size(entry.size as u64));
                                    });

                                    row.col(|ui| {
                                        let dl_btn = ui.add_enabled(
                                            !has_active_download,
                                            egui::Button::new("⬇"),
                                        );
                                        if dl_btn.on_hover_text("Download").clicked() {
                                            self.start_download(entry.id, entry.size);
                                        }
                                    });
                                });
                            });
                    }
                }
            }

            // Erase all confirmation dialog
            let mut should_erase = false;
            let mut should_cancel_erase = false;
            if self.erase_confirm {
                egui::Window::new("Erase All Logs?")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.label("This will permanently delete all logs on the vehicle.");
                        ui.label("This action cannot be undone.");
                        ui.horizontal(|ui| {
                            if ui.button("Cancel").clicked() {
                                should_cancel_erase = true;
                            }
                            if ui
                                .button(egui::RichText::new("Erase All").color(egui::Color32::RED))
                                .clicked()
                            {
                                should_erase = true;
                            }
                        });
                    });
            }
            if should_cancel_erase {
                self.erase_confirm = false;
            }
            if should_erase {
                self.erase_confirm = false;
                self.erase_logs();
            }
        });
    }
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

fn format_utc_timestamp(time_utc: u32) -> String {
    if time_utc == 0 {
        return "Unknown".to_string();
    }
    let secs = time_utc as i64;
    let dt = chrono::DateTime::from_timestamp(secs, 0);
    match dt {
        Some(dt) => dt.format("%Y-%m-%d %H:%M:%S").to_string(),
        None => format!("{time_utc}"),
    }
}
