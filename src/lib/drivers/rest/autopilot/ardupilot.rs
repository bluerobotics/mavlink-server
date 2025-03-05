use super::{arducopter, arduplane, ardurover, ardusub, VehicleType};

pub fn flight_mode(
    vehicle_type: VehicleType,
    base_mode: mavlink::ardupilotmega::MavModeFlag,
    custom_mode: u32,
) -> String {
    if base_mode.bits() == 0 {
        "pre-flight".to_string()
    } else {
        match vehicle_type {
            // ArduSub
            VehicleType::Submarine => ardusub::CustomMode::from_u32(custom_mode).to_string(),
            // ArduRover
            VehicleType::GroundRover | VehicleType::SurfaceBoat => {
                ardurover::CustomMode::from_u32(custom_mode).to_string()
            }
            // ArduPlane
            VehicleType::FlappingWing
            | VehicleType::VtolTiltrotor
            | VehicleType::VtolTailsitterQuadrotor
            | VehicleType::VtolTailsitterDuorotor
            | VehicleType::FixedWing => arduplane::CustomMode::from_u32(custom_mode).to_string(),
            // ArduCopter
            VehicleType::Tricopter
            | VehicleType::Coaxial
            | VehicleType::Hexarotor
            | VehicleType::Helicopter
            | VehicleType::Octorotor
            | VehicleType::Dodecarotor
            | VehicleType::Quadrotor => arducopter::CustomMode::from_u32(custom_mode).to_string(),
            _ => {
                format!("TODO: {vehicle_type:?} {base_mode:?} {custom_mode:?}")
            }
        }
    }
}
