mod client;
mod protocol;
mod transport;

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use anyhow::Result;
use lazy_static::lazy_static;
use serde::Serialize;
use tokio::sync::RwLock;
use tracing::*;

use crate::drivers::rest::{autopilot::ardupilot::Capabilities, control};

use self::protocol::{ListingEntry, ListingEntryKind};

pub use protocol::{FtpError, FtpNakError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FtpClientError {
    TimedOut,
}

impl std::fmt::Display for FtpClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TimedOut => write!(f, "FTP request timed out"),
        }
    }
}

impl std::error::Error for FtpClientError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DirEntryType {
    Directory,
    File,
    Skip,
}

#[derive(Debug, Clone, Serialize)]
pub struct DirEntry {
    #[serde(rename = "type")]
    pub entry_type: DirEntryType,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

impl From<ListingEntry> for DirEntry {
    fn from(entry: ListingEntry) -> Self {
        Self {
            entry_type: match entry.kind {
                ListingEntryKind::Directory => DirEntryType::Directory,
                ListingEntryKind::File => DirEntryType::File,
                ListingEntryKind::Skip => DirEntryType::Skip,
            },
            name: entry.name,
            size: entry.size,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FtpTarget {
    pub system_id: u8,
    pub component_id: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct DownloadProgress {
    pub download_id: String,
    pub filename: String,
    pub bytes_downloaded: u64,
    pub total_bytes: u64,
    pub status: DownloadStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DownloadStatus {
    Downloading,
    Complete,
    Error,
}

#[instrument(level = "debug")]
pub async fn available_targets() -> Vec<FtpTarget> {
    let vehicles = control::vehicles().await;
    let mut targets = Vec::new();

    for vehicle in &vehicles {
        for (comp_id, component) in &vehicle.components {
            let control::VehicleComponents::Autopilot(comp) = component;
            if let Some(version) = &comp.version
                && version.capabilities.contains(Capabilities::FTP)
            {
                let vtype = comp
                    .vehicle_type
                    .map(|vehicle_type| format!("{vehicle_type:?}"))
                    .unwrap_or_else(|| "Unknown".to_string());
                targets.push(FtpTarget {
                    system_id: vehicle.vehicle_id,
                    component_id: *comp_id,
                    description: Some(format!("{vtype} v{}", version.version)),
                });
            }
        }
    }

    targets
}

#[instrument(level = "debug")]
pub async fn list_directory(system_id: u8, component_id: u8, path: &str) -> Result<Vec<DirEntry>> {
    Ok(
        client::list_directory_entries(system_id, component_id, path)
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    )
}

#[instrument(level = "debug")]
pub async fn read_file(
    system_id: u8,
    component_id: u8,
    path: &str,
    max_size: Option<usize>,
) -> Result<Vec<u8>> {
    client::read_file(system_id, component_id, path, max_size).await
}

#[instrument(level = "debug")]
pub async fn start_download(system_id: u8, component_id: u8, path: &str, file_size: u64) -> String {
    let download_id = uuid::Uuid::new_v4().to_string();
    let filename = path.rsplit('/').next().unwrap_or(path).to_string();

    let session = Arc::new(DownloadSession {
        filename,
        bytes_downloaded: AtomicU64::new(0),
        total_bytes: file_size,
        status: RwLock::new(DownloadStatus::Downloading),
        error: RwLock::new(None),
        data: RwLock::new(None),
    });

    ACTIVE_DOWNLOADS
        .write()
        .await
        .insert(download_id.clone(), session.clone());

    let download_path = path.to_string();
    let id = download_id.clone();
    tokio::spawn(async move {
        match client::read_file_with_progress(
            system_id,
            component_id,
            &download_path,
            &session.bytes_downloaded,
        )
        .await
        {
            Ok(data) => {
                *session.data.write().await = Some(data);
                *session.status.write().await = DownloadStatus::Complete;
            }
            Err(error) => {
                *session.error.write().await = Some(error.to_string());
                *session.status.write().await = DownloadStatus::Error;
            }
        }
    });

    id
}

#[instrument(level = "debug")]
pub async fn get_download_progress(download_id: &str) -> Option<DownloadProgress> {
    let downloads = ACTIVE_DOWNLOADS.read().await;
    let session = downloads.get(download_id)?;
    let status = session.status.read().await.clone();
    let error = session.error.read().await.clone();
    Some(DownloadProgress {
        download_id: download_id.to_string(),
        filename: session.filename.clone(),
        bytes_downloaded: session.bytes_downloaded.load(Ordering::Relaxed),
        total_bytes: session.total_bytes,
        status,
        error,
    })
}

#[instrument(level = "debug")]
pub async fn take_download_data(download_id: &str) -> Option<(String, Vec<u8>)> {
    let downloads = ACTIVE_DOWNLOADS.read().await;
    let session = downloads.get(download_id)?;
    if *session.status.read().await != DownloadStatus::Complete {
        return None;
    }

    let data = session.data.write().await.take()?;
    let filename = session.filename.clone();
    drop(downloads);
    ACTIVE_DOWNLOADS.write().await.remove(download_id);
    Some((filename, data))
}

#[instrument(level = "debug", skip(data))]
pub async fn write_file(system_id: u8, component_id: u8, path: &str, data: &[u8]) -> Result<()> {
    client::write_file(system_id, component_id, path, data).await
}

#[instrument(level = "debug")]
pub async fn remove_file(system_id: u8, component_id: u8, path: &str) -> Result<()> {
    client::remove_file(system_id, component_id, path).await
}

#[instrument(level = "debug")]
pub async fn create_directory(system_id: u8, component_id: u8, path: &str) -> Result<()> {
    client::create_directory(system_id, component_id, path).await
}

#[instrument(level = "debug")]
pub async fn remove_directory(system_id: u8, component_id: u8, path: &str) -> Result<()> {
    client::remove_directory(system_id, component_id, path).await
}

#[instrument(level = "debug")]
pub async fn rename(system_id: u8, component_id: u8, from: &str, to: &str) -> Result<()> {
    client::rename(system_id, component_id, from, to).await
}

#[instrument(level = "debug")]
pub async fn truncate_file(system_id: u8, component_id: u8, path: &str, offset: u32) -> Result<()> {
    client::truncate_file(system_id, component_id, path, offset).await
}

#[instrument(level = "debug")]
pub async fn calc_crc32(system_id: u8, component_id: u8, path: &str) -> Result<u32> {
    client::calc_crc32(system_id, component_id, path).await
}

struct DownloadSession {
    filename: String,
    bytes_downloaded: AtomicU64,
    total_bytes: u64,
    status: RwLock<DownloadStatus>,
    error: RwLock<Option<String>>,
    data: RwLock<Option<Vec<u8>>>,
}

type DownloadMap = HashMap<String, Arc<DownloadSession>>;

lazy_static! {
    static ref ACTIVE_DOWNLOADS: RwLock<DownloadMap> = RwLock::new(HashMap::new());
}
