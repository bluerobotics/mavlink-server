use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use anyhow::{Result, bail};
use tokio::sync::broadcast;
use tracing::*;

use crate::protocol::Protocol;

use super::{
    LogClientError, LogEntry,
    protocol::{LOG_CHUNK_SIZE, LOG_DATA_PACKET_SIZE},
    transport,
};

const LOG_TIMEOUT_MS: u64 = 2000;
const LOG_DATA_TIMEOUT_MS: u64 = 500;
const LOG_MAX_RETRIES: u32 = 10;
const LOG_STALL_TIMEOUT_SECS: u64 = 30;
const LOG_DATA_RECV_POLL_MS: u64 = 100;
const LOG_REQUEST_END_SETTLE_MS: u64 = 100;

#[instrument(level = "debug")]
pub(super) async fn list_log_entries(system_id: u8, component_id: u8) -> Result<Vec<LogEntry>> {
    let mut hub_receiver = transport::subscribe_hub().await?;
    let mut entries: HashMap<u16, LogEntry> = HashMap::new();
    let mut expected_count: Option<u16> = None;
    let mut retries: u32 = 0;

    transport::send_log_request_list(system_id, component_id, 0, 0xFFFF);

    loop {
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(LOG_TIMEOUT_MS),
            transport::recv_log_entry(&mut hub_receiver, system_id, component_id),
        )
        .await;

        match result {
            Ok(Ok(entry)) => {
                retries = 0;
                if entry.num_logs == 0 {
                    return Ok(Vec::new());
                }

                expected_count = Some(entry.num_logs);

                entries.insert(
                    entry.id,
                    LogEntry {
                        id: entry.id,
                        time_utc: entry.time_utc,
                        size: entry.size,
                    },
                );

                if entries.len() as u16 >= entry.num_logs {
                    break;
                }
            }
            Ok(Err(error)) => {
                warn!("Error receiving log entry: {error}");
                retries += 1;
                if retries > LOG_MAX_RETRIES {
                    return Err(LogClientError::TimedOut.into());
                }
            }
            Err(_) => {
                retries += 1;
                if retries > LOG_MAX_RETRIES {
                    break;
                }

                if let Some(count) = expected_count {
                    for id in 0..count {
                        if !entries.contains_key(&id) {
                            debug!("Re-requesting missing log entry {id}");
                            transport::send_log_request_list(system_id, component_id, id, id);
                            break;
                        }
                    }
                } else {
                    transport::send_log_request_list(system_id, component_id, 0, 0xFFFF);
                }
            }
        }
    }

    let mut logs: Vec<LogEntry> = entries.into_values().collect();
    logs.sort_by_key(|entry| entry.id);
    info!("Found {} logs", logs.len());
    Ok(logs)
}

/// Core log download using a chunk-based, gap-filling strategy (matching QGC).
/// Downloads in ~`LOG_CHUNK_SIZE` chunks so the autopilot finishes each quickly
/// and accepts retries; missing bins inside a chunk are re-requested before
/// advancing to the next chunk.
#[instrument(level = "debug", skip(progress, cancelled))]
pub(super) async fn download_log(
    system_id: u8,
    component_id: u8,
    log_id: u16,
    file_size: u64,
    progress: &AtomicU64,
    cancelled: &AtomicBool,
) -> Result<Vec<u8>> {
    let file_size = file_size as usize;
    if file_size == 0 {
        transport::send_log_request_end(system_id, component_id);
        return Ok(Vec::new());
    }

    let mut hub_receiver = transport::subscribe_hub().await?;

    transport::send_log_request_end(system_id, component_id);
    tokio::time::sleep(std::time::Duration::from_millis(LOG_REQUEST_END_SETTLE_MS)).await;

    let mut state = DownloadState::new(file_size);
    let start_time = std::time::Instant::now();
    info!(
        "Starting log download: id={log_id} size={file_size} bins={num_bins} chunks={num_chunks}",
        num_bins = state.num_bins,
        num_chunks = state.num_chunks,
    );

    while state.received_count < state.num_bins {
        check_cancelled(cancelled, system_id, component_id)?;

        let chunk = match state.current_chunk_window(file_size) {
            Some(window) => window,
            None => continue,
        };

        let (req_ofs, req_count) = match chunk.first_gap_request(&state.received) {
            Some(request) => request,
            None => {
                state.current_chunk += 1;
                continue;
            }
        };

        transport::send_log_request_data(
            system_id,
            component_id,
            log_id,
            req_ofs as u32,
            req_count as u32,
        );

        download_chunk(
            system_id,
            component_id,
            log_id,
            chunk,
            &mut state,
            &mut hub_receiver,
            progress,
            cancelled,
        )
        .await?;

        if chunk.is_complete(&state.received) {
            state.current_chunk += 1;
        }
    }

    transport::send_log_request_end(system_id, component_id);

    let elapsed = start_time.elapsed();
    info!(
        "Log download complete: id={log_id} size={file_size} time={:.1}s speed={:.1} KB/s \
         ({total_packets} pkts, {duplicate_packets} dupes)",
        elapsed.as_secs_f64(),
        file_size as f64 / 1024.0 / elapsed.as_secs_f64(),
        total_packets = state.total_packets,
        duplicate_packets = state.duplicate_packets,
    );

    Ok(state.file_data)
}

