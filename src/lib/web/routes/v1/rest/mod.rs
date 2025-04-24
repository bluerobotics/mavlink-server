pub mod mavlink;
pub mod vehicles;
pub mod websocket;

use axum::{
    routing::{get, post},
    Router,
};
use tower_http::{cors::CorsLayer, trace::TraceLayer};
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
        .route("/vehicles/version", get(vehicles::version))
        .route(
            "/vehicles/available_parameters",
            get(vehicles::available_parameters),
        )
        .route("/vehicles/parameters", get(vehicles::parameters))
        .route(
            "/vehicles/parameters/ws",
            get(vehicles::parameters_websocket_handler),
        )
        .route("/vehicles/set_parameter", post(vehicles::set_parameter))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
}
