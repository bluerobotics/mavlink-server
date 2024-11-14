use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use axum::{
    extract::{
        ws::{self, WebSocket},
        ConnectInfo, WebSocketUpgrade,
    },
    response::Response,
};
use futures::{SinkExt, StreamExt};
use lazy_static::lazy_static;
use tokio::sync::{broadcast, mpsc, RwLock};
use tracing::*;
use uuid::Uuid;

lazy_static! {
    static ref SERVER: Arc<SingletonServer> = {
        let (message_tx, _message_rx) = broadcast::channel(100);
        let clients = Arc::new(RwLock::new(HashMap::new()));
        let state = AppState {
            clients,
            message_tx,
        };
        Arc::new(SingletonServer { state })
    };
}

struct SingletonServer {
    state: AppState,
}

#[derive(Clone, Debug)]
struct AppState {
    clients: Arc<RwLock<HashMap<Uuid, ClientSender>>>,
    message_tx: broadcast::Sender<String>,
}

type ClientSender = mpsc::UnboundedSender<ws::Message>;

#[instrument(level = "debug", skip_all)]
pub(crate) async fn websocket_handler(
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Response {
    ws.on_upgrade(move |socket| websocket_connection(socket, addr))
}

#[instrument(level = "debug", skip(socket))]
async fn websocket_connection(socket: WebSocket, addr: SocketAddr) {
    let identifier = Uuid::new_v4();
    debug!("WS client connected with ID: {identifier}");

    let state = &SERVER.state;

    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<ws::Message>();
    state.clients.write().await.insert(identifier, tx);

    // Spawn a task to forward messages from the channel to the websocket
    let send_task = tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            if sender.send(message).await.is_err() {
                break;
            }
        }
    });

    // Handle incoming messages
    while let Some(Ok(message)) = receiver.next().await {
        match message {
            ws::Message::Text(text) => {
                trace!("WS client received from {identifier}: {text}");
                if let Err(error) = state.message_tx.send(text.clone()) {
                    error!("Failed to send message to main loop: {error:?}");
                }
                broadcast(identifier, ws::Message::Text(text)).await;
            }
            ws::Message::Close(frame) => {
                debug!("WS client {identifier} disconnected: {frame:?}");
                break;
            }
            _ => {}
        }
    }

    // We should be disconnected now, let's remove it
    state.clients.write().await.remove(&identifier);
    debug!("WS client {identifier} removed");
    send_task.await.unwrap();
}

pub(crate) async fn broadcast(sender_identifier: Uuid, message: ws::Message) {
    let state = &SERVER.state;

    let clients = state.clients.read().await;

    for (&client_identifier, tx) in clients.iter() {
        if client_identifier != sender_identifier {
            if let Err(error) = tx.send(message.clone()) {
                error!("Failed to send message to client {client_identifier}: {error:?}",);
            }
        }
    }
}

pub(crate) fn create_message_receiver() -> broadcast::Receiver<String> {
    SERVER.state.message_tx.subscribe()
}

pub(crate) fn send(message: String) -> Result<usize, broadcast::error::SendError<String>> {
    SERVER.state.message_tx.send(message)
}
