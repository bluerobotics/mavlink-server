pub mod mavlink;
pub mod websocket;

use axum::{routing::get, Router};
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
        .route("/mavlink/*path", get(mavlink::mavlink))
        .route(
            "/mavlink/message_id_from_name/*name",
            get(mavlink::message_id_from_name),
        )
}
