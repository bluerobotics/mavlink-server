use tracing::*;

pub(super) const LOG_DATA_PACKET_SIZE: usize = 90;
pub(super) const LOG_CHUNK_BINS: usize = 2048;
pub(super) const LOG_CHUNK_SIZE: usize = LOG_CHUNK_BINS * LOG_DATA_PACKET_SIZE;

/// A `LOG_DATA` packet decoded from a typed mavlink message. The target
/// system/component and log id are filtered upstream by `recv_log_data`, so
/// only the bytes that drive the chunk state machine are kept here.
#[derive(Clone, Debug)]
pub(super) struct LogDataPacket {
    pub(super) ofs: u32,
    pub(super) count: u8,
    pub(super) data: [u8; LOG_DATA_PACKET_SIZE],
}

impl LogDataPacket {
    #[instrument(level = "trace", skip(data))]
    pub(super) fn from_mavlink(data: &mavlink::ardupilotmega::LOG_DATA_DATA) -> Self {
        Self {
            ofs: data.ofs,
            count: data.count,
            data: data.data,
        }
    }
}
