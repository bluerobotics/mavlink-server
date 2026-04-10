use std::{sync::Arc, time::Duration};

use eframe::egui::{self, Context};
use egui::mutex::Mutex;
use ehttp::{Request, Response};
use serde::de::DeserializeOwned;

const FTP_PATH: &str = "/v1/mavftp";
const MAX_UPLOAD_SIZE: usize = 10 * 1024 * 1024; // 10 MB
const DOWNLOAD_POLL_INTERVAL: Duration = Duration::from_millis(500);

pub struct MavFtpTab {
    targets: Arc<Mutex<FetchState<Vec<FtpTarget>>>>,
    auto_select_first: Arc<Mutex<bool>>,
    selected_target: Option<FtpTarget>,
    current_path: String,
    entries: Arc<Mutex<FetchState<Vec<DirEntry>>>>,
    status_message: Arc<Mutex<Option<String>>>,
    delete_confirm: Option<(DirEntryType, String)>,
    new_folder_name: Option<String>,
    active_download: Arc<Mutex<Option<ActiveDownload>>>,
    upload_in_progress: Arc<Mutex<bool>>,
    needs_refresh: Arc<Mutex<bool>>,
    download_poll_in_flight: Arc<Mutex<bool>>,
    last_download_poll_at: Arc<Mutex<Option<f64>>>,
    download_data_in_flight: Arc<Mutex<bool>>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct FtpTarget {
    system_id: u8,
    component_id: u8,
    #[serde(default)]
    description: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct DirEntry {
    #[serde(rename = "type")]
    entry_type: DirEntryType,
    name: String,
    size: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum DirEntryType {
    Directory,
    File,
    Skip,
}

#[derive(Debug, Clone, Default)]
enum FetchState<T> {
    #[default]
    Idle,
    Loading,
    Done(T),
    Error(String),
}

#[derive(Debug, Clone, serde::Deserialize)]
struct DownloadStartResponse {
    download_id: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct ActiveDownload {
    download_id: String,
    filename: String,
    bytes_downloaded: u64,
    total_bytes: u64,
    status: DownloadStatus,
    error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum DownloadStatus {
    Downloading,
    Complete,
    Error,
}

#[derive(Debug, Clone, Copy)]
struct TargetPath {
    system_id: u8,
    component_id: u8,
}

#[derive(Debug, Clone, Copy)]
struct DownloadPath<'a> {
    download_id: &'a str,
}

#[derive(Debug, Clone, Copy)]
struct PathQuery<'a> {
    path: &'a str,
}

#[derive(Debug, Clone, Copy)]
struct DownloadStartQuery<'a> {
    path: &'a str,
    size: u64,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct ErrorResponse {
    error: String,
    #[serde(default)]
    code: Option<u8>,
}

impl DirEntryType {
    fn delete_endpoint(self) -> Option<&'static str> {
        match self {
            Self::Directory => Some("dir"),
            Self::File => Some("file"),
            Self::Skip => None,
        }
    }

    fn display_name(self) -> &'static str {
        match self {
            Self::Directory => "directory",
            Self::File => "file",
            Self::Skip => "entry",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Directory => "📁",
            Self::File => "📄",
            Self::Skip => "⊘",
        }
    }

    fn sort_order(self) -> u8 {
        match self {
            Self::Directory => 0,
            Self::File => 1,
            Self::Skip => 2,
        }
    }
}

impl From<&FtpTarget> for TargetPath {
    fn from(target: &FtpTarget) -> Self {
        Self {
            system_id: target.system_id,
            component_id: target.component_id,
        }
    }
}

impl TargetPath {
    fn endpoint_url(self, endpoint: &str) -> String {
        format!(
            "{FTP_PATH}/{}/{}/{endpoint}",
            self.system_id, self.component_id
        )
    }

    fn path_url(self, endpoint: &str, query: PathQuery<'_>) -> String {
        format!("{}?{}", self.endpoint_url(endpoint), query.as_query())
    }

    fn download_start_url(self, query: DownloadStartQuery<'_>) -> String {
        format!(
            "{}?{}",
            self.endpoint_url("download/start"),
            query.as_query()
        )
    }
}

impl DownloadPath<'_> {
    fn progress_url(self) -> String {
        format!("{FTP_PATH}/download/{}/progress", self.download_id)
    }

