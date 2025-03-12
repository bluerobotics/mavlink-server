use std::collections::HashMap;
use std::sync::Arc;

use super::autopilot::{self, ardupilot::Capabilities, AutoPilotType, Parameter};

use anyhow::Result;
use lazy_static::lazy_static;
use mavlink;
use serde::Serialize;
use tokio::sync::{broadcast, RwLock};
use tracing::*;

lazy_static! {
    static ref DATA: Data = Data {
        vehicles: Arc::new(RwLock::new(Default::default())),
    };
}

lazy_static! {
    static ref BROADCAST: broadcast::Sender<mavlink::ardupilotmega::MavMessage> =
        broadcast::channel(16).0;
}

lazy_static! {
    static ref BROADCAST_INNER: broadcast::Sender<(mavlink::MavHeader, mavlink::ardupilotmega::MavMessage)> =
        broadcast::channel(16).0;
}

lazy_static! {
    static ref BROADCAST_VEHICLES: broadcast::Sender<Vehicle> = broadcast::channel(16).0;
}

#[derive(Debug, Default)]
struct Data {
    vehicles: Arc<RwLock<Vehicles>>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Vehicles {
    vehicles: HashMap<u8, Vehicle>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Vehicle {
    vehicle_id: u8,
    armed: bool,
    autopilot: Option<autopilot::AutoPilotType>,
    vehicle_type: Option<autopilot::VehicleType>,
    mode: String,
    attitude: Attitude,
    position: Position,
    version: Option<Version>,
    parameters: HashMap<String, Parameter>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Version {
    capabilities: autopilot::ardupilot::Capabilities,
    version: semver::Version,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Attitude {
    pub roll: f32,
    pub pitch: f32,
    pub yaw: f32,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Position {
    pub latitude: f64,
    pub longitude: f64,
    pub altitude: f32,
}

impl Vehicle {
    pub fn update(
        &mut self,
        header: mavlink::MavHeader,
        message: mavlink::ardupilotmega::MavMessage,
    ) {
        if header.system_id != self.vehicle_id {
            return;
        }

        // Let's only take care of the vehicle and not the components
        if header.component_id != mavlink::ardupilotmega::MavComponent::MAV_COMP_ID_AUTOPILOT1 as u8
        {
            return;
        }

        let mut vehicle_updated = true;

        match message {
            mavlink::ardupilotmega::MavMessage::HEARTBEAT(heartbeat) => {
                self.armed = heartbeat.base_mode
                    & mavlink::ardupilotmega::MavModeFlag::MAV_MODE_FLAG_SAFETY_ARMED
                    == mavlink::ardupilotmega::MavModeFlag::MAV_MODE_FLAG_SAFETY_ARMED;

                self.autopilot = Some(autopilot::AutoPilotType::from_u8(heartbeat.autopilot as u8));
                self.vehicle_type = Some(autopilot::VehicleType::from_u8(heartbeat.mavtype as u8));

                if let Some(AutoPilotType::ArduPilotMega) = self.autopilot {
                    self.mode = autopilot::ardupilot::flight_mode(
                        self.vehicle_type.expect("Should have vehicle type already"),
                        heartbeat.base_mode,
                        heartbeat.custom_mode,
                    );
                }
            }
            mavlink::ardupilotmega::MavMessage::ATTITUDE(attitude) => {
                self.attitude = Attitude {
                    roll: attitude.roll,
                    pitch: attitude.pitch,
                    yaw: attitude.yaw,
                };
            }
            mavlink::ardupilotmega::MavMessage::GLOBAL_POSITION_INT(global_position) => {
                self.position = Position {
                    latitude: global_position.lat as f64 / 1e7,  //degE7
                    longitude: global_position.lon as f64 / 1e7, //degE7
                    altitude: global_position.alt as f32 / 1e3,  //mm
                };
            }
            mavlink::ardupilotmega::MavMessage::AUTOPILOT_VERSION(autopilot_version) => {
                let major = ((autopilot_version.flight_sw_version >> 24) & 0xff) as u64;
                let minor = ((autopilot_version.flight_sw_version >> 16) & 0xff) as u64;
                let patch = ((autopilot_version.flight_sw_version >> 8) & 0xff) as u64;
                self.version = Some(Version {
                    capabilities: Capabilities::from_bits_truncate(
                        autopilot_version.capabilities.bits() as u64,
                    ),
                    version: semver::Version::new(major, minor, patch),
                });

                request_parameters(self.vehicle_id, header.component_id);
                let firmware_type = autopilot::ardupilot::firmware_type(
                    self.vehicle_type.expect("Should have vehicle type already"),
                );
                let version_major_minor = format!("{}.{}", major, minor);
                let parameters = autopilot::parameters::get_parameters(
                    firmware_type.to_string(),
                    version_major_minor,
                );
            }
            mavlink::ardupilotmega::MavMessage::PARAM_VALUE(param_value) => {
                self.parameters.insert(
                    autopilot::Parameter::string_from_param_id(&param_value.param_id),
                    autopilot::Parameter::from_param_value(param_value),
                );
            }
            _ => {
                vehicle_updated = false;
            }
        }

        if vehicle_updated {
            let _ = BROADCAST_VEHICLES.send(self.clone());
        }
    }
}

fn request_parameters(vehicle_id: u8, component_id: u8) {
    let message = mavlink::ardupilotmega::MavMessage::PARAM_REQUEST_LIST(
        mavlink::ardupilotmega::PARAM_REQUEST_LIST_DATA {
            target_system: vehicle_id,
            target_component: component_id,
        },
    );

    send_mavlink_message(message);
}

pub async fn version(
    vehicle_id: Option<u8>,
    component_id: Option<u8>,
) -> Result<mavlink::ardupilotmega::MavMessage> {
    let vehicle_id = vehicle_id.unwrap_or(1); // default system_id
    let component_id =
        component_id.unwrap_or(mavlink::ardupilotmega::MavComponent::MAV_COMP_ID_AUTOPILOT1 as u8);

    let message = mavlink::ardupilotmega::MavMessage::COMMAND_LONG(
        mavlink::ardupilotmega::COMMAND_LONG_DATA {
            param1: 148.0, // AUTOPILOT_VERSION
            param2: 0.0,
            param3: 0.0,
            param4: 0.0,
            param5: 0.0,
            param6: 0.0,
            param7: 0.0,
            command: mavlink::ardupilotmega::MavCmd::MAV_CMD_REQUEST_MESSAGE,
            target_system: vehicle_id,
            target_component: component_id,
            confirmation: 0,
        },
    );

    send_mavlink_message(message);
    wait_for_message(vehicle_id, component_id, |message| {
        matches!(
            message,
            mavlink::ardupilotmega::MavMessage::AUTOPILOT_VERSION(_)
        )
    })
    .await
}

pub async fn wait_for_message<F>(
    vehicle_id: u8,
    component_id: u8,
    condition: F,
) -> Result<mavlink::ardupilotmega::MavMessage>
where
    F: Fn(&mavlink::ardupilotmega::MavMessage) -> bool,
{
    let mut receiver = BROADCAST_INNER.subscribe();
    let receive = async {
        loop {
            if let Ok((header, message)) = receiver.recv().await {
                if (header.system_id == vehicle_id && header.component_id == component_id)
                    && condition(&message)
                {
                    return Ok(message);
                }
            }
        }
    };

    tokio::time::timeout(std::time::Duration::from_secs(3), receive).await?
}

pub fn arm(vehicle_id: Option<u8>, component_id: Option<u8>, force: Option<bool>) -> Result<()> {
    generic_arming(vehicle_id, component_id, force, true)
}

pub fn disarm(vehicle_id: Option<u8>, component_id: Option<u8>, force: Option<bool>) -> Result<()> {
    generic_arming(vehicle_id, component_id, force, false)
}

pub fn generic_arming(
    vehicle_id: Option<u8>,
    component_id: Option<u8>,
    force: Option<bool>,
    arm: bool,
) -> Result<()> {
    let vehicle_id = vehicle_id.unwrap_or(1); // default system_id
    let component_id =
        component_id.unwrap_or(mavlink::ardupilotmega::MavComponent::MAV_COMP_ID_AUTOPILOT1 as u8);
    let force = force.unwrap_or(false);
    // // 21196: force arming/disarming (e.g. override preflight checks and disarming in flight)
    let force = if force { 21196.0 } else { 0.0 };
    let arm = if arm { 1.0 } else { 0.0 };
    let message = mavlink::ardupilotmega::MavMessage::COMMAND_LONG(
        mavlink::ardupilotmega::COMMAND_LONG_DATA {
            param1: arm,
            param2: force,
            param3: 0.0,
            param4: 0.0,
            param5: 0.0,
            param6: 0.0,
            param7: 0.0,
            command: mavlink::ardupilotmega::MavCmd::MAV_CMD_COMPONENT_ARM_DISARM,
            target_system: vehicle_id,
            target_component: component_id,
            confirmation: 0,
        },
    );

    send_mavlink_message(message);
    Ok(())
}

pub fn send_mavlink_message(message: mavlink::ardupilotmega::MavMessage) {
    if let Err(e) = BROADCAST.send(message) {
        error!("Failed to send mavlink message: {e:?}");
    }
}

pub fn subscribe_mavlink_message() -> broadcast::Receiver<mavlink::ardupilotmega::MavMessage> {
    BROADCAST.subscribe()
}

pub fn subscribe_vehicles() -> broadcast::Receiver<Vehicle> {
    BROADCAST_VEHICLES.subscribe()
}

pub async fn update((header, message): (mavlink::MavHeader, mavlink::ardupilotmega::MavMessage)) {
    let mut vehicles = DATA.vehicles.write().await;
    let vehicle = vehicles
        .vehicles
        .entry(header.system_id)
        .or_insert(Vehicle {
            vehicle_id: header.system_id,
            ..Default::default()
        });
    vehicle.update(header, message.clone());
    let _ = BROADCAST_INNER.send((header, message));
}

pub async fn vehicles() -> Vec<Vehicle> {
    let vehicles = DATA.vehicles.read().await;
    vehicles.vehicles.values().cloned().collect()
}
