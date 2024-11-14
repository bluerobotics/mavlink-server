use axum::{response::IntoResponse, routing::get, Json, Router};
use serde::Serialize;
use tracing::*;

#[derive(Serialize, Debug, Default)]
pub struct InfoContent {
    /// Name of the program
    name: String,
    /// Version/tag
    version: String,
    /// Git SHA
    sha: String,
    build_date: String,
    /// Authors name
    authors: String,
}

#[derive(Serialize, Debug, Default)]
pub struct Info {
    /// Version of the REST API
    version: u32,
    /// Service information
    service: InfoContent,
}

pub fn router() -> Router {
    Router::new()
        .route("/", get(info))
        .route("/full", get(info_full))
}

#[instrument(level = "trace")]
async fn info() -> Json<Info> {
    Json(Info {
        version: 0,
        service: InfoContent {
            name: env!("CARGO_PKG_NAME").into(),
            version: env!("VERGEN_GIT_DESCRIBE").into(),
            sha: env!("VERGEN_GIT_SHA").into(),
            build_date: env!("VERGEN_BUILD_TIMESTAMP").into(),
            authors: env!("CARGO_PKG_AUTHORS").into(),
        },
    })
}

#[instrument(level = "trace")]
async fn info_full() -> impl IntoResponse {
    let toml = std::str::from_utf8(include_bytes!("../../../../../Cargo.toml")).unwrap();
    let content: serde_json::Value = toml::from_str(toml).unwrap();
    serde_json::to_string(&content).unwrap()
}
