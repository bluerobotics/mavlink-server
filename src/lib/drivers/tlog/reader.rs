use std::{path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use chrono::DateTime;
use mavlink::ardupilotmega::MavMessage;
use mavlink_codec::Packet;
use tokio::sync::{RwLock, broadcast};
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
pub struct TlogReader {
    pub path: PathBuf,
    name: arc_swap::ArcSwap<String>,
    uuid: DriverUuid,
    on_message_input: Callbacks<Arc<Protocol>>,
    stats: Arc<RwLock<AccumulatedDriverStats>>,
}

pub struct TlogReaderBuilder(TlogReader);

impl TlogReaderBuilder {
    pub fn build(self) -> TlogReader {
        self.0
    }

    pub fn on_message_input<C>(self, callback: C) -> Self
    where
        C: MessageCallback<Arc<Protocol>>,
    {
        self.0.on_message_input.add_callback(callback.into_boxed());
        self
    }
}

impl TlogReader {
    #[instrument(level = "debug")]
    pub fn builder(name: &str, path: PathBuf) -> TlogReaderBuilder {
        let path = std::fs::canonicalize(&path)
            .inspect_err(|_| {
                warn!("Failed canonicalizing path: {path:?}, using the non-canonized instead.")
            })
            .unwrap_or(path);

        let name = Arc::new(name.to_string());
        let path_str = path.clone().display().to_string();

        TlogReaderBuilder(Self {
            path,
            name: arc_swap::ArcSwap::new(name.clone()),
            uuid: Self::generate_uuid(&path_str),
            on_message_input: Callbacks::default(),
            stats: Arc::new(RwLock::new(AccumulatedDriverStats::new(
                name,
                &TlogReaderInfo,
            ))),
        })
    }

    #[instrument(level = "debug", skip(self, reader, hub_sender))]
    async fn handle_file(
        &self,
        reader: tokio::io::BufReader<tokio::fs::File>,
        hub_sender: broadcast::Sender<Arc<Protocol>>,
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
                    Ok(message) => Protocol::new_with_timestamp(
                        us_since_epoch,
                        &source_name,
                        Packet::from(message),
                    ),
                    Err(error) => {
                        match error {
                            mavlink::error::MessageReadError::Io(_) => (),
                            mavlink::error::MessageReadError::Parse(_) => {
                                error!("Failed to parse MAVLink message: {error:?}")
                            }
                        }

                        continue;
                    }
                };
            reader.consume(message.bytes().len() - 1);

            trace!("Parsed message: {:?}", message.bytes());

            let message = Arc::new(message);

            self.stats.write().await.stats.update_input(&message);

            for future in self.on_message_input.call_all(message.clone()) {
                if let Err(error) = future.await {
                    debug!("Dropping message: on_message_input callback returned error: {error:?}");
                    continue;
                }
            }

            if let Err(error) = hub_sender.send(message) {
                error!("Failed to send message to hub: {error:?}");
            }
        }
    }
}

#[async_trait::async_trait]
impl Driver for TlogReader {
    #[instrument(level = "debug", skip(self, hub_sender))]
    async fn run(&self, hub_sender: broadcast::Sender<Arc<Protocol>>) -> Result<()> {
        let file = tokio::fs::File::open(self.path.clone()).await?;
        let reader = tokio::io::BufReader::with_capacity(1024, file);

        TlogReader::handle_file(self, reader, hub_sender).await
    }

    #[instrument(level = "debug", skip(self))]
    fn info(&self) -> Box<dyn DriverInfo> {
        return Box::new(TlogReaderInfo);
    }

    fn name(&self) -> Arc<String> {
        self.name.load_full()
    }

    fn uuid(&self) -> &DriverUuid {
        &self.uuid
    }
}

#[async_trait::async_trait]
impl AccumulatedDriverStatsProvider for TlogReader {
    async fn stats(&self) -> AccumulatedDriverStats {
        self.stats.read().await.clone()
    }

    async fn reset_stats(&self) {
        let mut stats = self.stats.write().await;
        stats.stats.input = None;
        stats.stats.output = None
    }
}

