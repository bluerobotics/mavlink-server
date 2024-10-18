use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use anyhow::Result;
use futures::{Sink, SinkExt, Stream, StreamExt};
use mavlink_codec::{codec::MavlinkCodec, error::DecoderError, Packet};
use tokio::{
    net::UdpSocket,
    sync::{broadcast, RwLock},
    task::JoinHandle,
};
use tokio_util::udp::UdpFramed;
use tracing::*;

use crate::{
    callbacks::{Callbacks, MessageCallback},
    drivers::{generic_tasks::SendReceiveContext, Driver, DriverInfo},
    protocol::Protocol,
    stats::{
        accumulated::driver::{AccumulatedDriverStats, AccumulatedDriverStatsProvider},
        driver::DriverUuid,
    },
};

#[derive(Debug)]
pub struct UdpServer {
    pub local_addr: String,
    name: arc_swap::ArcSwap<String>,
    uuid: DriverUuid,
    clients: Arc<RwLock<Clients>>,
    on_message_input: Callbacks<Arc<Protocol>>,
    on_message_output: Callbacks<Arc<Protocol>>,
    stats: Arc<RwLock<AccumulatedDriverStats>>,
}

type Clients = HashMap<
    (u8, u8),
    (
        SocketAddr,
        JoinHandle<std::result::Result<(), anyhow::Error>>,
    ),
>;

pub struct UdpServerBuilder(UdpServer);

impl UdpServerBuilder {
    pub fn build(self) -> UdpServer {
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

impl UdpServer {
    #[instrument(level = "debug")]
    pub fn builder(name: &str, local_addr: &str) -> UdpServerBuilder {
        let name = Arc::new(name.to_string());

        UdpServerBuilder(Self {
            local_addr: local_addr.to_string(),
            name: arc_swap::ArcSwap::new(name.clone()),
            uuid: Self::generate_uuid(local_addr),
            clients: Arc::new(RwLock::new(HashMap::new())),
            on_message_input: Callbacks::new(),
            on_message_output: Callbacks::new(),
            stats: Arc::new(RwLock::new(AccumulatedDriverStats::new(
                name,
                &UdpServerInfo,
            ))),
        })
    }
}

#[async_trait::async_trait]
impl Driver for UdpServer {
    #[instrument(level = "debug", skip(self, hub_sender))]
    async fn run(&self, hub_sender: broadcast::Sender<Arc<Protocol>>) -> Result<()> {
        let local_addr = self.local_addr.parse::<SocketAddr>()?;
        let clients = self.clients.clone();

        let context = SendReceiveContext {
            hub_sender,
            on_message_output: self.on_message_output.clone(),
            on_message_input: self.on_message_input.clone(),
            stats: self.stats.clone(),
        };

        let mut first = true;
        loop {
            if !first {
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                first = false;
            }

            debug!("Trying to bind to address {local_addr:?}...");

            let socket = match UdpSocket::bind(&local_addr).await {
                Ok(socket) => Arc::new(socket),
                Err(error) => {
                    error!("Failed binding UdpServer to address {local_addr:?}: {error:?}");
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                    continue;
                }
            };

            debug!("Waiting for clients...");

            let codec = MavlinkCodec::<true, true, false, false, false, false>::default();
            let (_writer, mut reader) = UdpFramed::new(socket.clone(), codec).split();

            if let Err(error) =
                udp_receive_task(&mut reader, socket, clients.clone(), local_addr, &context).await
            {
                error!("Error in receive task for {local_addr}: {error:?}");
            }
        }
    }

    #[instrument(level = "debug", skip(self))]
    fn info(&self) -> Box<dyn DriverInfo> {
        return Box::new(UdpServerInfo);
    }

    fn name(&self) -> Arc<String> {
        self.name.load_full()
    }

    fn uuid(&self) -> &DriverUuid {
        &self.uuid
    }
}

/// Receives messages from a Stream and sends them to the HUB Channel
#[instrument(level = "debug", skip(reader, context))]
async fn udp_receive_task<T>(
    reader: &mut T,
    socket: Arc<UdpSocket>,
    clients: Arc<RwLock<Clients>>,
    local_addr: SocketAddr,
    context: &SendReceiveContext,
) -> Result<()>
where
    T: Stream<Item = std::io::Result<(std::result::Result<Packet, DecoderError>, SocketAddr)>>
        + std::marker::Unpin,
{
    loop {
        let (packet, client_addr) = match reader.next().await {
            Some(Ok((Ok(packet), client_addr))) => (packet, client_addr),
            Some(Ok((Err(decode_error), client_addr))) => {
                error!(origin = ?client_addr, "Failed to decode packet: {decode_error:?}");
                continue;
            }
            Some(Err(io_error)) => {
                error!("Critical error trying to decode data from: {io_error:?}");
                break;
            }
            None => break,
        };

        let message = Arc::new(Protocol::new(&client_addr.to_string(), packet));

        trace!(origin = ?client_addr, "Received message: {message:?}");

        context.stats.write().await.stats.update_input(&message);

        for future in context.on_message_input.call_all(message.clone()) {
            if let Err(error) = future.await {
                debug!(origin = ?client_addr, "Dropping message: on_message_input callback returned error: {error:?}");
                continue;
            }
        }

        // Update clients
        let sysid = *message.system_id();
        let compid = *message.component_id();

        {
            let mut clients_guard = clients.write().await;

            let mut should_spawn = false;
            if let Some((old_client_addr, task)) = clients_guard.get(&(sysid, compid)) {
                if old_client_addr != &client_addr {
                    should_spawn = true;
                    task.abort();
                }
            } else {
                should_spawn = true;
            }

            if should_spawn {
                let codec = MavlinkCodec::<true, true, false, false, false, false>::default();
                let (mut writer, _reader) = UdpFramed::new(socket.clone(), codec).split();
                let context = context.clone();

                let task = tokio::spawn(async move {
                    udp_send_task(&mut writer, local_addr, client_addr, context).await
                });

                if let Some(old_client_addr) =
                    clients_guard.insert((sysid, compid), (client_addr, task))
                {
                    debug!("Client ({sysid},{compid}) updated from {old_client_addr:?} (OLD) to {client_addr:?} (NEW)");
                } else {
                    debug!("New client added: ({sysid},{compid}) -> {client_addr:?}");
                }
            }
        }

        if let Err(send_error) = context.hub_sender.send(message) {
            error!(origin = ?client_addr, "Failed to send message to hub: {send_error:?}");
            continue;
        }

        trace!(origin = ?client_addr, "Message sent to hub");
    }

    debug!("Driver receiver task stopped!");

    Ok(())
}

/// Receives messages from the HUB Channel and sends them to a Sink
#[instrument(level = "debug", skip(writer, context))]
async fn udp_send_task<S>(
    writer: &mut S,
    local_addr: SocketAddr,
    client_addr: SocketAddr,
    context: SendReceiveContext,
) -> Result<()>
where
    S: Sink<(Packet, SocketAddr), Error = std::io::Error> + std::marker::Unpin,
{
    let mut hub_receiver = context.hub_sender.subscribe();

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

        if message.origin.eq(&client_addr.to_string()) {
            continue; // Don't do loopback
        }

        context.stats.write().await.stats.update_output(&message);

        for future in context.on_message_output.call_all(message.clone()) {
            if let Err(error) = future.await {
                debug!(
                    client = ?client_addr, "Dropping message: on_message_output callback returned error: {error:?}"
                );
                continue;
            }
        }

        if let Err(error) = writer.send(((**message).clone(), client_addr)).await {
            error!(client = ?client_addr, "Failed to send message: {error:?}");
            break;
        }

        trace!("Message sent to {client_addr}: {:?}", message.as_slice());
    }
    Ok(())
}

#[async_trait::async_trait]
impl AccumulatedDriverStatsProvider for UdpServer {
    async fn stats(&self) -> AccumulatedDriverStats {
        self.stats.read().await.clone()
    }

