mod endpoints;

use std::net::SocketAddr;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    http::StatusCode,
    response::Response,
    routing::get,
    Router,
};
use futures::{sink::SinkExt, stream::StreamExt};
use tokio::{
    signal,
    sync::{broadcast, mpsc, RwLock},
};
use tracing::*;

fn default_router() -> Router {
    Router::new()
        .route("/", get(endpoints::root))
        .route("/:path", get(endpoints::root))
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
        .fallback(get(|| async { (StatusCode::NOT_FOUND, "Not found :(") }))
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

pub async fn run(address: std::net::SocketAddrV4) {
    let router = default_router();

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
