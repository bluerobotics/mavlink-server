use std::ops::{Deref, DerefMut};

use mavlink_codec::Packet;
use serde::Serialize;

#[derive(Debug, PartialEq, Serialize)]
pub struct Protocol {
    pub origin: String,
    pub timestamp: u64,
    #[serde(skip)]
    packet: Packet,
}

impl Protocol {
    pub fn new(origin: &str, packet: Packet) -> Self {
        Self {
            origin: origin.to_string(),
            timestamp: chrono::Utc::now().timestamp_micros() as u64,
            packet,
        }
    }

    pub fn new_with_timestamp(timestamp: u64, origin: &str, packet: Packet) -> Self {
        Self {
            origin: origin.to_string(),
            timestamp,
            packet,
        }
    }
}

impl Deref for Protocol {
    type Target = Packet;

    fn deref(&self) -> &Self::Target {
        &self.packet
    }
}

impl DerefMut for Protocol {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.packet
    }
}
