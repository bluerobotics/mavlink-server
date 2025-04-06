use std::sync::Arc;

use clap::Parser;
use once_cell::sync::OnceCell;
use tracing::*;

use crate::drivers;

static MANAGER: OnceCell<Manager> = OnceCell::new();

struct Manager {
    clap_matches: Args,
}

#[derive(Debug, Parser)]
#[command(
    version = env!("CARGO_PKG_VERSION"),
    author = env!("CARGO_PKG_AUTHORS"),
    about = env!("CARGO_PKG_DESCRIPTION")
)]
pub struct Args {
    /// Space-separated list of endpoints.
    /// At least one endpoint is required.
    /// Possible endpoints types are:
    ///
    /// udpc:dest_ip:port (udp, client mode)
    ///
    /// udpb:broadcast_ip:port (udp, broadcast mode)
    ///
    /// tcps:listen_ip:port (tcp, server mode)
    ///
    /// tcpc:dest_ip:port (tcp, client mode)
    ///
    /// serial:port:baudrate (serial)
    ///
    /// udps:listen_ip:port (udp, server mode)
    #[arg(
        required_if_eq("allow_no_endpoints", "false"),
        num_args = 1..,
        value_delimiter = ' ',
        value_parser = endpoints_parser,
        help = "Space-separated list of endpoints.",
        long_help = build_endpoints_help(),
    )]
    endpoints: Vec<Arc<dyn drivers::Driver>>,

    /// Turns all log categories up to Debug, for more information check RUST_LOG env variable.
    #[arg(short, long)]
    verbose: bool,

    /// Sets the IP and port that the server will be provided
    #[arg(long, default_value = "0.0.0.0:8080")]
    web_server: std::net::SocketAddrV4,

    /// Turns all log categories up to Trace to the log file, for more information check RUST_LOG env variable.
    #[arg(long)]
    enable_tracing_level_log_file: bool,

    /// Specifies the path in which the logs will be stored.
    #[arg(long, default_value = "./logs")]
    log_path: Option<String>,

    /// Disables stream request (not implemented)
    #[arg(long, default_value = "true")]
    streamreq_disable: bool,

    /// The timeout duration (in seconds) after which inactive UDP clients will be discarded.
    #[arg(long, default_value = "10")]
    udp_server_timeout: i16,

    /// Sets MAVLink system ID for this service
    #[arg(long, default_value = "1")]
    mavlink_system_id: u8,

    /// Sets the MAVLink component ID for this service, for more information, check: https://mavlink.io/en/messages/common.html#MAV_COMPONENT")
    /// note: the default value 191 means MAV_COMP_ID_ONBOARD_COMPUTER
    #[arg(long, default_value = "191")]
    mavlink_component_id: u8,

    /// Sets the frequency of the MAVLink heartbeat message sent by the Mavlink Server
    #[arg(long, default_value = "1")]
    mavlink_heartbeat_frequency: f32,

    /// Sends a burst of initial heartbeats to the autopilot spaced by 0.1 seconds to wake up MAVLink connection (useful for PX4-like autopilots)
    #[arg(long, default_value = "false")]
    send_initial_heartbeats: bool,

    /// Sets the MAVLink version used to communicate. This changes the heartbeat messages sent
    #[arg(long, default_value = "2", value_names = ["1", "2"])]
    mavlink_version: u8,

    /// Sets the default version used by the REST API, this will remove the prefix used by its path.
    #[arg(long, default_value = "1", value_names = ["1"])]
    default_api_version: u8,

    /// Only useful for test and debug
    #[arg(long, hide = true, default_value = "false")]
    allow_no_endpoints: bool,
}

#[instrument(level = "trace")]
fn build_endpoints_help() -> String {
    let mut help = vec!["Space-separated list of endpoints.".to_string()];
    let endpoints = drivers::endpoints()
        .iter()
        .map(|endpoint| {
            let info = &endpoint.driver_ext;
            let name = info.name();
            let help_schemas = info
                .valid_schemes()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<String>>()
                .join(",");

            let help_legacy = info
                .cli_example_legacy()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<String>>()
                .join("\n\t\t ");

            let help_url = info
                .cli_example_url()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<String>>()
                .join("\n\t\t ");

            [
                format!("{name}\t {help_schemas}").to_string(),
                format!("\t legacy: {help_legacy}").to_string(),
                format!("\t    url: {help_url}\n").to_string(),
            ]
            .join("\n")
        })
        .collect::<Vec<String>>();

    help.extend(endpoints);
    help.join("\n")
}

#[instrument(level = "debug")]
fn endpoints_parser(entry: &str) -> Result<Arc<dyn drivers::Driver>, String> {
    drivers::create_driver_from_entry(entry)
}

