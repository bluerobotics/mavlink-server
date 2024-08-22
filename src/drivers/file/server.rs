use std::sync::Arc;

use anyhow::Result;
use chrono::DateTime;
use mavlink::ardupilotmega::MavMessage;
use std::path::PathBuf;
use tokio::io::AsyncReadExt;
use tokio::sync::broadcast;
use tracing::*;

use crate::drivers::{Driver, DriverInfo};
use crate::protocol::Protocol;

#[derive(Clone, Debug)]
pub struct FileServer {
    pub path: PathBuf,
}

impl FileServer {
    #[instrument(level = "debug")]
    pub fn try_new(file_path: &str) -> Result<Self> {
        let path = PathBuf::from(file_path);
        Ok(Self { path })
    }

    #[instrument(level = "debug", skip(reader, hub_sender))]
    async fn handle_client(
        server: FileServer,
        mut reader: tokio::io::BufReader<tokio::fs::File>,
        hub_sender: Arc<broadcast::Sender<Protocol>>,
    ) -> Result<()> {
        let source_name = server.path.as_path().display().to_string();
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
            if (reader.buffer().is_empty()) {
                info!("End of file reached");
                break;
            }

            // Since the source is a tlog file that includes timestamps + raw mavlink messages.
            // We first need to be sure that the next byte is the start of a mavlink message,
            // otherwise the `read_v2_raw_message_async` will process valid timestamps as garbage.
            if (reader.buffer()[0] != mavlink::MAV_STX_V2) {
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
        let file = tokio::fs::File::open(self.path.clone()).await.unwrap();
        let mut reader = tokio::io::BufReader::new(file);

        tokio::spawn(FileServer::handle_client(
            self.clone(),
            reader,
            Arc::new(hub_sender),
        ));

        Ok(())
    }

    #[instrument(level = "debug", skip(self))]
    fn info(&self) -> DriverInfo {
        DriverInfo {
            name: "FileServer".to_string(),
        }
    }
}
