use std::sync::Arc;

use anyhow::Result;
use mavlink_server::callbacks::{Callbacks, MessageCallback};
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::{broadcast, RwLock},
};
use tracing::*;

use crate::{
    drivers::{Driver, DriverInfo},
    protocol::Protocol,
    stats::accumulated::driver::{AccumulatedDriverStats, AccumulatedDriverStatsProvider},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MAVLinkMessage<T> {
    pub header: mavlink::MavHeader,
    pub message: T,
}

pub struct Rest {
    on_message_input: Callbacks<Arc<Protocol>>,
    on_message_output: Callbacks<Arc<Protocol>>,
    stats: Arc<RwLock<AccumulatedDriverStats>>,
}

pub struct RestBuilder(Rest);

impl RestBuilder {
    pub fn build(self) -> Rest {
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

impl Rest {
    #[instrument(level = "debug")]
    pub fn builder() -> RestBuilder {
        RestBuilder(Self {
            on_message_input: Callbacks::new(),
            on_message_output: Callbacks::new(),
            stats: Arc::new(RwLock::new(AccumulatedDriverStats::default())),
        })
    }

    #[instrument(level = "debug", skip(on_message_input))]
    async fn receive_task(
        hub_sender: broadcast::Sender<Arc<Protocol>>,
        on_message_input: &Callbacks<Arc<Protocol>>,
        ws_receiver: &mut broadcast::Receiver<String>,
    ) -> Result<()> {
        match ws_receiver.recv().await {
            Ok(message) => {
                if let Ok(content) =
                    json5::from_str::<MAVLinkMessage<mavlink::ardupilotmega::MavMessage>>(&message)
                {
                    let mut message_raw = mavlink::MAVLinkV2MessageRaw::new();
                    message_raw.serialize_message(content.header, &content.message);
                    let bus_message = Arc::new(Protocol::new("Ws", message_raw));

                    for future in on_message_input.call_all(bus_message.clone()) {
                        if let Err(error) = future.await {
                            debug!("Dropping message: on_message_input callback returned error: {error:?}");
                            continue;
                        }
                    }

                    if let Err(error) = hub_sender.send(bus_message) {
                        error!("Failed to send message to hub: {error:?}");
                    }
                    return Ok(());
                } else if let Ok(content) =
                    json5::from_str::<MAVLinkMessage<mavlink::common::MavMessage>>(&message)
                {
                    let mut message_raw = mavlink::MAVLinkV2MessageRaw::new();
                    message_raw.serialize_message(content.header, &content.message);
                    let bus_message = Arc::new(Protocol::new("Ws", message_raw));

                    for future in on_message_input.call_all(bus_message.clone()) {
                        if let Err(error) = future.await {
                            debug!("Dropping message: on_message_input callback returned error: {error:?}");
                            continue;
                        }
                    }

                    if let Err(error) = hub_sender.send(bus_message) {
                        error!("Failed to send message to hub: {error:?}");
                    }
                    return Ok(());
                }
                return Err(anyhow::anyhow!(
                    "Failed to parse message, not a valid MAVLinkMessage: {message:?}"
                ));
            }
            // We got problems
            Err(error) => {
                return Err(anyhow::anyhow!(
                    "Failed to receive message from ws: {error:?}"
                ));
            }
        }
    }

    #[instrument(level = "debug", skip(on_message_output))]
    async fn send_task(
        mut hub_receiver: broadcast::Receiver<Arc<Protocol>>,
        on_message_output: &Callbacks<Arc<Protocol>>,
    ) -> Result<()> {
        loop {
            match hub_receiver.recv().await {
                Ok(message) => {
                    for future in on_message_output.call_all(message.clone()) {
                        if let Err(error) = future.await {
                            debug!("Dropping message: on_message_output callback returned error: {error:?}");
                            continue;
                        }
                    }

                    let mut bytes =
                        mavlink::async_peek_reader::AsyncPeekReader::new(message.raw_bytes());
                    let (header, message): (
                        mavlink::MavHeader,
                        mavlink::ardupilotmega::MavMessage,
                    ) = mavlink::read_v2_msg_async(&mut bytes).await.unwrap();
                    crate::web::send_message(parse_query(&MAVLinkMessage {
                        header: header,
                        message: message,
                    }))
                    .await;
                }
                Err(error) => {
                    error!("Failed to receive message from hub: {error:?}");
                }
            }
        }
    }
}

pub fn parse_query<T: serde::ser::Serialize>(message: &T) -> String {
    let error_message =
        "Not possible to parse mavlink message, please report this issue!".to_string();
    serde_json::to_string_pretty(&message).unwrap_or(error_message)
}

#[async_trait::async_trait]
impl Driver for Rest {
    #[instrument(level = "debug", skip(self, hub_sender))]
    async fn run(&self, hub_sender: broadcast::Sender<Arc<Protocol>>) -> Result<()> {
        loop {
            let hub_sender = hub_sender.clone();
            let hub_receiver = hub_sender.subscribe();
            let mut ws_receiver = crate::web::create_message_receiver();

            tokio::select! {
                result = Rest::send_task(hub_receiver, &self.on_message_output) => {
                    if let Err(e) = result {
                        error!("Error in rest sender task: {e:?}");
                    }
                }
                result = Rest::receive_task(hub_sender, &self.on_message_input, &mut ws_receiver) => {
                    if let Err(e) = result {
                        error!("Error in rest receive task: {e:?}");
                    }
                }
            }
        }
    }

    #[instrument(level = "debug", skip(self))]
    fn info(&self) -> Box<dyn DriverInfo> {
        Box::new(RestInfo)
    }
}

#[async_trait::async_trait]
impl AccumulatedDriverStatsProvider for Rest {
    async fn stats(&self) -> AccumulatedDriverStats {
        self.stats.read().await.clone()
    }

    async fn reset_stats(&self) {
        *self.stats.write().await = AccumulatedDriverStats {
            input: None,
            output: None,
        }
    }
}

pub struct RestInfo;
impl DriverInfo for RestInfo {
    fn name(&self) -> &str {
        "Rest"
    }

    fn valid_schemes(&self) -> Vec<String> {
        vec![]
    }

    fn cli_example_legacy(&self) -> Vec<String> {
        vec![]
    }

    fn cli_example_url(&self) -> Vec<String> {
        vec![]
    }

    fn create_endpoint_from_url(&self, url: &url::Url) -> Option<Arc<dyn Driver>> {
        Some(Arc::new(Rest::builder().build()))
    }
}
