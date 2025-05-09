#[repr(u8)]
#[derive(Debug, Eq, PartialEq, strum_macros::EnumString, strum_macros::Display)]
#[strum(serialize_all = "kebab-case")]
pub enum CustomMode {
    // Mode not set by vehicle yet
    PreFlight = u8::MAX,
    // Manual control
    Manual = 0,
    // Automatic circular flight with automatic throttle
    Circle = 1,
    // Manual airframe angle with manual throttle
    Stabilize = 2,
    // Training mode
    Training = 3,
    // Manual body-frame angular rate with manual throttle
    Acro = 4,
    // Fly-By-Wire A mode
    FlyByWireA = 5,
    // Fly-By-Wire B mode
    FlyByWireB = 6,
    // Cruise mode
    Cruise = 7,
    // Automatically tune the vehicle's roll and pitch gains
    AutoTune = 8,
    // Fully automatic waypoint control using mission commands
    Auto = 10,
    // Automatic return to launching point
    Rtl = 11,
    // Automatic horizontal acceleration with automatic throttle
    Loiter = 12,
    // Automatic takeoff
    Takeoff = 13,
    // Automatic avoidance of obstacles in the macro scale - e.g. full-sized aircraft
    AvoidAdsb = 14,
    // Fully automatic fly to coordinate or fly at velocity/direction using GCS immediate commands
    Guided = 15,
    // System is initializing
    Initialising = 16,
    // QuadPlane VTOL mode - stabilize
    QStabilize = 17,
    // QuadPlane VTOL mode - hover
    QHover = 18,
    // QuadPlane VTOL mode - loiter
    QLoiter = 19,
    // QuadPlane VTOL mode - land
    QLand = 20,
    // QuadPlane VTOL mode - RTL
    QRtl = 21,
    // QuadPlane VTOL mode - autotune
    QAutoTune = 22,
    // QuadPlane VTOL mode - acro
    QAcro = 23,
    // Thermal soaring mode
    Thermal = 24,
    // Loiter to altitude then QLAND
    LoiterAltQLand = 25,
    // Unknown
    #[strum(to_string = "Unknown ({0})")]
    Unknown(u32),
}

impl CustomMode {
    pub fn from_u32(value: u32) -> Self {
        match value {
            255 => Self::PreFlight,
            0 => Self::Manual,
            1 => Self::Circle,
            2 => Self::Stabilize,
            3 => Self::Training,
            4 => Self::Acro,
            5 => Self::FlyByWireA,
            6 => Self::FlyByWireB,
            7 => Self::Cruise,
            8 => Self::AutoTune,
            10 => Self::Auto,
            11 => Self::Rtl,
            12 => Self::Loiter,
            13 => Self::Takeoff,
            14 => Self::AvoidAdsb,
            15 => Self::Guided,
            16 => Self::Initialising,
            17 => Self::QStabilize,
            18 => Self::QHover,
            19 => Self::QLoiter,
            20 => Self::QLand,
            21 => Self::QRtl,
            22 => Self::QAutoTune,
            23 => Self::QAcro,
            24 => Self::Thermal,
            25 => Self::LoiterAltQLand,
            _ => Self::Unknown(value),
        }
    }
}