    fn data_url(self) -> String {
        format!("{FTP_PATH}/download/{}/data", self.download_id)
    }
}

impl PathQuery<'_> {
    fn as_query(self) -> String {
        format!("path={}", encode_query_path(self.path))
    }
}

impl DownloadStartQuery<'_> {
    fn as_query(self) -> String {
        format!("path={}&size={}", encode_query_path(self.path), self.size)
    }
}

impl Default for MavFtpTab {
    fn default() -> Self {
        Self {
            targets: Arc::new(Mutex::new(FetchState::Idle)),
            auto_select_first: Arc::new(Mutex::new(false)),
            selected_target: None,
            current_path: "/".to_string(),
            entries: Arc::new(Mutex::new(FetchState::Idle)),
            status_message: Arc::new(Mutex::new(None)),
            delete_confirm: None,
            new_folder_name: None,
            active_download: Arc::new(Mutex::new(None)),
            upload_in_progress: Arc::new(Mutex::new(false)),
            needs_refresh: Arc::new(Mutex::new(false)),
            download_poll_in_flight: Arc::new(Mutex::new(false)),
            last_download_poll_at: Arc::new(Mutex::new(None)),
            download_data_in_flight: Arc::new(Mutex::new(false)),
        }
    }
}

impl MavFtpTab {
    fn fetch_targets(&self) {
        ehttp::fetch(Request::get(format!("{FTP_PATH}/available")), {
            *self.targets.lock() = FetchState::Loading;
            let targets = self.targets.clone();
            let auto_select = self.auto_select_first.clone();

            move |res| {
                let mut targets_guard = targets.lock();
                match res {
                    Ok(Response { bytes, status, .. }) => {
                        if status != 200 {
                            *targets_guard = FetchState::Error(response_error_text(status, &bytes));
                            return;
                        }

                        match parse_json_response::<Vec<FtpTarget>>(&bytes) {
                            Ok(parsed) => {
                                *targets_guard = FetchState::Done(parsed);
                                *auto_select.lock() = true;
                            }
                            Err(error) => *targets_guard = FetchState::Error(error),
                        }
                    }
                    Err(error) => *targets_guard = FetchState::Error(error),
                }
            }
        });
    }

    fn fetch_listing(&self) {
        let Some(target) = &self.selected_target else {
            return;
        };

        let url = TargetPath::from(target).path_url(
            "list",
            PathQuery {
                path: &self.current_path,
            },
        );

        ehttp::fetch(Request::get(url), {
            *self.entries.lock() = FetchState::Loading;
            let entries = self.entries.clone();

            move |res| {
                let mut entries_guard = entries.lock();
                match res {
                    Ok(Response { bytes, status, .. }) => {
                        if status != 200 {
                            *entries_guard = FetchState::Error(response_error_text(status, &bytes));
                            return;
                        }

                        match parse_json_response::<Vec<DirEntry>>(&bytes) {
                            Ok(mut parsed) => {
                                sort_dir_entries(&mut parsed);
                                *entries_guard = FetchState::Done(parsed);
                            }
                            Err(error) => *entries_guard = FetchState::Error(error),
                        }
                    }
                    Err(error) => *entries_guard = FetchState::Error(error),
                }
            }
        });
    }

    fn navigate_to(&mut self, path: String) {
        self.current_path = path;
        self.fetch_listing();
    }

    fn navigate_into(&mut self, dir_name: &str) {
        if dir_name == ".." {
            self.current_path = parent_path(&self.current_path);
        } else if dir_name == "." {
            // stay
        } else {
            self.current_path = join_path(&self.current_path, dir_name);
        }
        self.fetch_listing();
    }

    fn delete_entry(&self, entry_type: DirEntryType, name: &str) {
        let Some(target) = &self.selected_target else {
            return;
        };

        let Some(endpoint) = entry_type.delete_endpoint() else {
            return;
        };
        let full_path = join_path(&self.current_path, name);
        let url = TargetPath::from(target).path_url(endpoint, PathQuery { path: &full_path });

        let request = request_with_method("DELETE", url);

        ehttp::fetch(request, {
            let entries = self.entries.clone();
            let status_message = self.status_message.clone();
            let name = name.to_string();

            move |res| match res {
                Ok(Response { status, bytes, .. }) => {
                    if status == 200 {
                        *status_message.lock() = Some(format!("Deleted: {full_path}"));
                        if let FetchState::Done(ref mut list) = *entries.lock() {
                            list.retain(|e| e.name != name);
                        }
                    } else {
                        *status_message.lock() = Some(format!(
                            "Delete failed: {}",
                            response_error_text(status, &bytes)
                        ));
                    }
                }
                Err(error) => {
                    *status_message.lock() = Some(format!("Delete error: {error}"));
                }
            }
        });
    }

