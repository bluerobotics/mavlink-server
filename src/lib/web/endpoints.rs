use axum::{
    extract::Path,
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use include_dir::{include_dir, Dir};
use mime_guess::from_path;
use serde::{Deserialize, Serialize};

use crate::stats;

static HTML_DIST: Dir = include_dir!("src/webpage/dist");

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
