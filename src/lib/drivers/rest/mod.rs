pub mod data;

use std::sync::Arc;

use anyhow::Result;
use axum::extract::ws;
use tokio::sync::{broadcast, RwLock};
use tracing::*;

use crate::{
    callbacks::{Callbacks, MessageCallback},
    drivers::{generic_tasks::SendReceiveContext, Driver, DriverInfo},
    mavlink_json::MAVLinkJSON,
    protocol::Protocol,
    stats::{
        accumulated::driver::{AccumulatedDriverStats, AccumulatedDriverStatsProvider},
        driver::DriverUuid,
    },
    web::routes::v1::rest::websocket,
};

#[derive(Debug)]
pub struct Rest {
    name: arc_swap::ArcSwap<String>,
    uuid: DriverUuid,
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
    pub fn builder(name: &str) -> RestBuilder {
        let name = Arc::new(name.to_string());

        RestBuilder(Self {
            name: arc_swap::ArcSwap::new(name.clone()),
            uuid: Self::generate_uuid(&name),
            on_message_input: Callbacks::default(),
            on_message_output: Callbacks::default(),
            stats: Arc::new(RwLock::new(AccumulatedDriverStats::new(name, &RestInfo))),
        })
    }

    #[instrument(level = "debug", skip_all)]
    async fn receive_task(
        context: &SendReceiveContext,
        ws_receiver: &mut broadcast::Receiver<String>,
    ) -> Result<()> {
        while let Ok(message) = ws_receiver.recv().await {
            let Ok(content) =
                json5::from_str::<MAVLinkJSON<mavlink::ardupilotmega::MavMessage>>(&message)
            else {
                debug!("Failed to parse message, not a valid MAVLinkMessage: {message:?}");
                continue;
            };

            let bus_message = Arc::new(Protocol::from_mavlink_raw(
                content.header.inner,
                &content.message,
                "Ws",
            ));

            trace!("Received message: {bus_message:?}");

            context.stats.write().await.stats.update_input(&bus_message);

            for future in context.on_message_input.call_all(bus_message.clone()) {
                if let Err(error) = future.await {
                    debug!("Dropping message: on_message_input callback returned error: {error:?}");
                    continue;
                }
            }

            if let Err(error) = context.hub_sender.send(bus_message) {
                error!("Failed to send message to hub: {error:?}");
                continue;
            }

            trace!("Message sent to hub");
        }

        debug!("Driver receiver task stopped!");

        Ok(())
    }

    #[instrument(level = "debug", skip_all)]
    async fn send_task(context: &SendReceiveContext) -> Result<()> {
        let mut hub_receiver = context.hub_sender.subscribe();

        let origin = "Ws";
        let uuid = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, origin.as_bytes());

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

            if message.origin.eq(origin) {
                continue; // Don't do loopback
            }

            context.stats.write().await.stats.update_output(&message);

            for future in context.on_message_output.call_all(message.clone()) {
                if let Err(error) = future.await {
                    debug!(
                        "Dropping message: on_message_output callback returned error: {error:?}"
                    );
                    continue;
                }
            }

            let Ok(mavlink_json) = message.to_mavlink_json().await else {
                continue;
            };

            let json_string = parse_query(&mavlink_json);
            data::update((mavlink_json.header, mavlink_json.message));

            websocket::broadcast(uuid, ws::Message::Text(json_string)).await;
        }

        debug!("Driver sender task stopped!");

        Ok(())
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
        let context = SendReceiveContext {
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

            let mut ws_receiver = websocket::create_message_receiver();

            tokio::select! {
                result = Rest::send_task(&context) => {
                    if let Err(e) = result {
                        error!("Error in rest sender task: {e:?}");
                    }
                }
                result = Rest::receive_task(&context, &mut ws_receiver) => {
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

    fn name(&self) -> Arc<String> {
        self.name.load_full()
    }

    fn uuid(&self) -> &DriverUuid {
        &self.uuid
    }
}

#[async_trait::async_trait]
impl AccumulatedDriverStatsProvider for Rest {
    async fn stats(&self) -> AccumulatedDriverStats {
        self.stats.read().await.clone()
    }

    async fn reset_stats(&self) {
        let mut stats = self.stats.write().await;
        stats.stats.input = None;
        stats.stats.output = None
    }
}

pub struct RestInfo;
impl DriverInfo for RestInfo {
    fn name(&self) -> &'static str {
        "Rest"
    }

    fn valid_schemes(&self) -> &'static [&'static str] {
        &[]
    }

    fn cli_example_legacy(&self) -> Vec<String> {
        vec![]
    }

    fn cli_example_url(&self) -> Vec<String> {
        vec![]
    }

    fn create_endpoint_from_url(&self, _url: &url::Url) -> Option<Arc<dyn Driver>> {
        None
    }
}
