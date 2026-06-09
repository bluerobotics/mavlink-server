use std::sync::Arc;

use anyhow::Result;
use mavlink_codec::codec::MavlinkCodec;
use tokio::sync::{RwLock, broadcast};
use tracing::*;

use crate::{
    callbacks::{Callbacks, MessageCallback},
    drivers::{
        Driver, DriverInfo,
        generic_tasks::{SendReceiveContext, default_send_receive_run},
        zenoh::raw_adapter::ZenohRawAdapter,
    },
    protocol::Protocol,
    stats::{
        accumulated::driver::{AccumulatedDriverStats, AccumulatedDriverStatsProvider},
        driver::DriverUuid,
    },
};

const TOPIC_PREFIX: &str = "mavlink_raw";
const DRIVER_IDENTIFIER: &str = "zenohraw";

#[derive(Debug)]
pub struct ZenohRaw {
    name: arc_swap::ArcSwap<String>,
    uuid: DriverUuid,
    on_message_input: Callbacks<Arc<Protocol>>,
    on_message_output: Callbacks<Arc<Protocol>>,
    stats: Arc<RwLock<AccumulatedDriverStats>>,
}

pub struct ZenohRawBuilder(ZenohRaw);

impl ZenohRawBuilder {
    pub fn build(self) -> ZenohRaw {
        self.0
    }

    pub fn on_message_input<C>(self, callback: C) -> Self
    where
        C: MessageCallback<Arc<Protocol>>,
    {
        self.0.on_message_input.add_callback(callback.into_boxed());
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

impl ZenohRaw {
    #[instrument(level = "debug")]
    pub fn builder(name: &str) -> ZenohRawBuilder {
        let name = Arc::new(name.to_string());

        ZenohRawBuilder(Self {
            name: arc_swap::ArcSwap::new(name.clone()),
            uuid: Self::generate_uuid(&name),
            on_message_input: Callbacks::default(),
            on_message_output: Callbacks::default(),
            stats: Arc::new(RwLock::new(AccumulatedDriverStats::new(
                name,
                &ZenohRawInfo,
            ))),
        })
    }
}

#[async_trait::async_trait]
impl Driver for ZenohRaw {
    #[instrument(level = "debug", skip(self, hub_sender))]
    async fn run(&self, hub_sender: broadcast::Sender<Arc<Protocol>>) -> Result<()> {
        let context = SendReceiveContext {
            direction: crate::drivers::Direction::Both,
            hub_sender,
            on_message_output: self.on_message_output.clone(),
            on_message_input: self.on_message_input.clone(),
            stats: self.stats.clone(),
        };

        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
        let mut first = true;
        loop {
            if first {
                first = false;
            } else {
                interval.tick().await;
            }

            debug!("Trying to connect...");

            let session = super::session::session().await;

            let subscriber = match session
                .declare_subscriber(format!("{TOPIC_PREFIX}/in"))
                .await
            {
                Ok(subscriber) => subscriber,
                Err(error) => {
                    error!("Failed to create subscriber for mavlink raw data: {error:?}");
                    continue;
                }
            };

            let publisher = match session
                .declare_publisher(format!("{TOPIC_PREFIX}/out"))
                .encoding(zenoh::bytes::Encoding::APPLICATION_OCTET_STREAM.with_schema("mavlink"))
                .congestion_control(zenoh::qos::CongestionControl::Block)
                .priority(zenoh::qos::Priority::RealTime)
                .express(false)
                .await
            {
                Ok(publisher) => publisher,
                Err(error) => {
                    error!("Failed to create publisher for mavlink raw data: {error:?}");
                    continue;
                }
            };

            debug!("Successfully connected");

            let codec = MavlinkCodec::<true, true, false, false, false, false>::default();
            let (writer, reader) = ZenohRawAdapter::new(subscriber, publisher, codec).split();

            if let Err(reason) =
                default_send_receive_run(writer, reader, DRIVER_IDENTIFIER, &context).await
            {
                warn!("ZenohRaw send/receive tasks closed: {reason:?}");
            }

            debug!("Restarting connection loop...");
        }
    }

    #[instrument(level = "debug", skip(self))]
    fn info(&self) -> Box<dyn DriverInfo> {
        Box::new(ZenohRawInfo)
    }

    fn name(&self) -> Arc<String> {
        self.name.load_full()
    }

    fn uuid(&self) -> &DriverUuid {
        &self.uuid
    }
}

#[async_trait::async_trait]
impl AccumulatedDriverStatsProvider for ZenohRaw {
    async fn stats(&self) -> AccumulatedDriverStats {
        self.stats.read().await.clone()
    }

    async fn reset_stats(&self) {
        let mut stats = self.stats.write().await;
        stats.stats.input = None;
        stats.stats.output = None
    }
}

pub struct ZenohRawInfo;
impl DriverInfo for ZenohRawInfo {
    fn name(&self) -> &'static str {
        "ZenohRaw"
    }
    fn valid_schemes(&self) -> &'static [&'static str] {
        &["zenohraw"]
    }

    fn cli_example_legacy(&self) -> Vec<String> {
        let first_schema = self.valid_schemes()[0];

        vec![format!("{first_schema}:<IP>:<PORT>")]
    }

    fn cli_example_url(&self) -> Vec<String> {
        let first_schema = &self.valid_schemes()[0];
        vec![format!("{first_schema}://<IP>:<PORT>").to_string()]
    }

    fn create_endpoint_from_url(&self, url: &url::Url) -> Option<Arc<dyn Driver>> {
        println!("{}", &url);
        let _host = url.host_str().unwrap();
        let _port = url.port().unwrap();
        Some(Arc::new(ZenohRaw::builder("ZenohRaw").build()))
    }
}
