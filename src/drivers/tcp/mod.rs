use std::sync::Arc;

use anyhow::Result;
use mavlink_server::callbacks::Callbacks;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::tcp::{OwnedReadHalf, OwnedWriteHalf},
    sync::{broadcast, RwLock},
};
use tracing::*;

use crate::{
    protocol::{read_all_messages, Protocol},
    stats::accumulated::driver::AccumulatedDriverStats,
};

pub mod client;
pub mod server;

/// Receives messages from the TCP Socket and sends them to the HUB Channel
#[instrument(level = "debug", skip(socket, hub_sender, on_message_input))]
async fn tcp_receive_task(
    mut socket: OwnedReadHalf,
    remote_addr: &str,
    hub_sender: Arc<broadcast::Sender<Arc<Protocol>>>,
    on_message_input: &Callbacks<Arc<Protocol>>,
    stats: &Arc<RwLock<AccumulatedDriverStats>>,
) -> Result<()> {
    let mut buf = Vec::with_capacity(1024);

    loop {
        let bytes_received = socket.read_buf(&mut buf).await?;
        if bytes_received == 0 {
            warn!("TCP connection closed by {remote_addr}.");
            break;
        }

        trace!("Received TCP packet: {buf:?}");

        read_all_messages(remote_addr, &mut buf, |message| async {
            let message = Arc::new(message);

            stats.write().await.update_input(&message).await;

            for future in on_message_input.call_all(message.clone()) {
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

    debug!("TCP Receive task for {remote_addr} finished");
    Ok(())
}

/// Receives messages from the HUB Channel and sends them to the TCP Socket
#[instrument(level = "debug", skip(socket, hub_receiver, on_message_output))]
async fn tcp_send_task(
    mut socket: OwnedWriteHalf,
    remote_addr: &str,
    mut hub_receiver: broadcast::Receiver<Arc<Protocol>>,
    on_message_output: &Callbacks<Arc<Protocol>>,
    stats: &Arc<RwLock<AccumulatedDriverStats>>,
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

        stats.write().await.update_output(&message).await;

        for future in on_message_output.call_all(message.clone()) {
            if let Err(error) = future.await {
                debug!("Dropping message: on_message_output callback returned error: {error:?}");
                continue;
            }
        }

        socket.write_all(message.raw_bytes()).await?;

        trace!(
            "Message sent to {remote_addr} from TCP server: {:?}",
            message.raw_bytes()
        );
    }

    debug!("TCP Send task for {remote_addr} finished");
    Ok(())
}
