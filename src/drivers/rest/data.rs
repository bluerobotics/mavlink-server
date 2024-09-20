use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use lazy_static::lazy_static;
use mavlink::{self, Message};
use serde::{Deserialize, Serialize};

lazy_static! {
    static ref DATA: Data = Data {
        messages: Arc::new(Mutex::new(MAVLinkVehiclesData::default())),
    };
}

#[derive(Debug)]
struct Data {
    messages: Arc<Mutex<MAVLinkVehiclesData>>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct MAVLinkVehiclesData {
    vehicles: HashMap<u8, MAVLinkVehicleData>,
}

impl MAVLinkVehiclesData {
    fn update(&mut self, message: MAVLinkMessage<mavlink::ardupilotmega::MavMessage>) {
        let vehicle_id = message.header.system_id;
        self.vehicles
            .entry(vehicle_id)
            .or_insert(MAVLinkVehicleData {
                id: vehicle_id,
                components: HashMap::new(),
            })
            .update(message);
    }

    fn pointer(&self, path: &str) -> String {
        if path.is_empty() {
            return serde_json::to_string_pretty(self).unwrap();
        }

        let path = format!("/{path}");

        dbg!(&path);

        if path == "/vehicles" {
            return serde_json::to_string_pretty(&self.vehicles).unwrap();
        };

        let value = serde_json::to_value(self).unwrap();
        return match value.pointer(&path) {
            Some(content) => serde_json::to_string_pretty(content).unwrap(),
            None => "None".into(),
        };
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct MAVLinkVehicleData {
    id: u8,
    components: HashMap<u8, MAVLinkVehicleComponentData>,
}

impl MAVLinkVehicleData {
    fn update(&mut self, message: MAVLinkMessage<mavlink::ardupilotmega::MavMessage>) {
        let component_id = message.header.component_id;
        self.components
            .entry(component_id)
            .or_insert(MAVLinkVehicleComponentData {
                id: component_id,
                messages: HashMap::new(),
            })
            .update(message);
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct MAVLinkVehicleComponentData {
    id: u8,
    messages: HashMap<String, MAVLinkMessageStatus>,
}

impl MAVLinkVehicleComponentData {
    fn update(&mut self, message: MAVLinkMessage<mavlink::ardupilotmega::MavMessage>) {
        let message_name = message.message.message_name().to_string();
        self.messages
            .entry(message_name)
            .or_insert(MAVLinkMessageStatus {
                message: message.message.clone(),
                status: Status::default(),
            })
            .update(message);
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct MAVLinkMessageStatus {
    message: mavlink::ardupilotmega::MavMessage,
    status: Status,
}

impl MAVLinkMessageStatus {
    fn update(&mut self, message: MAVLinkMessage<mavlink::ardupilotmega::MavMessage>) {
        self.message = message.message;
        self.status.update();
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct MAVLinkMessage<T> {
    pub header: mavlink::MavHeader,
    pub message: T,
}

#[derive(Default, Debug, Deserialize, Serialize)]
struct Status {
    time: Temporal,
}

impl Status {
    fn update(&mut self) -> &mut Self {
        self.time.update();
        self
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct Temporal {
    first_update_us: u64,
    last_update_us: u64,
    counter: u64,
    frequency: f32,
}

impl Default for Temporal {
    fn default() -> Self {
        let now_us = chrono::Local::now().timestamp_micros() as u64;
        Self {
            first_update_us: now_us,
            last_update_us: now_us,
            counter: 1,
            frequency: 0.0,
        }
    }
}

impl Temporal {
    fn update(&mut self) {
        self.last_update_us = chrono::Local::now().timestamp_micros() as u64;
        self.counter = self.counter.wrapping_add(1);
        self.frequency =
            (10e6 * self.counter as f32) / ((self.last_update_us - self.first_update_us) as f32);
    }
}

pub fn update((header, message): (mavlink::MavHeader, mavlink::ardupilotmega::MavMessage)) {
    DATA.messages
        .lock()
        .unwrap()
        .update(MAVLinkMessage { header, message });
}

pub fn messages(path: &str) -> String {
    DATA.messages.lock().unwrap().pointer(path)
}
