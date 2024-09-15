mod endpoints;

use axum::{http::StatusCode, routing::get, Router};
use std::sync::{Arc, Mutex};
use tracing::*;

use lazy_static::lazy_static;

fn default_router() -> Router {
    Router::new()
        .route("/", get(endpoints::root))
        .route("/:path", get(endpoints::root))
        .route("/info", get(endpoints::info))
        .fallback(get(|| async { (StatusCode::NOT_FOUND, "Not found :(") }))
}

lazy_static! {
    static ref SERVER: Arc<SingletonServer> = Arc::new(SingletonServer {
        router: Mutex::new(default_router()),
    });
}

struct SingletonServer {
    router: Mutex<Router>,
}

pub fn start_server(address: String) {
    let router = SERVER.router.lock().unwrap().clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            let listener = match tokio::net::TcpListener::bind(&address).await {
                Ok(listener) => listener,
                Err(e) => {
                    error!("WebServer TCP bind error: {}", e);
                    continue;
                }
            };
            if let Err(e) = axum::serve(listener, router.clone()).await {
                error!("WebServer error: {}", e);
            }
        }
    });
}

pub fn configure_router<F>(modifier: F)
where
    F: FnOnce(&mut Router),
{
    let mut router = SERVER.router.lock().unwrap();
    modifier(&mut router);
}
