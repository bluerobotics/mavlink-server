use std::{path::PathBuf, sync::Arc};

use anyhow::Result;
use tokio::{
    io::{AsyncWriteExt, BufWriter},
    sync::{broadcast, RwLock},
};
use tracing::*;

use crate::{
    callbacks::{Callbacks, MessageCallback},
    drivers::{Driver, DriverInfo},
    protocol::Protocol,
    stats::{
        accumulated::driver::{AccumulatedDriverStats, AccumulatedDriverStatsProvider},
        driver::DriverUuid,
    },
};

#[derive(Debug)]
pub struct TlogWriter {
    pub path: PathBuf,
    name: arc_swap::ArcSwap<String>,
    uuid: DriverUuid,
    on_message_output: Callbacks<Arc<Protocol>>,
    stats: Arc<RwLock<AccumulatedDriverStats>>,
}

pub struct TlogWriterBuilder(TlogWriter);

impl TlogWriterBuilder {
    pub fn build(self) -> TlogWriter {
        self.0
    }

    pub fn on_message_output<C>(self, callback: C) -> Self
    where
        C: MessageCallback<Arc<Protocol>>,
    {
        self.0.on_message_output.add_callback(callback.into_boxed());
        self
    }
}

impl TlogWriter {
    #[instrument(level = "debug")]
    pub fn builder(name: &str, path: PathBuf) -> TlogWriterBuilder {
        let name = Arc::new(name.to_string());
        let path_str = path.clone().display().to_string();

        TlogWriterBuilder(Self {
            path,
            name: arc_swap::ArcSwap::new(name.clone()),
            uuid: Self::generate_uuid(&path_str),
            on_message_output: Callbacks::default(),
            stats: Arc::new(RwLock::new(AccumulatedDriverStats::new(
                name,
                &TlogWriterInfo,
            ))),
        })
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
                    let timestamp = chrono::Utc::now().timestamp_micros() as u64;

                    self.stats.write().await.stats.update_output(&message);

                    for future in self.on_message_output.call_all(message.clone()) {
                        if let Err(error) = future.await {
                            debug!(
                                "Dropping message: on_message_input callback returned error: {error:?}"
                            );
                            continue;
                        }
                    }

                    let raw_bytes = message.bytes();
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

        debug!("TlogClient write task finished");
        Ok(())
    }
}

#[async_trait::async_trait]
impl Driver for TlogWriter {
    #[instrument(level = "debug", skip(self, hub_sender))]
    async fn run(&self, hub_sender: broadcast::Sender<Arc<Protocol>>) -> Result<()> {
        let file = tokio::fs::File::create(self.path.clone()).await?;
        let writer = tokio::io::BufWriter::with_capacity(1024, file);
        let hub_receiver = hub_sender.subscribe();

        TlogWriter::handle_client(self, writer, hub_receiver).await
    }

    #[instrument(level = "debug", skip(self))]
    fn info(&self) -> Box<dyn DriverInfo> {
        return Box::new(TlogWriterInfo);
    }

    fn name(&self) -> Arc<String> {
        self.name.load_full()
    }

    fn uuid(&self) -> &DriverUuid {
        &self.uuid
    }
}

#[async_trait::async_trait]
impl AccumulatedDriverStatsProvider for TlogWriter {
    async fn stats(&self) -> AccumulatedDriverStats {
        self.stats.read().await.clone()
    }

    async fn reset_stats(&self) {
        let mut stats = self.stats.write().await;
        stats.stats.input = None;
        stats.stats.output = None
    }
}
pub struct TlogWriterInfo;
impl DriverInfo for TlogWriterInfo {
    fn name(&self) -> &'static str {
        "TlogWriter"
    }
    fn valid_schemes(&self) -> &'static [&'static str] {
        &["tlogwriter", "tlogw"]
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
        Some(Arc::new(
            TlogWriter::builder("TlogWriter", url.path().into()).build(),
        ))
    }
}
