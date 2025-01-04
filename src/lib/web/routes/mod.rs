pub mod v1;

use axum::{
    extract::Path,
    http::{header, StatusCode},
    response::IntoResponse,
    routing::get,
    Router,
};
use include_dir::{include_dir, Dir};
use mime_guess::from_path;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::*;

use crate::cli;

static HTML_DIST: Dir = include_dir!("$CARGO_MANIFEST_DIR/src/webpage/dist");

#[instrument(level = "trace")]
pub fn router() -> Router {
    let app = Router::new()
        .route_service("/", get(root))
        .route("/:path", get(root))
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

    HTML_DIST.get_file(&filename).map_or_else(
        || handle_404().into_response(),
        |file| {
            // Determine the MIME type based on the file extension
            let mime_type = from_path(&filename).first_or_octet_stream();
            let content = file.contents();
            ([(header::CONTENT_TYPE, mime_type.as_ref())], content).into_response()
        },
    )
}

fn handle_404() -> (StatusCode, &'static str) {
    (StatusCode::NOT_FOUND, "404 Not Found")
}
