use std::sync::Arc;

use crate::drivers::tcp::{tcp_receive_task, tcp_send_task};
use crate::protocol::Protocol;
use anyhow::Result;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, Mutex};
use tracing::*;

use crate::drivers::{Driver, DriverInfo};

pub struct TcpServer {
    pub local_addr: String,
}

impl TcpServer {
    #[instrument(level = "debug")]
    pub fn new(local_addr: &str) -> Self {
        Self {
            local_addr: local_addr.to_string(),
        }
    }

    /// Handles communication with a single client
    #[instrument(level = "debug", skip(socket, hub_sender))]
    async fn handle_client(
        socket: Arc<Mutex<TcpStream>>,
        remote_addr: String,
        hub_sender: Arc<broadcast::Sender<Protocol>>,
    ) -> Result<()> {
        let hub_receiver = hub_sender.subscribe();

        tokio::select! {
            result = tcp_receive_task(socket.clone(), &remote_addr, hub_sender) => {
                if let Err(e) = result {
                    error!("Error in TCP receive task for {remote_addr}: {e:?}");
                }
            }
            result = tcp_send_task(socket, &remote_addr, hub_receiver) => {
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
    async fn run(&self, hub_sender: broadcast::Sender<Protocol>) -> Result<()> {
        let listener = TcpListener::bind(&self.local_addr).await?;
        let hub_sender = Arc::new(hub_sender);

        loop {
            match listener.accept().await {
                Ok((socket, remote_addr)) => {
                    let remote_addr = remote_addr.to_string();
                    let hub_sender_cloned = Arc::clone(&hub_sender);
                    let socket = Arc::new(Mutex::new(socket));

                    tokio::spawn(TcpServer::handle_client(
                        socket,
                        remote_addr,
                        hub_sender_cloned,
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
    fn info(&self) -> DriverInfo {
        DriverInfo {
            name: "TcpServer".to_string(),
        }
    }
}
