use axum::{
    body::Bytes,
    extract::Path,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use include_dir::{include_dir, Dir};
use mime_guess::from_path;
use serde::Serialize;

use crate::hub;

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
            version: "0.0.0".into(), //env!("VERGEN_GIT_SEMVER").into(),
            sha: env!("VERGEN_GIT_SHA").into(),
            build_date: env!("VERGEN_BUILD_TIMESTAMP").into(),
            authors: env!("CARGO_PKG_AUTHORS").into(),
        },
    };

    Json(info)
}

pub async fn mavlink(path: Option<Path<String>>) -> impl IntoResponse {
    let path = match path {
        Some(path) => path.0.to_string(),
        None => String::default(),
    };
    crate::drivers::rest::data::messages(&path)
}

pub async fn driver_stats() -> impl IntoResponse {
    Json(hub::drivers_stats().await.unwrap())
}

pub async fn hub_stats() -> impl IntoResponse {
    Json(hub::hub_stats().await.unwrap())
}

pub async fn hub_messages_stats() -> impl IntoResponse {
    Json(hub::hub_messages_stats().await.unwrap())
}
