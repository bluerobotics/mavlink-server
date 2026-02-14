use std::{
    fmt::Write as FmtWrite,
    io::{self, Write},
    sync::{Arc, Mutex},
};

use chrono::Utc;
use lazy_static::lazy_static;
use ringbuffer::{AllocRingBuffer, RingBuffer};
use tokio::sync::broadcast::{Receiver, Sender};
use tracing::metadata::LevelFilter;
use tracing::{Event, Level, Subscriber};
use tracing_log::LogTracer;
use tracing_subscriber::{
    EnvFilter, Layer,
    fmt::{self, MakeWriter},
    layer::{Context, SubscriberExt},
    registry::LookupSpan,
};

use crate::{cli, zenoh_service};
use zenoh_service::{FoxgloveLog, FoxgloveTimestamp};

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

struct FoxgloveBroadcast {
    sender: Sender<FoxgloveLog>,
}

impl FoxgloveBroadcast {
    fn new() -> Self {
        let (sender, _) = tokio::sync::broadcast::channel(100);
        Self { sender }
    }

    fn send(&self, log: FoxgloveLog) {
        let _ = self.sender.send(log);
    }

    fn subscribe(&self) -> Receiver<FoxgloveLog> {
        self.sender.subscribe()
    }
}

lazy_static! {
    static ref MANAGER: Arc<Mutex<Manager>> = Default::default();
    pub static ref HISTORY: Arc<Mutex<History>> = Default::default();
    static ref FOXGLOVE_BROADCAST: FoxgloveBroadcast = FoxgloveBroadcast::new();
}

struct FoxgloveLayer;

impl<S> tracing_subscriber::Layer<S> for FoxgloveLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        // Transform the message to the Foxglove log format
        // https://docs.foxglove.dev/docs/visualization/message-schemas/log
        let metadata = event.metadata();
        let now = Utc::now();
        let total_ns = now.timestamp_nanos_opt().unwrap_or(0);

        // Extract the message from event fields
        let mut message = String::new();
        // The tracing library uses the visitor pattern for field access.
        // Event doesn't expose fields directly, it's necessary to call event.record(&mut visitor)
        // which invokes your visitor's methods for each field.
        let mut visitor = MessageVisitor(&mut message);
        event.record(&mut visitor);

        let level = match *metadata.level() {
            Level::TRACE => 0,
            Level::DEBUG => 1,
            Level::INFO => 2,
            Level::WARN => 3,
            Level::ERROR => 4,
        };

        let foxglove_log = FoxgloveLog {
            timestamp: FoxgloveTimestamp {
                sec: total_ns / 1_000_000_000,
                nsec: total_ns % 1_000_000_000,
            },
            level,
            message,
            name: metadata.target().to_string(),
            file: metadata.file().unwrap_or("").to_string(),
            line: metadata.line().unwrap_or(0),
        };

        FOXGLOVE_BROADCAST.send(foxglove_log);
    }
}

struct MessageVisitor<'a>(&'a mut String);

impl tracing::field::Visit for MessageVisitor<'_> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.0, "{value:?}");
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.0.push_str(value);
        }
    }
}

// Start logger, should be done inside main
pub fn init(log_path: String, is_verbose: bool, is_tracing: bool) {
    // Redirect all logs from libs using "Log"
    LogTracer::init_with_filter(tracing::log::LevelFilter::Trace).expect("Failed to set logger");

    // Configure the console log
    let console_env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| {
            if is_verbose {
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
    let file_env_filter = if is_tracing {
        EnvFilter::new(LevelFilter::TRACE.to_string())
    } else {
        EnvFilter::new(LevelFilter::DEBUG.to_string())
    };
    let file_appender = custom_rolling_appender(
        log_path,
        tracing_appender::rolling::Rotation::HOURLY,
        "mavlink-server",
        "log",
    );
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

    let foxglove_layer = FoxgloveLayer;

    let history = HISTORY.clone();
    let mut foxglove_rx = FOXGLOVE_BROADCAST.subscribe();
    MANAGER.lock().unwrap().process = Some(tokio::spawn(async move {
        loop {
            tokio::select! {
                result = rx.recv() => {
                    match result {
                        Ok(message) => {
                            history.lock().unwrap().push(message);
                        }
                        Err(_) => {
                            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                        }
                    }
                }
                result = foxglove_rx.recv() => {
                    if let Ok(foxglove_log) = result {
                        let _ = zenoh_service::publish_foxglove_log(
                            "services/mavlink-server/log",
                            &foxglove_log,
                        )
                        .await;
                    }
                }
            }
        }
    }));

    // Configure the default subscriber
    let subscriber = tracing_subscriber::registry()
        .with(console_layer)
        .with(file_layer)
        .with(foxglove_layer)
        .with(server_layer);
    tracing::subscriber::set_global_default(subscriber).expect("Unable to set a global subscriber");
}

fn custom_rolling_appender<P: AsRef<std::path::Path>>(
    dir: P,
    rotation: tracing_appender::rolling::Rotation,
    prefix: &str,
    suffix: &str,
) -> tracing_appender::rolling::RollingFileAppender {
    tracing_appender::rolling::RollingFileAppender::builder()
        .rotation(rotation)
        .filename_prefix(prefix)
        .filename_suffix(suffix)
        .build(dir)
        .expect("failed to initialize rolling file appender")
}
