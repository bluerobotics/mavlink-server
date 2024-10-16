use std::sync::Arc;

use anyhow::Result;
use futures::{Sink, SinkExt, Stream, StreamExt};
use mavlink_codec::{error::DecoderError, Packet};
use tokio::sync::{broadcast, RwLock};
use tracing::*;

use crate::{
    callbacks::Callbacks, protocol::Protocol, stats::accumulated::driver::AccumulatedDriverStats,
};

#[instrument(
    level = "debug",
    skip(
        writer,
        reader,
        hub_sender,
        hub_receiver,
        on_message_input,
        on_message_output,
    )
)]
pub async fn default_send_receive_run<S, T>(
    mut writer: S,
    mut reader: T,
    identifier: &str,
    hub_sender: &broadcast::Sender<Arc<Protocol>>,
    hub_receiver: &mut broadcast::Receiver<Arc<Protocol>>,
    on_message_output: &Callbacks<Arc<Protocol>>,
    on_message_input: &Callbacks<Arc<Protocol>>,
    stats: Arc<RwLock<AccumulatedDriverStats>>,
) -> Result<()>
where
    S: Sink<Packet, Error = std::io::Error> + std::marker::Unpin,
    T: Stream<Item = std::io::Result<std::result::Result<Packet, DecoderError>>>
        + std::marker::Unpin,
{
    tokio::select! {
        result = default_send_task(&mut writer, identifier, hub_receiver, on_message_output, &stats) => {
            if let Err(error) = result {
                error!("Error in send task for {identifier}: {error:?}");
            }
        }
        result = default_receive_task(&mut reader, identifier, hub_sender, on_message_input, &stats) => {
            if let Err(error) = result {
                error!("Error in receive task for {identifier}: {error:?}");
            }
        }
    }

    Ok(())
}

/// Receives messages from a Stream and sends them to the HUB Channel
#[instrument(level = "debug", skip(reader, hub_sender, on_message_input))]
pub async fn default_receive_task<T>(
    reader: &mut T,
    identifier: &str,
    hub_sender: &broadcast::Sender<Arc<Protocol>>,
    on_message_input: &Callbacks<Arc<Protocol>>,
    stats: &Arc<RwLock<AccumulatedDriverStats>>,
) -> Result<()>
where
    T: Stream<Item = std::io::Result<std::result::Result<Packet, DecoderError>>>
        + std::marker::Unpin,
{
    loop {
        let packet = match reader.next().await {
            Some(Ok(Ok(packet))) => packet,
            Some(Ok(Err(decode_error))) => {
                error!("Failed to decode packet: {decode_error:?}");
                continue;
            }
            Some(Err(io_error)) => {
                error!("Critical error trying to decode data from: {io_error:?}");
                break;
            }
            None => break,
        };

        let message = Arc::new(Protocol::new(identifier, packet));

        trace!("Received message: {message:?}");

        stats.write().await.stats.update_input(&message);

        for future in on_message_input.call_all(message.clone()) {
            if let Err(error) = future.await {
                debug!("Dropping message: on_message_input callback returned error: {error:?}");
                continue;
            }
        }

        if let Err(send_error) = hub_sender.send(message) {
            error!("Failed to send message to hub: {send_error:?}");
            continue;
        }

        trace!("Message sent to hub");
    }

    debug!("Driver receiver task stopped!");

    Ok(())
}

/// Receives messages from the HUB Channel and sends them to a Sink
#[instrument(level = "debug", skip(writer, hub_receiver, on_message_output))]
pub async fn default_send_task<S>(
    writer: &mut S,
    identifier: &str,
    hub_receiver: &mut broadcast::Receiver<Arc<Protocol>>,
    on_message_output: &Callbacks<Arc<Protocol>>,
    stats: &Arc<RwLock<AccumulatedDriverStats>>,
) -> Result<()>
where
    S: Sink<Packet, Error = std::io::Error> + std::marker::Unpin,
{
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

        if message.origin.eq(&identifier) {
            continue; // Don't do loopback
        }

        stats.write().await.stats.update_output(&message);

        for future in on_message_output.call_all(message.clone()) {
            if let Err(error) = future.await {
                debug!("Dropping message: on_message_output callback returned error: {error:?}");
                continue;
            }
        }

        if let Err(error) = writer.send((**message).clone()).await {
            error!("Failed to send message: {error:?}");
            break;
        }

        trace!("Message sent to {identifier}: {:?}", message.as_slice());
    }
    Ok(())
}
