use std::sync::Arc;

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
    stats::accumulated::driver::{AccumulatedDriverStats, AccumulatedDriverStatsProvider},
};

pub struct UdpClient {
    pub remote_addr: String,
    on_message_input: Callbacks<Arc<Protocol>>,
    on_message_output: Callbacks<Arc<Protocol>>,
    stats: Arc<RwLock<AccumulatedDriverStats>>,
}

pub struct UdpClientBuilder(UdpClient);

impl UdpClientBuilder {
    pub fn build(self) -> UdpClient {
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

impl UdpClient {
    #[instrument(level = "debug")]
    pub fn builder(remote_addr: &str) -> UdpClientBuilder {
        UdpClientBuilder(Self {
            remote_addr: remote_addr.to_string(),
            on_message_input: Callbacks::new(),
            on_message_output: Callbacks::new(),
            stats: Arc::new(RwLock::new(AccumulatedDriverStats::default())),
        })
    }

    #[instrument(level = "debug", skip(self, socket))]
    async fn udp_receive_task(
        &self,
        socket: Arc<UdpSocket>,
        hub_sender: Arc<broadcast::Sender<Arc<Protocol>>>,
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
                            .update_input(&message);

                        for future in self.on_message_input.call_all(message.clone()) {
                            if let Err(error) = future.await {
                                debug!("Dropping message: on_message_input callback returned error: {error:?}");
                                continue;
                            }
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

        debug!("UdpClient Receiver task finished");
        Ok(())
    }

    #[instrument(level = "debug", skip(self, socket, hub_receiver))]
    async fn udp_send_task(
        &self,
        socket: Arc<UdpSocket>,
        mut hub_receiver: broadcast::Receiver<Arc<Protocol>>,
    ) -> Result<()> {
        loop {
            match hub_receiver.recv().await {
                Ok(message) => {
                    if message.origin.eq(&socket.peer_addr()?.to_string()) {
                        continue; // Don't do loopback
                    }

                    self.stats.write().await.update_output(&message);

                    for future in self.on_message_output.call_all(message.clone()) {
                        if let Err(error) = future.await {
                            debug!(
                                "Dropping message: on_message_output callback returned error: {error:?}"
                            );
                            continue;
                        }
                    }

                    match socket.send(message.raw_bytes()).await {
                        Ok(_) => {
                            // Message sent successfully
                        }
                        Err(ref error) if error.kind() == std::io::ErrorKind::ConnectionRefused => {
                            // error!("UDP connection refused: {error:?}");
                            continue;
                        }
                        Err(error) => {
                            error!("Failed to send UDP message: {error:?}");
                            break;
                        }
                    }
                }
                Err(error) => {
                    error!("Failed to receive message from hub: {error:?}");
                }
            }
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl Driver for UdpClient {
    #[instrument(level = "debug", skip(self, hub_sender))]
    async fn run(&self, hub_sender: broadcast::Sender<Arc<Protocol>>) -> Result<()> {
        let local_addr = "0.0.0.0:0";
        let remote_addr = self.remote_addr.clone();

        loop {
            let socket = match UdpSocket::bind(local_addr).await {
                Ok(socket) => Arc::new(socket),
                Err(error) => {
                    error!("Failed binding UdpClient to address {local_addr:?}: {error:?}");
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                    continue;
                }
            };

            debug!("UdpClient successfully bound to {local_addr}. Connecting UdpClient to {remote_addr:?}...");

            if let Err(error) = socket.connect(&remote_addr).await {
                error!("Failed connecting UdpClient to {remote_addr:?}: {error:?}");
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                continue;
            };

            debug!("UdpClient successfully connected to {remote_addr:?}");

            let hub_sender = Arc::new(hub_sender.clone());
            let hub_receiver = hub_sender.subscribe();

            tokio::select! {
                result = self.udp_receive_task(socket.clone(), hub_sender) => {
                    if let Err(error) = result {
                        error!("Error in receiving UDP messages: {error:?}");
                    }
                }
                result = self.udp_send_task(socket, hub_receiver) => {
                    if let Err(error) = result {
                        error!("Error in sending UDP messages: {error:?}");
                    }
                }
            }
        }
    }

    #[instrument(level = "debug", skip(self))]
    fn info(&self) -> Box<dyn DriverInfo> {
        return Box::new(UdpClientInfo);
    }
}

#[async_trait::async_trait]
impl AccumulatedDriverStatsProvider for UdpClient {
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
            format!("{first_schema}://<IP>:<PORT>").to_string(),
            url::Url::parse(&format!("{first_schema}://0.0.0.0:14550"))
                .unwrap()
                .to_string(),
            url::Url::parse(&format!("{second_schema}://192.168.2.1:14660"))
                .unwrap()
                .to_string(),
        ]
    }

    fn create_endpoint_from_url(&self, url: &url::Url) -> Option<Arc<dyn Driver>> {
        let host = url.host_str().unwrap();
        let port = url.port().unwrap();
        Some(Arc::new(
            UdpClient::builder(&format!("{host}:{port}")).build(),
        ))
    }
}
