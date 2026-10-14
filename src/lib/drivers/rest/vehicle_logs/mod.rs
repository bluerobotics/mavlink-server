mod client;
mod protocol;
mod transport;

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use anyhow::Result;
use lazy_static::lazy_static;
use serde::Serialize;
use tokio::sync::RwLock;
use tracing::*;

const CANCELLED_ERROR_MESSAGE: &str = "Cancelled";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogClientError {
    TimedOut,
}

impl std::fmt::Display for LogClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TimedOut => write!(f, "Log request timed out"),
        }
    }
}

impl std::error::Error for LogClientError {}

#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub id: u16,
    pub time_utc: u32,
    pub size: u32,
}

#[derive(Debug, Serialize, Clone)]
pub struct LogDownloadProgress {
    pub download_id: String,
    pub filename: String,
    pub bytes_downloaded: u64,
    pub total_bytes: u64,
    pub status: LogDownloadStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LogDownloadStatus {
    Downloading,
    Complete,
    Error,
}

#[instrument(level = "debug")]
pub async fn list_logs(system_id: u8, component_id: u8) -> Result<Vec<LogEntry>> {
    client::list_log_entries(system_id, component_id).await
}

#[instrument(level = "debug")]
pub async fn erase_logs(system_id: u8, component_id: u8) -> Result<()> {
    transport::send_log_erase(system_id, component_id);
    info!("Sent log erase command to {system_id}:{component_id}");
    Ok(())
}

/// Start an asynchronous log download. Returns a download ID for progress tracking.
#[instrument(level = "debug")]
pub async fn start_log_download(
    system_id: u8,
    component_id: u8,
    log_id: u16,
    file_size: u64,
) -> String {
    let download_id = uuid::Uuid::new_v4().to_string();
    let filename = format!("log_{log_id}.bin");

    let session = Arc::new(LogDownloadSession {
        filename,
        bytes_downloaded: AtomicU64::new(0),
        total_bytes: file_size,
        status: RwLock::new(LogDownloadStatus::Downloading),
        error: RwLock::new(None),
        data: RwLock::new(None),
        cancelled: AtomicBool::new(false),
    });

    ACTIVE_DOWNLOADS
        .write()
        .await
        .insert(download_id.clone(), session.clone());

    let id = download_id.clone();
    tokio::spawn(async move {
        let result = client::download_log(
            system_id,
            component_id,
            log_id,
            session.total_bytes,
            &session.bytes_downloaded,
            &session.cancelled,
        )
        .await;

        match result {
            Ok(data) => {
                *session.data.write().await = Some(data);
                *session.status.write().await = LogDownloadStatus::Complete;
            }
            Err(error) => {
                if !session.cancelled.load(Ordering::Relaxed) {
                    *session.error.write().await = Some(error.to_string());
                    *session.status.write().await = LogDownloadStatus::Error;
                }
            }
        }
    });

    id
}

#[instrument(level = "debug")]
pub async fn get_download_progress(download_id: &str) -> Option<LogDownloadProgress> {
    let session = {
        let downloads = ACTIVE_DOWNLOADS.read().await;
        downloads.get(download_id).cloned()
    }?;
    let status = session.status.read().await.clone();
    let error = session.error.read().await.clone();
    Some(LogDownloadProgress {
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
    let session = {
        let downloads = ACTIVE_DOWNLOADS.read().await;
        downloads.get(download_id).cloned()
    }?;
    if *session.status.read().await != LogDownloadStatus::Complete {
        return None;
    }

    let data = session.data.write().await.take()?;
    let filename = session.filename.clone();
    ACTIVE_DOWNLOADS.write().await.remove(download_id);
    Some((filename, data))
}

#[instrument(level = "debug")]
pub async fn cancel_download(download_id: &str, system_id: u8, component_id: u8) -> Result<()> {
    let session = {
        let downloads = ACTIVE_DOWNLOADS.read().await;
        downloads.get(download_id).cloned()
    };
    if let Some(session) = session {
        session.cancelled.store(true, Ordering::Relaxed);
        *session.status.write().await = LogDownloadStatus::Error;
        *session.error.write().await = Some(CANCELLED_ERROR_MESSAGE.to_string());
    }

    transport::send_log_request_end(system_id, component_id);
    ACTIVE_DOWNLOADS.write().await.remove(download_id);
    Ok(())
}

struct LogDownloadSession {
    filename: String,
    bytes_downloaded: AtomicU64,
    total_bytes: u64,
    status: RwLock<LogDownloadStatus>,
    error: RwLock<Option<String>>,
    data: RwLock<Option<Vec<u8>>>,
    cancelled: AtomicBool,
}

type DownloadMap = HashMap<String, Arc<LogDownloadSession>>;

lazy_static! {
    static ref ACTIVE_DOWNLOADS: RwLock<DownloadMap> = RwLock::new(HashMap::new());
}
