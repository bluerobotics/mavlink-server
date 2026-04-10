use axum::{
    Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, Query},
    http::{StatusCode, header},
    response::{IntoResponse, Json, Response},
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::*;

use crate::drivers::rest::mavftp;

const MAX_UPLOAD_BYTES: usize = 10 * 1024 * 1024;
const MAX_SYNC_DOWNLOAD_BYTES: usize = 1024 * 1024;

#[derive(Debug, Deserialize)]
pub struct PathQuery {
    path: String,
}

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
    path: String,
    size: u64,
}

#[derive(Debug, Deserialize)]
pub struct RenameQuery {
    from: String,
    to: String,
}

#[derive(Debug, Deserialize)]
pub struct TruncateQuery {
    path: String,
    offset: u32,
}

#[derive(Debug, Serialize)]
pub struct DownloadStartResponse {
    download_id: String,
}

#[derive(Debug, Serialize)]
pub struct Crc32Response {
    crc32: u32,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<u8>,
}

#[instrument(level = "trace")]
pub fn router() -> Router {
    Router::new()
        .route("/available", get(available))
        .route("/{system_id}/{component_id}/list", get(list_directory))
        .route("/{system_id}/{component_id}/download", get(download_file))
        .route(
            "/{system_id}/{component_id}/download/start",
            post(download_start),
        )
        .route("/download/{download_id}/progress", get(download_progress))
        .route("/download/{download_id}/data", get(download_data))
        .route(
            "/{system_id}/{component_id}/upload",
            post(upload_file).layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES)),
        )
        .route("/{system_id}/{component_id}/file", delete(remove_file))
        .route("/{system_id}/{component_id}/mkdir", post(mkdir))
        .route("/{system_id}/{component_id}/dir", delete(rmdir))
        .route("/{system_id}/{component_id}/rename", post(rename))
        .route("/{system_id}/{component_id}/truncate", post(truncate))
        .route("/{system_id}/{component_id}/crc32", get(crc32))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
}

#[instrument(level = "debug", skip_all)]
async fn available() -> impl IntoResponse {
    Json(mavftp::available_targets().await)
}

#[instrument(level = "debug", skip_all)]
async fn list_directory(
    Path(target): Path<TargetPath>,
    Query(query): Query<PathQuery>,
) -> Response {
    debug!(
        "FTP list_directory: sysid={} compid={} path={}",
        target.system_id, target.component_id, query.path
    );
    match mavftp::list_directory(target.system_id, target.component_id, &query.path).await {
        Ok(entries) => Json(entries).into_response(),
        Err(error) => ftp_error_to_response(error),
    }
}

#[instrument(level = "debug", skip_all)]
async fn download_file(Path(target): Path<TargetPath>, Query(query): Query<PathQuery>) -> Response {
    debug!(
        "FTP download: sysid={} compid={} path={}",
        target.system_id, target.component_id, query.path
    );
    match mavftp::read_file(
        target.system_id,
        target.component_id,
        &query.path,
        Some(MAX_SYNC_DOWNLOAD_BYTES),
    )
    .await
    {
        Ok(data) => {
            let filename = query.path.rsplit('/').next().unwrap_or("download");
            (
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
                .into_response()
        }
        Err(error) => ftp_error_to_response(error),
    }
}

#[instrument(level = "debug", skip_all)]
async fn download_start(
    Path(target): Path<TargetPath>,
    Query(query): Query<DownloadStartQuery>,
) -> Response {
    debug!(
        "FTP download_start: sysid={} compid={} path={} size={}",
        target.system_id, target.component_id, query.path, query.size
    );
    let download_id = mavftp::start_download(
        target.system_id,
        target.component_id,
        &query.path,
        query.size,
    )
    .await;
    Json(DownloadStartResponse { download_id }).into_response()
}

#[instrument(level = "debug", skip_all)]
async fn download_progress(Path(path): Path<DownloadPath>) -> Response {
    match mavftp::get_download_progress(&path.download_id).await {
        Some(progress) => Json(progress).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Download not found".to_string(),
                code: None,
            }),
        )
            .into_response(),
    }
}

#[instrument(level = "debug", skip_all)]
async fn download_data(Path(path): Path<DownloadPath>) -> Response {
    match mavftp::take_download_data(&path.download_id).await {
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
                code: None,
            }),
        )
            .into_response(),
    }
}