#[instrument(level = "debug", skip(state, hub_receiver, progress, cancelled))]
async fn download_chunk(
    system_id: u8,
    component_id: u8,
    log_id: u16,
    chunk: ChunkWindow,
    state: &mut DownloadState,
    hub_receiver: &mut broadcast::Receiver<Arc<Protocol>>,
    progress: &AtomicU64,
    cancelled: &AtomicBool,
) -> Result<()> {
    let mut last_data_time = std::time::Instant::now();
    let mut retries: u32 = 0;
    let chunk_stall_start = std::time::Instant::now();

    loop {
        check_cancelled(cancelled, system_id, component_id)?;

        if chunk_stall_start.elapsed().as_secs() >= LOG_STALL_TIMEOUT_SECS {
            let pct = state.percent_complete();
            transport::send_log_request_end(system_id, component_id);
            warn!("Log download stalled for {LOG_STALL_TIMEOUT_SECS}s at {pct:.1}%");
            return Err(LogClientError::TimedOut.into());
        }

        if last_data_time.elapsed().as_millis() >= LOG_DATA_TIMEOUT_MS as u128 {
            retries += 1;
            if retries > LOG_MAX_RETRIES {
                return Ok(());
            }

            let Some((gap_byte_ofs, remaining)) = chunk.first_gap_request(&state.received) else {
                return Ok(());
            };

            let pct = state.percent_complete();
            debug!(
                "Chunk {current}/{total} retry {retries}: {pct:.1}% gap@byte={gap_byte_ofs}",
                current = state.current_chunk,
                total = state.num_chunks,
            );
            transport::send_log_request_data(
                system_id,
                component_id,
                log_id,
                gap_byte_ofs as u32,
                remaining as u32,
            );
            last_data_time = std::time::Instant::now();
        }

        let result = tokio::time::timeout(
            std::time::Duration::from_millis(LOG_DATA_RECV_POLL_MS),
            transport::recv_log_data(hub_receiver, system_id, component_id, log_id),
        )
        .await;

        match result {
            Ok(Ok(packet)) => {
                let new_bins = state.apply_packet(&packet);
                if new_bins {
                    last_data_time = std::time::Instant::now();
                    retries = 0;
                }

                progress.store(state.bytes_received(), Ordering::Relaxed);

                if state.received_count >= state.num_bins || chunk.is_complete(&state.received) {
                    return Ok(());
                }
            }
            Ok(Err(error)) => {
                bail!("Log data hub receive failed: {error}");
            }
            Err(_) => {}
        }
    }
}

fn check_cancelled(cancelled: &AtomicBool, system_id: u8, component_id: u8) -> Result<()> {
    if cancelled.load(Ordering::Relaxed) {
        transport::send_log_request_end(system_id, component_id);
        bail!("Download cancelled");
    }
    Ok(())
}

struct DownloadState {
    file_data: Vec<u8>,
    received: Vec<bool>,
    received_count: usize,
    num_bins: usize,
    num_chunks: usize,
    current_chunk: usize,
    total_packets: u64,
    duplicate_packets: u64,
}

impl DownloadState {
    fn new(file_size: usize) -> Self {
        let num_bins = file_size.div_ceil(LOG_DATA_PACKET_SIZE);
        let num_chunks = file_size.div_ceil(LOG_CHUNK_SIZE);
        Self {
            file_data: vec![0u8; file_size],
            received: vec![false; num_bins],
            received_count: 0,
            num_bins,
            num_chunks,
            current_chunk: 0,
            total_packets: 0,
            duplicate_packets: 0,
        }
    }

    fn current_chunk_window(&self, file_size: usize) -> Option<ChunkWindow> {
        let chunk_ofs = self.current_chunk * LOG_CHUNK_SIZE;
        if chunk_ofs >= file_size {
            return None;
        }
        let chunk_end = (chunk_ofs + LOG_CHUNK_SIZE).min(file_size);
        Some(ChunkWindow {
            first_bin: chunk_ofs / LOG_DATA_PACKET_SIZE,
            last_bin: (chunk_end - 1) / LOG_DATA_PACKET_SIZE,
            end_byte: chunk_end,
        })
    }

    fn apply_packet(&mut self, packet: &super::protocol::LogDataPacket) -> bool {
        let ofs = packet.ofs as usize;
        let count = packet.count as usize;
        if ofs >= self.file_data.len() || count == 0 {
            return false;
        }

        self.total_packets += 1;
        let end = (ofs + count).min(self.file_data.len());
        let data_len = end - ofs;
        self.file_data[ofs..end].copy_from_slice(&packet.data[..data_len]);

        let first_bin = ofs / LOG_DATA_PACKET_SIZE;
        let last_bin = (end - 1) / LOG_DATA_PACKET_SIZE;
        let mut new_bins = false;
        for bin in first_bin..=last_bin {
            if bin < self.received.len() && !self.received[bin] {
                self.received[bin] = true;
                self.received_count += 1;
                new_bins = true;
            }
        }
        if !new_bins {
            self.duplicate_packets += 1;
        }
        new_bins
    }

    fn bytes_received(&self) -> u64 {
        (self.received_count as u64 * LOG_DATA_PACKET_SIZE as u64).min(self.file_data.len() as u64)
    }

    fn percent_complete(&self) -> f64 {
        if self.num_bins == 0 {
            return 100.0;
        }
        self.received_count as f64 / self.num_bins as f64 * 100.0
    }
}

#[derive(Clone, Copy, Debug)]
struct ChunkWindow {
    first_bin: usize,
    last_bin: usize,
    end_byte: usize,
}

impl ChunkWindow {
    fn first_gap_request(&self, received: &[bool]) -> Option<(usize, usize)> {
        let gap_start = received[self.first_bin..=self.last_bin]
            .iter()
            .position(|&seen| !seen)
            .map(|position| self.first_bin + position)?;
        let gap_byte_ofs = gap_start * LOG_DATA_PACKET_SIZE;
        let remaining = self.end_byte - gap_byte_ofs;
        Some((gap_byte_ofs, remaining))
    }

    fn is_complete(&self, received: &[bool]) -> bool {
        received[self.first_bin..=self.last_bin]
            .iter()
            .all(|&seen| seen)
    }
}
