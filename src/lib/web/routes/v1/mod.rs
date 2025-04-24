use axum::Router;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::*;

pub mod info;
pub mod log;
pub mod rest;
pub mod stats;

#[instrument(level = "trace")]
pub fn router() -> Router {
    Router::new()
        .nest("/rest", rest::router())
        .nest("/stats", stats::router())
        .nest("/log", log::router())
        .nest("/info", info::router())
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
}
