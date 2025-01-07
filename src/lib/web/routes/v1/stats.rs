use std::net::SocketAddr;

use axum::{
    extract::{
        ws::{self, WebSocket},
        ConnectInfo, WebSocketUpgrade,
    },
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tracing::*;
use uuid::Uuid;

use crate::stats;

#[derive(Serialize, Deserialize, Debug)]
pub struct Frequency {
    pub frequency: f32,
}

pub fn router() -> Router {
    Router::new()
        .route("/drivers", get(drivers_stats))
        .route("/frequency", get(stats_frequency).post(set_stats_frequency))
        .route("/hub", get(hub_stats))
        .route("/messages", get(hub_messages_stats))
        .route("/drivers/ws", get(drivers_stats_websocket_handler))
        .route("/hub/ws", get(hub_stats_websocket_handler))
        .route("/messages/ws", get(hub_messages_stats_websocket_handler))
}

async fn drivers_stats() -> impl IntoResponse {
    Json(stats::drivers_stats().await.unwrap())
}

async fn hub_stats() -> impl IntoResponse {
    Json(stats::hub_stats().await.unwrap())
}

async fn hub_messages_stats() -> impl IntoResponse {
    Json(stats::hub_messages_stats().await.unwrap())
}

async fn stats_frequency() -> impl IntoResponse {
    let frequency = 1. / stats::period().await.unwrap().as_secs_f32();
    Json(Frequency { frequency })
}

async fn set_stats_frequency(Json(Frequency { frequency }): Json<Frequency>) -> impl IntoResponse {
    let period = tokio::time::Duration::from_secs_f32(1. / frequency.clamp(0.1, 10.));

    let frequency = 1. / stats::set_period(period).await.unwrap().as_secs_f32();
    Json(Frequency { frequency })
}

#[instrument(level = "debug", skip_all)]
async fn hub_messages_stats_websocket_handler(
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Response {
    ws.on_upgrade(move |socket| hub_messages_stats_websocket_connection(socket, addr))
}

#[instrument(level = "debug", skip(socket))]
async fn hub_messages_stats_websocket_connection(socket: WebSocket, addr: SocketAddr) {
    let identifier = Uuid::new_v4();
    debug!("WS client, connected with ID: {identifier}");

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

                if let Err(error) = sender.send(ws::Message::Text(json.into())).await {
                    warn!("Failed to send message to WebSocket: {error:?}");
                    return;
                }
            }
        }
    });
    if let Err(error) = periodic_task.await {
        error!("Failed finishing task Hub Messages Stats WebSocket task: {error:?}");
    }

    debug!("WS client {identifier} ended");
}

#[instrument(level = "debug", skip_all)]
async fn hub_stats_websocket_handler(
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Response {
    ws.on_upgrade(move |socket| hub_stats_websocket_connection(socket, addr))
}

#[instrument(level = "debug", skip(socket))]
async fn hub_stats_websocket_connection(socket: WebSocket, addr: SocketAddr) {
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

                if let Err(error) = sender.send(ws::Message::Text(json.into())).await {
                    warn!("Failed to send message to WebSocket: {error:?}");
                    return;
                }
            }
        }
    });
    if let Err(error) = periodic_task.await {
        error!("Failed finishing task Hub Stats WebSocket task: {error:?}");
    }

    debug!("WS client {identifier} ended");
}

#[instrument(level = "debug", skip_all)]
async fn drivers_stats_websocket_handler(
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Response {
    ws.on_upgrade(move |socket| drivers_stats_websocket_connection(socket, addr))
}

#[instrument(level = "debug", skip(socket))]
async fn drivers_stats_websocket_connection(socket: WebSocket, addr: SocketAddr) {
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

                if let Err(error) = sender.send(ws::Message::Text(json.into())).await {
                    warn!("Failed to send message to WebSocket: {error:?}");
                    return;
                }
            }
        }
    });
    if let Err(error) = periodic_task.await {
        error!("Failed finishing task Drivers Stats WebSocket task: {error:?}");
    }

    debug!("WS client {identifier} ended");
}
