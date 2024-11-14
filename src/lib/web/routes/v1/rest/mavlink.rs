use std::net::SocketAddr;

use axum::{
    extract::{ConnectInfo, Path},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use tracing::*;

pub(crate) async fn mavlink(path: Option<Path<String>>) -> impl IntoResponse {
    let path = match path {
        Some(path) => path.0.to_string(),
        None => String::default(),
    };
    crate::drivers::rest::data::messages(&path)
}

pub(crate) async fn post_mavlink(
    ConnectInfo(address): ConnectInfo<SocketAddr>,
    message: String,
) -> impl IntoResponse {
    use crate::web::routes::v1::rest::websocket;

    debug!("Got message from: {address:?}, {message}");
    if let Err(error) = websocket::send(message) {
        error!("Failed to send message to main loop: {error:?}");
    }
}

pub(crate) async fn message_id_from_name(name: Path<String>) -> impl IntoResponse {
    use mavlink::{self, Message};
    mavlink::ardupilotmega::MavMessage::message_id_from_name(&name.0.to_ascii_uppercase())
        .map(|id| (StatusCode::OK, Json(id)).into_response())
        .unwrap_or_else(|_| (StatusCode::NOT_FOUND, "404 Not Found").into_response())
}
