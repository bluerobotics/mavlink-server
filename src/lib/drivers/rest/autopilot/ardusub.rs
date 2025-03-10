// for ardusub only
#[repr(u8)]
#[derive(Debug, Eq, PartialEq, strum_macros::EnumString, strum_macros::Display)]
#[strum(serialize_all = "kebab-case")]
pub enum CustomMode {
    // Mode not set by vehicle yet
    PreFlight = u8::MAX,
    // Manual angle with manual depth/throttle
    Stabilize = 0,
    // Manual body-frame angular rate with manual depth/throttle
    Acro = 1,
    // Manual angle with automatic depth/throttle
    AltHold = 2,
    // Fully automatic waypoint control using mission commands
    Auto = 3,
    // Fully automatic fly to coordinate or fly at velocity/direction using GCS immediate commands
    Guided = 4,
    // Automatic circular flight with automatic throttle
    Circle = 7,
    // Automatically return to surface, pilot maintains horizontal control
    Surface = 9,
    // Automatic position hold with manual override, with automatic throttle
    PosHold = 16,
    // Pass-through input with no stabilization
    Manual = 19,
    // Automatically detect motors orientation
    MotorDetect = 20,
    // Manual angle with automatic depth/throttle (from rangefinder altitude)
    SurfTrak = 21,
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
            7 => Self::Circle,
            9 => Self::Surface,
            16 => Self::PosHold,
            19 => Self::Manual,
            20 => Self::MotorDetect,
            21 => Self::SurfTrak,
            _ => Self::Unknown(value),
        }
    }
}
