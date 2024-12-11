use std::net::SocketAddr;

use axum::{
    extract::{ConnectInfo, Path, Query},
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use clap::error;
use serde::Deserialize;
use tracing::*;

use crate::{
    drivers::rest::parse_query,
    mavlink_json::{MAVLinkJSON, MAVLinkJSONHeader},
};

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

#[derive(Deserialize)]
pub struct MessageInfo {
    pub name: String,
}

pub(crate) async fn helper(name: Query<MessageInfo>) -> impl IntoResponse {
    let message_name = name.0.name.to_ascii_uppercase();

    let result = <mavlink::ardupilotmega::MavMessage as mavlink::Message>::message_id_from_name(
        &message_name,
    )
    .and_then(|id| {
        <mavlink::ardupilotmega::MavMessage as mavlink::Message>::default_message_from_id(id)
    });

    match result {
        Ok(message) => {
            let header = MAVLinkJSONHeader {
                inner: Default::default(),
                message_id: Some(mavlink::Message::message_id(&message)),
            };
            let json = serde_json::to_string_pretty(&MAVLinkJSON { header, message }).unwrap();
            ([(header::CONTENT_TYPE, "application/json")], json).into_response()
        }
        Err(error) => {
            let error_json = serde_json::to_string_pretty(&error).unwrap();
            (
                StatusCode::NOT_FOUND,
                [(header::CONTENT_TYPE, "application/json")],
                error_json,
            )
                .into_response()
        }
    }
}

pub(crate) async fn message_id_from_name(name: Path<String>) -> impl IntoResponse {
    use mavlink::{self, Message};
    mavlink::ardupilotmega::MavMessage::message_id_from_name(&name.0.to_ascii_uppercase())
        .map(|id| (StatusCode::OK, Json(id)).into_response())
        .unwrap_or_else(|_| (StatusCode::NOT_FOUND, "404 Not Found").into_response())
}
