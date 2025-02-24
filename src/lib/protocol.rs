use std::ops::{Deref, DerefMut};

use anyhow::Result;
use mavlink_codec::Packet;
use serde::Serialize;

use crate::{
    cli,
    mavlink_json::{MAVLinkJSON, MAVLinkJSONHeader},
};

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

    pub fn from_mavlink_raw<M>(header: mavlink::MavHeader, message: &M, origin: &str) -> Self
    where
        M: mavlink::Message,
    {
        let packet = match cli::mavlink_version() {
            1 => {
                let mut message_raw = mavlink::MAVLinkV1MessageRaw::new();
                message_raw.serialize_message(header, message);
                Packet::from(message_raw)
            }
            2 => {
                let mut message_raw = mavlink::MAVLinkV2MessageRaw::new();
                message_raw.serialize_message(header, message);
                Packet::from(message_raw)
            }
            _ => unreachable!(),
        };

        Self {
            origin: origin.to_string(),
            timestamp: chrono::Utc::now().timestamp_micros() as u64,
            packet,
        }
    }

    pub async fn to_mavlink<M>(&self) -> Result<(mavlink::MavHeader, M)>
    where
        M: mavlink::Message,
    {
        let mut reader = mavlink::async_peek_reader::AsyncPeekReader::new(self.as_slice());

        match cli::mavlink_version() {
            1 => mavlink::read_v1_msg_async::<M, _>(&mut reader).await,
            2 => mavlink::read_v2_msg_async::<M, _>(&mut reader).await,
            _ => unreachable!(),
        }
        .map_err(anyhow::Error::msg)
    }

    pub async fn to_mavlink_json<M>(&self) -> Result<MAVLinkJSON<M>>
    where
        M: mavlink::Message,
    {
        let (header, message) = self.to_mavlink().await?;

        let header = MAVLinkJSONHeader {
            inner: header,
            message_id: Some(mavlink::Message::message_id(&message)),
        };

        Ok(MAVLinkJSON { header, message })
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
