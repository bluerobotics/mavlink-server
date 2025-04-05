use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use ringbuffer::{ConstGenericRingBuffer, RingBuffer};

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
    pub fields: BTreeMap<String, FieldValue>,
}

#[derive(Clone, Debug)]
pub enum FieldValue {
    Numeric(FieldInfo<f64>),
    Text(FieldInfo<String>),
}

#[derive(Clone, Default, Debug)]
pub struct FieldInfo<T>
where
    T: Clone + std::fmt::Debug,
{
    pub history: ConstGenericRingBuffer<(DateTime<Utc>, T), BUFFERS_CAPACITY>,
}

impl<T> FieldInfo<T>
where
    T: Copy + std::fmt::Debug,
    f64: From<T>,
{
    pub fn to_f64(&self) -> FieldInfo<f64> {
        let mut new_field_info = FieldInfo::default();
        for (time, value) in self.history.iter() {
            new_field_info.history.push((*time, f64::from(*value)));
        }
        new_field_info
    }
}
