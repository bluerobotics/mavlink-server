use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use anyhow::{Context, Result, anyhow};
use byteorder::{LittleEndian, ReadBytesExt};
use lazy_static::lazy_static;
use tokio::sync::{Mutex, RwLock, broadcast};
use tracing::*;

use crate::protocol::Protocol;

use super::{
    FtpClientError,
    protocol::{
        FTP_DATA_MAX_LEN, FtpError, FtpOpcode, FtpPayload, ListingEntry, check_nak,
        parse_listing_entries,
    },
    transport,
};

const FTP_TIMEOUT_MS: u64 = 1000;
const FTP_BURST_TIMEOUT_MS: u64 = 200;
const FTP_MAX_RETRIES: u32 = 5;

lazy_static! {
    static ref TARGET_LOCKS: RwLock<TargetLockMap> = RwLock::new(HashMap::new());
}

type TargetLockMap = HashMap<(u8, u8), Arc<Mutex<()>>>;

#[instrument(level = "debug")]
pub async fn list_directory_entries(
    system_id: u8,
    component_id: u8,
    path: &str,
) -> Result<Vec<ListingEntry>> {
    let lock = target_lock(system_id, component_id).await;
    let _guard = lock.lock().await;

    let mut all_entries = Vec::new();
    let mut offset: u32 = 0;
    let path_bytes = path.as_bytes();

    loop {
        let mut req = transport::new_request(FtpOpcode::ListDirectory);
        req.offset = offset;
        req.data = path_bytes.to_vec();
        req.size = path_bytes.len() as u8;

        let resp = send_and_receive(system_id, component_id, &mut req).await?;

        if let Err(nak) = check_nak(&resp) {
            if nak.err_code == FtpError::Eof {
                break;
            }
            return Err(nak.into());
        }

        let entries = parse_listing_entries(&resp.data);
        if entries.is_empty() {
            break;
        }

        offset += entries.len() as u32;
        all_entries.extend(entries);
    }

    Ok(all_entries)
}

#[instrument(level = "debug")]
pub async fn read_file(
    system_id: u8,
    component_id: u8,
    path: &str,
    max_size: Option<usize>,
) -> Result<Vec<u8>> {
    burst_read_file_inner(system_id, component_id, path, None, max_size).await
}

#[instrument(level = "debug", skip(data))]
pub async fn write_file(system_id: u8, component_id: u8, path: &str, data: &[u8]) -> Result<()> {
    let lock = target_lock(system_id, component_id).await;
    let _guard = lock.lock().await;

    reset_sessions(system_id, component_id)
        .await
        .context("ResetSessions failed before CreateFile")?;

    let path_bytes = path.as_bytes();
    let mut req = transport::new_request(FtpOpcode::CreateFile);
    req.data = path_bytes.to_vec();
    req.size = path_bytes.len() as u8;

    let resp = send_and_receive(system_id, component_id, &mut req)
        .await
        .context("CreateFile failed")?;
    check_nak(&resp).map_err(|error| anyhow!(error))?;

    let session = resp.session;
    let mut offset: u32 = 0;

    for chunk in data.chunks(FTP_DATA_MAX_LEN) {
        let mut req = transport::new_request(FtpOpcode::WriteFile);
        req.session = session;
        req.offset = offset;
        req.data = chunk.to_vec();
        req.size = chunk.len() as u8;

        let resp = send_and_receive(system_id, component_id, &mut req).await;
        match resp {
            Ok(resp) => {
                if let Err(nak) = check_nak(&resp) {
                    terminate_session_best_effort(
                        system_id,
                        component_id,
                        session,
                        "after WriteFile NAK",
                    )
                    .await;
                    return Err(nak.into());
                }
                offset += chunk.len() as u32;
            }
            Err(error) => {
                terminate_session_best_effort(
                    system_id,
                    component_id,
                    session,
                    "after WriteFile transport failure",
                )
                .await;
                return Err(error);
            }
        }
    }

    terminate_session(system_id, component_id, session)
        .await
        .context("TerminateSession failed after WriteFile")?;
    Ok(())
}

#[instrument(level = "debug")]
pub async fn remove_file(system_id: u8, component_id: u8, path: &str) -> Result<()> {
    let lock = target_lock(system_id, component_id).await;
    let _guard = lock.lock().await;

    let path_bytes = path.as_bytes();
    let mut req = transport::new_request(FtpOpcode::RemoveFile);
    req.data = path_bytes.to_vec();
    req.size = path_bytes.len() as u8;

    let resp = send_and_receive(system_id, component_id, &mut req).await?;
    check_nak(&resp).map_err(|error| anyhow!(error))?;
    Ok(())
}

