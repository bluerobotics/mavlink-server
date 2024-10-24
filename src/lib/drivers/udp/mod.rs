use std::net::SocketAddr;

use anyhow::Result;
use futures::{Sink, SinkExt};
use mavlink_codec::Packet;
use tokio::sync::broadcast;
use tracing::*;

use super::generic_tasks::SendReceiveContext;

pub mod client;
pub mod server;

/// Receives messages from the HUB Channel and sends them to a Sink
#[instrument(level = "debug", skip(writer, context))]
async fn udp_send_task<S>(
    writer: &mut S,
    remote_addr: &SocketAddr,
    context: &SendReceiveContext,
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

        if message.origin.eq(&remote_addr.to_string()) {
            continue; // Don't do loopback
        }

        context.stats.write().await.stats.update_output(&message);

        for future in context.on_message_output.call_all(message.clone()) {
            if let Err(error) = future.await {
                debug!(
                    client = ?remote_addr, "Dropping message: on_message_output callback returned error: {error:?}"
                );
                continue;
            }
        }

        if let Err(io_error) = writer.send(((**message).clone(), *remote_addr)).await {
            match io_error.kind() {
                std::io::ErrorKind::ConnectionRefused => {
                    trace!(client = ?remote_addr, "Failed send message: {io_error}");
                    continue;
                }
                _ => {
                    error!(client = ?remote_addr, "Failed to send message: {io_error:?}");
                }
            }
            break;
        }

        trace!("Message sent to {remote_addr}: {:?}", message.as_slice());
    }
    Ok(())
}
