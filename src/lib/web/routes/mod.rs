pub mod v1;

use axum::{
    Router,
    extract::Path,
    http::{StatusCode, header},
    response::IntoResponse,
    routing::get,
};
use include_dir::{Dir, include_dir};
use mime_guess::from_path;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::*;

use crate::cli;

static HTML_DIST: Dir = include_dir!("$CARGO_MANIFEST_DIR/src/webpage/dist");

#[instrument(level = "trace")]
pub fn router() -> Router {
    let app = Router::new()
        .route_service("/", get(root))
        .route_service("/{*path}", get(root))
        .nest("/v1", v1::router())
        .fallback(handle_404())
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    match cli::default_api_version() {
        1 => app.merge(v1::router()),
        _ => unimplemented!(),
    }
}

#[instrument(level = "trace")]
async fn root(filename: Option<Path<String>>) -> impl IntoResponse {
    let filename = filename
        .map(|Path(name)| {
            if name.is_empty() {
                "index.html".into()
            } else {
                name
            }
        })
        .unwrap_or_else(|| "index.html".into());

    // Determine the MIME type based on the file extension
    let mime_type = from_path(&filename).first_or_octet_stream();

    match HTML_DIST.get_file(format!("{filename}.gz")) {
        Some(file) => (
            [
                (header::CONTENT_TYPE, mime_type.as_ref()),
                (header::CONTENT_ENCODING, "gzip"),
            ],
            file.contents(),
        )
            .into_response(),
        None => match HTML_DIST.get_file(&filename) {
            Some(file) => (
                [(header::CONTENT_TYPE, mime_type.as_ref())],
                file.contents(),
            )
                .into_response(),
            None => return handle_404().into_response(),
        },
    }
}

fn handle_404() -> (StatusCode, &'static str) {
    (StatusCode::NOT_FOUND, "404 Not Found")
}
