use serde::{Deserialize, Serialize};

pub mod arducopter;
pub mod ardupilot;
pub mod arduplane;
pub mod ardurover;
pub mod ardusub;
pub mod parameters;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[repr(u8)]
pub enum AutoPilotType {
    /// Generic autopilot, full support for everything
    Generic = 0,
    /// Reserved for future use
    Reserved = 1,
    /// SLUGS autopilot, http://slugsuav.soe.ucsc.edu
    Slugs = 2,
    /// ArduPilot - Plane/Copter/Rover/Sub/Tracker, https://ardupilot.org
    ArduPilotMega = 3,
    /// OpenPilot, http://openpilot.org
    OpenPilot = 4,
    /// Generic autopilot only supporting simple waypoints
    GenericWaypointsOnly = 5,
    /// Generic autopilot supporting waypoints and other simple navigation commands
    GenericWaypointsAndSimpleNavigation = 6,
    /// Generic autopilot supporting the full mission command set
    GenericMissionFull = 7,
    /// No valid autopilot, e.g. a GCS or other MAVLink component
    Invalid = 8,
    /// PPZ UAV - http://nongnu.org/paparazzi
    Ppz = 9,
    /// UAV Dev Board
    Udb = 10,
    /// FlexiPilot
    Fp = 11,
    /// PX4 Autopilot - http://px4.io/
    Px4 = 12,
    /// SMACCMPilot - http://smaccmpilot.org
    SmaccmPilot = 13,
    /// AutoQuad -- http://autoquad.org
    AutoQuad = 14,
    /// Armazila -- http://armazila.com
    Armazila = 15,
    /// Aerob -- http://aerob.ru
    Aerob = 16,
    /// ASLUAV autopilot -- http://www.asl.ethz.ch
    Asluav = 17,
    /// SmartAP Autopilot - http://sky-drones.com
    SmartAp = 18,
    /// AirRails - http://uaventure.com
    AirRails = 19,
    /// Fusion Reflex - https://fusion.engineering
    ReflexFusion = 20,
    /// Unknown
    Unknown(u8),
}

impl AutoPilotType {
    pub fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::Generic,
            1 => Self::Reserved,
            2 => Self::Slugs,
            3 => Self::ArduPilotMega,
            4 => Self::OpenPilot,
            5 => Self::GenericWaypointsOnly,
            6 => Self::GenericWaypointsAndSimpleNavigation,
            7 => Self::GenericMissionFull,
            8 => Self::Invalid,
            9 => Self::Ppz,
            10 => Self::Udb,
            11 => Self::Fp,
            12 => Self::Px4,
            13 => Self::SmaccmPilot,
            14 => Self::AutoQuad,
            15 => Self::Armazila,
            16 => Self::Aerob,
            17 => Self::Asluav,
            18 => Self::SmartAp,
            19 => Self::AirRails,
            20 => Self::ReflexFusion,
            _ => Self::Unknown(value),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[repr(u8)]
pub enum VehicleType {
    /// Generic micro air vehicle
    Generic = 0,
    /// Fixed wing aircraft
    FixedWing = 1,
    /// Quadrotor
    Quadrotor = 2,
    /// Coaxial helicopter
    Coaxial = 3,
    /// Normal helicopter with tail rotor
    Helicopter = 4,
    /// Ground installation
    AntennaTracker = 5,
    /// Operator control unit / ground control station
    Gcs = 6,
    /// Airship, controlled
    Airship = 7,
    /// Free balloon, uncontrolled
    FreeBalloon = 8,
    /// Rocket
    Rocket = 9,
    /// Ground rover
    GroundRover = 10,
    /// Surface vessel, boat, ship
    SurfaceBoat = 11,
    /// Submarine
    Submarine = 12,
    /// Hexarotor
    Hexarotor = 13,
    /// Octorotor
    Octorotor = 14,
    /// Tricopter
    Tricopter = 15,
    /// Flapping wing
    FlappingWing = 16,
    /// Kite
    Kite = 17,
    /// Onboard companion controller
    OnboardController = 18,
    /// Two-rotor Tailsitter VTOL that additionally uses control surfaces in vertical operation
    VtolTailsitterDuorotor = 19,
    /// Quad-rotor Tailsitter VTOL using a V-shaped quad config in vertical operation
    VtolTailsitterQuadrotor = 20,
    /// Tiltrotor VTOL. Fuselage and wings stay horizontal in all flight phases. Can tilt rotors for cruise thrust
    VtolTiltrotor = 21,
    /// VTOL with separate fixed rotors for hover and cruise flight
    VtolFixedrotor = 22,
    /// Tailsitter VTOL. Fuselage and wings orientation changes depending on flight phase
    VtolTailsitter = 23,
    /// Tiltwing VTOL. Wing and engines can tilt between vertical and horizontal
    VtolTiltwing = 24,
    /// VTOL reserved 5
    VtolReserved5 = 25,
    /// Gimbal
    Gimbal = 26,
    /// ADSB system
    Adsb = 27,
    /// Steerable, nonrigid airfoil
    Parafoil = 28,
    /// Dodecarotor
    Dodecarotor = 29,
    /// Camera
    Camera = 30,
    /// Charging station
    ChargingStation = 31,
    /// FLARM collision avoidance system
    Flarm = 32,
    /// Servo
    Servo = 33,
    /// Open Drone ID
    Odid = 34,
    /// Decarotor
    Decarotor = 35,
    /// Battery
    Battery = 36,
    /// Parachute
    Parachute = 37,
    /// Log
    Log = 38,
    /// OSD
    Osd = 39,
    /// IMU
    Imu = 40,
    /// GPS
    Gps = 41,
    /// Winch
    Winch = 42,
    /// Generic multirotor that does not fit a specific type or is unknown
    GenericMultirotor = 43,
    /// Illuminator (external light source like torch/searchlight)
    Illuminator = 44,
    /// Unknown
    Unknown(u8),
}

impl VehicleType {
    pub fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::Generic,
            1 => Self::FixedWing,
            2 => Self::Quadrotor,
            3 => Self::Coaxial,
            4 => Self::Helicopter,
            5 => Self::AntennaTracker,
            6 => Self::Gcs,
            7 => Self::Airship,
            8 => Self::FreeBalloon,
            9 => Self::Rocket,
            10 => Self::GroundRover,
            11 => Self::SurfaceBoat,
            12 => Self::Submarine,
            13 => Self::Hexarotor,
            14 => Self::Octorotor,
            15 => Self::Tricopter,
            16 => Self::FlappingWing,
            17 => Self::Kite,
            18 => Self::OnboardController,
            19 => Self::VtolTailsitterDuorotor,
            20 => Self::VtolTailsitterQuadrotor,
            21 => Self::VtolTiltrotor,
            22 => Self::VtolFixedrotor,
            23 => Self::VtolTailsitter,
            24 => Self::VtolTiltwing,
            25 => Self::VtolReserved5,
            26 => Self::Gimbal,
            27 => Self::Adsb,
            28 => Self::Parafoil,
            29 => Self::Dodecarotor,
            30 => Self::Camera,
            31 => Self::ChargingStation,
            32 => Self::Flarm,
            33 => Self::Servo,
            34 => Self::Odid,
            35 => Self::Decarotor,
            36 => Self::Battery,
            37 => Self::Parachute,
            38 => Self::Log,
            39 => Self::Osd,
            40 => Self::Imu,
            41 => Self::Gps,
            42 => Self::Winch,
            43 => Self::GenericMultirotor,
            44 => Self::Illuminator,
            _ => Self::Unknown(value),
        }
    }
}
