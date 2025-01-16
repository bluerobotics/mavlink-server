use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use anyhow::Result;
use futures::{Stream, StreamExt};
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
pub struct UdpServer {
    pub local_addr: String,
    name: arc_swap::ArcSwap<String>,
    uuid: DriverUuid,
    direction: Direction,
    on_message_input: Callbacks<Arc<Protocol>>,
    on_message_output: Callbacks<Arc<Protocol>>,
    stats: Arc<RwLock<AccumulatedDriverStats>>,
}

type Clients = HashMap<
    SocketAddr,
    (
        JoinHandle<std::result::Result<(), anyhow::Error>>,
        tokio::time::Instant,
    ),
>;

pub struct UdpServerBuilder(UdpServer);

impl UdpServerBuilder {
    pub fn build(self) -> UdpServer {
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

impl UdpServer {
    #[instrument(level = "debug")]
    pub fn builder(name: &str, local_addr: &str) -> UdpServerBuilder {
        let name = Arc::new(name.to_string());

        UdpServerBuilder(Self {
            local_addr: local_addr.to_string(),
            name: arc_swap::ArcSwap::new(name.clone()),
            uuid: Self::generate_uuid(local_addr),
            direction: Direction::Both,
            on_message_input: Callbacks::default(),
            on_message_output: Callbacks::default(),
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

            let socket = match UdpSocket::bind(&local_addr).await {
                Ok(socket) => Arc::new(socket),
                Err(error) => {
                    error!("Failed binding UdpServer to address {local_addr:?}: {error:?}");
                    continue;
                }
            };

            debug!("Waiting for clients...");

            let codec = MavlinkCodec::<true, true, false, false, false, false>::default();
            let (_writer, mut reader) = UdpFramed::new(socket.clone(), codec).split();

            if let Err(error) = udp_receive_task(&mut reader, socket, local_addr, &context).await {
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
    local_addr: SocketAddr,
    context: &SendReceiveContext,
) -> Result<()>
where
    T: Stream<Item = std::io::Result<(std::result::Result<Packet, DecoderError>, SocketAddr)>>
        + std::marker::Unpin,
{
    let mut clients: Clients = HashMap::new();

    let client_timeout = crate::cli::udp_server_timeout();

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

        if context.direction.can_receive() {
            context.stats.write().await.stats.update_input(&message);

            for future in context.on_message_input.call_all(message.clone()) {
                if let Err(error) = future.await {
                    debug!(origin = ?client_addr, "Dropping message: on_message_input callback returned error: {error:?}");
                    continue;
                }
            }
        }

        // Update clients
        {
            let socket = socket.clone();

            clients
                .entry(client_addr)
                .and_modify(|(_task, time)| {
                    // Time refresh
                    *time = tokio::time::Instant::now();
                })
                .or_insert_with(move || {
                    let task = spawn_send_task(socket.clone(), client_addr, context);

                    debug!("New client added: {client_addr:?}");

                    let current_time = tokio::time::Instant::now();

                    (task, current_time)
                });
        }

        {
            let socket = socket.clone();

            clients.retain(move |addr, (task, time)| {
                // Client Timeout
                if let Some(timeout) = client_timeout {
                    if time.elapsed() > timeout {
                        debug!("Client {addr} timed out.");

                        task.abort();

                        return false;
                    }
                }

                // Client task recreation
                if task.is_finished() {
                    debug!("Recreating sending task for client {addr:?}");
                    *task = spawn_send_task(socket.clone(), *addr, context);
                }

                true
            });
        }

        if context.direction.can_receive() {
            if let Err(send_error) = context.hub_sender.send(message) {
                error!(origin = ?client_addr, "Failed to send message to hub: {send_error:?}");
                continue;
            }
        }

        trace!(origin = ?client_addr, "Message sent to hub");
    }

    debug!("Driver receiver task stopped!");

    Ok(())
}

fn spawn_send_task(
    socket: Arc<UdpSocket>,
    client_addr: SocketAddr,
    context: &SendReceiveContext,
) -> JoinHandle<std::result::Result<(), anyhow::Error>> {
    let codec = MavlinkCodec::<true, true, false, false, false, false>::default();
    let (mut writer, _reader) = UdpFramed::new(socket.clone(), codec).split();

    tokio::spawn({
        let context = context.clone();
        async move { udp_send_task(&mut writer, &client_addr, &context).await }
    })
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
            format!("{first_schema}://<IP>:<PORT>?direction=<DIRECTION|receiver,sender>")
                .to_string(),
            url::Url::parse(&format!("{first_schema}://0.0.0.0:14550"))
                .unwrap()
                .to_string(),
            url::Url::parse(&format!(
                "{second_schema}://127.0.0.1:14660?direction=receiver"
            ))
            .unwrap()
            .to_string(),
        ]
    }

    fn create_endpoint_from_url(&self, url: &url::Url) -> Option<Arc<dyn Driver>> {
        dbg!(url);
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

        Some(Arc::new(
            UdpServer::builder("UdpServer", &format!("{host}:{port}"))
                .direction(direction)
                .build(),
        ))
    }
}
