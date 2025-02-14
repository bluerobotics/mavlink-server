use std::{net::SocketAddr, sync::Arc};

use anyhow::Result;
use futures::{Sink, Stream, StreamExt};
use mavlink_codec::{codec::MavlinkCodec, error::DecoderError, Packet};
use tokio::{
    net::UdpSocket,
    sync::{broadcast, RwLock},
};
use tokio_util::udp::UdpFramed;
use tracing::*;

use crate::{
    callbacks::{Callbacks, MessageCallback},
    drivers::{
        generic_tasks::SendReceiveContext, udp::udp_send_task, Direction, Driver, DriverInfo,
    },
    protocol::Protocol,
    stats::{
        accumulated::driver::{AccumulatedDriverStats, AccumulatedDriverStatsProvider},
        driver::DriverUuid,
    },
};

#[derive(Debug)]
pub struct UdpClient {
    pub remote_addr: String,
    name: arc_swap::ArcSwap<String>,
    uuid: DriverUuid,
    direction: Direction,
    on_message_input: Callbacks<Arc<Protocol>>,
    on_message_output: Callbacks<Arc<Protocol>>,
    stats: Arc<RwLock<AccumulatedDriverStats>>,
}

pub struct UdpClientBuilder(UdpClient);

impl UdpClientBuilder {
    pub fn build(self) -> UdpClient {
        self.0
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

impl UdpClient {
    #[instrument(level = "debug")]
    pub fn builder(name: &str, remote_addr: &str) -> UdpClientBuilder {
        let name = Arc::new(name.to_string());

        UdpClientBuilder(Self {
            remote_addr: remote_addr.to_string(),
            name: arc_swap::ArcSwap::new(name.clone()),
            uuid: Self::generate_uuid(remote_addr),
            direction: Direction::Both,
            on_message_input: Callbacks::default(),
            on_message_output: Callbacks::default(),
            stats: Arc::new(RwLock::new(AccumulatedDriverStats::new(
                name,
                &UdpClientInfo,
            ))),
        })
    }
}

#[async_trait::async_trait]
impl Driver for UdpClient {
    #[instrument(level = "debug", skip(self, hub_sender))]
    async fn run(&self, hub_sender: broadcast::Sender<Arc<Protocol>>) -> Result<()> {
        let local_addr = "0.0.0.0:0".parse::<SocketAddr>().unwrap();
        let remote_addr = self.remote_addr.parse::<SocketAddr>()?;

        let context = SendReceiveContext {
            direction: self.direction,
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

            debug!("Trying to bind to address {local_addr:?}...");

            let socket = match UdpSocket::bind(local_addr).await {
                Ok(socket) => socket,
                Err(error) => {
                    error!("Failed binding UdpClient to address {local_addr:?}: {error:?}");
                    continue;
                }
            };

            debug!("UdpClient successfully bound to {local_addr}. Connecting UdpClient to {remote_addr:?}...");

            if let Err(error) = socket.connect(&remote_addr).await {
                error!("Failed connecting UdpClient to {remote_addr:?}: {error:?}");
                continue;
            };

            debug!("UdpClient successfully connected to {remote_addr:?}");

            let codec = MavlinkCodec::<true, true, false, false, false, false>::default();
            let (writer, reader) = UdpFramed::new(socket, codec).split();

            if let Err(reason) = udp_send_receive_run(writer, reader, &remote_addr, &context).await
            {
                warn!("Driver send/receive tasks closed: {reason:?}");
            }
        }
    }

    #[instrument(level = "debug", skip(self))]
    fn info(&self) -> Box<dyn DriverInfo> {
        return Box::new(UdpClientInfo);
    }

    fn name(&self) -> Arc<String> {
        self.name.load_full()
    }

    fn uuid(&self) -> &DriverUuid {
        &self.uuid
    }
}

#[instrument(level = "debug", skip(writer, reader, context,))]
async fn udp_send_receive_run<S, T>(
    mut writer: S,
    mut reader: T,
    remote_addr: &SocketAddr,
    context: &SendReceiveContext,
) -> Result<()>
where
    S: Sink<(Packet, SocketAddr), Error = std::io::Error> + std::marker::Unpin,
    T: Stream<Item = std::io::Result<(std::result::Result<Packet, DecoderError>, SocketAddr)>>
        + std::marker::Unpin,
{
    if context.direction.send_only() {
        return udp_send_task(&mut writer, remote_addr, context).await;
    }

    if context.direction.receive_only() {
        return udp_receive_task(&mut reader, remote_addr, context).await;
    }

    tokio::select! {
        result = udp_send_task(&mut writer, remote_addr, context) => {
            if let Err(error) = result {
                error!("Error in send task for {remote_addr}: {error:?}");
            }
        }
        result = udp_receive_task(&mut reader, remote_addr, context) => {
            if let Err(error) = result {
                error!("Error in receive task for {remote_addr}: {error:?}");
            }
        }
    }

    Ok(())
}

/// Receives messages from a Stream and sends them to the HUB Channel
#[instrument(level = "debug", skip(reader, context))]
async fn udp_receive_task<T>(
    reader: &mut T,
    remote_addr: &SocketAddr,
    context: &SendReceiveContext,
) -> Result<()>
where
    T: Stream<Item = std::io::Result<(std::result::Result<Packet, DecoderError>, SocketAddr)>>
        + std::marker::Unpin,
{
    loop {
        let (packet, remote_addr) = match reader.next().await {
            Some(Ok((Ok(packet), remote_addr))) => (packet, remote_addr),
            Some(Ok((Err(decode_error), remote_addr))) => {
                error!(origin = ?remote_addr, "Failed to decode packet: {decode_error:?}");
                continue;
            }
            Some(Err(io_error)) => {
                error!("Critical error trying to decode data from: {io_error:?}");
                break;
            }
            None => break,
        };

        let message = Arc::new(Protocol::new(&remote_addr.to_string(), packet));

        trace!(origin = ?remote_addr, "Received message: {message:?}");

        context.stats.write().await.stats.update_input(&message);

        for future in context.on_message_input.call_all(message.clone()) {
            if let Err(error) = future.await {
                debug!(origin = ?remote_addr, "Dropping message: on_message_input callback returned error: {error:?}");
                continue;
            }
        }

        if let Err(send_error) = context.hub_sender.send(message) {
            error!(origin = ?remote_addr, "Failed to send message to hub: {send_error:?}");
            continue;
        }

        trace!(origin = ?remote_addr, "Message sent to hub");
    }

    debug!("Driver receiver task stopped!");

    Ok(())
}

#[async_trait::async_trait]
impl AccumulatedDriverStatsProvider for UdpClient {
    async fn stats(&self) -> AccumulatedDriverStats {
        self.stats.read().await.clone()
    }

    async fn reset_stats(&self) {
        let mut stats = self.stats.write().await;
        stats.stats.input = None;
        stats.stats.output = None
    }
}

pub struct UdpClientInfo;
impl DriverInfo for UdpClientInfo {
    fn name(&self) -> &'static str {
        "UdpClient"
    }
    fn valid_schemes(&self) -> &'static [&'static str] {
        &["udpclient", "udpout", "udpc"]
    }

