mod endpoints;

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    http::StatusCode,
    response::Response,
    routing::{get, post},
    Router,
};
use futures::{sink::SinkExt, stream::StreamExt};
use tokio::{
    signal,
    sync::{broadcast, mpsc, RwLock},
};
use tracing::*;
use uuid::Uuid;

use lazy_static::lazy_static;

use crate::stats;

fn default_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(endpoints::root))
        .route("/:path", get(endpoints::root))
        .route("/log", get(log_websocket_handler))
        .route("/info", get(endpoints::info))
        .route("/info_full", get(endpoints::info_full))
        .route("/stats/drivers", get(endpoints::drivers_stats))
        .route(
            "/stats/frequency",
            get(endpoints::stats_frequency).post(endpoints::set_stats_frequency),
        )
        .route("/stats/hub", get(endpoints::hub_stats))
        .route("/stats/messages", get(endpoints::hub_messages_stats))
        .route("/stats/drivers/ws", get(drivers_stats_websocket_handler))
        .route("/stats/hub/ws", get(hub_stats_websocket_handler))
        .route(
            "/stats/messages/ws",
            get(hub_messages_stats_websocket_handler),
        )
        .route("/rest/ws", get(websocket_handler))
        // We are matching all possible keys for the user
        .route("/rest/mavlink", get(endpoints::mavlink))
        .route("/rest/mavlink", post(endpoints::post_mavlink))
        .route("/rest/mavlink/", get(endpoints::mavlink))
        .route("/rest/mavlink/*path", get(endpoints::mavlink))
        .route(
            "/rest/mavlink/message_id_from_name/*name",
            get(endpoints::message_id_from_name),
        )
        .fallback(get(|| async { (StatusCode::NOT_FOUND, "Not found :(") }))
        .with_state(state)
}

async fn websocket_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
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

async fn log_websocket_handler(ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(log_websocket_connection)
}

#[instrument(level = "debug", skip_all)]
async fn log_websocket_connection(socket: WebSocket) {
    let (mut websocket_sender, mut _websocket_receiver) = socket.split();
    let (mut receiver, history) = crate::logger::HISTORY.lock().unwrap().subscribe();

    for message in history {
        if websocket_sender.send(Message::Text(message)).await.is_err() {
            return;
        }
    }

    while let Ok(message) = receiver.recv().await {
        if websocket_sender.send(Message::Text(message)).await.is_err() {
            break;
        }
    }
}

#[instrument(level = "debug", skip_all)]
async fn hub_messages_stats_websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> Response {
    ws.on_upgrade(|socket| hub_messages_stats_websocket_connection(socket, state))
}

#[instrument(level = "debug", skip_all)]
async fn hub_messages_stats_websocket_connection(socket: WebSocket, state: AppState) {
    let identifier = Uuid::new_v4();
    debug!("WS client connected with ID: {identifier}");

    let (mut sender, _receiver) = socket.split();

    let periodic_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
        let mut first = true;
        loop {
            if first {
                first = false;
            } else {
                interval.tick().await;
            }

            let mut hub_messages_stats_stream = match stats::hub_messages_stats_stream().await {
                Ok(hub_messages_stats_stream) => hub_messages_stats_stream,
                Err(error) => {
                    warn!("Failed getting Hub Messages Stats Stream: {error:?}");
                    continue;
                }
            };

            while let Some(hub_message_stats) = hub_messages_stats_stream.recv().await {
                let json = match serde_json::to_string(&hub_message_stats) {
                    Ok(json) => json,
                    Err(error) => {
                        warn!("Failed to create json from Hub Messages Stats: {error:?}");
                        continue;
                    }
                };

                if let Err(error) = sender.send(Message::Text(json)).await {
                    warn!("Failed to send message to WebSocket: {error:?}");
                    return;
                }
            }
        }
    });
    if let Err(error) = periodic_task.await {
        error!("Failed finishing task Hub Messages Stats WebSocket task: {error:?}");
    }

    // Clean up when the connection is closed
    state.clients.write().await.remove(&identifier);
    debug!("WS client {identifier} removed");
}

#[instrument(level = "debug", skip_all)]
async fn hub_stats_websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> Response {
    ws.on_upgrade(|socket| hub_stats_websocket_connection(socket, state))
}

#[instrument(level = "debug", skip_all)]
async fn hub_stats_websocket_connection(socket: WebSocket, state: AppState) {
    let identifier = Uuid::new_v4();
    debug!("WS client connected with ID: {identifier}");

    let (mut sender, _receiver) = socket.split();

    let periodic_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
        let mut first = true;
        loop {
            if first {
                first = false;
            } else {
                interval.tick().await;
            }

            let mut hub_stats_stream = match stats::hub_stats_stream().await {
                Ok(hub_stats_stream) => hub_stats_stream,
                Err(error) => {
                    warn!("Failed getting Hub Stats Stream: {error:?}");
                    continue;
                }
            };

            while let Some(hub_message_stats) = hub_stats_stream.recv().await {
                let json = match serde_json::to_string(&hub_message_stats) {
                    Ok(json) => json,
                    Err(error) => {
                        warn!("Failed to create json from Hub Stats: {error:?}");
                        continue;
                    }
                };

                if let Err(error) = sender.send(Message::Text(json)).await {
                    warn!("Failed to send message to WebSocket: {error:?}");
                    return;
                }
            }
        }
    });
    if let Err(error) = periodic_task.await {
        error!("Failed finishing task Hub Stats WebSocket task: {error:?}");
    }

    // Clean up when the connection is closed
    state.clients.write().await.remove(&identifier);
    debug!("WS client {identifier} removed");
}

