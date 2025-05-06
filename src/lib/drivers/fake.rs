use std::sync::Arc;

use anyhow::Result;
use bytes::{BufMut, BytesMut};
use mavlink::{Message, MessageData};
use mavlink_codec::{Packet, v2::V2Packet};
use serde::{Deserialize, Serialize};
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
pub struct FakeSink {
    name: arc_swap::ArcSwap<String>,
    uuid: DriverUuid,
    on_message_input: Callbacks<Arc<Protocol>>,
    print_as_bytes: bool,
    print_as_json: bool,
    force_parse: bool,
    stats: Arc<RwLock<AccumulatedDriverStats>>,
}

impl FakeSink {
    pub fn builder() -> FakeSinkBuilder {
        Self::builder_from_params(FakeSinkParams::default())
    }

    pub fn builder_from_params(params: FakeSinkParams) -> FakeSinkBuilder {
        let name = Arc::new(params.name.to_string());

        FakeSinkBuilder(Self {
            name: arc_swap::ArcSwap::new(name.clone()),
            uuid: Self::generate_uuid(&name),
            on_message_input: Callbacks::default(),
            print_as_bytes: params.print_as_bytes,
            print_as_json: params.print_as_json,
            force_parse: params.force_parse,
            stats: Arc::new(RwLock::new(AccumulatedDriverStats::new(
                name,
                &FakeSinkInfo,
            ))),
        })
    }
}

pub struct FakeSinkBuilder(FakeSink);

impl FakeSinkBuilder {
    pub fn build(self) -> FakeSink {
        self.0
    }

    pub fn name(self, name: &str) -> Self {
        self.0.name.store(Arc::new(name.to_string()));
        self
    }

    pub fn print_packet(mut self, print_packet: bool) -> Self {
        self.0.print_as_bytes = print_packet;
        self
    }

    pub fn print_decoded(mut self, print_decoded: bool) -> Self {
        self.0.print_as_json = print_decoded;
        self
    }

    pub fn force_parse(mut self, force_parse: bool) -> Self {
        self.0.force_parse = force_parse;
        self
    }

    pub fn on_message_input<C>(self, callback: C) -> Self
    where
        C: MessageCallback<Arc<Protocol>>,
    {
        self.0.on_message_input.add_callback(callback.into_boxed());
        self
    }
}

#[async_trait::async_trait]
impl Driver for FakeSink {
    async fn run(&self, hub_sender: broadcast::Sender<Arc<Protocol>>) -> Result<()> {
        let mut hub_receiver = hub_sender.subscribe();

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

            self.stats.write().await.stats.update_input(&message);

            for future in self.on_message_input.call_all(message.clone()) {
                if let Err(error) = future.await {
                    debug!("Dropping message: on_message_input callback returned error: {error:?}");
                    continue;
                }
            }

            if self.print_as_bytes {
                println!("Message received: {message:?}");
            } else {
                trace!("Message received: {message:?}");
            }

            if !(self.force_parse || self.print_as_json) {
                continue;
            }

            let Ok(mavlink_json) = message
                .to_mavlink_json::<mavlink::ardupilotmega::MavMessage>()
                .await
                .inspect_err(|error| debug!("Failed converting message to json: {error:?}"))
            else {
                continue;
            };

            let header = mavlink_json.header;
            let message = mavlink_json.message;

            if self.print_as_json {
                println!("Message received: {header:?} {message:?}");
            } else {
                trace!("Message received: {header:?} {message:?}");
            }
        }

        Ok(())
    }

    #[instrument(level = "debug", skip(self))]
    fn info(&self) -> Box<dyn DriverInfo> {
        Box::new(FakeSinkInfo)
    }

    fn name(&self) -> Arc<String> {
        self.name.load_full()
    }

    fn uuid(&self) -> &DriverUuid {
        &self.uuid
    }
}

#[async_trait::async_trait]
impl AccumulatedDriverStatsProvider for FakeSink {
    async fn stats(&self) -> AccumulatedDriverStats {
        self.stats.read().await.clone()
    }

