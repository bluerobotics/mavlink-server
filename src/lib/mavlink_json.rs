use serde::{Deserialize, Serialize};

/// Improved and back-compatible with our previous struct called `MAVLinkMessage`
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
pub struct MAVLinkJSON<T: mavlink::Message> {
    pub header: MAVLinkJSONHeader,
    pub message: T,
}

/// Improved and back-compatible with mavlink::MavHeader
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
pub struct MAVLinkJSONHeader {
    #[serde(flatten)]
    /// The original MavHeader
    pub inner: mavlink::MavHeader,
    /// Optional Message ID, missing in the original header
    pub message_id: Option<u32>,
}