/// Constructs our manager, Should be done inside main
#[instrument(level = "debug")]
pub fn init() {
    let expanded_args = std::env::args()
        .map(|arg| {
            // Fallback to the original if it fails to expand
            shellexpand::env(&arg.clone())
                .inspect_err(|_| {
                    warn!("Failed expanding arg: {arg:?}, using the non-expanded instead.")
                })
                .unwrap_or_else(|_| arg.into())
                .into_owned()
        })
        .collect::<Vec<String>>();

    let reparsed_expanded_args = Args::parse_from(expanded_args);

    init_with(reparsed_expanded_args);
}

/// Constructs our manager, Should be done inside main
/// Note: differently from init(), this doesn't expand env variables
#[instrument(level = "debug")]
pub fn init_with(args: Args) {
    MANAGER.get_or_init(|| Manager { clap_matches: args });
}

/// Local acessor to the parsed Args
fn args() -> &'static Args {
    &MANAGER.get().unwrap().clap_matches
}

/// Checks if the verbosity parameter was used
#[instrument(level = "debug")]
pub fn is_verbose() -> bool {
    args().verbose
}

#[instrument(level = "debug")]
pub fn is_tracing() -> bool {
    args().enable_tracing_level_log_file
}

#[instrument(level = "debug")]
pub fn log_path() -> String {
    let log_path = args()
        .log_path
        .clone()
        .expect("Clap arg \"log-path\" should always be \"Some(_)\" because of the default value.")
        .parse::<std::path::PathBuf>()
        .expect("Failed parsing the passed log-path");

    std::fs::canonicalize(&log_path)
        .inspect_err(|_| {
            warn!("Failed canonicalizing path: {log_path:?}, using the non-canonized instead.")
        })
        .unwrap_or(log_path)
        .into_os_string()
        .into_string()
        .expect("Failed converting PathBuf into string")
}

pub fn endpoints() -> Vec<Arc<dyn drivers::Driver>> {
    let default_endpoints = Arc::new(crate::drivers::rest::Rest::builder("Rest (default)").build());
    let mut endpoints = args().endpoints.clone();
    endpoints.push(default_endpoints);
    endpoints
}

#[instrument(level = "debug")]
pub fn command_line_string() -> String {
    std::env::args().collect::<Vec<String>>().join(" ")
}

/// Returns a pretty string of the current Args struct
#[instrument(level = "debug")]
pub fn command_line() -> String {
    format!("{:#?}", args())
}

#[instrument(level = "debug")]
pub fn udp_server_timeout() -> Option<tokio::time::Duration> {
    let seconds = args().udp_server_timeout;

    if seconds < 0 {
        return None;
    }

    Some(tokio::time::Duration::from_secs(seconds as u64))
}

#[instrument(level = "debug")]
pub fn web_server() -> std::net::SocketAddrV4 {
    args().web_server
}

#[instrument(level = "debug")]
pub fn mavlink_system_id() -> u8 {
    args().mavlink_system_id
}

#[instrument(level = "debug")]
pub fn mavlink_component_id() -> u8 {
    args().mavlink_component_id
}

#[instrument(level = "debug")]
pub fn mavlink_heartbeat_frequency() -> f32 {
    args().mavlink_heartbeat_frequency
}

#[instrument(level = "debug")]
pub fn send_initial_heartbeats() -> bool {
    args().send_initial_heartbeats
}

#[instrument(level = "debug")]
pub fn mavlink_version() -> u8 {
    args().mavlink_version
}

#[instrument(level = "debug")]
pub fn default_api_version() -> u8 {
    args().default_api_version
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_endpoints() {
        let endpoints = vec![
            ("serial:/dev/ttyACM0:115200", true),
            ("serial:/dev/ttyS0:9600", true),
            ("serial:COM1:115200", true),
            ("tcpc:10.0.0.1:4000", true),
            ("tcpc:127.0.0.1:7000", true),
            ("tcps:0.0.0.0:5000", true),
            ("tcps:192.168.1.10:6000", true),
            ("udpb:192.168.0.255:3000", false),
            ("udpb:255.255.255.255:9999", false),
            ("udpc:127.0.0.1:1234", true),
            ("udpc:192.168.1.100:8080", true),
            ("udpout:127.0.0.1:1234", true),
            ("udpout:192.168.1.100:8080", true),
            ("udps:0.0.0.0:5000", true),
            ("udps:192.168.1.5:6789", true),
            ("udpin:0.0.0.0:5000", true),
            ("udpin:192.168.1.5:6789", true),
            ("tlogr:tests/files/00025-2024-04-22_18-49-07.tlog", true),
            ("tlogw:/tmp/little_potato.tlog", true),
            (
                "tlogreader:tests/files/00025-2024-04-22_18-49-07.tlog",
                true,
            ),
            ("tlogwriter:/tmp/little_potato.tlog", true),
        ];

        for (endpoint, expected) in endpoints {
            let result = endpoints_parser(endpoint);
            assert_eq!(result.is_ok(), expected);
        }
    }
}
