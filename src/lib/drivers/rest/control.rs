use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use super::autopilot::{
    self, AutoPilotType, Parameter,
    ardupilot::{Capabilities, FirmwareType},
    parameters::Parameter as ParameterMetadata,
};

use anyhow::Result;
use lazy_static::lazy_static;
use mavlink;
use serde::Serialize;
use tokio::sync::{RwLock, broadcast};
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

lazy_static! {
    static ref BROADCAST_VEHICLES_PARAMETERS: broadcast::Sender<HashMap<u8, HashMap<String, ParameterData>>> =
        broadcast::channel(16).0;
}

#[derive(Debug, Default)]
struct Data {
    vehicles: Arc<RwLock<Vehicles>>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Vehicles {
    vehicles: HashMap<u8, Vehicle>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Vehicle {
    vehicle_id: u8,
    components: HashMap<u8, VehicleComponents>,
}

impl Default for Vehicle {
    fn default() -> Self {
        Self {
            // Default vehicle id expected, can be different for multiple vehicles in the network
            vehicle_id: 1,
            // Every vehicle should have an autopilot component
            // https://mavlink.io/en/messages/common.html#MAV_COMP_ID_AUTOPILOT1
            components: HashMap::from([(
                mavlink::ardupilotmega::MavComponent::MAV_COMP_ID_AUTOPILOT1 as u8,
                VehicleComponents::Autopilot(VehicleComponent::default()),
            )]),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type")]
pub enum VehicleComponents {
    Autopilot(VehicleComponent),
}

#[derive(Debug, Serialize)]
pub struct VehicleComponent {
    component_id: u8,
    armed: bool,
    autopilot: Option<autopilot::AutoPilotType>,
    vehicle_type: Option<autopilot::VehicleType>,
    mode: String,
    attitude: Attitude,
    position: Position,
    version: Option<Version>,

    // Inner logic control
    #[serde(skip_serializing)]
    context: VehicleContext,
}

impl Clone for VehicleComponent {
    fn clone(&self) -> Self {
        Self {
            component_id: self.component_id,
            armed: self.armed,
            autopilot: self.autopilot,
            vehicle_type: self.vehicle_type,
            mode: self.mode.clone(),
            attitude: self.attitude.clone(),
            position: self.position.clone(),
            version: self.version.clone(),
            // We don't clone the context because is for inner logic
            context: Default::default(),
        }
    }
}

impl Default for VehicleComponent {
    fn default() -> Self {
        Self {
            component_id: 1,
            armed: false,
            autopilot: None,
            vehicle_type: None,
            mode: String::new(),
            attitude: Attitude::default(),
            position: Position::default(),
            version: None,
            context: VehicleContext::default(),
        }
    }
}

// This should not implement Clone because it is for inner logic and it's quite heavy
#[derive(Debug)]
pub struct VehicleContext {
    parameters: HashMap<String, ParameterData>,
    parameters_metadata: HashMap<String, ParameterMetadata>,
    firmware_type: Option<FirmwareType>,
    last_update: Instant,
}

impl Default for VehicleContext {
    fn default() -> Self {
        Self {
            parameters: Default::default(),
            parameters_metadata: Default::default(),
            firmware_type: Default::default(),
            last_update: Instant::now(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ParameterData {
    parameter: Parameter,
    metadata: Option<ParameterMetadata>,
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
    pub async fn update(
        &mut self,
        header: mavlink::MavHeader,
        message: mavlink::ardupilotmega::MavMessage,
    ) {
        if header.system_id != self.vehicle_id {
            return;
        }

        if header.component_id != mavlink::ardupilotmega::MavComponent::MAV_COMP_ID_AUTOPILOT1 as u8
        {
            return;
        }

        let component = match self.components.entry(header.component_id) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::hash_map::Entry::Vacant(_) => return,
        };
        let VehicleComponents::Autopilot(component) = component;

        let mut vehicle_updated = true;

        match message {
            mavlink::ardupilotmega::MavMessage::HEARTBEAT(heartbeat) => {
                component.armed = heartbeat.base_mode
                    & mavlink::ardupilotmega::MavModeFlag::MAV_MODE_FLAG_SAFETY_ARMED
                    == mavlink::ardupilotmega::MavModeFlag::MAV_MODE_FLAG_SAFETY_ARMED;

                component.autopilot =
                    Some(autopilot::AutoPilotType::from_u8(heartbeat.autopilot as u8));
                component.vehicle_type =
                    Some(autopilot::VehicleType::from_u8(heartbeat.mavtype as u8));

                if let Some(AutoPilotType::ArduPilotMega) = component.autopilot {
                    component.mode = autopilot::ardupilot::flight_mode(
                        component
                            .vehicle_type
                            .expect("Should have vehicle type already"),
                        heartbeat.base_mode,
                        heartbeat.custom_mode,
                    );
                }

                // Request version to take care of initial values
                if component.version.is_none() {
                    send_version_request(self.vehicle_id, header.component_id);
                }
            }
            mavlink::ardupilotmega::MavMessage::ATTITUDE(attitude) => {
                component.attitude = Attitude {
                    roll: attitude.roll,
                    pitch: attitude.pitch,
                    yaw: attitude.yaw,
                };
            }
            mavlink::ardupilotmega::MavMessage::GLOBAL_POSITION_INT(global_position) => {
                component.position = Position {
                    latitude: global_position.lat as f64 / 1e7,  //degE7
                    longitude: global_position.lon as f64 / 1e7, //degE7
                    altitude: global_position.alt as f32 / 1e3,  //mm
                };
            }
            mavlink::ardupilotmega::MavMessage::AUTOPILOT_VERSION(autopilot_version) => {
                let major = ((autopilot_version.flight_sw_version >> 24) & 0xff) as u64;
                let minor = ((autopilot_version.flight_sw_version >> 16) & 0xff) as u64;
                let patch = ((autopilot_version.flight_sw_version >> 8) & 0xff) as u64;
                let version = Some(Version {
                    capabilities: Capabilities::from_bits_truncate(
                        autopilot_version.capabilities.bits(),
                    ),
                    version: semver::Version::new(major, minor, patch),
                });

                let firmware_type = autopilot::ardupilot::firmware_type(
                    component
                        .vehicle_type
                        .expect("Should have vehicle type already"),
                );
                let version_major_minor = format!("{}.{}", major, minor);

                // First time requesting parameters
                if component.version.is_none() {
                    component.version = version;
                    component.context.firmware_type = Some(firmware_type.clone());

                    component.context.parameters_metadata = autopilot::parameters::get_parameters(
                        firmware_type.to_string(),
                        version_major_minor,
                    );
                    request_parameters(self.vehicle_id, header.component_id);
                } else if let Some(version) = version {
                    // Version or vehicle type changed
                    let self_version = component.version.as_ref().unwrap();
                    if self_version.version != version.version
                        || component.context.firmware_type != Some(firmware_type.clone())
                    {
                        component.version = Some(version);
                        component.context.firmware_type = Some(firmware_type.clone());

                        component.context.parameters_metadata =
                            autopilot::parameters::get_parameters(
                                firmware_type.to_string(),
                                version_major_minor,
                            );
                        request_parameters(self.vehicle_id, header.component_id);
                    }
                }
            }
            mavlink::ardupilotmega::MavMessage::PARAM_VALUE(param_value) => {
                let parameter_name =
                    autopilot::Parameter::string_from_param_id(&param_value.param_id);
                component.context.parameters.insert(
                    parameter_name.clone(),
                    ParameterData {
                        parameter: autopilot::Parameter::from_param_value(param_value),
                        metadata: component
                            .context
                            .parameters_metadata
                            .get(&parameter_name)
                            .cloned(),
                    },
                );

                // We update only a single parameter via websocket
                let mut parameter = HashMap::new();
                let mut vehicle = HashMap::new();
                parameter.insert(
                    parameter_name.clone(),
                    component
                        .context
                        .parameters
                        .get(&parameter_name)
                        .cloned()
                        .unwrap(),
                );
                vehicle.insert(self.vehicle_id, parameter);
                let _ = BROADCAST_VEHICLES_PARAMETERS.send(vehicle);
            }
            _ => {
                vehicle_updated = false;
            }
        }

        if vehicle_updated {
            // Limit update to 16Hz
            if component.context.last_update.elapsed() > std::time::Duration::from_millis(1000 / 16)
            {
                component.context.last_update = Instant::now();
                let vehicle_clone = self.clone();
                let _ = BROADCAST_VEHICLES.send(vehicle_clone);
            }
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

pub async fn set_parameter(
    vehicle_id: Option<u8>,
    component_id: Option<u8>,
    parameter_name: String,
    value: f64,
) -> Result<mavlink::ardupilotmega::MavMessage, String> {
    let vehicle_id = vehicle_id.unwrap_or(1); // default system_id
    let component_id =
        component_id.unwrap_or(mavlink::ardupilotmega::MavComponent::MAV_COMP_ID_AUTOPILOT1 as u8);

    let message =
        mavlink::ardupilotmega::MavMessage::PARAM_SET(mavlink::ardupilotmega::PARAM_SET_DATA {
            param_value: value as f32,
            target_system: vehicle_id,
            target_component: component_id,
            param_id: parameter_name.as_str().into(),
            param_type: mavlink::ardupilotmega::MavParamType::MAV_PARAM_TYPE_REAL64,
        });

    send_mavlink_message(message);
    wait_for_message(vehicle_id, component_id, |message| {
        if let mavlink::ardupilotmega::MavMessage::PARAM_VALUE(param_value) = message {
            Parameter::string_from_param_id(&param_value.param_id) == parameter_name
        } else {
            false
        }
    })
    .await
    .map_err(|error| error.to_string())
}

pub async fn version(
    vehicle_id: Option<u8>,
    component_id: Option<u8>,
) -> Result<mavlink::ardupilotmega::MavMessage, String> {
    let vehicle_id = vehicle_id.unwrap_or(1); // default system_id
    let component_id =
        component_id.unwrap_or(mavlink::ardupilotmega::MavComponent::MAV_COMP_ID_AUTOPILOT1 as u8);

    send_version_request(vehicle_id, component_id);
    wait_for_message(vehicle_id, component_id, |message| {
        matches!(
            message,
            mavlink::ardupilotmega::MavMessage::AUTOPILOT_VERSION(_)
        )
    })
    .await
    // We are returning the anyhow error as a simple string to workaround a panic that was happening when propagating the error.
    .map_err(|error| error.to_string())
}

pub fn send_version_request(vehicle_id: u8, component_id: u8) {
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

pub fn subscribe_parameters() -> broadcast::Receiver<HashMap<u8, HashMap<String, ParameterData>>> {
    BROADCAST_VEHICLES_PARAMETERS.subscribe()
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
    vehicle.update(header, message.clone()).await;
    let _ = BROADCAST_INNER.send((header, message));
}

pub async fn vehicles() -> Vec<Vehicle> {
    let vehicles = DATA.vehicles.read().await;
    vehicles.vehicles.values().cloned().collect()
}

pub async fn parameters() -> HashMap<u8, HashMap<String, ParameterData>> {
    let vehicles = DATA.vehicles.read().await;
    vehicles
        .vehicles
        .iter()
        .map(|(vehicle_id, vehicle)| {
            let parameters = vehicle
                .components
                .get(&(mavlink::ardupilotmega::MavComponent::MAV_COMP_ID_AUTOPILOT1 as u8))
                .map(|component| {
                    #[allow(irrefutable_let_patterns)]
                    if let VehicleComponents::Autopilot(component) = component {
                        component.context.parameters.clone()
                    } else {
                        HashMap::new()
                    }
                })
                .unwrap_or_default();
            (*vehicle_id, parameters)
        })
        .collect()
}
