use std::sync::Arc;

use anyhow::Result;
use mavlink::ardupilotmega::MavMessage;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::{broadcast, Mutex},
};
use tracing::*;

use crate::protocol::Protocol;

pub mod client;
pub mod server;

/// Receives messages from the TCP Socket and sends them to the HUB Channel
#[instrument(level = "debug", skip(socket, hub_sender))]
async fn tcp_receive_task(
    socket: Arc<Mutex<TcpStream>>,
    remote_addr: &str,
    hub_sender: Arc<broadcast::Sender<Protocol>>,
) -> Result<()> {
    let mut buf = Vec::with_capacity(1024);

    loop {
        buf.clear();

        let bytes_received = socket.lock().await.read_buf(&mut buf).await?;

        if bytes_received == 0 {
            warn!("TCP connection closed by {remote_addr}.");
            break;
        }

        let message = match mavlink::read_v2_raw_message_async::<MavMessage, _>(
            &mut (&buf[..bytes_received]),
        )
        .await
        {
            Ok(message) => message,
            Err(error) => {
                error!("Failed to parse MAVLink message: {error:?}");
                continue; // Skip this iteration on error
            }
        };

        let message = Protocol::new(&remote_addr, message);

        trace!("Received TCP message: {message:?}");
        if let Err(error) = hub_sender.send(message) {
            error!("Failed to send message to hub: {error:?}");
        }
    }

    debug!("TCP Receive task for {remote_addr} finished");
    Ok(())
}

/// Receives messages from the HUB Channel and sends them to the TCP Socket
#[instrument(level = "debug", skip(socket, hub_receiver))]
async fn tcp_send_task(
    socket: Arc<Mutex<TcpStream>>,
    remote_addr: &str,
    mut hub_receiver: broadcast::Receiver<Protocol>,
) -> Result<()> {
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

        if message.origin.eq(&remote_addr) {
            continue; // Don't do loopback
        }

        socket.lock().await.write_all(message.raw_bytes()).await?;

        trace!("Message sent to {remote_addr} from TCP server: {message:?}");
    }

    debug!("TCP Send task for {remote_addr} finished");
    Ok(())
}
