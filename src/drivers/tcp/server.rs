use std::sync::Arc;

use anyhow::Result;
use mavlink_server::callbacks::{Callbacks, MessageCallback};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{broadcast, RwLock},
};
use tracing::*;

use crate::{
    drivers::{
        tcp::{tcp_receive_task, tcp_send_task},
        Driver, DriverInfo,
    },
    protocol::Protocol,
    stats::driver::{AccumulatedDriverStats, AccumulatedDriverStatsProvider},
};

#[derive(Clone)]
pub struct TcpServer {
    pub local_addr: String,
    on_message_input: Callbacks<Arc<Protocol>>,
    on_message_output: Callbacks<Arc<Protocol>>,
    stats: Arc<RwLock<AccumulatedDriverStats>>,
}

pub struct TcpServerBuilder(TcpServer);

impl TcpServerBuilder {
    pub fn build(self) -> TcpServer {
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

impl TcpServer {
    #[instrument(level = "debug")]
    pub fn builder(local_addr: &str) -> TcpServerBuilder {
        TcpServerBuilder(Self {
            local_addr: local_addr.to_string(),
            on_message_input: Callbacks::new(),
            on_message_output: Callbacks::new(),
            stats: Arc::new(RwLock::new(AccumulatedDriverStats::default())),
        })
    }

    /// Handles communication with a single client
    #[instrument(
        level = "debug",
        skip(socket, hub_sender, on_message_input, on_message_output)
    )]
    async fn handle_client(
        socket: TcpStream,
        remote_addr: String,
        hub_sender: Arc<broadcast::Sender<Arc<Protocol>>>,
        on_message_input: Callbacks<Arc<Protocol>>,
        on_message_output: Callbacks<Arc<Protocol>>,
        stats: Arc<RwLock<AccumulatedDriverStats>>,
    ) -> Result<()> {
        let hub_receiver = hub_sender.subscribe();

        let (read, write) = socket.into_split();

        tokio::select! {
            result = tcp_receive_task(read, &remote_addr, hub_sender, &on_message_input, &stats) => {
                if let Err(e) = result {
                    error!("Error in TCP receive task for {remote_addr}: {e:?}");
                }
            }
            result = tcp_send_task(write, &remote_addr, hub_receiver, &on_message_output, &stats) => {
                if let Err(e) = result {
                    error!("Error in TCP send task for {remote_addr}: {e:?}");
                }
            }
        }

        debug!("Finished handling connection with {remote_addr}");
        Ok(())
    }
}

#[async_trait::async_trait]
impl Driver for TcpServer {
    #[instrument(level = "debug", skip(self, hub_sender))]
    async fn run(&self, hub_sender: broadcast::Sender<Arc<Protocol>>) -> Result<()> {
        let listener = TcpListener::bind(&self.local_addr).await?;
        let hub_sender = Arc::new(hub_sender);

        loop {
            match listener.accept().await {
                Ok((socket, remote_addr)) => {
                    let remote_addr = remote_addr.to_string();
                    let hub_sender = Arc::clone(&hub_sender);

                    tokio::spawn(TcpServer::handle_client(
                        socket,
                        remote_addr,
                        hub_sender,
                        self.on_message_input.clone(),
                        self.on_message_output.clone(),
                        self.stats.clone(),
                    ));
                }
                Err(error) => {
                    error!("Failed to accept TCP connection: {error:?}");
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                }
            }
        }
    }

    #[instrument(level = "debug", skip(self))]
    fn info(&self) -> Box<dyn DriverInfo> {
        return Box::new(TcpServerInfo);
    }
}

#[async_trait::async_trait]
impl AccumulatedDriverStatsProvider for TcpServer {
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

pub struct TcpServerInfo;
impl DriverInfo for TcpServerInfo {
    fn name(&self) -> &str {
        "TcpServer"
    }

    fn valid_schemes(&self) -> Vec<String> {
        vec!["tcps".to_string(), "tcpserver".to_string()]
    }

    fn cli_example_legacy(&self) -> Vec<String> {
        let first_schema = &self.valid_schemes()[0];
        let second_schema = &self.valid_schemes()[1];
        vec![
            format!("{first_schema}:<IP?>:<PORT>"),
            format!("{first_schema}:14550"),
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
            TcpServer::builder(&format!("{host}:{port}")).build(),
        ))
    }
}
