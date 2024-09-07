use std::sync::{Arc, Mutex};

use clap::Parser;
use lazy_static::lazy_static;
use tracing::*;

use crate::drivers;

#[derive(Parser)]
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
    endpoints: Vec<Arc<dyn drivers::Driver>>,

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
fn endpoints_parser(entry: &str) -> Result<Arc<dyn drivers::Driver>, String> {
    drivers::create_driver_from_entry(entry)
}

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

pub fn endpoints() -> Vec<Arc<dyn drivers::Driver>> {
    MANAGER.clap_matches.endpoints.clone()
}

#[instrument(level = "debug")]
pub fn command_line_string() -> String {
    std::env::args().collect::<Vec<String>>().join(" ")
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
        ];

        for (endpoint, expected) in endpoints {
            let result = endpoints_parser(endpoint);
            assert_eq!(result.is_ok(), expected);
        }
    }
}
