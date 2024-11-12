use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use ringbuffer::ConstGenericRingBuffer;

const BUFFERS_CAPACITY: usize = 128;

pub type VehiclesMessages =
    BTreeMap<VehicleID, BTreeMap<ComponentID, BTreeMap<MessageID, MessageInfo>>>;
pub type VehicleID = u8;
pub type ComponentID = u8;
pub type MessageID = u32;

#[derive(Clone)]
pub struct MessageInfo {
    pub name: String,
    pub last_sample_time: DateTime<Utc>,
    pub fields: BTreeMap<String, FieldInfo<f64>>,
}

#[derive(Clone, Default, Debug)]
pub struct FieldInfo<T>
where
    f64: std::convert::From<T>,
    T: Copy + std::fmt::Debug,
{
    pub history: ConstGenericRingBuffer<(DateTime<Utc>, T), BUFFERS_CAPACITY>,
}
