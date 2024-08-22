use std::sync::{Arc, Mutex};

use clap::Parser;
use lazy_static::lazy_static;
use tracing::*;

#[derive(Parser, Debug)]
#[command(
    version = env!("CARGO_PKG_VERSION"),
    author = env!("CARGO_PKG_AUTHORS"),
    about = env!("CARGO_PKG_DESCRIPTION")
)]
struct Args {
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
        required = true,
        num_args = 1..,
        value_delimiter = ' ',
        value_parser = endpoints_parser,
    )]
    endpoints: Vec<String>,

    /// Turns all log categories up to Debug, for more information check RUST_LOG env variable.
    #[arg(short, long)]
    verbose: bool,

    /// Turns all log categories up to Trace to the log file, for more information check RUST_LOG env variable.
    #[arg(long)]
    enable_tracing_level_log_file: bool,

    /// Specifies the path in which the logs will be stored.
    #[arg(long, default_value = "./logs")]
    log_path: Option<String>,

    #[arg(long, default_value = "true")]
    streamreq_disable: bool,
}

#[instrument(level = "debug")]
fn endpoints_parser(endpoint: &str) -> Result<String, String> {
    let endpoint = endpoint.to_lowercase();

    let mut split = endpoint.split(':');
    if split.clone().count() != 2 && split.clone().count() != 3 {
        return Err("Wrong endpoint format".to_string());
    }

    let kind = split.next().expect(
        "Endpoint should start with one of the kinds: file, udps, udpc, udpb, tcps, tcpc, or serial",
    );
    if !matches!(
        kind,
        "file" | "udps" | "udpc" | "udpb" | "tcps" | "tcpc" | "serial"
    ) {
        return Err(format!("Unknown kind: {kind:?} for endpoint"));
    }

    // Add your custom validation logic here if needed
    Ok(endpoint)
}

#[derive(Debug)]
struct Manager {
    clap_matches: Args,
}

lazy_static! {
    static ref MANAGER: Arc<Manager> = Arc::new(Manager::new());
    static ref UDP_ENDPOINTS: Arc<Mutex<Vec<u16>>> = Arc::new(Mutex::new(Vec::new()));
}

impl Manager {
    fn new() -> Self {
        Self {
            clap_matches: Args::parse(),
        }
    }
}

/// Constructs our manager, Should be done inside main

#[instrument(level = "debug")]
pub fn init() {
    MANAGER.as_ref();
}

/// Checks if the verbosity parameter was used

#[instrument(level = "debug")]
pub fn is_verbose() -> bool {
    MANAGER.clap_matches.verbose
}

#[instrument(level = "debug")]
pub fn is_tracing() -> bool {
    MANAGER.clap_matches.enable_tracing_level_log_file
}

/// Our log path

#[instrument(level = "debug")]
pub fn log_path() -> String {
    let log_path =
        MANAGER.clap_matches.log_path.clone().expect(
            "Clap arg \"log-path\" should always be \"Some(_)\" because of the default value.",
        );

    shellexpand::full(&log_path)
        .expect("Failed to expand path")
        .to_string()
}

#[instrument(level = "debug")]
pub fn tcp_client_endpoints() -> Vec<String> {
    get_endpoint_with_kind("tcpc")
}

#[instrument(level = "debug")]
pub fn file_server_endpoints() -> Vec<String> {
    get_endpoint_with_kind("file")
}

#[instrument(level = "debug")]
pub fn tcp_server_endpoints() -> Vec<String> {
    get_endpoint_with_kind("tcps")
}

#[instrument(level = "debug")]
pub fn udp_client_endpoints() -> Vec<String> {
    get_endpoint_with_kind("udpc")
}

#[instrument(level = "debug")]
pub fn udp_server_endpoints() -> Vec<String> {
    get_endpoint_with_kind("udps")
}

#[instrument(level = "debug")]
pub fn udp_broadcast_endpoints() -> Vec<String> {
    get_endpoint_with_kind("udpb")
}

#[instrument(level = "debug")]
pub fn serial_endpoints() -> Vec<String> {
    get_endpoint_with_kind("serial")
}

#[instrument(level = "debug")]
fn get_endpoint_with_kind(kind: &str) -> Vec<String> {
    let mut endpoints = vec![];

    for endpoint in MANAGER.clap_matches.endpoints.clone() {
        let mut s = endpoint.split(':');

        let Some(this_kind) = s.next() else {
            warn!("No kind for endpoint: {endpoint:?}");
            continue;
        };

        if !this_kind.eq(kind) {
            trace!("ignoring: {this_kind:?}");
            continue;
        }

        endpoints.push(s.clone().collect::<Vec<&str>>().join(":"));
    }

    endpoints
}

// Return the command line used to start this application

#[instrument(level = "debug")]
pub fn command_line_string() -> String {
    std::env::args().collect::<Vec<String>>().join(" ")
}

// Return a clone of current Args struct

#[instrument(level = "debug")]
pub fn command_line() -> String {
    format!("{:#?}", MANAGER.clap_matches)
}
