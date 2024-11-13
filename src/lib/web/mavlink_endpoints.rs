use std::net::SocketAddr;

use axum::{
    extract::{
        connect_info::ConnectInfo,
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, State,
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};

use futures::{sink::SinkExt, stream::StreamExt};
use tokio::sync::mpsc;
use tracing::*;
use uuid::Uuid;

use crate::web::{broadcast_message_websockets, AppState};

pub async fn mavlink(path: Option<Path<String>>) -> impl IntoResponse {
    let path = match path {
        Some(path) => path.0.to_string(),
        None => String::default(),
    };
    crate::drivers::rest::data::messages(&path)
}

pub async fn post_mavlink(
    ConnectInfo(address): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    message: String,
) {
    debug!("Got message from: {address:?}, {message}");
    if let Err(error) = state.message_tx.send(message) {
        error!("Failed to send message to main loop: {error:?}");
    }
}

pub async fn message_id_from_name(name: Path<String>) -> impl IntoResponse {
    use mavlink::{self, Message};
    mavlink::ardupilotmega::MavMessage::message_id_from_name(&name.0.to_ascii_uppercase())
        .map(|id| (StatusCode::OK, Json(id)).into_response())
        .unwrap_or_else(|_| (StatusCode::NOT_FOUND, "404 Not Found").into_response())
}

pub async fn websocket_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.on_upgrade(|socket| websocket_connection(socket, state))
}

#[instrument(level = "debug", skip_all)]
async fn websocket_connection(socket: WebSocket, state: AppState) {
    let identifier = Uuid::new_v4();
    debug!("WS client connected with ID: {identifier}");

    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
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
            Message::Text(text) => {
                trace!("WS client received from {identifier}: {text}");
                if let Err(error) = state.message_tx.send(text.clone()) {
                    error!("Failed to send message to main loop: {error:?}");
                }
                broadcast_message_websockets(&state, identifier, Message::Text(text)).await;
            }
            Message::Close(frame) => {
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
