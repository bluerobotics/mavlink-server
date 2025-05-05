use super::{VehicleType, arducopter, arduplane, ardurover, ardusub};
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, strum_macros::EnumString, strum_macros::Display)]
pub enum FirmwareType {
    #[strum(serialize = "Copter")]
    Copter,
    #[strum(serialize = "Plane")]
    Plane,
    #[strum(serialize = "Rover")]
    Rover,
    #[strum(serialize = "Sub")]
    Submarine,
}

pub fn flight_mode(
    vehicle_type: VehicleType,
    base_mode: mavlink::ardupilotmega::MavModeFlag,
    custom_mode: u32,
) -> String {
    if base_mode.bits() == 0 {
        "pre-flight".to_string()
    } else {
        match firmware_type(vehicle_type) {
            FirmwareType::Copter => arducopter::CustomMode::from_u32(custom_mode).to_string(),
            FirmwareType::Plane => arduplane::CustomMode::from_u32(custom_mode).to_string(),
            FirmwareType::Rover => ardurover::CustomMode::from_u32(custom_mode).to_string(),
            FirmwareType::Submarine => ardusub::CustomMode::from_u32(custom_mode).to_string(),
        }
    }
}

pub fn firmware_type(vehicle_type: VehicleType) -> FirmwareType {
    match vehicle_type {
        // ArduSub
        VehicleType::Submarine => FirmwareType::Submarine,
        // ArduRover
        VehicleType::GroundRover | VehicleType::SurfaceBoat => FirmwareType::Rover,
        // ArduPlane
        VehicleType::FlappingWing
        | VehicleType::VtolTiltrotor
        | VehicleType::VtolTailsitterQuadrotor
        | VehicleType::VtolTailsitterDuorotor
        | VehicleType::FixedWing => FirmwareType::Plane,
        // ArduCopter
        VehicleType::Tricopter
        | VehicleType::Coaxial
        | VehicleType::Hexarotor
        | VehicleType::Helicopter
        | VehicleType::Octorotor
        | VehicleType::Dodecarotor
        | VehicleType::Quadrotor => FirmwareType::Copter,
        _ => {
            todo!("TODO: {vehicle_type:?}")
        }
    }
}

bitflags::bitflags! {
    #[derive(Serialize, Debug, Clone)]
    #[serde(transparent)]
    /// (Bitmask) Bitmask of (optional) autopilot capabilities (64 bit). If a bit is set, the autopilot supports this capability.
    pub struct Capabilities: u64 {
        /// Autopilot supports the MISSION_ITEM float message type. Note that MISSION_ITEM is deprecated, and autopilots should use MISSION_INT instead.
        const MISSION_FLOAT = 1;
        /// Autopilot supports the new param float message type. DEPRECATED: Replaced By MAV_PROTOCOL_CAPABILITY_PARAM_ENCODE_C_CAST (2022-03)
        const PARAM_FLOAT = 2;
        /// Autopilot supports MISSION_ITEM_INT scaled integer message type. Note that this flag must always be set if missions are supported, because missions must always use MISSION_ITEM_INT (rather than MISSION_ITEM, which is deprecated).
        const MISSION_INT = 4;
        /// Autopilot supports COMMAND_INT scaled integer message type.
        const COMMAND_INT = 8;
        /// Parameter protocol uses byte-wise encoding of parameter values into param_value (float) fields: https://mavlink.io/en/services/parameter.html#parameter-encoding. Note that either this flag or MAV_PROTOCOL_CAPABILITY_PARAM_ENCODE_C_CAST should be set if the parameter protocol is supported.
        const PARAM_ENCODE_BYTEWISE = 16;
        /// Autopilot supports the File Transfer Protocol v1: https://mavlink.io/en/services/ftp.html.
        const FTP = 32;
        /// Autopilot supports commanding attitude offboard.
        const SET_ATTITUDE_TARGET = 64;
        /// Autopilot supports commanding position and velocity targets in local NED frame.
        const SET_POSITION_TARGET_LOCAL_NED = 128;
        /// Autopilot supports commanding position and velocity targets in global scaled integers.
        const SET_POSITION_TARGET_GLOBAL_INT = 256;
        /// Autopilot supports terrain protocol / data handling.
        const TERRAIN = 512;
        /// Reserved for future use.
        const RESERVED3 = 1024;
        /// Autopilot supports the MAV_CMD_DO_FLIGHTTERMINATION command (flight termination).
        const FLIGHT_TERMINATION = 2048;
        /// Autopilot supports onboard compass calibration.
        const COMPASS_CALIBRATION = 4096;
        /// Autopilot supports MAVLink version 2.
        const MAVLINK2 = 8192;
        /// Autopilot supports mission fence protocol.
        const MISSION_FENCE = 16384;
        /// Autopilot supports mission rally point protocol.
        const MISSION_RALLY = 32768;
        /// Reserved for future use.
        const RESERVED2 = 65536;
        /// Parameter protocol uses C-cast of parameter values to set the param_value (float) fields: https://mavlink.io/en/services/parameter.html#parameter-encoding. Note that either this flag or MAV_PROTOCOL_CAPABILITY_PARAM_ENCODE_BYTEWISE should be set if the parameter protocol is supported.
        const PARAM_ENCODE_C_CAST = 131072;
        /// This component implements/is a gimbal manager. This means the GIMBAL_MANAGER_INFORMATION, and other messages can be requested.
        const COMPONENT_IMPLEMENTS_GIMBAL_MANAGER = 262144;
        /// Component supports locking control to a particular GCS independent of its system (via MAV_CMD_REQUEST_OPERATOR_CONTROL). WORK IN PROGRESS: Do not use in stable production environments (it may change).
        const COMPONENT_ACCEPTS_GCS_CONTROL = 524288;
    }
}