    fn start_download(&self, name: &str, file_size: u64) {
        let Some(target) = &self.selected_target else {
            return;
        };

        if self.active_download.lock().is_some() {
            *self.status_message.lock() = Some("A download is already in progress".to_string());
            return;
        }

        let full_path = join_path(&self.current_path, name);
        let url = TargetPath::from(target).download_start_url(DownloadStartQuery {
            path: &full_path,
            size: file_size,
        });

        let request = request_with_method("POST", url);

        ehttp::fetch(request, {
            let active_download = self.active_download.clone();
            let status_message = self.status_message.clone();
            let download_poll_in_flight = self.download_poll_in_flight.clone();
            let last_download_poll_at = self.last_download_poll_at.clone();
            let download_data_in_flight = self.download_data_in_flight.clone();
            let filename = name.to_string();

            move |res| match res {
                Ok(Response { bytes, status, .. }) => {
                    if status == 200 {
                        if let Ok(response) = parse_json_response::<DownloadStartResponse>(&bytes) {
                            *download_poll_in_flight.lock() = false;
                            *last_download_poll_at.lock() = None;
                            *download_data_in_flight.lock() = false;
                            *active_download.lock() = Some(ActiveDownload {
                                download_id: response.download_id,
                                filename,
                                bytes_downloaded: 0,
                                total_bytes: file_size,
                                status: DownloadStatus::Downloading,
                                error: None,
                            });
                            return;
                        }
                        *status_message.lock() =
                            Some("Failed to parse download response".to_string());
                    } else {
                        *status_message.lock() = Some(format!(
                            "Download start failed: {}",
                            response_error_text(status, &bytes)
                        ));
                    }
                }
                Err(error) => {
                    *status_message.lock() = Some(format!("Download start error: {error}"));
                }
            }
        });
    }

    fn start_upload(&self) {
        let Some(target) = &self.selected_target else {
            return;
        };

        if *self.upload_in_progress.lock() {
            *self.status_message.lock() = Some("An upload is already in progress".to_string());
            return;
        }

        wasm_bindgen_futures::spawn_local({
            let system_id = target.system_id;
            let component_id = target.component_id;
            let current_path = self.current_path.clone();
            let status_message = self.status_message.clone();
            let uploading = self.upload_in_progress.clone();
            let needs_refresh = self.needs_refresh.clone();

            *uploading.lock() = true;

            async move {
                let file = rfd::AsyncFileDialog::new().pick_file().await;
                let Some(file) = file else {
                    *uploading.lock() = false;
                    return;
                };

                let filename = file.file_name();
                let data = file.read().await;

                if data.len() > MAX_UPLOAD_SIZE {
                    *status_message.lock() = Some(format!(
                        "File too large: {} (max {})",
                        format_size(data.len() as u64),
                        format_size(MAX_UPLOAD_SIZE as u64)
                    ));
                    *uploading.lock() = false;
                    return;
                }

                *status_message.lock() = Some(format!(
                    "Uploading: {filename} ({})...",
                    format_size(data.len() as u64)
                ));

                let full_path = join_path(&current_path, &filename);
                let url = TargetPath {
                    system_id,
                    component_id,
                }
                .path_url("upload", PathQuery { path: &full_path });

                let mut request = ehttp::Request::post(&url, data);
                request
                    .headers
                    .insert("Content-Type", "application/octet-stream");

                ehttp::fetch(request, {
                    let fname = filename.clone();

                    move |res| {
                        match res {
                            Ok(Response { status, bytes, .. }) => {
                                if status == 200 {
                                    *status_message.lock() = Some(format!("Uploaded: {fname}"));
                                    *needs_refresh.lock() = true;
                                } else {
                                    *status_message.lock() = Some(format!(
                                        "Upload failed: {}",
                                        response_error_text(status, &bytes)
                                    ));
                                }
                            }
                            Err(error) => {
                                *status_message.lock() = Some(format!("Upload error: {error}"));
                            }
                        }
                        *uploading.lock() = false;
                    }
                });
            }
        });
    }