pub struct TlogReaderInfo;
impl DriverInfo for TlogReaderInfo {
    fn name(&self) -> &'static str {
        "TlogReader"
    }
    fn valid_schemes(&self) -> &'static [&'static str] {
        &["tlogreader", "tlogr"]
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

    fn url_from_legacy(
        &self,
        legacy_entry: crate::drivers::DriverDescriptionLegacy,
    ) -> Result<url::Url> {
        let scheme = self.default_scheme().context("No default scheme")?;
        let path_string = legacy_entry.arg1;
        std::fs::metadata(&path_string).context("Failed to get metadata for file")?;

        if let Some(arg2) = legacy_entry.arg2 {
            warn!("Ignoring extra argument: {arg2:?}");
        }

        url::Url::parse(&format!("{scheme}://{path_string}")).context("Failed to parse URL")
    }

    fn create_endpoint_from_url(&self, url: &url::Url) -> Option<Arc<dyn Driver>> {
        Some(Arc::new(
            TlogReader::builder("TlogReader", url.path().into()).build(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, str::FromStr};

    use anyhow::Result;
    use mavlink::{MavFrame, error::ParserError};
    use tokio::sync::RwLock;

    use super::*;
    #[tokio::test]
    async fn read_all_messages() -> Result<()> {
        let (sender, _receiver) = tokio::sync::broadcast::channel(1000000);

        let messages_received_per_id =
            Arc::new(RwLock::new(BTreeMap::<u32, Vec<Arc<Protocol>>>::new()));

        let tlog_file = PathBuf::from_str("tests/files/00025-2024-04-22_18-49-07.tlog").unwrap();

        let driver = TlogReader::builder("test", tlog_file.clone())
            .on_message_input({
                let messages_received_per_id = messages_received_per_id.clone();
                move |message: Arc<Protocol>| {
                    let messages_received = messages_received_per_id.clone();

                    async move {
                        let message_id = message.message_id();

                        let mut messages_received = messages_received.write().await;
                        if let Some(samples) = messages_received.get_mut(&message_id) {
                            samples.push(message);
                        } else {
                            messages_received.insert(message_id, Vec::from([message]));
                        }

                        Ok(())
                    }
                }
            })
            .build();

        let receiver_task_handle = tokio::spawn({
            let sender = sender.clone();
            async move { driver.run(sender).await }
        });

        // let file_v1_messages = 2; // TODO:  Add support for V1 messages
        let file_v2_messages = 30437;
        let file_messages = file_v2_messages;
        let mut total_messages_read = 0;
        let timeout_time = tokio::time::Duration::from_secs(1);
        let res = tokio::time::timeout(timeout_time, async {
            loop {
                let messages_received_per_id = messages_received_per_id.read().await.clone();
                total_messages_read = messages_received_per_id
                    .values()
                    .into_iter()
                    .fold(0, |acc, samples| acc + samples.len());

                if total_messages_read > file_messages {
                    panic!("Reading duplicates!");
                } else if total_messages_read == file_messages {
                    break;
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
            }
        })
        .await;

        let messages_received_per_id = messages_received_per_id.read().await;

        let messages_count_per_id = messages_received_per_id
            .iter()
            .map(|(id, samples)| (*id, samples.len()))
            .collect::<Vec<(u32, usize)>>();
        let _messages_count_per_id_fmt = messages_count_per_id
            .iter()
            .map(|sample| format!("{}:{}", sample.0, sample.1))
            .collect::<Vec<String>>();

        let mut messages_received = messages_received_per_id
            .values()
            .flatten()
            .cloned()
            .collect::<Vec<Arc<Protocol>>>();
        messages_received.sort_by_key(|sample| sample.timestamp);

        let messages_parsed = messages_received
            .iter()
            .cloned()
            .map(|message| {
                let parsed_message = mavlink::MavFrame::<MavMessage>::deser(
                    mavlink::MavlinkVersion::V2,
                    &message.bytes()[4..],
                );

                (message.timestamp, parsed_message)
            })
            .collect::<Vec<(u64, std::result::Result<MavFrame<MavMessage>, ParserError>)>>();

        let messages_parsed_fmt = messages_parsed
            .iter()
            .map(|sample| {
                let us_since_epoch = sample.0;
                let datetime = DateTime::from_timestamp_micros(us_since_epoch as i64)
                    .map(|d| d.to_string())
                    .unwrap_or(format!("error parsing timestamp: {us_since_epoch}"));

                let frame = &sample.1;

                format!("{datetime}: {frame:?}")
            })
            .collect::<Vec<String>>();
        let str = format!("{messages_parsed_fmt:#?}");
        let path = format!(
            "{}/untracked",
            tlog_file.parent().unwrap().to_str().unwrap()
        );
        let dump_file = format!(
            "{path}/{}-test-dump.txt",
            tlog_file.file_stem().unwrap().to_str().unwrap(),
        );
        std::fs::create_dir_all(path).unwrap();
        let mut file = std::fs::File::create(dump_file)?;
        std::io::Write::write_all(&mut file, str.as_bytes())?;

        res.expect(
            format!("Timeout: read only {total_messages_read:?}/{file_messages:?}").as_str(),
        );

        receiver_task_handle.abort();

        Ok(())
    }
}