#[instrument(level = "debug", skip_all)]
async fn drivers_stats_websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> Response {
    ws.on_upgrade(|socket| drivers_stats_websocket_connection(socket, state))
}

#[instrument(level = "debug", skip_all)]
async fn drivers_stats_websocket_connection(socket: WebSocket, state: AppState) {
    let identifier = Uuid::new_v4();
    debug!("WS client connected with ID: {identifier}");

    let (mut sender, _receiver) = socket.split();

    let periodic_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
        let mut first = true;
        loop {
            if first {
                first = false;
            } else {
                interval.tick().await;
            }

            let mut drivers_stats_stream = match stats::drivers_stats_stream().await {
                Ok(drivers_stats_stream) => drivers_stats_stream,
                Err(error) => {
                    warn!("Failed getting Drivers Stats Stream: {error:?}");
                    continue;
                }
            };

            while let Some(hub_message_stats) = drivers_stats_stream.recv().await {
                let json = match serde_json::to_string(&hub_message_stats) {
                    Ok(json) => json,
                    Err(error) => {
                        warn!("Failed to create json from Drivers Stats: {error:?}");
                        continue;
                    }
                };

                if let Err(error) = sender.send(Message::Text(json)).await {
                    warn!("Failed to send message to WebSocket: {error:?}");
                    return;
                }
            }
        }
    });
    if let Err(error) = periodic_task.await {
        error!("Failed finishing task Drivers Stats WebSocket task: {error:?}");
    }

    // Clean up when the connection is closed
    state.clients.write().await.remove(&identifier);
    debug!("WS client {identifier} removed");
}

async fn broadcast_message_websockets(state: &AppState, sender_identifier: Uuid, message: Message) {
    let mut clients = state.clients.write().await;

    for (&client_identifier, tx) in clients.iter_mut() {
        if client_identifier != sender_identifier {
            if let Err(error) = tx.send(message.clone()) {
                error!(
                    "Failed to send message to client {}: {:?}",
                    client_identifier, error
                );
            }
        }
    }
}

pub async fn send_message_to_all_clients(message: Message) {
    let state = SERVER.state.clone();
    let clients = state.clients.read().await;
    for (&client_identifier, tx) in clients.iter() {
        if let Err(error) = tx.send(message.clone()) {
            error!(
                "Failed to send message to client {}: {:?}",
                client_identifier, error
            );
        } else {
            debug!("Sent message to client {}", client_identifier);
        }
    }
}

pub async fn send_message(message: String) {
    let state = SERVER.state.clone();
    broadcast_message_websockets(
        &state,
        Uuid::parse_str("00000000-0000-4000-0000-000000000000").unwrap(),
        Message::Text(message),
    )
    .await;
}

pub fn create_message_receiver() -> broadcast::Receiver<String> {
    SERVER.state.message_tx.subscribe()
}

lazy_static! {
    static ref SERVER: Arc<SingletonServer> = {
        let (message_tx, _message_rx) = broadcast::channel(100);
        let clients = Arc::new(RwLock::new(HashMap::new()));
        let state = AppState {
            clients,
            message_tx,
        };
        let router = Mutex::new(default_router(state.clone()));
        Arc::new(SingletonServer { router, state })
    };
}

struct SingletonServer {
    router: Mutex<Router>,
    state: AppState,
}

#[derive(Clone)]
struct AppState {
    clients: Arc<RwLock<HashMap<Uuid, ClientSender>>>,
    message_tx: broadcast::Sender<String>,
}

type ClientSender = mpsc::UnboundedSender<Message>;

pub async fn run(address: std::net::SocketAddrV4) {
    let router = SERVER.router.lock().unwrap().clone();

    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
    let mut first = true;
    loop {
        if first {
            first = false;
        } else {
            interval.tick().await;
        }

        let listener = match tokio::net::TcpListener::bind(&address).await {
            Ok(listener) => listener,
            Err(error) => {
                error!("WebServer TCP bind error: {error}");
                continue;
            }
        };

        info!("Running web server on address {address:?}");

        if let Err(error) = axum::serve(
            listener,
            router
                .clone()
                .into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(shutdown_signal())
        .await
        {
            error!("WebServer error: {error}");
            continue;
        }

        break;
    }
}

pub fn configure_router<F>(modifier: F)
where
    F: FnOnce(&mut Router),
{
    let mut router = SERVER.router.lock().unwrap();
    modifier(&mut router);
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