    async fn reset_stats(&self) {
        let mut stats = self.stats.write().await;
        stats.stats.input = None;
        stats.stats.output = None
    }
}

pub struct UdpServerInfo;
impl DriverInfo for UdpServerInfo {
    fn name(&self) -> &'static str {
        "UdpServer"
    }
    fn valid_schemes(&self) -> &'static [&'static str] {
        &["udpserver", "udpin", "udps"]
    }

    fn cli_example_legacy(&self) -> Vec<String> {
        let first_schema = &self.valid_schemes()[0];
        let second_schema = &self.valid_schemes()[1];
        vec![
            format!("{first_schema}:<IP>:<PORT>"),
            format!("{first_schema}:0.0.0.0:14550"),
            format!("{second_schema}:127.0.0.1:14660"),
        ]
    }

    fn cli_example_url(&self) -> Vec<String> {
        let first_schema = &self.valid_schemes()[0];
        let second_schema = &self.valid_schemes()[1];
        vec![
            format!("{first_schema}://<IP>:<PORT>").to_string(),
            url::Url::parse(&format!("{first_schema}://0.0.0.0:14550"))
                .unwrap()
                .to_string(),
            url::Url::parse(&format!("{second_schema}://127.0.0.1:14660"))
                .unwrap()
                .to_string(),
        ]
    }

    fn create_endpoint_from_url(&self, url: &url::Url) -> Option<Arc<dyn Driver>> {
        let host = url.host_str().unwrap();
        let port = url.port().unwrap();
        Some(Arc::new(
            UdpServer::builder("UdpServer", &format!("{host}:{port}")).build(),
        ))
    }
}
