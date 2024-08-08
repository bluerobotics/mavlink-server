use std::marker::Send;
use std::sync::{Arc, RwLock};

use mavlink::{common::MavMessage, MavConnection, MavHeader};
use tokio::sync::broadcast;
use tracing::*;

pub struct Connection {
    inner: Arc<RwLock<ConnectionInner>>,
}

struct ConnectionInner {
    address: String,
    connection: Option<Box<dyn MavConnection<MavMessage> + Sync + Send>>,
    sender: broadcast::Sender<Message>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Received((MavHeader, MavMessage)),
    ToBeSent((MavHeader, MavMessage)),
}

impl Connection {
    #[instrument(level = "debug")]
    pub fn new(address: &str) -> Self {
        let (sender, _receiver) = broadcast::channel(100);

        let this = Self {
            inner: Arc::new(RwLock::new(ConnectionInner {
                address: address.to_string(),
                connection: None,
                sender,
            })),
        };

        let connection = this.inner.clone();
        std::thread::Builder::new()
            .name("MavSender".into())
            .spawn(move || Connection::sender_loop(connection))
            .expect("Failed to spawn MavSender thread");

        let connection = this.inner.clone();
        std::thread::Builder::new()
            .name("MavReceiver".into())
            .spawn(move || Connection::receiver_loop(connection))
            .expect("Failed to spawn MavReceiver thread");

        this
    }

    #[instrument(level = "debug", skip(inner))]
    fn receiver_loop(inner: Arc<RwLock<ConnectionInner>>) {
        loop {
            loop {
                let Ok(inner_guard) = inner.read() else {
                    break; // Break to trigger reconnection
                };

                let Some(mavlink) = inner_guard.connection.as_deref() else {
                    break; // Break to trigger reconnection
                };

                // Receive from the Mavlink network
                let (header, message) = match mavlink.recv() {
                    Ok(message) => message,
                    Err(error) => {
                        trace!("Failed receiving from mavlink: {error:?}");

                        match &error {
                            mavlink::error::MessageReadError::Parse(_) => continue,
                            mavlink::error::MessageReadError::Io(io_error) => {
                                // The mavlink connection is handled by the sender_loop, so we can just silently skip the WouldBlocks
                                if io_error.kind() == std::io::ErrorKind::WouldBlock {
                                    continue;
                                }
                            }
                        }

                        error!("Failed receiving message from Mavlink Connection: {error:?}");
                        break; // Break to trigger reconnection
                    }
                };

                debug!("Message accepted: {header:?}, {message:?}");

                // Send the received message to the other side
                if let Err(error) = inner_guard
                    .sender
                    .send(Message::Received((header, message)))
                {
                    error!("Failed handling message: {error:?}");
                    continue;
                }
            }

            // Reconnects
            {
                let mut inner = inner.write().unwrap();
                let address = inner.address.clone();
                inner.connection.replace(ConnectionInner::connect(&address));
            }

            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }

    #[instrument(level = "debug", skip(inner))]
    fn sender_loop(inner: Arc<RwLock<ConnectionInner>>) {
        let mut receiver = { inner.read().unwrap().sender.subscribe() };

        loop {
            loop {
                // Receive answer from the other side
                let (header, message) = match receiver.blocking_recv() {
                    Ok(Message::ToBeSent(message)) => message,
                    Ok(Message::Received(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => {
                        unreachable!(
                            "Closed channel: This should never happen, this channel is static!"
                        );
                    }
                    Err(broadcast::error::RecvError::Lagged(samples)) => {
                        warn!("Channel is lagged behind by {samples} messages. Expect degraded performance on the mavlink responsiviness.");
                        continue;
                    }
                };

                let Ok(inner_guard) = inner.read() else {
                    break; // Break to trigger reconnection
                };
                let Some(mavlink) = inner_guard.connection.as_deref() else {
                    break; // Break to trigger reconnection
                };

                // Send the response from the other side to the Mavlink network
                if let Err(error) = mavlink.send(&header, &message) {
                    error!("Failed sending message to Mavlink Connection: {error:?}");

                    break; // Break to trigger reconnection
                }

                debug!("Message sent: {header:?}, {message:?}");
            }

            // Reconnects
            {
                let mut inner = inner.write().unwrap();
                let address = inner.address.clone();
                inner.connection.replace(ConnectionInner::connect(&address));
            }

            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }
}

impl ConnectionInner {
    #[instrument(level = "debug")]
    fn connect(address: &str) -> Box<dyn MavConnection<MavMessage> + Sync + Send> {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));

            debug!("Connecting...");

            match mavlink::connect(address) {
                Ok(connection) => {
                    info!("Successfully connected");
                    return connection;
                }
                Err(error) => {
                    error!("Failed to connect, trying again in one second. Reason: {error:?}.");
                }
            }
        }
    }
}
