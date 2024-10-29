use std::net::SocketAddr;

use axum::{
    extract::{connect_info::ConnectInfo, Path, State},
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use include_dir::{include_dir, Dir};
use mime_guess::from_path;
use serde::{Deserialize, Serialize};
use tracing::*;

use crate::stats;
use crate::web::AppState;

static HTML_DIST: Dir = include_dir!("src/webpage/dist");

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

#[derive(Serialize, Deserialize, Debug)]
pub struct Frequency {
    pub frequency: f32,
}

pub async fn root(filename: Option<Path<String>>) -> impl IntoResponse {
    let filename = filename
        .map(|Path(name)| {
            if name.is_empty() {
                "index.html".into()
            } else {
                name
            }
        })
        .unwrap_or_else(|| "index.html".into());

    HTML_DIST.get_file(&filename).map_or_else(
        || {
            // Return 404 Not Found if the file doesn't exist
            (StatusCode::NOT_FOUND, "404 Not Found").into_response()
        },
        |file| {
            // Determine the MIME type based on the file extension
            let mime_type = from_path(&filename).first_or_octet_stream();
            let content = file.contents();
            ([(header::CONTENT_TYPE, mime_type.as_ref())], content).into_response()
        },
    )
}

pub async fn info() -> Json<Info> {
    let info = Info {
        version: 0,
        service: InfoContent {
            name: env!("CARGO_PKG_NAME").into(),
            version: env!("VERGEN_GIT_DESCRIBE").into(),
            sha: env!("VERGEN_GIT_SHA").into(),
            build_date: env!("VERGEN_BUILD_TIMESTAMP").into(),
            authors: env!("CARGO_PKG_AUTHORS").into(),
        },
    };

    Json(info)
}

pub async fn info_full() -> impl IntoResponse {
    let toml = std::str::from_utf8(include_bytes!("../../../Cargo.toml")).unwrap();
    let content: serde_json::Value = toml::from_str(toml).unwrap();
    serde_json::to_string(&content).unwrap()
}

pub async fn mavlink(path: Option<Path<String>>) -> impl IntoResponse {
    let path = match path {
        Some(path) => path.0.to_string(),
        None => String::default(),
    };
    crate::drivers::rest::data::messages(&path)
}

pub async fn post_mavlink(
    ConnectInfo(address): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    message: String,
) {
    debug!("Got message from: {address:?}, {message}");
    if let Err(error) = state.message_tx.send(message) {
        error!("Failed to send message to main loop: {error:?}");
    }
}

pub async fn message_id_from_name(name: Path<String>) -> impl IntoResponse {
    use mavlink::{self, Message};
    mavlink::ardupilotmega::MavMessage::message_id_from_name(&name.0.to_ascii_uppercase())
        .map(|id| (StatusCode::OK, Json(id)).into_response())
        .unwrap_or_else(|_| (StatusCode::NOT_FOUND, "404 Not Found").into_response())
}

pub async fn drivers_stats() -> impl IntoResponse {
    Json(stats::drivers_stats().await.unwrap())
}

pub async fn hub_stats() -> impl IntoResponse {
    Json(stats::hub_stats().await.unwrap())
}

pub async fn hub_messages_stats() -> impl IntoResponse {
    Json(stats::hub_messages_stats().await.unwrap())
}

pub async fn stats_frequency() -> impl IntoResponse {
    let frequency = 1. / stats::period().await.unwrap().as_secs_f32();
    Json(Frequency { frequency })
}

pub async fn set_stats_frequency(
    Json(Frequency { frequency }): Json<Frequency>,
) -> impl IntoResponse {
    let period = tokio::time::Duration::from_secs_f32(1. / frequency.clamp(0.1, 10.));

    let frequency = 1. / stats::set_period(period).await.unwrap().as_secs_f32();
    Json(Frequency { frequency })
}
