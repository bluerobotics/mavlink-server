use std::{path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use tokio::{
    io::{AsyncWriteExt, BufWriter},
    sync::{RwLock, broadcast},
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
    file_creation_condition: FileCreationCondition,
    name: arc_swap::ArcSwap<String>,
    uuid: DriverUuid,
    on_message_output: Callbacks<Arc<Protocol>>,
    stats: Arc<RwLock<AccumulatedDriverStats>>,
}

#[derive(Debug, strum_macros::EnumString)]
#[strum(serialize_all = "snake_case")]
pub enum FileCreationCondition {
    WhileArmed(ExpectedOrigin),
    Always,
}

#[derive(Debug, Default)]
pub struct ExpectedOrigin {
    pub system_id: RwLock<Option<u8>>,
    pub component_id: u8,
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
    pub fn builder(
        name: &str,
        path: PathBuf,
        file_creation_condition: FileCreationCondition,
    ) -> TlogWriterBuilder {
        let path = std::fs::canonicalize(&path)
            .inspect_err(|_| {
                warn!("Failed canonicalizing path: {path:?}, using the non-canonized instead.")
            })
            .unwrap_or(path);

        let name = Arc::new(name.to_string());
        let path_str = path.clone().display().to_string();

        TlogWriterBuilder(Self {
            path,
            name: arc_swap::ArcSwap::new(name.clone()),
            file_creation_condition,
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
            let message = match hub_receiver.recv().await {
                Ok(message) => message,
                Err(broadcast::error::RecvError::Closed) => {
                    error!("Hub channel closed!");
                    break;
                }
                Err(broadcast::error::RecvError::Lagged(count)) => {
                    warn!("Channel lagged by {count} messages.");
                    continue;
                }
            };

            let timestamp = chrono::Utc::now().timestamp_micros() as u64;

            self.stats.write().await.stats.update_output(&message);

            for future in self.on_message_output.call_all(message.clone()) {
                if let Err(error) = future.await {
                    debug!("Dropping message: on_message_input callback returned error: {error:?}");
                    continue;
                }
            }

            let raw_bytes = message.bytes();
            writer.write_all(&timestamp.to_be_bytes()).await?;
            writer.write_all(raw_bytes).await?;
            writer.flush().await?;

            if let FileCreationCondition::WhileArmed(ExpectedOrigin {
                system_id,
                component_id,
            }) = &self.file_creation_condition
            {
                let system_id = system_id.read().await.expect(
                            "System ID should always be Some at this point because it was replaced when armed, which is the condition to reach this part",
                        );

                if *message.system_id() != system_id || message.component_id() != component_id {
                    continue;
                }

                if let Some(ArmState::Disarmed) = check_arm_state(&message) {
                    debug!("Vehicle disarmed, finishing tlog file writer until next arm...");
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
        let mut armed = false;

        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
        let mut first = true;
        loop {
            if first {
                first = false;
            } else {
                interval.tick().await;
            }

            if !armed {
                if let FileCreationCondition::WhileArmed(expected_origin) =
                    &self.file_creation_condition
                {
                    debug!(
                        "TlogWriter waiting for its arm condition as {:?}",
                        self.file_creation_condition
                    );
                    let hub_receiver = hub_sender.subscribe();
                    if let Err(error) = wait_for_arm(hub_receiver, expected_origin).await {
                        warn!("Failed waiting for arm: {error:?}");
                        continue;
                    }
                    debug!(
                        "TlogWriter has reached its arm condition as {:?}",
                        self.file_creation_condition
                    );
                    armed = true;
                }
            }

            let file = match create_tlog_file(self.path.clone()).await {
                Ok(file) => file,
                Err(error) => {
                    warn!("Failed creating tlog file: {error:?}");
                    continue;
                }
            };

            debug!("Writing to tlog file: {file:?}");

            let writer = tokio::io::BufWriter::with_capacity(1024, file);
            let hub_receiver = hub_sender.subscribe();

            if let Err(error) = TlogWriter::handle_client(self, writer, hub_receiver).await {
                debug!("TlogWriter client ended with an error: {error:?}");
            }

            armed = false;
        }
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
            format!("{first_schema}:<TLOGS_OUTPUT_DIR>"),
            format!("{first_schema}:/tmp/potato.tlog"),
            format!("{first_schema}:/tmp/tlogs_output_dir/"),
            format!("{first_schema}:<tmp/tlogs_output_dir>/?when=always"),
            format!("{first_schema}:<tmp/tlogs_output_dir>/?when=while_armed"),
        ]
    }

    fn cli_example_url(&self) -> Vec<String> {
        let first_schema = &self.valid_schemes()[0];
        vec![
            format!("{first_schema}://<FILE>").to_string(),
            format!("{first_schema}://<TLOGS_OUTPUT_DIR>").to_string(),
            url::Url::parse(&format!("{first_schema}:///tmp/potato.tlog"))
                .unwrap()
                .to_string(),
            url::Url::parse(&format!("{first_schema}:///tmp/tlogs_output_dir/"))
                .unwrap()
                .to_string(),
            url::Url::parse(&format!(
                "{first_schema}:///tmp/tlogs_output_dir/?when=while_armed"
            ))
            .unwrap()
            .to_string(),
        ]
    }

    fn create_endpoint_from_url(&self, url: &url::Url) -> Option<Arc<dyn Driver>> {
        let params: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();

        let expected_system_id = params.get("system_id").and_then(|v| v.parse::<u8>().ok());
        let expected_component_id = params
            .get("component_id")
            .and_then(|v| v.parse::<u8>().ok())
            .unwrap_or(mavlink::ardupilotmega::MavComponent::MAV_COMP_ID_AUTOPILOT1 as u8);

        let file_creation_condition = params
            .get("when")
            .and_then(|v| {
                v.parse()
                    .map(|cond| match cond {
                        FileCreationCondition::WhileArmed(_) => {
                            FileCreationCondition::WhileArmed(ExpectedOrigin {
                                system_id: RwLock::new(expected_system_id),
                                component_id: expected_component_id,
                            })
                        }
                        other => other,
                    })
                    .ok()
            })
            .unwrap_or(FileCreationCondition::Always);

        Some(Arc::new(
            TlogWriter::builder("TlogWriter", url.path().into(), file_creation_condition).build(),
        ))
    }
}

async fn create_tlog_file(path: PathBuf) -> Result<tokio::fs::File> {
    let file_path: PathBuf = if path.extension().and_then(|ext| ext.to_str()) == Some("tlog") {
        path.clone()
    } else {
        if !std::path::Path::new(&path).exists() {
            tokio::fs::create_dir_all(&path).await?;
        }

        let sequence = get_sequence(&path).await.unwrap_or_default();
        let timestamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");
        let file_name = format!("{sequence:05}-{timestamp}.tlog");

        let mut file_path = path.clone();
        file_path.push(file_name);
        file_path
    };

    tokio::fs::File::create(file_path)
        .await
        .map_err(anyhow::Error::msg)
}

async fn get_sequence(path: &PathBuf) -> Result<u32> {
    let re = regex::Regex::new(r"^(\d{5})-\d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2}\.tlog$")
        .expect("Failed to compile regex");

    let mut max_seq: u32 = 0;
    let mut files_in_dir = tokio::fs::read_dir(&path).await?;

    while let Some(entry) = files_in_dir.next_entry().await? {
        let entry_path = entry.path();

        if let Some(sequence) = entry_path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|file_name| re.captures(file_name))
            .and_then(|captures| captures.get(1))
            .and_then(|seq_match| seq_match.as_str().parse::<u32>().ok())
        {
            max_seq = max_seq.max(sequence);
        }
    }

    Ok(max_seq + 1)
}

async fn wait_for_arm(
    mut hub_receiver: broadcast::Receiver<Arc<Protocol>>,
    ExpectedOrigin {
        system_id,
        component_id,
    }: &ExpectedOrigin,
) -> Result<()> {
    loop {
        let message = hub_receiver.recv().await?;

        if message.component_id() == component_id
            && matches!(check_arm_state(&message), Some(ArmState::Armed))
        {
            debug!(
                "Received arm from system {:?}. Current: {system_id:?}",
                message.system_id()
            );

            let current_system_id = *system_id.read().await;

            let system_id = match current_system_id {
                Some(system_id) => system_id,
                None => {
                    system_id
                        .write()
                        .await
                        .replace(*message.system_id())
                        .context("This should always be None")
                        .expect_err("This should never be Ok");

                    debug!("Expected System ID updated to {system_id:?}");

                    *message.system_id()
                }
            };

            if *message.system_id() == system_id {
                break;
            }
        }
    }

    Ok(())
}

enum ArmState {
    Disarmed,
    Armed,
}

/// A performant way of checking if the vehicle is armed without parsing the message into a heartbeat message type (from Mavlink crate)
fn check_arm_state(message: &Arc<Protocol>) -> Option<ArmState> {
    use mavlink::MessageData;

    if message.message_id() != mavlink::ardupilotmega::HEARTBEAT_DATA::ID {
        return None;
    }

    const BASE_MODE_BYTE: usize = 6; // From: https://mavlink.io/en/messages/common.html#HEARTBEAT

    let base_mode = message
        .payload()
        .get(BASE_MODE_BYTE)
        .cloned()
        .unwrap_or_else(|| mavlink::ardupilotmega::MavModeFlag::empty().bits());

    match base_mode & mavlink::ardupilotmega::MavModeFlag::MAV_MODE_FLAG_SAFETY_ARMED.bits() {
        0 => Some(ArmState::Disarmed),
        _ => Some(ArmState::Armed),
    }
}
