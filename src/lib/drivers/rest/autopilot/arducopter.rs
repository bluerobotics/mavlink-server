#[repr(u8)]
#[derive(Debug, Eq, PartialEq, strum_macros::EnumString, strum_macros::Display)]
#[strum(serialize_all = "kebab-case")]
pub enum CustomMode {
    // Mode not set by vehicle yet
    PreFlight = u8::MAX,
    // Manual airframe angle with manual throttle
    Stabilize = 0,
    // Manual body-frame angular rate with manual throttle
    Acro = 1,
    // Manual airframe angle with automatic throttle
    AltHold = 2,
    // Fully automatic waypoint control using mission commands
    Auto = 3,
    // Fully automatic fly to coordinate or fly at velocity/direction using GCS immediate commands
    Guided = 4,
    // Automatic horizontal acceleration with automatic throttle
    Loiter = 5,
    // Automatic return to launching point
    Rtl = 6,
    // Automatic circular flight with automatic throttle
    Circle = 7,
    // Automatic landing with horizontal position control
    Land = 9,
    // Semi-autonomous position, yaw and throttle control
    Drift = 11,
    // Manual earth-frame angular rate control with manual throttle
    Sport = 13,
    // Automatically flip the vehicle on the roll axis
    Flip = 14,
    // Automatically tune the vehicle's roll and pitch gains
    AutoTune = 15,
    // Automatic position hold with manual override, with automatic throttle
    PosHold = 16,
    // Full-brake using inertial/GPS system, no pilot input
    Brake = 17,
    // Throw to launch mode using inertial/GPS system, no pilot input
    Throw = 18,
    // Automatic avoidance of obstacles in the macro scale - e.g. full-sized aircraft
    AvoidAdsb = 19,
    // Guided mode but only accepts attitude and altitude
    GuidedNoGps = 20,
    // Smart_Rtl returns to home by retracing its steps
    SmartRtl = 21,
    // Flowhold holds position with optical flow without rangefinder
    FlowHold = 22,
    // Follow attempts to follow another vehicle or ground station
    Follow = 23,
    // Zigzag mode is able to fly in a zigzag manner with predefined point A and point B
    ZigZag = 24,
    // System ID mode produces automated system identification signals in the controllers
    SystemId = 25,
    // Autonomous autorotation
    AutoRotate = 26,
    // Auto RTL, this is not a true mode
    // Auto will report as this mode if entered to perform a DO_LAND_START Landing sequence
    AutoRtl = 27,
    // Flip over after crash
    Turtle = 28,
    // Unknown
    #[strum(to_string = "Unknown ({0})")]
    Unknown(u32),
}

impl CustomMode {
    pub fn from_u32(value: u32) -> Self {
        match value {
            255 => Self::PreFlight,
            0 => Self::Stabilize,
            1 => Self::Acro,
            2 => Self::AltHold,
            3 => Self::Auto,
            4 => Self::Guided,
            5 => Self::Loiter,
            6 => Self::Rtl,
            7 => Self::Circle,
            9 => Self::Land,
            11 => Self::Drift,
            13 => Self::Sport,
            14 => Self::Flip,
            15 => Self::AutoTune,
            16 => Self::PosHold,
            17 => Self::Brake,
            18 => Self::Throw,
            19 => Self::AvoidAdsb,
            20 => Self::GuidedNoGps,
            21 => Self::SmartRtl,
            22 => Self::FlowHold,
            23 => Self::Follow,
            24 => Self::ZigZag,
            25 => Self::SystemId,
            26 => Self::AutoRotate,
            27 => Self::AutoRtl,
            28 => Self::Turtle,
            _ => Self::Unknown(value),
        }
    }
}