#[instrument(level = "debug")]
pub async fn create_directory(system_id: u8, component_id: u8, path: &str) -> Result<()> {
    let lock = target_lock(system_id, component_id).await;
    let _guard = lock.lock().await;

    let path_bytes = path.as_bytes();
    let mut req = transport::new_request(FtpOpcode::CreateDirectory);
    req.data = path_bytes.to_vec();
    req.size = path_bytes.len() as u8;

    let resp = send_and_receive(system_id, component_id, &mut req).await?;
    check_nak(&resp).map_err(|error| anyhow!(error))?;
    Ok(())
}

#[instrument(level = "debug")]
pub async fn remove_directory(system_id: u8, component_id: u8, path: &str) -> Result<()> {
    let lock = target_lock(system_id, component_id).await;
    let _guard = lock.lock().await;

    let path_bytes = path.as_bytes();
    let mut req = transport::new_request(FtpOpcode::RemoveDirectory);
    req.data = path_bytes.to_vec();
    req.size = path_bytes.len() as u8;

    let resp = send_and_receive(system_id, component_id, &mut req).await?;
    check_nak(&resp).map_err(|error| anyhow!(error))?;
    Ok(())
}

#[instrument(level = "debug")]
pub async fn rename(system_id: u8, component_id: u8, from: &str, to: &str) -> Result<()> {
    let lock = target_lock(system_id, component_id).await;
    let _guard = lock.lock().await;

    let mut path_data = from.as_bytes().to_vec();
    path_data.push(0);
    path_data.extend_from_slice(to.as_bytes());

    let mut req = transport::new_request(FtpOpcode::Rename);
    req.data = path_data;
    req.size = req.data.len() as u8;

    let resp = send_and_receive(system_id, component_id, &mut req).await?;
    check_nak(&resp).map_err(|error| anyhow!(error))?;
    Ok(())
}

#[instrument(level = "debug")]
pub async fn truncate_file(system_id: u8, component_id: u8, path: &str, offset: u32) -> Result<()> {
    let lock = target_lock(system_id, component_id).await;
    let _guard = lock.lock().await;

    let path_bytes = path.as_bytes();
    let mut req = transport::new_request(FtpOpcode::TruncateFile);
    req.data = path_bytes.to_vec();
    req.size = path_bytes.len() as u8;
    req.offset = offset;

    let resp = send_and_receive(system_id, component_id, &mut req).await?;
    check_nak(&resp).map_err(|error| anyhow!(error))?;
    Ok(())
}

#[instrument(level = "debug")]
pub async fn calc_crc32(system_id: u8, component_id: u8, path: &str) -> Result<u32> {
    let lock = target_lock(system_id, component_id).await;
    let _guard = lock.lock().await;

    let path_bytes = path.as_bytes();
    let mut req = transport::new_request(FtpOpcode::CalcFileCRC32);
    req.data = path_bytes.to_vec();
    req.size = path_bytes.len() as u8;

    let resp = send_and_receive(system_id, component_id, &mut req).await?;
    check_nak(&resp).map_err(|error| anyhow!(error))?;

    if resp.data.len() >= 4 {
        let mut cursor = std::io::Cursor::new(&resp.data[..4]);
        Ok(cursor.read_u32::<LittleEndian>()?)
    } else {
        Err(anyhow!("CRC32 response too short"))
    }
}

#[instrument(level = "debug", skip(progress))]
pub(super) async fn read_file_with_progress(
    system_id: u8,
    component_id: u8,
    path: &str,
    progress: &AtomicU64,
) -> Result<Vec<u8>> {
    burst_read_file_inner(system_id, component_id, path, Some(progress), None).await
}

#[instrument(
    level = "debug",
    skip(payload),
    fields(opcode = ?payload.opcode, seq_number = payload.seq_number)
)]
async fn send_and_receive(
    target_system: u8,
    target_component: u8,
    payload: &mut FtpPayload,
) -> Result<FtpPayload> {
    let mut last_error = anyhow!(FtpClientError::TimedOut);
    let mut hub_receiver = transport::subscribe().await?;

    for attempt in 0..FTP_MAX_RETRIES {
        if attempt > 0 {
            trace!(
                "FTP retry attempt {attempt} for opcode {:?}",
                payload.opcode
            );
        }

        let seq = payload.seq_number;
        transport::send_ftp_message(target_system, target_component, payload);

        let result = tokio::time::timeout(
            std::time::Duration::from_millis(FTP_TIMEOUT_MS),
            transport::recv_ftp(
                &mut hub_receiver,
                target_system,
                target_component,
                Some(seq),
            ),
        )
        .await;

        match result {
            Ok(Ok(resp)) => return Ok(resp),
            Ok(Err(error)) => {
                last_error = error;
            }
            Err(_) => {
                last_error = anyhow!(FtpClientError::TimedOut);
            }
        }
    }

    Err(last_error)
}

