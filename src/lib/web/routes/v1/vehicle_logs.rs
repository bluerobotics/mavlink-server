use axum::{
    Router,
    extract::{Path, Query},
    http::{StatusCode, header},
    response::{IntoResponse, Json, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::*;

use crate::drivers::rest::vehicle_logs;

#[derive(Debug, Deserialize)]
pub struct TargetPath {
    system_id: u8,
    component_id: u8,
}

#[derive(Debug, Deserialize)]
pub struct DownloadPath {
    download_id: String,
}

#[derive(Debug, Deserialize)]
pub struct DownloadStartQuery {
    id: u16,
    size: u64,
}

#[derive(Debug, Deserialize)]
pub struct CancelQuery {
    system_id: u8,
    component_id: u8,
}

#[derive(Debug, Serialize)]
pub struct DownloadStartResponse {
    download_id: String,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    error: String,
}

#[instrument(level = "trace")]
pub fn router() -> Router {
    Router::new()
        .route("/{system_id}/{component_id}/list", get(list_logs))
        .route(
            "/{system_id}/{component_id}/download/start",
            post(download_start),
        )
        .route("/download/{download_id}/progress", get(download_progress))
        .route("/download/{download_id}/data", get(download_data))
        .route("/download/{download_id}/cancel", post(download_cancel))
        .route("/{system_id}/{component_id}/erase", post(erase_logs))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
}

#[instrument(level = "debug", skip_all)]
async fn list_logs(Path(target): Path<TargetPath>) -> Response {
    debug!(
        "Log list: sysid={} compid={}",
        target.system_id, target.component_id
    );
    match vehicle_logs::list_logs(target.system_id, target.component_id).await {
        Ok(entries) => Json(entries).into_response(),
        Err(error) => log_error_to_response(error),
    }
}

#[instrument(level = "debug", skip_all)]
async fn download_start(
    Path(target): Path<TargetPath>,
    Query(query): Query<DownloadStartQuery>,
) -> Response {
    debug!(
        "Log download_start: sysid={} compid={} id={} size={}",
        target.system_id, target.component_id, query.id, query.size
    );
    let download_id = vehicle_logs::start_log_download(
        target.system_id,
        target.component_id,
        query.id,
        query.size,
    )
    .await;
    Json(DownloadStartResponse { download_id }).into_response()
}

#[instrument(level = "debug", skip_all)]
async fn download_progress(Path(path): Path<DownloadPath>) -> Response {
    match vehicle_logs::get_download_progress(&path.download_id).await {
        Some(progress) => Json(progress).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Download not found".to_string(),
            }),
        )
            .into_response(),
    }
}

#[instrument(level = "debug", skip_all)]
async fn download_data(Path(path): Path<DownloadPath>) -> Response {
    match vehicle_logs::take_download_data(&path.download_id).await {
        Some((filename, data)) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "application/octet-stream".to_string()),
                (
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{filename}\""),
                ),
            ],
            data,
        )
            .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Download not ready or not found".to_string(),
            }),
        )
            .into_response(),
    }
}

#[instrument(level = "debug", skip_all)]
async fn download_cancel(
    Path(path): Path<DownloadPath>,
    Query(query): Query<CancelQuery>,
) -> Response {
    debug!(
        "Log download_cancel: id={} sysid={} compid={}",
        path.download_id, query.system_id, query.component_id
    );
    match vehicle_logs::cancel_download(&path.download_id, query.system_id, query.component_id)
        .await
    {
        Ok(()) => StatusCode::OK.into_response(),
        Err(error) => log_error_to_response(error),
    }
}

#[instrument(level = "debug", skip_all)]
async fn erase_logs(Path(target): Path<TargetPath>) -> Response {
    debug!(
        "Log erase: sysid={} compid={}",
        target.system_id, target.component_id
    );
    match vehicle_logs::erase_logs(target.system_id, target.component_id).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(error) => log_error_to_response(error),
    }
}

#[instrument(level = "debug", skip(error))]
fn log_error_to_response(error: anyhow::Error) -> Response {
    if error
        .downcast_ref::<vehicle_logs::LogClientError>()
        .is_some()
    {
        let body = ErrorResponse {
            error: error.to_string(),
        };
        return (StatusCode::GATEWAY_TIMEOUT, Json(body)).into_response();
    }

    let body = ErrorResponse {
        error: error.to_string(),
    };
    (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
}
