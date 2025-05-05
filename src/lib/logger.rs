use std::{
    io::{self, Write},
    sync::{Arc, Mutex},
};

use lazy_static::lazy_static;
use ringbuffer::{AllocRingBuffer, RingBuffer};
use tokio::sync::broadcast::{Receiver, Sender};
use tracing::{metadata::LevelFilter, *};
use tracing_log::LogTracer;
use tracing_subscriber::{
    EnvFilter, Layer,
    fmt::{self, MakeWriter},
    layer::SubscriberExt,
};

use crate::cli;

struct BroadcastWriter {
    sender: Sender<String>,
}

impl Write for BroadcastWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let message = String::from_utf8_lossy(buf).to_string();
        let _ = self.sender.send(message);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct BroadcastMakeWriter {
    sender: Sender<String>,
}

impl<'a> MakeWriter<'a> for BroadcastMakeWriter {
    type Writer = BroadcastWriter;

    fn make_writer(&'a self) -> Self::Writer {
        BroadcastWriter {
            sender: self.sender.clone(),
        }
    }
}

#[derive(Debug, Default)]
pub struct Manager {
    pub process: Option<tokio::task::JoinHandle<()>>,
}

pub struct History {
    pub history: AllocRingBuffer<String>,
    pub sender: Sender<String>,
}

impl Default for History {
    fn default() -> Self {
        let (sender, _receiver) = tokio::sync::broadcast::channel(100);
        Self {
            history: AllocRingBuffer::new(10 * 1024),
            sender,
        }
    }
}

impl History {
    pub fn push(&mut self, message: String) {
        self.history.push(message.clone());
        let _ = self.sender.send(message);
    }

    pub fn subscribe(&self) -> (Receiver<String>, Vec<String>) {
        let reader = self.sender.subscribe();
        (reader, self.history.to_vec())
    }
}

lazy_static! {
    static ref MANAGER: Arc<Mutex<Manager>> = Default::default();
    pub static ref HISTORY: Arc<Mutex<History>> = Default::default();
}

// Start logger, should be done inside main
pub fn init() {
    // Redirect all logs from libs using "Log"
    LogTracer::init_with_filter(tracing::log::LevelFilter::Trace).expect("Failed to set logger");

    // Configure the console log
    let console_env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| {
            if cli::is_verbose() {
                EnvFilter::new(LevelFilter::DEBUG.to_string())
            } else {
                EnvFilter::new(LevelFilter::INFO.to_string())
            }
        })
        // Hyper is used for http request by our thread leak test
        // And it's pretty verbose when it's on
        .add_directive("hyper=off".parse().unwrap());
    let console_layer = fmt::Layer::new()
        .with_writer(std::io::stdout)
        .with_ansi(true)
        .with_file(true)
        .with_line_number(true)
        .with_span_events(fmt::format::FmtSpan::NONE)
        .with_target(false)
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_filter(console_env_filter);

    // Configure the file log
    let file_env_filter = if cli::is_tracing() {
        EnvFilter::new(LevelFilter::TRACE.to_string())
    } else {
        EnvFilter::new(LevelFilter::DEBUG.to_string())
    };
    let dir = cli::log_path();

    match std::fs::metadata(&dir) {
        Ok(metadata) => {
            if metadata.permissions().readonly() {
                eprintln!("Error: Log directory {dir:?} does not have write permissions",);
                std::process::exit(1);
            }
        }
        Err(error) => {
            println!("Error: Could not check permissions for log directory {dir:?}: {error:?}");
        }
    }
    let file_appender = tracing_appender::rolling::hourly(dir, "mavlink-router-rs.", ".log");
    let file_layer = fmt::Layer::new()
        .with_writer(file_appender)
        .with_ansi(false)
        .with_file(true)
        .with_line_number(true)
        .with_span_events(fmt::format::FmtSpan::NONE)
        .with_target(false)
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_filter(file_env_filter);

    // Configure the server log
    let server_env_filter = if cli::is_tracing() {
        EnvFilter::new(LevelFilter::TRACE.to_string())
    } else {
        EnvFilter::new(LevelFilter::DEBUG.to_string())
    };
    let (tx, mut rx) = tokio::sync::broadcast::channel(100);
    let server_layer = fmt::Layer::new()
        .with_writer(BroadcastMakeWriter { sender: tx.clone() })
        .with_ansi(false)
        .with_file(true)
        .with_line_number(true)
        .with_span_events(fmt::format::FmtSpan::NONE)
        .with_target(false)
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_filter(server_env_filter);

    let history = HISTORY.clone();
    MANAGER.lock().unwrap().process = Some(tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(message) => {
                    history.lock().unwrap().push(message);
                }
                Err(_error) => {
                    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                }
            }
        }
    }));

    // Configure the default subscriber
    let subscriber = tracing_subscriber::registry()
        .with(console_layer)
        .with(file_layer)
        .with(server_layer);
    tracing::subscriber::set_global_default(subscriber).expect("Unable to set a global subscriber");

    info!(
        "{}, version: {}-{}, build date: {}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
        env!("VERGEN_GIT_SHA"),
        env!("VERGEN_BUILD_DATE")
    );
    info!(
        "Starting at {}",
        chrono::Local::now().format("%Y-%m-%dT%H:%M:%S"),
    );
    debug!("Command line call: {}", cli::command_line_string());
    debug!("Command line input struct call: {}", cli::command_line());
}
