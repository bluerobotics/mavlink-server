use std::sync::Arc;

use anyhow::Result;
use chrono::DateTime;
use mavlink::ardupilotmega::MavMessage;
use std::path::PathBuf;
use tokio::io::AsyncReadExt;
use tokio::sync::broadcast;
use tracing::*;

use crate::drivers::{Driver, DriverExt, DriverInfo};
use crate::protocol::Protocol;

pub struct FileServer {
    pub path: PathBuf,
}

impl FileServer {
    #[instrument(level = "debug")]
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl FileServer {
    #[instrument(level = "debug")]
    pub fn try_new(file_path: &str) -> Result<Self> {
        let path = PathBuf::from(file_path);
        Ok(Self { path })
    }

    #[instrument(level = "debug", skip(self, reader, hub_sender))]
    async fn handle_client(
        &self,
        reader: tokio::io::BufReader<tokio::fs::File>,
        hub_sender: broadcast::Sender<Protocol>,
    ) -> Result<()> {
        let source_name = self.path.as_path().display().to_string();
        loop {
            // Tlog files follow the byte format of <unix_timestamp_us><raw_mavlink_messsage>
            let Ok(us_since_epoch) = reader.read_u64().await else {
                info!("End of file reached");
                break;
            };

            let Some(_date_time) = DateTime::from_timestamp_micros(us_since_epoch as i64) else {
                warn!("Failed to convert unix time");
                continue;
            };

            // Ensure that we have at least a single byte before checking for a valid mavlink message
            if reader.buffer().is_empty() {
                info!("End of file reached");
                break;
            }

            // Since the source is a tlog file that includes timestamps + raw mavlink messages.
            // We first need to be sure that the next byte is the start of a mavlink message,
            // otherwise the `read_v2_raw_message_async` will process valid timestamps as garbage.
            if reader.buffer()[0] != mavlink::MAV_STX_V2 {
                warn!("Invalid MAVLink start byte, skipping");
                continue;
            }

            let message =
                match mavlink::read_v2_raw_message_async::<MavMessage, _>(&mut reader).await {
                    Ok(message) => message,
                    Err(error) => {
                        error!("Failed to parse MAVLink message: {error:?}");
                        continue; // Skip this iteration on error
                    }
                };

            let message = Protocol::new(&source_name, message);

            trace!("Received File message: {message:?}");
            if let Err(error) = hub_sender.send(message) {
                error!("Failed to send message to hub: {error:?}");
            }
        }

        debug!("File Receive task for {source_name} finished");
        Ok(())
    }
}

#[async_trait::async_trait]
impl Driver for FileServer {
    #[instrument(level = "debug", skip(self, hub_sender))]
    async fn run(&self, hub_sender: broadcast::Sender<Protocol>) -> Result<()> {
        let file = tokio::fs::File::open(self.path.clone()).await?;
        let reader = tokio::io::BufReader::with_capacity(1024, file);

        FileServer::handle_client(self, reader, hub_sender).await
    }

    #[instrument(level = "debug", skip(self))]
    fn info(&self) -> DriverInfo {
        DriverInfo {
            name: "FileServer".to_string(),
        }
    }
}

pub struct FileServerExt;
impl DriverExt for FileServerExt {
    fn valid_schemes(&self) -> Vec<String> {
        vec![
            "filesource".to_string(),
            "fileserver".to_string(),
            "files".to_string(),
        ]
    }

    fn url_from_legacy(
        &self,
        legacy_entry: crate::drivers::DriverDescriptionLegacy,
    ) -> Result<url::Url, String> {
        let scheme = self.default_scheme();
        let path = legacy_entry.arg1;
        if let Err(error) = std::fs::metadata(&path) {
            return Err(format!("Failed to get metadata for file: {error:?}"));
        }
        // Get absolute path of file
        let path = match std::fs::canonicalize(&path) {
            Ok(path) => path,
            Err(error) => {
                return Err(format!("Failed to get absolute path for file: {error:?}"));
            }
        };

        let Some(path_string) = path.to_str() else {
            return Err(format!("Failed to convert path to string: {path:?}"));
        };

        if let Some(arg2) = legacy_entry.arg2 {
            warn!("Ignoring extra argument: {arg2:?}");
        }

        match url::Url::parse(&format!("{scheme}://{path_string}")) {
            Ok(url) => Ok(url),
            Err(error) => Err(format!("Failed to parse URL: {error:?}")),
        }
    }

    fn create_endpoint_from_url(&self, url: &url::Url) -> Option<Arc<dyn Driver>> {
        let host = url.host_str().unwrap();
        let path = std::fs::canonicalize(&host).expect("Failed to get absolute path");
        return Some(Arc::new(FileServer::new(path)));
    }
}