#[instrument(level = "debug", skip(progress))]
async fn burst_read_file_inner(
    system_id: u8,
    component_id: u8,
    path: &str,
    progress: Option<&AtomicU64>,
    max_size: Option<usize>,
) -> Result<Vec<u8>> {
    let lock = target_lock(system_id, component_id).await;
    let _guard = lock.lock().await;

    let OpenReadSession { session, file_size } =
        open_read_session(system_id, component_id, path, max_size).await?;
    let mut state = BurstReadState::new(file_size);
    let mut retries = 0;
    let mut hub_receiver = transport::subscribe().await?;

    info!("Burst read: file_size={file_size}, session={session}");
    let burst_start = std::time::Instant::now();

    while let BurstCycleOutcome::Continue = run_burst_cycle(
        system_id,
        component_id,
        session,
        &mut hub_receiver,
        &mut state,
        &mut retries,
        progress,
    )
    .await?
    {}

    let burst_elapsed = burst_start.elapsed();
    info!(
        "Burst phase complete: {bytes_written}/{file_size} bytes in {:.1}s, {total_packets} packets, {} missing blocks, speed={:.1} KB/s",
        burst_elapsed.as_secs_f64(),
        state.missing_blocks.len(),
        state.bytes_written as f64 / 1024.0 / burst_elapsed.as_secs_f64(),
        bytes_written = state.bytes_written,
        file_size = file_size,
        total_packets = state.total_packets,
    );

    fill_missing_blocks(system_id, component_id, session, &mut state, progress).await?;
    finish_read_session(system_id, component_id, session, state, progress).await
}

#[instrument(level = "debug")]
async fn open_read_session(
    system_id: u8,
    component_id: u8,
    path: &str,
    max_size: Option<usize>,
) -> Result<OpenReadSession> {
    reset_sessions(system_id, component_id)
        .await
        .context("ResetSessions failed before OpenFileRO")?;

    let path_bytes = path.as_bytes();
    let mut req = transport::new_request(FtpOpcode::OpenFileRO);
    req.data = path_bytes.to_vec();
    req.size = path_bytes.len() as u8;

    let resp = send_and_receive(system_id, component_id, &mut req)
        .await
        .context("OpenFileRO failed")?;
    check_nak(&resp).map_err(|error| anyhow!(error))?;

    let session = resp.session;
    let file_size = if resp.size >= 4 && resp.data.len() >= 4 {
        let mut cursor = std::io::Cursor::new(&resp.data[..4]);
        cursor.read_u32::<LittleEndian>()? as usize
    } else {
        0
    };

    if let Some(max) = max_size
        && file_size > max
    {
        terminate_session_best_effort(
            system_id,
            component_id,
            session,
            "after rejecting oversized file",
        )
        .await;
        return Err(anyhow!(
            "File size {file_size} bytes exceeds limit of {max} bytes"
        ));
    }

    Ok(OpenReadSession { session, file_size })
}