    fn create_folder(&self, name: &str) {
        let Some(target) = &self.selected_target else {
            return;
        };

        let full_path = join_path(&self.current_path, name);
        let url = TargetPath::from(target).path_url("mkdir", PathQuery { path: &full_path });

        let request = request_with_method("POST", url);

        ehttp::fetch(request, {
            let status_message = self.status_message.clone();
            let needs_refresh = self.needs_refresh.clone();
            let folder_name = name.to_string();

            move |res| match res {
                Ok(Response { status, bytes, .. }) => {
                    if status == 200 {
                        *status_message.lock() = Some(format!("Created folder: {folder_name}"));
                        *needs_refresh.lock() = true;
                    } else {
                        *status_message.lock() = Some(format!(
                            "Mkdir failed: {}",
                            response_error_text(status, &bytes)
                        ));
                    }
                }
                Err(error) => {
                    *status_message.lock() = Some(format!("Mkdir error: {error}"));
                }
            }
        });
    }

    fn poll_download_progress(&self, now: f64) {
        let Some(active_download_snapshot) = self.active_download.lock().clone() else {
            return;
        };
        if active_download_snapshot.status != DownloadStatus::Downloading {
            return;
        }

        {
            let mut download_poll_in_flight = self.download_poll_in_flight.lock();
            if *download_poll_in_flight {
                return;
            }

            let mut last_download_poll_at = self.last_download_poll_at.lock();
            if let Some(last_download_poll_at) = *last_download_poll_at
                && now - last_download_poll_at < DOWNLOAD_POLL_INTERVAL.as_secs_f64()
            {
                return;
            }

            *download_poll_in_flight = true;
            *last_download_poll_at = Some(now);
        }

        let url = DownloadPath {
            download_id: &active_download_snapshot.download_id,
        }
        .progress_url();

        ehttp::fetch(Request::get(url), {
            let active_download = self.active_download.clone();
            let download_poll_in_flight = self.download_poll_in_flight.clone();

            move |res| {
                *download_poll_in_flight.lock() = false;
                match res {
                    Ok(Response { bytes, status, .. }) => {
                        if status != 200 {
                            fail_active_download(
                                &active_download,
                                format!(
                                    "Progress fetch failed: {}",
                                    response_error_text(status, &bytes)
                                ),
                            );
                            return;
                        }

                        match parse_json_response::<ActiveDownload>(&bytes) {
                            Ok(progress) => {
                                let mut guard = active_download.lock();
                                if guard.is_some() {
                                    *guard = Some(progress);
                                }
                            }
                            Err(error) => fail_active_download(&active_download, error),
                        }
                    }
                    Err(error) => {
                        fail_active_download(
                            &active_download,
                            format!("Progress fetch error: {error}"),
                        );
                    }
                }
            }
        });
    }

    fn fetch_completed_download(&self) {
        let Some(active_download_snapshot) = self.active_download.lock().clone() else {
            return;
        };
        if active_download_snapshot.status != DownloadStatus::Complete {
            return;
        }

        {
            let mut download_data_in_flight = self.download_data_in_flight.lock();
            if *download_data_in_flight {
                return;
            }
            *download_data_in_flight = true;
        }

        let url = DownloadPath {
            download_id: &active_download_snapshot.download_id,
        }
        .data_url();

        ehttp::fetch(Request::get(url), {
            let active_download = self.active_download.clone();
            let download_data_in_flight = self.download_data_in_flight.clone();
            let status_message = self.status_message.clone();
            let download_id = active_download_snapshot.download_id.clone();

            move |res| match res {
                Ok(Response { bytes, status, .. }) => {
                    *download_data_in_flight.lock() = false;
                    if status == 200 {
                        trigger_browser_download(&active_download_snapshot.filename, &bytes);
                        *status_message.lock() = Some(format!(
                            "Downloaded: {} ({})",
                            active_download_snapshot.filename,
                            format_size(bytes.len() as u64)
                        ));
                    } else {
                        *status_message.lock() = Some(format!(
                            "Failed to fetch download data: {}",
                            response_error_text(status, &bytes)
                        ));
                    }
                    clear_active_download_if_matches(&active_download, &download_id);
                }
                Err(error) => {
                    *download_data_in_flight.lock() = false;
                    *status_message.lock() = Some(format!("Fetch error: {error}"));
                    clear_active_download_if_matches(&active_download, &download_id);
                }
            }
        });
    }

