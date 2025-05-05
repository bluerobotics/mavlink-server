use std::net::SocketAddr;

use crate::drivers::rest::{autopilot, control};
use axum::{
    extract::{
        ConnectInfo, Json, Query, WebSocketUpgrade,
        ws::{self, WebSocket},
    },
    http::header,
    response::IntoResponse,
    response::Response,
};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use tracing::*;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct ArmPayload {
    vehicle_id: Option<u8>,
    component_id: Option<u8>,
    force: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
pub struct ThisVehiclePayload {
    vehicle_id: Option<u8>,
    component_id: Option<u8>,
}

#[derive(Debug, Default, Deserialize)]
pub struct SetParameterPayload {
    vehicle_id: Option<u8>,
    component_id: Option<u8>,
    parameter_name: String,
    value: f64,
}

pub(crate) async fn parameters() -> impl IntoResponse {
    let parameters = control::parameters().await;
    let json = serde_json::to_string_pretty(&parameters).unwrap();
    ([(header::CONTENT_TYPE, "application/json")], json).into_response()
}

pub(crate) async fn available_parameters() -> impl IntoResponse {
    let parameters = autopilot::parameters::parameters();
    let json = serde_json::to_string_pretty(&parameters).unwrap();
    ([(header::CONTENT_TYPE, "application/json")], json).into_response()
}

pub(crate) async fn vehicles() -> impl IntoResponse {
    let vehicles = control::vehicles().await;
    let json = serde_json::to_string_pretty(&vehicles).unwrap();
    ([(header::CONTENT_TYPE, "application/json")], json).into_response()
}

pub(crate) async fn arm(Json(payload): Json<ArmPayload>) -> impl IntoResponse {
    debug!("Arming vehicle: {:#?}", &payload);
    let _ = control::arm(payload.vehicle_id, payload.component_id, payload.force);
}

pub(crate) async fn disarm(Json(payload): Json<ArmPayload>) -> impl IntoResponse {
    debug!("Disarming vehicle: {:#?}", &payload);
    let _ = control::disarm(payload.vehicle_id, payload.component_id, payload.force);
}

//TODO: Needs to deal with optional arguments for system and component id
pub(crate) async fn version(Query(payload): Query<ThisVehiclePayload>) -> impl IntoResponse {
    let version = control::version(payload.vehicle_id, payload.component_id).await;
    dbg!(&version);
    let answer = match version {
        Ok(version) => serde_json::to_string_pretty(&version).unwrap(),
        Err(err) => {
            format!("{{\"error\": \"{}\"}}", err)
        }
    };
    ([(header::CONTENT_TYPE, "application/json")], answer).into_response()
}

pub(crate) async fn set_parameter(Json(payload): Json<SetParameterPayload>) -> impl IntoResponse {
    let result = control::set_parameter(
        payload.vehicle_id,
        payload.component_id,
        payload.parameter_name,
        payload.value,
    )
    .await;
    let answer = match result {
        Ok(_) => "Parameter set successfully".into(),
        Err(err) => {
            format!("{{\"error\": \"{}\"}}", err)
        }
    };
    ([(header::CONTENT_TYPE, "application/json")], answer).into_response()
}

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

    let mut vehicles_receiver = control::subscribe_vehicles();

    let (mut sender, mut receiver) = socket.split();

    let send_task = tokio::spawn(async move {
        while let Ok(vehicle) = vehicles_receiver.recv().await {
            if sender
                .send(ws::Message::Text(
                    serde_json::to_string_pretty(&vehicle).unwrap().into(),
                ))
                .await
                .is_err()
            {
                break;
            }
        }
    });

    // Handle incoming messages
    while let Some(Ok(message)) = receiver.next().await {
        match message {
            ws::Message::Text(text) => {
                trace!("WS client received from {identifier}: {text}");
            }
            ws::Message::Close(frame) => {
                debug!("WS client {identifier} disconnected: {frame:?}");
                break;
            }
            _ => {}
        }
    }

    // We should be disconnected now, let's remove it
    debug!("WS client {identifier} removed");
    if let Err(error) = send_task.await {
        warn!("Sender task finished with error: {error:?}");
    }
}

#[instrument(level = "debug", skip_all)]
pub(crate) async fn parameters_websocket_handler(
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Response {
    ws.on_upgrade(move |socket| parameters_websocket_connection(socket, addr))
}

#[instrument(level = "debug", skip(socket))]
async fn parameters_websocket_connection(socket: WebSocket, addr: SocketAddr) {
    let identifier = Uuid::new_v4();
    debug!("WS client connected with ID: {identifier}");

    let mut parameters_receiver = control::subscribe_parameters();

    let (mut sender, mut receiver) = socket.split();

    let send_task = tokio::spawn(async move {
        let parameters = control::parameters().await;
        if sender
            .send(ws::Message::Text(
                serde_json::to_string_pretty(&parameters).unwrap().into(),
            ))
            .await
            .is_err()
        {
            error!("Failed to send initial parameters to WS client");
            return;
        }

        while let Ok(parameters) = parameters_receiver.recv().await {
            if sender
                .send(ws::Message::Text(
                    serde_json::to_string_pretty(&parameters).unwrap().into(),
                ))
                .await
                .is_err()
            {
                break;
            }
        }
    });

    // Handle incoming messages
    while let Some(Ok(message)) = receiver.next().await {
        match message {
            ws::Message::Text(text) => {
                trace!("WS client received from {identifier}: {text}");
            }
            ws::Message::Close(frame) => {
                debug!("WS client {identifier} disconnected: {frame:?}");
                break;
            }
            _ => {}
        }
    }

    // We should be disconnected now, let's remove it
    debug!("WS client {identifier} removed");
    if let Err(error) = send_task.await {
        warn!("Sender task finished with error: {error:?}");
    }
}
