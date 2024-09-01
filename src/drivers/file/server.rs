use std::sync::Arc;

use anyhow::Result;
use chrono::DateTime;
use mavlink::ardupilotmega::MavMessage;
use std::path::PathBuf;
use tokio::sync::broadcast;
use tracing::*;

use crate::drivers::{Driver, DriverExt, DriverInfo, OnMessageCallback, OnMessageCallbackExt};
use crate::protocol::Protocol;

pub struct FileServer {
    pub path: PathBuf,
    on_message: OnMessageCallback<(u64, Protocol)>,
}

pub struct FileServerBuilder(FileServer);

impl FileServerBuilder {
    pub fn build(self) -> FileServer {
        self.0
    }

    pub fn on_message<F>(mut self, callback: F) -> Self
    where
        F: OnMessageCallbackExt<(u64, Protocol)> + Send + Sync + 'static,
    {
        self.0.on_message = Some(Box::new(callback));
        self
    }
}

impl FileServer {
    #[instrument(level = "debug")]
    pub fn new(path: PathBuf) -> FileServerBuilder {
        FileServerBuilder(Self {
            path,
            on_message: None,
        })
    }

    #[instrument(level = "debug", skip(self, reader, hub_sender))]
    async fn handle_client(
        &self,
        reader: tokio::io::BufReader<tokio::fs::File>,
        hub_sender: broadcast::Sender<Protocol>,
    ) -> Result<()> {
        let source_name = self.path.as_path().display().to_string();

        let mut reader = mavlink::async_peek_reader::AsyncPeekReader::new(reader);
        let mut timestamp_bytes = [0u8; 8];

        loop {
            // Tlog files follow the byte format of <unix_timestamp_us><raw_mavlink_messsage>
            let bytes = loop {
                let bytes = reader.peek_exact(9).await?;
                if bytes[8] == mavlink::MAV_STX_V2 {
                    break &bytes[..8];
                }

                reader.consume(1);
            };

            let us_since_epoch = {
                timestamp_bytes.copy_from_slice(bytes);
                u64::from_be_bytes(timestamp_bytes)
            };

            if DateTime::from_timestamp_micros(us_since_epoch as i64).is_none() {
                warn!("Failed to convert unix time: {us_since_epoch:?}");

                reader.consume(1);
                continue;
            };

            // Gets the bufferr starting at the magic byte
            reader.consume(8);
            assert_eq!(reader.peek_exact(1).await?[0], mavlink::MAV_STX_V2);

            let message =
                match mavlink::read_v2_raw_message_async::<MavMessage, _>(&mut reader).await {
                    Ok(message) => Protocol::new(&source_name, message),
                    Err(error) => {
                        match error {
                            mavlink::error::MessageReadError::Io(_) => (),
                            mavlink::error::MessageReadError::Parse(_) => {
                                error!("Failed to parse MAVLink message: {error:?}")
                            }
                        }

                        continue; // Skip this iteration on error
                    }
                };
            reader.consume(message.raw_bytes().len() - 1);

            trace!("Parsed message: {:?}", message.raw_bytes());

            if let Some(callback) = &self.on_message {
                callback
                    .call((us_since_epoch, message.clone()))
                    .await
                    .unwrap();
            }

            if let Err(error) = hub_sender.send(message) {
                error!("Failed to send message to hub: {error:?}");
            }
        }
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
        Some(Arc::new(FileServer::new(url.path().into()).build()))
    }
}
    }
}
