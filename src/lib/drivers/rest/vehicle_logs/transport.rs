use std::sync::Arc;

use anyhow::{Result, anyhow};
use mavlink::MessageData;
use tokio::sync::broadcast;
use tracing::*;

use crate::{hub, protocol::Protocol};

use super::protocol::LogDataPacket;

#[instrument(level = "debug")]
pub(super) async fn subscribe_hub() -> Result<broadcast::Receiver<Arc<Protocol>>> {
    let sender = hub::sender().await?;
    Ok(sender.subscribe())
}

#[instrument(level = "debug")]
pub(super) fn send_log_request_list(system_id: u8, component_id: u8, start: u16, end: u16) {
    let msg = mavlink::ardupilotmega::MavMessage::LOG_REQUEST_LIST(
        mavlink::ardupilotmega::LOG_REQUEST_LIST_DATA {
            target_system: system_id,
            target_component: component_id,
            start,
            end,
        },
    );
    crate::drivers::rest::control::send_mavlink_message(msg);
}

#[instrument(level = "debug")]
pub(super) fn send_log_request_end(system_id: u8, component_id: u8) {
    let msg = mavlink::ardupilotmega::MavMessage::LOG_REQUEST_END(
        mavlink::ardupilotmega::LOG_REQUEST_END_DATA {
            target_system: system_id,
            target_component: component_id,
        },
    );
    crate::drivers::rest::control::send_mavlink_message(msg);
}

#[instrument(level = "debug")]
pub(super) fn send_log_erase(system_id: u8, component_id: u8) {
    let msg =
        mavlink::ardupilotmega::MavMessage::LOG_ERASE(mavlink::ardupilotmega::LOG_ERASE_DATA {
            target_system: system_id,
            target_component: component_id,
        });
    crate::drivers::rest::control::send_mavlink_message(msg);
}

#[instrument(level = "debug")]
pub(super) fn send_log_request_data(
    system_id: u8,
    component_id: u8,
    log_id: u16,
    ofs: u32,
    count: u32,
) {
    debug!("Sending LOG_REQUEST_DATA: id={log_id} ofs={ofs} count={count}");
    let msg = mavlink::ardupilotmega::MavMessage::LOG_REQUEST_DATA(
        mavlink::ardupilotmega::LOG_REQUEST_DATA_DATA {
            target_system: system_id,
            target_component: component_id,
            id: log_id,
            ofs,
            count,
        },
    );
    crate::drivers::rest::control::send_mavlink_message(msg);
}

/// Receive `LOG_ENTRY` messages from the hub matching the requested target.
#[instrument(level = "debug", skip(receiver))]
pub(super) async fn recv_log_entry(
    receiver: &mut broadcast::Receiver<Arc<Protocol>>,
    system_id: u8,
    component_id: u8,
) -> Result<mavlink::ardupilotmega::LOG_ENTRY_DATA> {
    loop {
        match receiver.recv().await {
            Ok(protocol) => {
                if protocol.message_id() != mavlink::ardupilotmega::LOG_ENTRY_DATA::ID
                    || *protocol.system_id() != system_id
                    || *protocol.component_id() != component_id
                {
                    continue;
                }
                let Ok((_header, msg)) = protocol
                    .to_mavlink::<mavlink::ardupilotmega::MavMessage>()
                    .await
                else {
                    continue;
                };
                if let mavlink::ardupilotmega::MavMessage::LOG_ENTRY(entry) = msg {
                    return Ok(entry);
                }
            }
            Err(broadcast::error::RecvError::Lagged(count)) => {
                warn!("Log hub receiver lagged by {count} messages");
                continue;
            }
            Err(broadcast::error::RecvError::Closed) => {
                return Err(anyhow!("Hub broadcast channel closed"));
            }
        }
    }
}

/// Receive a `LOG_DATA` packet from the hub matching the requested target and log id.
#[instrument(level = "debug", skip(receiver))]
pub(super) async fn recv_log_data(
    receiver: &mut broadcast::Receiver<Arc<Protocol>>,
    system_id: u8,
    component_id: u8,
    log_id: u16,
) -> Result<LogDataPacket> {
    loop {
        match receiver.recv().await {
            Ok(protocol) => {
                if protocol.message_id() != mavlink::ardupilotmega::LOG_DATA_DATA::ID
                    || *protocol.system_id() != system_id
                    || *protocol.component_id() != component_id
                {
                    continue;
                }
                let Ok((_header, msg)) = protocol
                    .to_mavlink::<mavlink::ardupilotmega::MavMessage>()
                    .await
                else {
                    continue;
                };
                if let mavlink::ardupilotmega::MavMessage::LOG_DATA(data) = msg
                    && data.id == log_id
                {
                    return Ok(LogDataPacket::from_mavlink(&data));
                }
            }
            Err(broadcast::error::RecvError::Lagged(count)) => {
                warn!("Log hub receiver lagged by {count} messages");
                continue;
            }
            Err(broadcast::error::RecvError::Closed) => {
                return Err(anyhow!("Hub broadcast channel closed"));
            }
        }
    }
}
