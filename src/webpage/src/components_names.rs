pub fn get_component_name(id: u8) -> &'static str {
    match id {
        0 => "MAV_COMP_ID_ALL",
        1 => "MAV_COMP_ID_AUTOPILOT1",
        25 => "MAV_COMP_ID_USER1",
        26 => "MAV_COMP_ID_USER2",
        27 => "MAV_COMP_ID_USER3",
        28 => "MAV_COMP_ID_USER4",
        29 => "MAV_COMP_ID_USER5",
        30 => "MAV_COMP_ID_USER6",
        31 => "MAV_COMP_ID_USER7",
        32 => "MAV_COMP_ID_USER8",
        33 => "MAV_COMP_ID_USER9",
        34 => "MAV_COMP_ID_USER10",
        35 => "MAV_COMP_ID_USER11",
        36 => "MAV_COMP_ID_USER12",
        37 => "MAV_COMP_ID_USER13",
        38 => "MAV_COMP_ID_USER14",
        39 => "MAV_COMP_ID_USER15",
        40 => "MAV_COMP_ID_USER16",
        41 => "MAV_COMP_ID_USER17",
        42 => "MAV_COMP_ID_USER18",
        43 => "MAV_COMP_ID_USER19",
        44 => "MAV_COMP_ID_USER20",
        45 => "MAV_COMP_ID_USER21",
        46 => "MAV_COMP_ID_USER22",
        47 => "MAV_COMP_ID_USER23",
        48 => "MAV_COMP_ID_USER24",
        49 => "MAV_COMP_ID_USER25",
        50 => "MAV_COMP_ID_USER26",
        51 => "MAV_COMP_ID_USER27",
        52 => "MAV_COMP_ID_USER28",
        53 => "MAV_COMP_ID_USER29",
        54 => "MAV_COMP_ID_USER30",
        55 => "MAV_COMP_ID_USER31",
        56 => "MAV_COMP_ID_USER32",
        57 => "MAV_COMP_ID_USER33",
        58 => "MAV_COMP_ID_USER34",
        59 => "MAV_COMP_ID_USER35",
        60 => "MAV_COMP_ID_USER36",
        61 => "MAV_COMP_ID_USER37",
        62 => "MAV_COMP_ID_USER38",
        63 => "MAV_COMP_ID_USER39",
        64 => "MAV_COMP_ID_USER40",
        65 => "MAV_COMP_ID_USER41",
        66 => "MAV_COMP_ID_USER42",
        67 => "MAV_COMP_ID_USER43",
        68 => "MAV_COMP_ID_TELEMETRY_RADIO",
        69 => "MAV_COMP_ID_USER45",
        70 => "MAV_COMP_ID_USER46",
        71 => "MAV_COMP_ID_USER47",
        72 => "MAV_COMP_ID_USER48",
        73 => "MAV_COMP_ID_USER49",
        74 => "MAV_COMP_ID_USER50",
        75 => "MAV_COMP_ID_USER51",
        76 => "MAV_COMP_ID_USER52",
        77 => "MAV_COMP_ID_USER53",
        78 => "MAV_COMP_ID_USER54",
        79 => "MAV_COMP_ID_USER55",
        80 => "MAV_COMP_ID_USER56",
        81 => "MAV_COMP_ID_USER57",
        82 => "MAV_COMP_ID_USER58",
        83 => "MAV_COMP_ID_USER59",
        84 => "MAV_COMP_ID_USER60",
        85 => "MAV_COMP_ID_USER61",
        86 => "MAV_COMP_ID_USER62",
        87 => "MAV_COMP_ID_USER63",
        88 => "MAV_COMP_ID_USER64",
        89 => "MAV_COMP_ID_USER65",
        90 => "MAV_COMP_ID_USER66",
        91 => "MAV_COMP_ID_USER67",
        92 => "MAV_COMP_ID_USER68",
        93 => "MAV_COMP_ID_USER69",
        94 => "MAV_COMP_ID_USER70",
        95 => "MAV_COMP_ID_USER71",
        96 => "MAV_COMP_ID_USER72",
        97 => "MAV_COMP_ID_USER73",
        98 => "MAV_COMP_ID_USER74",
        99 => "MAV_COMP_ID_USER75",
        100 => "MAV_COMP_ID_CAMERA",
        101 => "MAV_COMP_ID_CAMERA2",
        102 => "MAV_COMP_ID_CAMERA3",
        103 => "MAV_COMP_ID_CAMERA4",
        104 => "MAV_COMP_ID_CAMERA5",
        105 => "MAV_COMP_ID_CAMERA6",
        140 => "MAV_COMP_ID_SERVO1",
        141 => "MAV_COMP_ID_SERVO2",
        142 => "MAV_COMP_ID_SERVO3",
        143 => "MAV_COMP_ID_SERVO4",
        144 => "MAV_COMP_ID_SERVO5",
        145 => "MAV_COMP_ID_SERVO6",
        146 => "MAV_COMP_ID_SERVO7",
        147 => "MAV_COMP_ID_SERVO8",
        148 => "MAV_COMP_ID_SERVO9",
        149 => "MAV_COMP_ID_SERVO10",
        150 => "MAV_COMP_ID_SERVO11",
        151 => "MAV_COMP_ID_SERVO12",
        152 => "MAV_COMP_ID_SERVO13",
        153 => "MAV_COMP_ID_SERVO14",
        154 => "MAV_COMP_ID_GIMBAL",
        155 => "MAV_COMP_ID_LOG",
        156 => "MAV_COMP_ID_ADSB",
        157 => "MAV_COMP_ID_OSD",
        158 => "MAV_COMP_ID_PERIPHERAL",
        159 => "MAV_COMP_ID_QX1_GIMBAL",
        160 => "MAV_COMP_ID_FLARM",
        161 => "MAV_COMP_ID_PARACHUTE",
        169 => "MAV_COMP_ID_WINCH",
        171 => "MAV_COMP_ID_GIMBAL2",
        172 => "MAV_COMP_ID_GIMBAL3",
        173 => "MAV_COMP_ID_GIMBAL4",
        174 => "MAV_COMP_ID_GIMBAL5",
        175 => "MAV_COMP_ID_GIMBAL6",
        180 => "MAV_COMP_ID_BATTERY",
        181 => "MAV_COMP_ID_BATTERY2",
        189 => "MAV_COMP_ID_MAVCAN",
        190 => "MAV_COMP_ID_MISSIONPLANNER",
        191 => "MAV_COMP_ID_ONBOARD_COMPUTER",
        192 => "MAV_COMP_ID_ONBOARD_COMPUTER2",
        193 => "MAV_COMP_ID_ONBOARD_COMPUTER3",
        194 => "MAV_COMP_ID_ONBOARD_COMPUTER4",
        195 => "MAV_COMP_ID_PATHPLANNER",
        196 => "MAV_COMP_ID_OBSTACLE_AVOIDANCE",
        197 => "MAV_COMP_ID_VISUAL_INERTIAL_ODOMETRY",
        198 => "MAV_COMP_ID_PAIRING_MANAGER",
        200 => "MAV_COMP_ID_IMU",
        201 => "MAV_COMP_ID_IMU_2",
        202 => "MAV_COMP_ID_IMU_3",
        220 => "MAV_COMP_ID_GPS",
        221 => "MAV_COMP_ID_GPS2",
        236 => "MAV_COMP_ID_ODID_TXRX_1",
        237 => "MAV_COMP_ID_ODID_TXRX_2",
        238 => "MAV_COMP_ID_ODID_TXRX_3",
        240 => "MAV_COMP_ID_UDP_BRIDGE",
        241 => "MAV_COMP_ID_UART_BRIDGE",
        242 => "MAV_COMP_ID_TUNNEL_NODE",
        243 => "MAV_COMP_ID_ILLUMINATOR",
        250 => "MAV_COMP_ID_SYSTEM_CONTROL",
        _ => "Unknown",
    }
}
