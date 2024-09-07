use std::{path::PathBuf, sync::Arc};

use anyhow::Result;
use tokio::{
    io::{AsyncWriteExt, BufWriter},
    sync::broadcast,
};
use tracing::*;

use crate::{
    drivers::{Driver, DriverInfo},
    protocol::Protocol,
};

pub struct FileClient {
    pub path: PathBuf,
}

pub struct FileClientBuilder(FileClient);

impl FileClientBuilder {
    pub fn build(self) -> FileClient {
        self.0
    }
}

impl FileClient {
    #[instrument(level = "debug")]
    pub fn new(path: PathBuf) -> FileClientBuilder {
        FileClientBuilder(Self { path })
    }

    #[instrument(level = "debug", skip(self, writer, hub_receiver))]
    async fn handle_client(
        &self,
        writer: BufWriter<tokio::fs::File>,
        mut hub_receiver: broadcast::Receiver<Arc<Protocol>>,
    ) -> Result<()> {
        let mut writer = writer;

        loop {
            match hub_receiver.recv().await {
                Ok(message) => {
                    let raw_bytes = message.raw_bytes();
                    let timestamp = chrono::Utc::now().timestamp_micros() as u64;
                    writer.write_all(&timestamp.to_be_bytes()).await?;
                    writer.write_all(raw_bytes).await?;
                    writer.flush().await?;
                }
                Err(error) => {
                    error!("Failed to receive message from hub: {error:?}");
                    break;
                }
            }
        }

        debug!("FileClient write task finished");
        Ok(())
    }
}

#[async_trait::async_trait]
impl Driver for FileClient {
    #[instrument(level = "debug", skip(self, hub_sender))]
    async fn run(&self, hub_sender: broadcast::Sender<Arc<Protocol>>) -> Result<()> {
        let file = tokio::fs::File::create(self.path.clone()).await?;
        let writer = tokio::io::BufWriter::with_capacity(1024, file);
        let hub_receiver = hub_sender.subscribe();

        FileClient::handle_client(self, writer, hub_receiver).await
    }

    #[instrument(level = "debug", skip(self))]
    fn info(&self) -> Box<dyn DriverInfo> {
        return Box::new(FileClientInfo);
    }
}

pub struct FileClientInfo;
impl DriverInfo for FileClientInfo {
    fn name(&self) -> &str {
        "FileClient"
    }

    fn valid_schemes(&self) -> Vec<String> {
        vec![
            "fileclient".to_string(),
            "filewriter".to_string(),
            "filec".to_string(),
        ]
    }

    fn cli_example_legacy(&self) -> Vec<String> {
        let first_schema = &self.valid_schemes()[0];
        vec![
            format!("{first_schema}:<FILE>"),
            format!("{first_schema}:/tmp/potato.tlog"),
        ]
    }

    fn cli_example_url(&self) -> Vec<String> {
        let first_schema = &self.valid_schemes()[0];
        vec![
            format!("{first_schema}://<FILE>").to_string(),
            url::Url::parse(&format!("{first_schema}:///tmp/potato.tlog"))
                .unwrap()
                .to_string(),
        ]
    }

    fn create_endpoint_from_url(&self, url: &url::Url) -> Option<Arc<dyn Driver>> {
        Some(Arc::new(FileClient::new(url.path().into()).build()))
    }
}
