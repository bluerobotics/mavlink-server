use std::sync::Arc;

use clap::Parser;
use lazy_static::lazy_static;

#[derive(Parser, Debug)]
#[command(version = env!("CARGO_PKG_VERSION"), author = env!("CARGO_PKG_AUTHORS"), about = env!("CARGO_PKG_DESCRIPTION"))]
struct Args {
    /// Turns all log categories up to Debug, for more information check RUST_LOG env variable.
    #[arg(short, long)]
    verbose: bool,

    /// Turns all log categories up to Trace to the log file, for more information check RUST_LOG env variable.
    #[arg(long)]
    enable_tracing_level_log_file: bool,

    /// Specifies the path in witch the logs will be stored.
    #[arg(long, default_value = "./logs")]
    log_path: Option<String>,

    /// Sets the mavlink connections
    #[arg(
        long,
        value_name = "<TYPE>:<IP/SERIAL>:<PORT/BAUDRATE>",
        use_value_delimiter = true
    )]
    mavlink: Vec<String>,
}

#[derive(Debug)]
struct Manager {
    clap_matches: Args,
}

lazy_static! {
    static ref MANAGER: Arc<Manager> = Arc::new(Manager::new());
}

impl Manager {
    fn new() -> Self {
        Self {
            clap_matches: Args::parse(),
        }
    }
}

/// Constructs our manager, Should be done inside main
pub fn init() {
    MANAGER.as_ref();
}

/// Checks if the verbosity parameter was used
pub fn is_verbose() -> bool {
    MANAGER.clap_matches.verbose
}

pub fn is_tracing() -> bool {
    MANAGER.clap_matches.enable_tracing_level_log_file
}

/// Our log path
pub fn log_path() -> String {
    let log_path =
        MANAGER.clap_matches.log_path.clone().expect(
            "Clap arg \"log-path\" should always be \"Some(_)\" because of the default value.",
        );

    shellexpand::full(&log_path)
        .expect("Failed to expand path")
        .to_string()
}

/// Returns the mavlink connection string
pub fn mavlink_connections() -> Vec<String> {
    MANAGER.clap_matches.mavlink.clone()
}

// Return the command line used to start this application
pub fn command_line_string() -> String {
    std::env::args().collect::<Vec<String>>().join(" ")
}

// Return a clone of current Args struct
pub fn command_line() -> String {
    format!("{:#?}", MANAGER.clap_matches)
}