    fn render_download_bar(&self, ui: &mut egui::Ui) {
        let Some(active_download_snapshot) = self.active_download.lock().clone() else {
            return;
        };

        ui.separator();

        match active_download_snapshot.status {
            DownloadStatus::Downloading => {
                ui.ctx().request_repaint_after(DOWNLOAD_POLL_INTERVAL);
                ui.label(format!(
                    "Downloading: {}",
                    active_download_snapshot.filename
                ));

                let fraction = if active_download_snapshot.total_bytes > 0 {
                    active_download_snapshot.bytes_downloaded as f32
                        / active_download_snapshot.total_bytes as f32
                } else {
                    0.0
                };
                let progress_text = format!(
                    "{} / {} ({:.1}%)",
                    format_size(active_download_snapshot.bytes_downloaded),
                    format_size(active_download_snapshot.total_bytes),
                    fraction * 100.0
                );
                ui.add(egui::ProgressBar::new(fraction).text(progress_text));

                self.poll_download_progress(ui.ctx().input(|i| i.time));
            }
            DownloadStatus::Complete => {
                ui.ctx().request_repaint_after(DOWNLOAD_POLL_INTERVAL);
                ui.horizontal(|ui| {
                    ui.label(format!(
                        "Download complete: {} ({})",
                        active_download_snapshot.filename,
                        format_size(active_download_snapshot.total_bytes)
                    ));
                });
                ui.add(egui::ProgressBar::new(1.0).text("Saving..."));
                self.fetch_completed_download();
            }
            DownloadStatus::Error => {
                let error_message = active_download_snapshot
                    .error
                    .as_deref()
                    .unwrap_or("Unknown error");
                ui.colored_label(
                    egui::Color32::RED,
                    format!(
                        "Download failed: {} - {error_message}",
                        active_download_snapshot.filename
                    ),
                );
                if ui.button("Dismiss").clicked() {
                    *self.active_download.lock() = None;
                }
            }
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

        if *self.needs_refresh.lock() {
            *self.needs_refresh.lock() = false;
            self.fetch_listing();
        }

        if *self.auto_select_first.lock() {
            *self.auto_select_first.lock() = false;
            if let FetchState::Done(ref targets) = *self.targets.lock()
                && let Some(first) = targets.first()
            {
                self.selected_target = Some(first.clone());
                self.current_path = "/".to_string();
            }
            self.fetch_listing();
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
                    FetchState::Error(ref error) => {
                        ui.colored_label(egui::Color32::RED, format!("Error: {error}"));
                        if ui.button("Retry").clicked() {
                            self.fetch_targets();
                        }
                    }
                    FetchState::Done(ref targets) => {
                        if targets.is_empty() {
                            ui.label("No FTP-capable targets found");
                            if ui.button("Refresh").clicked() {
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

                            egui::ComboBox::from_id_salt("ftp_target_selector")
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
                                            self.current_path = "/".to_string();
                                            self.fetch_listing();
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
                ui.label("Path:");
                let parts: Vec<String> = self
                    .current_path
                    .split('/')
                    .filter(|p| !p.is_empty())
                    .map(|p| p.to_string())
                    .collect();

                if ui.button("/").clicked() {
                    self.navigate_to("/".to_string());
                }
                let mut accumulated = String::new();
                for part in &parts {
                    accumulated = format!("{accumulated}/{part}");
                    ui.label("/");
                    let path_clone = accumulated.clone();
                    if ui.button(part.as_str()).clicked() {
                        self.navigate_to(path_clone);
                    }
                }

                if ui.button("⟳").on_hover_text("Refresh listing").clicked() {
                    self.fetch_listing();
                }

                let is_uploading = *self.upload_in_progress.lock();
                let upload_button = ui.add_enabled(
                    !is_uploading,
                    egui::Button::new(if is_uploading {
                        "⬆ Uploading..."
                    } else {
                        "⬆ Upload"
                    }),
                );
                if upload_button
                    .on_hover_text("Upload a file to the current directory")
                    .clicked()
                {
                    self.start_upload();
                }

                if ui
                    .button("+ Folder")
                    .on_hover_text("Create a new folder")
                    .clicked()
                {
                    self.new_folder_name = Some(String::new());
                }
            });

            let status_message = self.status_message.lock().clone();
            if let Some(msg) = status_message {
                ui.horizontal(|ui| {
                    ui.label(&msg);
                    if ui.button("x").clicked() {
                        *self.status_message.lock() = None;
                    }
                });
            }

            self.render_download_bar(ui);

            ui.separator();

            let entries_state = self.entries.lock().clone();
            let has_active_download = self.active_download.lock().is_some();

            match entries_state {
                FetchState::Idle => {
                    ui.label("Select a target to browse files.");
                }
                FetchState::Loading => {
                    ui.spinner();
                    ui.label("Loading...");
                }
                FetchState::Error(ref error) => {
                    ui.colored_label(egui::Color32::RED, format!("Error: {error}"));
                }
                FetchState::Done(ref entries) => {
                    let id_salt = ui.make_persistent_id("mavftp_file_table");
                    egui_extras::TableBuilder::new(ui)
                        .id_salt(id_salt)
                        .striped(true)
                        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                        .column(egui_extras::Column::auto().at_least(30.0))
                        .column(egui_extras::Column::remainder().at_least(200.0))
                        .column(egui_extras::Column::auto().at_least(80.0))
                        .column(egui_extras::Column::auto().at_least(100.0))
                        .header(22.0, |mut header| {
                            header.col(|ui| {
                                ui.strong("");
                            });
                            header.col(|ui| {
                                ui.strong("Name");
                            });
                            header.col(|ui| {
                                ui.strong("Size");
                            });
                            header.col(|ui| {
                                ui.strong("Actions");
                            });
                        })
                        .body(|body| {
                            body.rows(22.0, entries.len(), |mut row| {
                                let entry = &entries[row.index()];

                                row.col(|ui| {
                                    ui.label(entry.entry_type.icon());
                                });

                                row.col(|ui| {
                                    if entry.entry_type == DirEntryType::Directory {
                                        let dir_name = entry.name.clone();
                                        if ui
                                            .link(&entry.name)
                                            .on_hover_text("Open directory")
                                            .clicked()
                                        {
                                            self.navigate_into(&dir_name);
                                        }
                                    } else {
                                        ui.label(&entry.name);
                                    }
                                });

                                row.col(|ui| {
                                    if let Some(size) = entry.size {
                                        ui.label(format_size(size));
                                    }
                                });

                                row.col(|ui| {
                                    if entry.name == "." || entry.name == ".." {
                                        return;
                                    }

                                    ui.horizontal(|ui| {
                                        if entry.entry_type == DirEntryType::File {
                                            let download_button = ui.add_enabled(
                                                !has_active_download,
                                                egui::Button::new("⬇"),
                                            );
                                            if download_button.on_hover_text("Download").clicked() {
                                                self.start_download(
                                                    &entry.name,
                                                    entry.size.unwrap_or(0),
                                                );
                                            }
                                        }
                                        if ui.button("🗑").on_hover_text("Delete").clicked() {
                                            self.delete_confirm =
                                                Some((entry.entry_type, entry.name.clone()));
                                        }
                                    });
                                });
                            });
                        });
                }
            }

            let confirm_data = self.delete_confirm.clone();
            let mut should_delete = None;
            let mut should_cancel = false;
            if let Some((entry_type, name)) = &confirm_data {
                let title = format!("Delete {} \"{name}\"?", entry_type.display_name());
                egui::Window::new(title)
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.label("This action cannot be undone.");
                        ui.horizontal(|ui| {
                            if ui.button("Cancel").clicked() {
                                should_cancel = true;
                            }
                            if ui
                                .button(egui::RichText::new("Delete").color(egui::Color32::RED))
                                .clicked()
                            {
                                should_delete = Some((*entry_type, name.clone()));
                            }
                        });
                    });
            }
            if should_cancel {
                self.delete_confirm = None;
            }
            if let Some((entry_type, name)) = should_delete {
                self.delete_confirm = None;
                self.delete_entry(entry_type, &name);
            }

            let mut should_create_folder = None;
            let mut should_close_folder_dialog = false;
            if let Some(ref mut folder_name) = self.new_folder_name {
                egui::Window::new("New Folder")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.label("Folder name:");
                        let response = ui.text_edit_singleline(folder_name);
                        if response.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter))
                            && !folder_name.trim().is_empty()
                        {
                            should_create_folder = Some(folder_name.trim().to_string());
                        }
                        ui.horizontal(|ui| {
                            if ui.button("Cancel").clicked() {
                                should_close_folder_dialog = true;
                            }
                            let name_valid = !folder_name.trim().is_empty();
                            if ui
                                .add_enabled(name_valid, egui::Button::new("Create"))
                                .clicked()
                            {
                                should_create_folder = Some(folder_name.trim().to_string());
                            }
                        });
                    });
            }
            if should_close_folder_dialog {
                self.new_folder_name = None;
            }
            if let Some(name) = should_create_folder {
                self.new_folder_name = None;
                self.create_folder(&name);
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

fn request_with_method(method: &str, url: String) -> Request {
    let mut request = Request::get(url);
    request.method = method.to_string();
    request
}

fn parse_json_response<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, String> {
    serde_json::from_slice(bytes).map_err(|error| format!("Parse error: {error}"))
}

fn response_error_text(status: impl std::fmt::Display, bytes: &[u8]) -> String {
    if let Ok(ErrorResponse { error, code }) = serde_json::from_slice::<ErrorResponse>(bytes) {
        if let Some(code) = code {
            return format!("{error} (code {code})");
        }
        return error;
    }

    let text = String::from_utf8_lossy(bytes).into_owned();
    if text.is_empty() {
        format!("Request failed with status {status}")
    } else {
        text
    }
}

fn encode_query_path(path: &str) -> String {
    String::from(js_sys::encode_uri_component(path))
}

fn join_path(base: &str, name: &str) -> String {
    let base = base.trim_end_matches('/');
    if base.is_empty() {
        format!("/{name}")
    } else {
        format!("{base}/{name}")
    }
}

fn parent_path(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    if let Some(pos) = trimmed.rfind('/') {
        let parent = &trimmed[..pos];
        if parent.is_empty() {
            "/".to_string()
        } else {
            parent.to_string()
        }
    } else {
        "/".to_string()
    }
}

fn sort_dir_entries(entries: &mut [DirEntry]) {
    entries.sort_by(|a, b| {
        a.entry_type
            .sort_order()
            .cmp(&b.entry_type.sort_order())
            .then_with(|| a.name.cmp(&b.name))
    });
}

fn fail_active_download(active_download: &Arc<Mutex<Option<ActiveDownload>>>, error: String) {
    let mut active_download = active_download.lock();
    if let Some(active_download) = active_download.as_mut() {
        active_download.status = DownloadStatus::Error;
        active_download.error = Some(error);
    }
}

fn clear_active_download_if_matches(
    active_download: &Arc<Mutex<Option<ActiveDownload>>>,
    download_id: &str,
) {
    let mut active_download = active_download.lock();
    if active_download
        .as_ref()
        .is_some_and(|active_download| active_download.download_id == download_id)
    {
        *active_download = None;
    }
}

fn trigger_browser_download(filename: &str, data: &[u8]) {
    use wasm_bindgen::JsCast;

    let window = match web_sys::window() {
        Some(w) => w,
        None => return,
    };
    let document = match window.document() {
        Some(d) => d,
        None => return,
    };

    let uint8arr = js_sys::Uint8Array::from(data);
    let array = js_sys::Array::new();
    array.push(&uint8arr.buffer());

    let blob = match web_sys::Blob::new_with_u8_array_sequence(&array) {
        Ok(b) => b,
        Err(_) => return,
    };

    let url = match web_sys::Url::create_object_url_with_blob(&blob) {
        Ok(u) => u,
        Err(_) => return,
    };

    let a = match document.create_element("a") {
        Ok(el) => el,
        Err(_) => {
            let _ = web_sys::Url::revoke_object_url(&url);
            return;
        }
    };

    let _ = a.set_attribute("href", &url);
    let _ = a.set_attribute("download", filename);
    let _ = a.set_attribute("style", "display: none");

    let body = match document.body() {
        Some(b) => b,
        None => {
            let _ = web_sys::Url::revoke_object_url(&url);
            return;
        }
    };
    let _ = body.append_child(&a);

    if let Some(html_el) = a.dyn_ref::<web_sys::HtmlElement>() {
        html_el.click();
    }

    let _ = body.remove_child(&a);
    let _ = web_sys::Url::revoke_object_url(&url);
}
