pub mod mavlink;
pub mod vehicles;
pub mod websocket;

use axum::{
    routing::{get, post},
    Router,
};
use tracing::*;

#[instrument(level = "trace")]
pub fn router() -> Router {
    Router::new()
        .route("/ws", get(websocket::websocket_handler))
        .route("/helper", get(mavlink::helper))
        .route(
            "/mavlink",
            get(mavlink::mavlink).post(mavlink::post_mavlink),
        )
        .route("/mavlink/{*path}", get(mavlink::mavlink))
        .route(
            "/mavlink/message_id_from_name/{*name}",
            get(mavlink::message_id_from_name),
        )
        .route("/vehicles", get(vehicles::vehicles))
        .route("/vehicles/ws", get(vehicles::websocket_handler))
        .route("/vehicles/arm", post(vehicles::arm))
        .route("/vehicles/disarm", post(vehicles::disarm))
}
