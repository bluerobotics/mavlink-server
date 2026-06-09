use std::sync::Arc;

use anyhow::Result;
use bytes::BytesMut;
use mavlink_codec::codec::MavlinkCodec;
use tokio::sync::{RwLock, broadcast};
use tokio_util::codec::Decoder;
use tracing::*;
use zenoh;

use crate::{
    callbacks::{Callbacks, MessageCallback},
    drivers::{Driver, DriverInfo, generic_tasks::SendReceiveContext},
    protocol::Protocol,
    stats::{
        accumulated::driver::{AccumulatedDriverStats, AccumulatedDriverStatsProvider},
        driver::DriverUuid,
    },
};

const TOPIC_PREFIX: &str = "mavlink_raw";

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

    #[instrument(level = "debug", skip_all)]
    async fn receive_task(
        context: &SendReceiveContext,
        session: Arc<zenoh::Session>,
        codec: &mut MavlinkCodec<true, true, false, false, false, false>,
        buffer: &mut BytesMut,
    ) -> Result<()> {
        let subscriber = match session
            .declare_subscriber(format!("{TOPIC_PREFIX}/in"))
            .await
        {
            Ok(subscriber) => subscriber,
            Err(error) => {
                return Err(anyhow::anyhow!(
                    "Failed to create subscriber for mavlink raw data: {error:?}"
                ));
            }
        };

        while let Ok(sample) = subscriber.recv_async().await {
            buffer.extend_from_slice(&sample.payload().to_bytes());

            loop {
                let packet = match codec.decode(buffer) {
                    Ok(Some(Ok(packet))) => packet,
                    Ok(Some(Err(decode_error))) => {
                        error!("Failed to decode packet: {decode_error:?}");
                        continue;
                    }
                    Ok(None) => break,
                    Err(error) => {
                        error!("Failed to decode packet: {error:?}");
                        continue;
                    }
                };

                let bus_message = Arc::new(Protocol::new("zenohraw", packet));

                trace!("Received message: {bus_message:?}");

                context.stats.write().await.stats.update_input(&bus_message);

                for future in context.on_message_input.call_all(bus_message.clone()) {
                    if let Err(error) = future.await {
                        debug!(
                            "Dropping message: on_message_input callback returned error: {error:?}"
                        );
                        continue;
                    }
                }

                if let Err(error) = context.hub_sender.send(bus_message) {
                    error!("Failed to send message to hub: {error:?}");
                    continue;
                }

                trace!("Message sent to hub");
            }
        }

        debug!("Driver receiver task stopped!");

        Ok(())
    }

    #[instrument(level = "debug", skip_all)]
    async fn send_task(context: &SendReceiveContext, session: Arc<zenoh::Session>) -> Result<()> {
        let mut hub_receiver = context.hub_sender.subscribe();
        let topic_name = format!("{TOPIC_PREFIX}/out");

        let publisher = match session.declare_publisher(&topic_name).await {
            Ok(publisher) => publisher,
            Err(error) => {
                return Err(anyhow::anyhow!(
                    "Failed to create publisher for mavlink raw data: {error:?}"
                ));
            }
        };

        'mainloop: loop {
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

            if message.origin.eq("zenohraw") {
                continue; // Don't do loopback
            }

            context.stats.write().await.stats.update_output(&message);

            for future in context.on_message_output.call_all(message.clone()) {
                if let Err(error) = future.await {
                    debug!(
                        "Dropping message: on_message_output callback returned error: {error:?}"
                    );
                    continue 'mainloop;
                }
            }

            if let Err(error) = publisher.put(message.as_slice()).await {
                error!("Failed to send message to {topic_name}: {error:?}");
            } else {
                trace!("Message sent to {topic_name}: {:?}", message.as_slice());
            }
        }

        debug!("Driver sender task stopped!");

        Ok(())
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

            let session = super::session().await;

            debug!("Successfully connected");

            let mut codec = MavlinkCodec::<true, true, false, false, false, false>::default();
            let mut buffer = BytesMut::new();

            tokio::select! {
                result = ZenohRaw::send_task(&context, session.clone()) => {
                    if let Err(error) = result {
                        error!("Error in send task: {error:?}");
                    }
                }
                result = ZenohRaw::receive_task(&context, session, &mut codec, &mut buffer) => {
                    if let Err(error) = result {
                        error!("Error in receive task: {error:?}");
                    }
                }
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
