use axum::{
    extract::Path,
    response::{Html, IntoResponse},
    Json,
};
use include_dir::{include_dir, Dir};
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
    let filename = if filename.is_none() {
        "index.html".to_string()
    } else {
        filename.unwrap().0
    };

    HTML_DIST.get_file(filename).map_or(
        Html("File not found".to_string()).into_response(),
        |file| {
            let content = file.contents_utf8().unwrap_or("");
            Html(content.to_string()).into_response()
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
