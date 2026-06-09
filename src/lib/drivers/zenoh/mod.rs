pub mod json;
pub mod raw;

use std::sync::Arc;

use tokio::sync::{OnceCell, watch};
use tracing::*;
use zenoh;

use crate::cli::zenoh_config_file;

static SESSION_RX: OnceCell<watch::Receiver<Option<Arc<zenoh::Session>>>> = OnceCell::const_new();

#[instrument(level = "debug")]
pub(crate) fn build_config() -> anyhow::Result<zenoh::Config> {
    let mut config = if let Some(zenoh_config_file) = zenoh_config_file() {
        zenoh::Config::from_file(zenoh_config_file)
            .map_err(|error| anyhow::anyhow!("Failed to load Zenoh config file: {error:?}"))?
    } else {
        let mut config = zenoh::Config::default();
        config
            .insert_json5("mode", r#""client""#)
            .expect("Failed to insert client mode");
        config
            .insert_json5("connect/endpoints", r#"["tcp/127.0.0.1:7447"]"#)
            .expect("Failed to insert connect endpoints");
        config
    };

    config
        .insert_json5("adminspace", r#"{"enabled": true}"#)
        .expect("Failed to insert adminspace");
    config
        .insert_json5("metadata", r#"{"name": "mavlink-server"}"#)
        .expect("Failed to insert metadata");

    Ok(config)
}

#[instrument(level = "debug", skip_all)]
async fn init_session_manager() {
    SESSION_RX
        .get_or_init(|| async {
            let (tx, rx) = watch::channel(None);

            tokio::spawn(async move {
                let config = match build_config() {
                    Ok(config) => config,
                    Err(error) => {
                        error!("Failed to build Zenoh config: {error:?}");
                        return;
                    }
                };

                let mut first = true;
                loop {
                    if first {
                        first = false;
                    } else {
                        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                    }

                    match zenoh::open(config.clone()).await {
                        Ok(session) => {
                            if tx.send(Some(Arc::new(session))).is_err() {
                                break;
                            }

                            loop {
                                tokio::time::sleep(tokio::time::Duration::from_secs(86400)).await;
                            }
                        }
                        Err(error) => {
                            error!("Failed to start zenoh session: {error:?}");
                        }
                    }
                }
            });

            rx
        })
        .await;
}

#[instrument(level = "debug", skip_all)]
pub(crate) async fn session() -> Arc<zenoh::Session> {
    init_session_manager().await;

    let mut rx = SESSION_RX
        .get()
        .expect("Zenoh session manager is not initialized")
        .clone();

    loop {
        if let Some(session) = rx.borrow_and_update().clone() {
            return session;
        }

        if rx.changed().await.is_err() {
            panic!("Zenoh session manager stopped");
        }
    }
}