    async fn reset_stats(&self) {
        let mut stats = self.stats.write().await;
        stats.stats.input = None;
        stats.stats.output = None
    }
}

pub struct FakeSinkInfo;
impl DriverInfo for FakeSinkInfo {
    fn name(&self) -> &'static str {
        "FakeSink"
    }

    fn valid_schemes(&self) -> &'static [&'static str] {
        &["fakesink", "fakeclient", "fakec"]
    }

    fn cli_example_legacy(&self) -> Vec<String> {
        let first_schema = &self.valid_schemes()[0];
        vec![
            format!("{first_schema}:<MODE>"),
            format!("{first_schema}:debug"),
        ]
    }

    fn cli_example_url(&self) -> Vec<String> {
        let first_schema = &self.valid_schemes()[0];
        let params = serde_urlencoded::to_string(FakeSinkParams::default()).unwrap();
        [
            format!("{first_schema}://?{params}"),
            format!("{first_schema}://"),
        ]
        .iter()
        .map(|s| {
            let url_str = url::Url::parse(s).unwrap().to_string();
            format!("'{url_str}'")
        })
        .collect()
    }

    fn create_endpoint_from_url(&self, url: &url::Url) -> Option<Arc<dyn Driver>> {
        let params: FakeSinkParams = url
            .query()
            .and_then(|qs| {
                serde_urlencoded::from_str(qs)
                    .inspect_err(|error| {
                        eprintln!("Failed to parse parameters: {error:?}. url: {url:?}");
                    })
                    .ok()
            })
            .unwrap_or_default();

        Some(Arc::new(FakeSink::builder_from_params(params).build()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FakeSinkParams {
    pub name: String,
    pub print_as_bytes: bool,
    pub print_as_json: bool,
    pub force_parse: bool,
}

impl Default for FakeSinkParams {
    fn default() -> Self {
        Self {
            name: crate::hub::generate_new_default_name(FakeSinkInfo.name()).unwrap(),
            print_as_bytes: false,
            print_as_json: true,
            force_parse: false,
        }
    }
}

#[derive(Debug)]
pub struct FakeSource {
    name: arc_swap::ArcSwap<String>,
    uuid: DriverUuid,
    period: std::time::Duration,
    on_message_output: Callbacks<Arc<Protocol>>,
    stats: Arc<RwLock<AccumulatedDriverStats>>,
    system_id: u8,
    component_id: u8,
    message_id: u32,
}

impl FakeSource {
    pub fn builder() -> FakeSourceBuilder {
        Self::builder_from_params(FakeSourceParams::default())
    }

    pub fn builder_from_params(params: FakeSourceParams) -> FakeSourceBuilder {
        let name = Arc::new(params.name.to_string());

        FakeSourceBuilder(Self {
            name: arc_swap::ArcSwap::new(name.clone()),
            uuid: Self::generate_uuid(&name),
            period: std::time::Duration::from_micros(params.period_us),
            on_message_output: Callbacks::default(),
            stats: Arc::new(RwLock::new(AccumulatedDriverStats::new(
                name,
                &FakeSourceInfo,
            ))),
            system_id: params.system_id,
            component_id: params.component_id,
            message_id: params.message_id,
        })
    }
}

pub struct FakeSourceBuilder(FakeSource);

impl FakeSourceBuilder {
    pub fn build(self) -> FakeSource {
        self.0
    }

    pub fn name(self, name: &str) -> Self {
        self.0.name.store(Arc::new(name.to_string()));
        self
    }

    pub fn period_us(mut self, period_us: u64) -> Self {
        self.0.period = std::time::Duration::from_micros(period_us);
        self
    }

    pub fn system_id(mut self, system_id: u8) -> Self {
        self.0.system_id = system_id;
        self
    }

    pub fn component_id(mut self, component_id: u8) -> Self {
        self.0.component_id = component_id;
        self
    }

    pub fn message_id(mut self, message_id: u32) -> Self {
        self.0.message_id = message_id;
        self
    }

    pub fn on_message_output<C>(self, callback: C) -> Self
    where
        C: MessageCallback<Arc<Protocol>>,
    {
        self.0.on_message_output.add_callback(callback.into_boxed());
        self
    }
}

#[async_trait::async_trait]
impl Driver for FakeSource {
    async fn run(&self, hub_sender: broadcast::Sender<Arc<Protocol>>) -> Result<()> {
        let (tx, mut rx) = tokio::sync::mpsc::channel(100);

        let generator_task = std::thread::Builder::new()
            .name(format!("gen_{}", self.name))
            .spawn({
                let period = self.period;
                let system_id = self.system_id;
                let component_id = self.component_id;
                let message_id = self.message_id;

                move || {
                    let mut header = mavlink::MavHeader {
                        sequence: 0,
                        system_id,
                        component_id,
                    };

                    let data =
                        mavlink::ardupilotmega::MavMessage::default_message_from_id(message_id)
                            .expect("Unknown message ID");

                    let mut current_instant: std::time::Instant;
                    let mut prev_instant = std::time::Instant::now();

                    loop {
                        current_instant = std::time::Instant::now();

                        if current_instant - prev_instant > period {
                            prev_instant = current_instant;

                            let buf = BytesMut::with_capacity(V2Packet::MAX_PACKET_SIZE);
                            let mut writer = buf.writer();

                            if let Err(error) = mavlink::write_v2_msg(&mut writer, header, &data) {
                                warn!("Failed to serialize message: {error:?}");
                                continue;
                            }

                            let packet = Packet::V2(V2Packet::new(writer.into_inner().freeze()));

                            let message = Arc::new(Protocol::new("fake_source", packet));

                            if let Err(error) = tx.blocking_send(message) {
                                warn!(
                                    "FakeSource's generator task failed sending message: {error:?}"
                                );
                                break;
                            }

                            header.sequence = header.sequence.overflowing_add(1).0;
                        }
                    }
                }
            })
            .expect("Failed spawning thread for generator task");

        while let Some(message) = rx.recv().await {
            self.stats.write().await.stats.update_output(&message);

            for future in self.on_message_output.call_all(message.clone()) {
                if let Err(error) = future.await {
                    debug!("Dropping message: on_message_input callback returned error: {error:?}");
                    continue;
                }
            }

            if let Err(error) = hub_sender.send(message) {
                error!("Failed to send message to hub: {error:?}");
            }
        }

        let _ = generator_task.join();

        Ok(())
    }

    fn info(&self) -> Box<dyn DriverInfo> {
        Box::new(FakeSourceInfo)
    }

    fn name(&self) -> Arc<String> {
        self.name.load_full()
    }

    fn uuid(&self) -> &DriverUuid {
        &self.uuid
    }
}

#[async_trait::async_trait]
impl AccumulatedDriverStatsProvider for FakeSource {
    async fn stats(&self) -> AccumulatedDriverStats {
        self.stats.read().await.clone()
    }

    async fn reset_stats(&self) {
        let mut stats = self.stats.write().await;
        stats.stats.input = None;
        stats.stats.output = None
    }
}

pub struct FakeSourceInfo;
impl DriverInfo for FakeSourceInfo {
    fn name(&self) -> &'static str {
        "FakeSource"
    }

    fn valid_schemes(&self) -> &'static [&'static str] {
        &["fakesource", "fakeserver", "fakesrc", "fakes"]
    }

    fn cli_example_legacy(&self) -> Vec<String> {
        let first_schema = &self.valid_schemes()[0];
        vec![
            format!("{first_schema}:<MODE>"),
            format!("{first_schema}:heartbeat"),
        ]
    }

    fn cli_example_url(&self) -> Vec<String> {
        let first_schema = &self.valid_schemes()[0];
        let params = serde_urlencoded::to_string(FakeSourceParams::default()).unwrap();
        [
            format!("{first_schema}://?{params}"),
            format!("{first_schema}://"),
        ]
        .iter()
        .map(|s| {
            let url_str = url::Url::parse(s).unwrap().to_string();
            format!("'{url_str}'")
        })
        .collect()
    }

    fn create_endpoint_from_url(&self, url: &url::Url) -> Option<Arc<dyn Driver>> {
        let params: FakeSourceParams = url
            .query()
            .and_then(|qs| {
                serde_urlencoded::from_str(qs)
                    .inspect_err(|error| {
                        eprintln!("Failed to parse parameters: {error:?}. url: {url:?}");
                    })
                    .ok()
            })
            .unwrap_or_default();

        Some(Arc::new(FakeSource::builder_from_params(params).build()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FakeSourceParams {
    pub name: String,
    pub period_us: u64,
    pub system_id: u8,
    pub component_id: u8,
    pub message_id: u32,
}

impl Default for FakeSourceParams {
    fn default() -> Self {
        Self {
            name: crate::hub::generate_new_default_name(FakeSourceInfo.name()).unwrap(),
            period_us: 1_000_000,
            system_id: 42,
            component_id: mavlink::common::MavComponent::MAV_COMP_ID_USER42 as u8,
            message_id: mavlink::common::HEARTBEAT_DATA::ID,
        }
    }
}

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use anyhow::Result;
    use clap::Parser;
    use tokio::sync::{RwLock, broadcast};

    use crate::cli;

    use super::*;

    #[tokio::test]
    async fn loopback_test() -> Result<()> {
        cli::init_with(cli::Args::parse_from(vec![
            "", // For some unknown reason, the first argument is ignored
            "--allow-no-endpoints",
        ]));

        let (hub_sender, _) = broadcast::channel(10000);

        let number_of_messages = 800;
        let message_period = tokio::time::Duration::from_micros(1);
        let timeout_time = tokio::time::Duration::from_secs(1);

        let source_messages = Arc::new(RwLock::new(Vec::<Arc<Protocol>>::with_capacity(1000)));
        let sink_messages = Arc::new(RwLock::new(Vec::<Arc<Protocol>>::with_capacity(1000)));

        // FakeSink and task
        let sink = FakeSink::builder()
            .name("test")
            .on_message_input({
                let sink_messages = sink_messages.clone();

                move |message: Arc<Protocol>| {
                    let sink_messages = sink_messages.clone();

                    async move {
                        sink_messages.write().await.push(message);
                        Ok(())
                    }
                }
            })
            .build();
        let sink_task = tokio::spawn({
            let hub_sender = hub_sender.clone();

            async move { sink.run(hub_sender).await }
        });

        // FakeSource and task
        let source = FakeSource::builder()
            .name("test")
            .period_us(message_period.as_micros() as u64)
            .on_message_output({
                let source_messages = source_messages.clone();
                move |message: Arc<Protocol>| {
                    let source_messages = source_messages.clone();

                    async move {
                        source_messages.write().await.push(message);
                        Ok(())
                    }
                }
            })
            .build();
        let source_task = tokio::spawn({
            let hub_sender = hub_sender.clone();

            async move { source.run(hub_sender).await }
        });

        // Monitoring task to wait the
        let sink_monitor_task = tokio::spawn({
            let sink_messages = sink_messages.clone();
            async move {
                loop {
                    if sink_messages.read().await.len() >= number_of_messages {
                        break;
                    }

                    tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                }
            }
        });
        let _ = tokio::time::timeout(timeout_time, sink_monitor_task)
            .await
            .expect(format!("sink messages: {:?}", sink_messages.read().await.len()).as_str());

        source_task.abort();
        sink_task.abort();

        // Compare the messages
        let source_messages = source_messages.read().await.clone();
        let sink_messages = sink_messages.read().await.clone();

        assert!(source_messages.len() >= number_of_messages);
        assert!(sink_messages.len() >= number_of_messages);
        assert_eq!(source_messages, sink_messages);

        Ok(())
    }
}