#[instrument(level = "debug", skip_all)]
async fn upload_file(
    Path(target): Path<TargetPath>,
    Query(query): Query<PathQuery>,
    body: Bytes,
) -> Response {
    debug!(
        "FTP upload: sysid={} compid={} path={} size={}",
        target.system_id,
        target.component_id,
        query.path,
        body.len(),
    );
    match mavftp::write_file(target.system_id, target.component_id, &query.path, &body).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(error) => ftp_error_to_response(error),
    }
}

#[instrument(level = "debug", skip_all)]
async fn remove_file(Path(target): Path<TargetPath>, Query(query): Query<PathQuery>) -> Response {
    debug!(
        "FTP remove_file: sysid={} compid={} path={}",
        target.system_id, target.component_id, query.path
    );
    match mavftp::remove_file(target.system_id, target.component_id, &query.path).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(error) => ftp_error_to_response(error),
    }
}

#[instrument(level = "debug", skip_all)]
async fn mkdir(Path(target): Path<TargetPath>, Query(query): Query<PathQuery>) -> Response {
    debug!(
        "FTP mkdir: sysid={} compid={} path={}",
        target.system_id, target.component_id, query.path
    );
    match mavftp::create_directory(target.system_id, target.component_id, &query.path).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(error) => ftp_error_to_response(error),
    }
}

#[instrument(level = "debug", skip_all)]
async fn rmdir(Path(target): Path<TargetPath>, Query(query): Query<PathQuery>) -> Response {
    debug!(
        "FTP rmdir: sysid={} compid={} path={}",
        target.system_id, target.component_id, query.path
    );
    match mavftp::remove_directory(target.system_id, target.component_id, &query.path).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(error) => ftp_error_to_response(error),
    }
}

#[instrument(level = "debug", skip_all)]
async fn rename(Path(target): Path<TargetPath>, Query(query): Query<RenameQuery>) -> Response {
    debug!(
        "FTP rename: sysid={} compid={} from={} to={}",
        target.system_id, target.component_id, query.from, query.to
    );
    match mavftp::rename(
        target.system_id,
        target.component_id,
        &query.from,
        &query.to,
    )
    .await
    {
        Ok(()) => StatusCode::OK.into_response(),
        Err(error) => ftp_error_to_response(error),
    }
}

#[instrument(level = "debug", skip_all)]
async fn truncate(Path(target): Path<TargetPath>, Query(query): Query<TruncateQuery>) -> Response {
    debug!(
        "FTP truncate: sysid={} compid={} path={} offset={}",
        target.system_id, target.component_id, query.path, query.offset
    );
    match mavftp::truncate_file(
        target.system_id,
        target.component_id,
        &query.path,
        query.offset,
    )
    .await
    {
        Ok(()) => StatusCode::OK.into_response(),
        Err(error) => ftp_error_to_response(error),
    }
}

#[instrument(level = "debug", skip_all)]
async fn crc32(Path(target): Path<TargetPath>, Query(query): Query<PathQuery>) -> Response {
    debug!(
        "FTP crc32: sysid={} compid={} path={}",
        target.system_id, target.component_id, query.path
    );
    match mavftp::calc_crc32(target.system_id, target.component_id, &query.path).await {
        Ok(crc32) => Json(Crc32Response { crc32 }).into_response(),
        Err(error) => ftp_error_to_response(error),
    }
}

#[instrument(level = "debug", skip(error))]
fn ftp_error_to_response(error: anyhow::Error) -> Response {
    if let Some(nak) = error.downcast_ref::<mavftp::FtpNakError>() {
        let status = match nak.err_code {
            mavftp::FtpError::FileNotFound => StatusCode::NOT_FOUND,
            mavftp::FtpError::FileExists => StatusCode::CONFLICT,
            mavftp::FtpError::FileProtected => StatusCode::FORBIDDEN,
            mavftp::FtpError::NoSessionsAvailable => StatusCode::SERVICE_UNAVAILABLE,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let body = ErrorResponse {
            error: nak.to_string(),
            code: Some(nak.err_code as u8),
        };
        return (status, Json(body)).into_response();
    }

    if error.downcast_ref::<mavftp::FtpClientError>().is_some() {
        let body = ErrorResponse {
            error: error.to_string(),
            code: None,
        };
        return (StatusCode::GATEWAY_TIMEOUT, Json(body)).into_response();
    }

    let error_message = error.to_string();
    let body = ErrorResponse {
        error: error_message,
        code: None,
    };
    (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
}