#[instrument(level = "debug", skip(hub_receiver, state, progress))]
async fn run_burst_cycle(
    system_id: u8,
    component_id: u8,
    session: u8,
    hub_receiver: &mut broadcast::Receiver<Arc<Protocol>>,
    state: &mut BurstReadState,
    retries: &mut u32,
    progress: Option<&AtomicU64>,
) -> Result<BurstCycleOutcome> {
    let burst_offset = state.contiguous_end;
    let mut burst_req = transport::new_request(FtpOpcode::BurstReadFile);
    burst_req.session = session;
    burst_req.offset = burst_offset;
    burst_req.size = FTP_DATA_MAX_LEN as u8;

    let burst_cycle_start = std::time::Instant::now();
    let mut cycle_packets: u32 = 0;
    debug!("Sending BurstReadFile at offset={burst_offset}");
    transport::send_ftp_message(system_id, component_id, &burst_req);

    loop {
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(FTP_BURST_TIMEOUT_MS),
            transport::recv_ftp(hub_receiver, system_id, component_id, None),
        )
        .await;

        match result {
            Ok(Ok(resp)) => {
                if let Err(nak) = check_nak(&resp) {
                    debug!(
                        "Got NAK: err_code={:?} at offset={}",
                        nak.err_code, state.contiguous_end
                    );
                    if nak.err_code == FtpError::Eof {
                        return Ok(BurstCycleOutcome::Eof);
                    }
                    terminate_session_best_effort(
                        system_id,
                        component_id,
                        session,
                        "after BurstReadFile NAK",
                    )
                    .await;
                    return Err(nak.into());
                }

                if resp.opcode != FtpOpcode::Ack || resp.req_opcode != FtpOpcode::BurstReadFile {
                    continue;
                }

                if !apply_burst_chunk(state, &resp, progress)? {
                    continue;
                }

                *retries = 0;
                cycle_packets += 1;
                state.total_packets += 1;

                if resp.burst_complete == 1 {
                    let cycle_time = burst_cycle_start.elapsed();
                    let burst_kb = cycle_packets as f64 * FTP_DATA_MAX_LEN as f64 / 1024.0;
                    debug!(
                        "Burst cycle: {cycle_packets} packets, {burst_kb:.1} KB in {:.3}s ({:.1} KB/s)",
                        cycle_time.as_secs_f64(),
                        burst_kb / cycle_time.as_secs_f64()
                    );
                    return Ok(BurstCycleOutcome::Continue);
                }
            }
            Ok(Err(error)) => {
                warn!("recv_ftp error: {error}");
            }
            Err(_) => {
                warn!(
                    "Burst timeout at offset={}, retries={retries}",
                    state.contiguous_end,
                );
                *retries += 1;
                if *retries > FTP_MAX_RETRIES {
                    terminate_session_best_effort(
                        system_id,
                        component_id,
                        session,
                        "after burst timeout exhaustion",
                    )
                    .await;
                    return Err(FtpClientError::TimedOut.into());
                }
                return Ok(BurstCycleOutcome::Continue);
            }
        }
    }
}

#[instrument(level = "debug", skip(state, resp, progress))]
fn apply_burst_chunk(
    state: &mut BurstReadState,
    resp: &FtpPayload,
    progress: Option<&AtomicU64>,
) -> Result<bool> {
    let chunk_size = resp.size as usize;
    if chunk_size == 0 {
        return Ok(false);
    }

    let chunk_offset = resp.offset;
    if chunk_offset > state.contiguous_end {
        state.missing_blocks.push(MissingBlock {
            offset: state.contiguous_end,
            size: chunk_offset - state.contiguous_end,
        });
    } else if chunk_offset < state.contiguous_end {
        return Ok(false);
    }

    copy_chunk_into_buffer(&mut state.file_data, chunk_offset, chunk_size, &resp.data)?;
    state.contiguous_end = chunk_offset + chunk_size as u32;
    state.record_bytes_written(chunk_size as u32, progress);
    Ok(true)
}

#[instrument(level = "debug", skip(state, progress))]
async fn fill_missing_blocks(
    system_id: u8,
    component_id: u8,
    session: u8,
    state: &mut BurstReadState,
    progress: Option<&AtomicU64>,
) -> Result<()> {
    for missing in state.missing_blocks.clone() {
        let mut filled: u32 = 0;
        while filled < missing.size {
            match read_missing_block_chunk(
                system_id,
                component_id,
                session,
                missing,
                filled,
                state,
                progress,
            )
            .await?
            {
                GapFillChunkOutcome::Filled(chunk_size) => {
                    filled += chunk_size;
                }
                GapFillChunkOutcome::NoData | GapFillChunkOutcome::EndOfFile => {
                    break;
                }
            }
        }
    }

    Ok(())
}

#[instrument(level = "debug", skip(state, progress))]
async fn read_missing_block_chunk(
    system_id: u8,
    component_id: u8,
    session: u8,
    missing: MissingBlock,
    filled: u32,
    state: &mut BurstReadState,
    progress: Option<&AtomicU64>,
) -> Result<GapFillChunkOutcome> {
    let read_offset = missing.offset + filled;
    let read_size = (missing.size - filled).min(FTP_DATA_MAX_LEN as u32);

    let mut req = transport::new_request(FtpOpcode::ReadFile);
    req.session = session;
    req.offset = read_offset;
    req.size = read_size as u8;

    let resp = match send_and_receive(system_id, component_id, &mut req).await {
        Ok(resp) => resp,
        Err(error) => {
            terminate_session_best_effort(
                system_id,
                component_id,
                session,
                "after ReadFile repair transport failure",
            )
            .await;
            return Err(error);
        }
    };

    if let Err(nak) = check_nak(&resp) {
        if nak.err_code == FtpError::Eof {
            debug!(
                "Reached EOF while filling missing block at offset={read_offset}, filled={filled}"
            );
            return Ok(GapFillChunkOutcome::EndOfFile);
        }
        terminate_session_best_effort(
            system_id,
            component_id,
            session,
            "after ReadFile repair NAK",
        )
        .await;
        return Err(nak.into());
    }

    let chunk_size = resp.size as usize;
    if chunk_size == 0 {
        return Ok(GapFillChunkOutcome::NoData);
    }

    copy_chunk_into_buffer(&mut state.file_data, read_offset, chunk_size, &resp.data)?;
    state.record_bytes_written(chunk_size as u32, progress);
    Ok(GapFillChunkOutcome::Filled(chunk_size as u32))
}

