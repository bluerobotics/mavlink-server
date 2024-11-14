use axum::Router;
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
}
