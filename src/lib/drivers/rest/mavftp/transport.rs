use std::sync::{
    Arc,
    atomic::{AtomicU16, Ordering},
};

use anyhow::{Result, anyhow};
use tokio::sync::broadcast;
use tracing::*;

use crate::{hub, protocol::Protocol};

use super::protocol::{FtpOpcode, FtpPayload};

const FTP_MESSAGE_ID: u32 = 110;
static SEQ_NUMBER: AtomicU16 = AtomicU16::new(0);

#[instrument(level = "debug", fields(opcode = ?opcode))]
pub(super) fn new_request(opcode: FtpOpcode) -> FtpPayload {
    FtpPayload::new_request(opcode, next_seq())
}

#[instrument(level = "debug")]
pub(super) async fn subscribe() -> Result<broadcast::Receiver<Arc<Protocol>>> {
    let sender = hub::sender().await?;
    Ok(sender.subscribe())
}

#[instrument(
    level = "debug",
    skip(payload),
    fields(opcode = ?payload.opcode, seq_number = payload.seq_number)
)]
pub(super) fn send_ftp_message(target_system: u8, target_component: u8, payload: &FtpPayload) {
    crate::drivers::rest::control::send_mavlink_message(build_ftp_message(
        target_system,
        target_component,
        payload,
    ));
}

#[instrument(level = "debug", skip(receiver))]
pub(super) async fn recv_ftp(
    receiver: &mut broadcast::Receiver<Arc<Protocol>>,
    target_system: u8,
    target_component: u8,
    expected_seq: Option<u16>,
) -> Result<FtpPayload> {
    loop {
        match receiver.recv().await {
            Ok(protocol) => {
                if protocol.message_id() != FTP_MESSAGE_ID
                    || *protocol.system_id() != target_system
                    || *protocol.component_id() != target_component
                {
                    continue;
                }

                let Ok((_header, msg)) = protocol
                    .to_mavlink::<mavlink::ardupilotmega::MavMessage>()
                    .await
                else {
                    continue;
                };

                if let mavlink::ardupilotmega::MavMessage::FILE_TRANSFER_PROTOCOL(ftp) = msg {
                    let resp = FtpPayload::decode(&ftp.payload)?;
                    if let Some(seq) = expected_seq
                        && (resp.seq_number != seq + 1
                            || (resp.opcode != FtpOpcode::Ack && resp.opcode != FtpOpcode::Nak))
                    {
                        continue;
                    }
                    return Ok(resp);
                }
            }
            Err(broadcast::error::RecvError::Lagged(count)) => {
                warn!("FTP hub receiver lagged by {count} messages");
                continue;
            }
            Err(broadcast::error::RecvError::Closed) => {
                return Err(anyhow!("Hub broadcast channel closed"));
            }
        }
    }
}

#[instrument(
    level = "debug",
    skip(payload),
    fields(opcode = ?payload.opcode, seq_number = payload.seq_number)
)]
fn build_ftp_message(
    target_system: u8,
    target_component: u8,
    payload: &FtpPayload,
) -> mavlink::ardupilotmega::MavMessage {
    mavlink::ardupilotmega::MavMessage::FILE_TRANSFER_PROTOCOL(
        mavlink::ardupilotmega::FILE_TRANSFER_PROTOCOL_DATA {
            target_network: 0,
            target_system,
            target_component,
            payload: payload.encode(),
        },
    )
}

#[instrument(level = "debug")]
fn next_seq() -> u16 {
    SEQ_NUMBER.fetch_add(1, Ordering::Relaxed)
}