    fn cli_example_legacy(&self) -> Vec<String> {
        let first_schema = &self.valid_schemes()[0];
        let second_schema = &self.valid_schemes()[1];
        vec![
            format!("{first_schema}:<IP>:<PORT>"),
            format!("{first_schema}:0.0.0.0:14550"),
            format!("{second_schema}:192.168.2.1:14660"),
        ]
    }

    fn cli_example_url(&self) -> Vec<String> {
        let first_schema = &self.valid_schemes()[0];
        let second_schema = &self.valid_schemes()[1];
        vec![
            format!("{first_schema}://<IP>:<PORT>?direction=<DIRECTION|sender>").to_string(),
            url::Url::parse(&format!("{first_schema}://0.0.0.0:14550"))
                .unwrap()
                .to_string(),
            url::Url::parse(&format!(
                "{second_schema}://192.168.2.1:14660?direction=sender"
            ))
            .unwrap()
            .to_string(),
        ]
    }

    fn create_endpoint_from_url(&self, url: &url::Url) -> Option<Arc<dyn Driver>> {
        let host = url.host_str().unwrap();
        let port = url.port().unwrap();

        let direction = url
            .query_pairs()
            .find_map(|(key, value)| {
                if key != "direction" {
                    return None;
                }
                value.parse().map(Direction::from).ok()
            })
            .unwrap_or(Direction::Both);

        if direction.receive_only() {
            info!("UdpClient cannot be created with direction: {direction:?}, there is no way to receive messages without sending them for UDP.");
            return None;
        }

        Some(Arc::new(
            UdpClient::builder("UdpClient", &format!("{host}:{port}"))
                .direction(direction)
                .build(),
        ))
    }
}
