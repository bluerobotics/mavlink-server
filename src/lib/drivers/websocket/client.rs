use std::{sync::Arc, time::Duration};

use anyhow::Result;
use futures::StreamExt;
use mavlink_codec::codec::MavlinkCodec;
use tokio::{
    sync::{RwLock, broadcast},
    time,
};
use tracing::*;
use url::Url;

use crate::{
    callbacks::{Callbacks, MessageCallback},
    drivers::{
        Direction, Driver, DriverInfo,
        generic_tasks::{SendReceiveContext, default_send_receive_run},
        websocket::client_adapter::WebSocketClientAdapter,
    },
    protocol::Protocol,
    stats::{
        accumulated::driver::{AccumulatedDriverStats, AccumulatedDriverStatsProvider},
        driver::DriverUuid,
    },
};

#[derive(Debug)]
pub struct WebSocketClientDriver {
    remote_addr: String,
    name: arc_swap::ArcSwap<String>,
    uuid: DriverUuid,
    scheme: String,
    direction: Direction,
    on_message_input: Callbacks<Arc<Protocol>>,
    on_message_output: Callbacks<Arc<Protocol>>,
    stats: Arc<RwLock<AccumulatedDriverStats>>,
}

pub struct WebSocketClientDriverBuilder(WebSocketClientDriver);

impl WebSocketClientDriverBuilder {
    pub fn build(self) -> WebSocketClientDriver {
        self.0
    }

    pub fn scheme(mut self, scheme: &str) -> Self {
        self.0.scheme = scheme.to_string();
        self
    }

    pub fn direction(mut self, direction: Direction) -> Self {
        self.0.direction = direction;
        self
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

impl WebSocketClientDriver {
    #[instrument(level = "debug")]
    pub fn builder(name: &str, remote_addr: &str) -> WebSocketClientDriverBuilder {
        let name = Arc::new(name.to_string());

        WebSocketClientDriverBuilder(Self {
            remote_addr: remote_addr.to_string(),
            name: arc_swap::ArcSwap::new(name.clone()),
            uuid: Self::generate_uuid(&format!("websocket-client:{remote_addr}")),
            scheme: "ws".to_string(),
            direction: Direction::Both,
            on_message_input: Callbacks::default(),
            on_message_output: Callbacks::default(),
            stats: Arc::new(RwLock::new(AccumulatedDriverStats::new(
                name,
                &WebSocketClientInfo,
            ))),
        })
    }
}

#[async_trait::async_trait]
impl Driver for WebSocketClientDriver {
    #[instrument(level = "debug", skip(self, hub_sender))]
    async fn run(&self, hub_sender: broadcast::Sender<Arc<Protocol>>) -> Result<()> {
        let server_addr = &self.remote_addr.to_string();
        let server_scheme = &self.scheme;
        let server_url = &format!("{server_scheme}://{server_addr}/");

        let context = SendReceiveContext {
            direction: self.direction,
            hub_sender,
            on_message_output: self.on_message_output.clone(),
            on_message_input: self.on_message_input.clone(),
            stats: self.stats.clone(),
        };

        let mut interval = time::interval(Duration::from_secs(5));
        let mut first = true;
        loop {
            if first {
                first = false;
            } else {
                interval.tick().await;
                debug!("Attempting to reconnect to WebSocket server...");
            }

            debug!("Trying to connect to {server_url:?}...");

            let (ws_stream, _) = match tokio_tungstenite::connect_async(server_url).await {
                Ok(result) => result,
                Err(error) => {
                    error!("WebSocket connection failed: {error:?}");
                    continue;
                }
            };

            debug!("Successfully connected");

            let codec = MavlinkCodec::<true, true, false, false, false, false>::default();
            let (writer, reader) = WebSocketClientAdapter::new(ws_stream, codec).split();

            if let Err(reason) =
                default_send_receive_run(writer, reader, server_addr, &context).await
            {
                warn!("WebSocket client send/receive tasks closed: {reason:?}");
            }

            debug!("Restarting connection loop...");
        }
    }

    #[instrument(level = "debug", skip(self))]
    fn info(&self) -> Box<dyn DriverInfo> {
        Box::new(WebSocketClientInfo)
    }

    fn name(&self) -> Arc<String> {
        self.name.load_full()
    }

    fn uuid(&self) -> &DriverUuid {
        &self.uuid
    }
}

#[async_trait::async_trait]
impl AccumulatedDriverStatsProvider for WebSocketClientDriver {
    async fn stats(&self) -> AccumulatedDriverStats {
        self.stats.read().await.clone()
    }

    async fn reset_stats(&self) {
        let mut stats = self.stats.write().await;
        stats.stats.input = None;
        stats.stats.output = None
    }
}

pub struct WebSocketClientInfo;
impl DriverInfo for WebSocketClientInfo {
    fn name(&self) -> &'static str {
        "WsClient"
    }
    fn valid_schemes(&self) -> &'static [&'static str] {
        &["wsclient", "wssclient"]
    }

    fn cli_example_legacy(&self) -> Vec<String> {
        let first_schema = self.valid_schemes()[0];

        vec![
            format!("{first_schema}:<HOST>:<PORT>"),
            format!("{first_schema}:0.0.0.0:9988"),
            format!("{first_schema}:192.168.1.100:9000"),
        ]
    }

    fn cli_example_url(&self) -> Vec<String> {
        let first_schema = &self.valid_schemes()[0];
        vec![
            format!("{first_schema}://<HOST>:<PORT>?direction=<DIRECTION|receiver,sender>"),
            url::Url::parse(&format!("{first_schema}://0.0.0.0:9988"))
                .unwrap()
                .to_string(),
            url::Url::parse(&format!(
                "{first_schema}://192.168.1.100:9000?direction=receiver"
            ))
            .unwrap()
            .to_string(),
        ]
    }

    fn create_endpoint_from_url(&self, url: &Url) -> Option<Arc<dyn Driver>> {
        let scheme = url.scheme().strip_suffix("client").unwrap();
        let host = url.host_str().unwrap();
        let port = url.port().unwrap();
        let remote_addr = format!("{host}:{port}");

        let direction = url
            .query_pairs()
            .find_map(|(key, value)| {
                if key != "direction" {
                    return None;
                }
                value.parse().ok()
            })
            .unwrap_or(Direction::Both);

        Some(Arc::new(
            WebSocketClientDriver::builder("WsClient", &remote_addr)
                .scheme(scheme)
                .direction(direction)
                .build(),
        ))
    }
}
