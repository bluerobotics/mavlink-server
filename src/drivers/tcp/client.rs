use std::sync::Arc;

use crate::drivers::tcp::{tcp_receive_task, tcp_send_task};
use crate::protocol::Protocol;
use anyhow::Result;
use tokio::net::TcpStream;
use tokio::sync::{broadcast, Mutex};
use tracing::*;

use crate::drivers::{Driver, DriverInfo};

pub struct TcpClient {
    pub remote_addr: String,
}

impl TcpClient {
    #[instrument(level = "debug")]
    pub fn new(remote_addr: &str) -> Self {
        Self {
            remote_addr: remote_addr.to_string(),
        }
    }
}

#[async_trait::async_trait]
impl Driver for TcpClient {
    #[instrument(level = "debug", skip(self, hub_sender))]
    async fn run(&self, hub_sender: broadcast::Sender<Protocol>) -> Result<()> {
        let server_addr = &self.remote_addr;
        let hub_sender = Arc::new(hub_sender);

        loop {
            debug!("Trying to connect to {server_addr:?}...");
            let socket = match TcpStream::connect(server_addr).await {
                Ok(socket) => Arc::new(Mutex::new(socket)),
                Err(error) => {
                    error!("Failed connecting to {server_addr:?}: {error:?}");
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                    continue;
                }
            };
            debug!("TcpClient successfully connected to {server_addr:?}");

            let hub_receiver = hub_sender.subscribe();
            let hub_sender_cloned = Arc::clone(&hub_sender);

            tokio::select! {
                result = tcp_receive_task(socket.clone(), server_addr, hub_sender_cloned) => {
                    if let Err(e) = result {
                        error!("Error in TCP receive task: {e:?}");
                    }
                }
                result = tcp_send_task(socket, server_addr, hub_receiver) => {
                    if let Err(e) = result {
                        error!("Error in TCP send task: {e:?}");
                    }
                }
            }

            debug!("Restarting TCP Client connection loop...");
        }
    }

    #[instrument(level = "debug", skip(self))]
    fn info(&self) -> DriverInfo {
        DriverInfo {
            name: "TcpClient".to_string(),
        }
    }
}
