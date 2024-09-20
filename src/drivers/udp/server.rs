use std::{collections::HashMap, sync::Arc};

use anyhow::Result;
use mavlink_server::callbacks::{Callbacks, MessageCallback};
use tokio::{
    net::UdpSocket,
    sync::{broadcast, RwLock},
};
use tracing::*;

use crate::{
    drivers::{Driver, DriverInfo},
    protocol::{read_all_messages, Protocol},
    stats::{
        accumulated::driver::{AccumulatedDriverStats, AccumulatedDriverStatsProvider},
        driver::DriverUuid,
    },
};

pub struct UdpServer {
    pub local_addr: String,
    name: arc_swap::ArcSwap<String>,
    uuid: DriverUuid,
    clients: Arc<RwLock<HashMap<(u8, u8), String>>>,
    on_message_input: Callbacks<Arc<Protocol>>,
    on_message_output: Callbacks<Arc<Protocol>>,
    stats: Arc<RwLock<AccumulatedDriverStats>>,
}

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

    #[instrument(level = "debug", skip(self, socket, hub_sender, clients))]
    async fn udp_receive_task(
        &self,
        socket: Arc<UdpSocket>,
        hub_sender: Arc<broadcast::Sender<Arc<Protocol>>>,
        clients: Arc<RwLock<HashMap<(u8, u8), String>>>,
    ) -> Result<()> {
        let mut buf = Vec::with_capacity(1024);

        loop {
            match socket.recv_buf_from(&mut buf).await {
                Ok((bytes_received, client_addr)) if bytes_received > 0 => {
                    let client_addr = &client_addr.to_string();

                    read_all_messages(client_addr, &mut buf, |message| async {
                        let message = Arc::new(message);

                        self.stats
                            .write()
                            .await
                            .stats
                            .update_input(&message);

                        for future in self.on_message_input.call_all(message.clone()) {
                            if let Err(error) = future.await {
                                debug!("Dropping message: on_message_input callback returned error: {error:?}");
                                continue;
                            }
                        }

                        // Update clients
                        let sysid = message.system_id();
                        let compid = message.component_id();
                        if let Some(old_client_addr) = clients
                            .write()
                            .await
                            .insert((sysid, compid), client_addr.clone())
                        {
                            debug!("Client ({sysid},{compid}) updated from {old_client_addr:?} (OLD) to {client_addr:?} (NEW)");
                        } else {
                            debug!("Client added: ({sysid},{compid}) -> {client_addr:?}");
                        }


                        if let Err(error) = hub_sender.send(message) {
                            error!("Failed to send message to hub: {error:?}");
                        }
                    })
                    .await;
                }
                Ok((_, client_addr)) => {
                    warn!("UDP connection closed by {client_addr}.");
                    break;
                }
                Err(error) => {
                    error!("Failed to receive UDP message: {error:?}");
                    break;
                }
            }
        }

        debug!("UdpServer Receiver task finished");
        Ok(())
    }

    #[instrument(level = "debug", skip(self, socket, hub_receiver, clients))]
    async fn udp_send_task(
        &self,
        socket: Arc<UdpSocket>,
        mut hub_receiver: broadcast::Receiver<Arc<Protocol>>,
        clients: Arc<RwLock<HashMap<(u8, u8), String>>>,
    ) -> Result<()> {
        loop {
            match hub_receiver.recv().await {
                Ok(message) => {
                    for ((_, _), client_addr) in clients.read().await.iter() {
                        if message.origin.eq(client_addr) {
                            continue; // Don't do loopback
                        }

                        self.stats.write().await.stats.update_output(&message);

                        for future in self.on_message_output.call_all(message.clone()) {
                            if let Err(error) = future.await {
                                debug!("Dropping message: on_message_output callback returned error: {error:?}");
                                continue;
                            }
                        }

                        match socket.send_to(message.raw_bytes(), client_addr).await {
                            Ok(_) => {
                                // Message sent successfully
                            }
                            Err(ref error)
                                if error.kind() == std::io::ErrorKind::ConnectionRefused =>
                            {
                                // error!("UDP connection refused: {error:?}");
                                continue;
                            }
                            Err(error) => {
                                error!("Failed to send UDP message: {error:?}");
                                break;
                            }
                        }
                    }
                }
                Err(error) => {
                    error!("Failed to receive message from hub: {error:?}");
                }
            }
        }
    }
}

#[async_trait::async_trait]
impl Driver for UdpServer {
    #[instrument(level = "debug", skip(self, hub_sender))]
    async fn run(&self, hub_sender: broadcast::Sender<Arc<Protocol>>) -> Result<()> {
        let local_addr = &self.local_addr;
        let clients = self.clients.clone();
        loop {
            let socket = match UdpSocket::bind(&local_addr).await {
                Ok(socket) => Arc::new(socket),
                Err(error) => {
                    error!("Failed binding UdpServer to address {local_addr:?}: {error:?}");
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                    continue;
                }
            };

            let hub_sender = Arc::new(hub_sender.clone());
            let hub_receiver = hub_sender.subscribe();

            tokio::select! {
                result = self.udp_receive_task(socket.clone(), hub_sender, clients.clone()) => {
                    if let Err(error) = result {
                        error!("Error in receiving UDP messages: {error:?}");
                    }
                }
                result = self.udp_send_task(socket, hub_receiver, clients.clone()) => {
                    if let Err(error) = result {
                        error!("Error in sending UDP messages: {error:?}");
                    }
                }
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
