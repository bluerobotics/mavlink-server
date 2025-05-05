pub mod routes;

use std::net::SocketAddr;

use axum::{ServiceExt, extract::Request};
use tokio::signal;
use tower::Layer;
use tower_http::normalize_path::NormalizePathLayer;
use tracing::*;

pub async fn run(address: std::net::SocketAddrV4) {
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
    let mut first = true;
    loop {
        if first {
            first = false;
        } else {
            interval.tick().await;
        }

        let listener = match tokio::net::TcpListener::bind(&address).await {
            Ok(listener) => listener,
            Err(error) => {
                error!("WebServer TCP bind error: {error}");
                continue;
            }
        };

        let app = NormalizePathLayer::trim_trailing_slash().layer(routes::router());
        let service = ServiceExt::<Request>::into_make_service_with_connect_info::<SocketAddr>(app);

        info!("Running web server on address {address:?}");

        if let Err(error) = axum::serve(listener, service)
            .with_graceful_shutdown(shutdown_signal())
            .await
        {
            error!("WebServer error: {error}");
            continue;
        }

        break;
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