#[instrument(level = "debug", skip(state, progress))]
async fn finish_read_session(
    system_id: u8,
    component_id: u8,
    session: u8,
    mut state: BurstReadState,
    progress: Option<&AtomicU64>,
) -> Result<Vec<u8>> {
    terminate_session(system_id, component_id, session)
        .await
        .context("TerminateSession failed after read")?;
    state.file_data.truncate(state.contiguous_end as usize);
    update_progress(progress, state.file_data.len() as u64);
    Ok(state.file_data)
}

#[instrument(level = "debug", skip(file_data, chunk_data))]
fn copy_chunk_into_buffer(
    file_data: &mut Vec<u8>,
    chunk_offset: u32,
    chunk_size: usize,
    chunk_data: &[u8],
) -> Result<()> {
    if chunk_data.len() < chunk_size {
        return Err(anyhow!(
            "FTP response declared {chunk_size} bytes but only provided {}",
            chunk_data.len()
        ));
    }

    let end = chunk_offset as usize + chunk_size;
    if end > file_data.len() {
        file_data.resize(end, 0);
    }
    file_data[chunk_offset as usize..end].copy_from_slice(&chunk_data[..chunk_size]);
    Ok(())
}

#[instrument(level = "debug", skip(progress))]
fn update_progress(progress: Option<&AtomicU64>, value: u64) {
    if let Some(progress) = progress {
        progress.store(value, Ordering::Relaxed);
    }
}

#[instrument(level = "debug")]
async fn target_lock(system_id: u8, component_id: u8) -> Arc<Mutex<()>> {
    TARGET_LOCKS
        .write()
        .await
        .entry((system_id, component_id))
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

#[instrument(level = "debug")]
async fn terminate_session(system_id: u8, component_id: u8, session: u8) -> Result<()> {
    let mut req = transport::new_request(FtpOpcode::TerminateSession);
    req.session = session;
    let resp = send_and_receive(system_id, component_id, &mut req).await?;
    check_nak(&resp).map_err(|error| anyhow!(error))?;
    Ok(())
}

#[instrument(level = "debug")]
async fn reset_sessions(system_id: u8, component_id: u8) -> Result<()> {
    let mut req = transport::new_request(FtpOpcode::ResetSessions);
    let resp = send_and_receive(system_id, component_id, &mut req).await?;
    check_nak(&resp).map_err(|error| anyhow!(error))?;
    Ok(())
}

#[instrument(level = "debug")]
async fn terminate_session_best_effort(system_id: u8, component_id: u8, session: u8, reason: &str) {
    if let Err(error) = terminate_session(system_id, component_id, session).await {
        warn!("Failed to terminate FTP session {reason}: {error}");
    }
}

struct OpenReadSession {
    session: u8,
    file_size: usize,
}

struct BurstReadState {
    file_data: Vec<u8>,
    // `contiguous_end` tracks the next offset the burst phase expects to extend.
    contiguous_end: u32,
    // `bytes_written` tracks all copied bytes, including later gap repair.
    bytes_written: u32,
    missing_blocks: Vec<MissingBlock>,
    total_packets: u64,
}

impl BurstReadState {
    fn new(file_size: usize) -> Self {
        Self {
            file_data: vec![0u8; file_size],
            contiguous_end: 0,
            bytes_written: 0,
            missing_blocks: Vec::new(),
            total_packets: 0,
        }
    }

    fn record_bytes_written(&mut self, chunk_size: u32, progress: Option<&AtomicU64>) {
        self.bytes_written += chunk_size;
        update_progress(progress, self.bytes_written as u64);
    }
}

enum BurstCycleOutcome {
    Continue,
    Eof,
}

enum GapFillChunkOutcome {
    Filled(u32),
    NoData,
    EndOfFile,
}

#[derive(Clone, Copy, Debug)]
struct MissingBlock {
    offset: u32,
    size: u32,
}
