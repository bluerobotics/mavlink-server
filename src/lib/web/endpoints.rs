use axum::{
    extract::Path,
    http::{header, StatusCode},
    response::IntoResponse,
};
use include_dir::{include_dir, Dir};
use mime_guess::from_path;

static HTML_DIST: Dir = include_dir!("src/webpage/dist");

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
