use std::sync::Arc;

use tokio::sync::{OnceCell, watch};
use tracing::*;
use zenoh;

use crate::cli::zenoh_config_file;

static SESSION_WATCH_RECEIVER: OnceCell<watch::Receiver<Option<Arc<zenoh::Session>>>> =
    OnceCell::const_new();

#[instrument(level = "debug", skip_all)]
pub(crate) async fn session() -> Arc<zenoh::Session> {
    // Here, we open the singleton shared session in the background
    // so the callers can wait until the session is successfully
    // created.

    let mut receiver = SESSION_WATCH_RECEIVER
        .get_or_init(|| async {
            let (sender, receiver) = watch::channel(None);

            tokio::spawn(open_zenoh_session(sender));

            receiver
        })
        .await
        .clone();

    let guard = receiver
        .wait_for(Option::is_some)
        .await
        .expect("Zenoh session manager stopped before a session was ready");

    guard
        .as_ref()
        .expect("Zenoh session manager is initialized")
        .clone()
}

#[instrument(level = "debug", skip_all)]
async fn open_zenoh_session(sender: watch::Sender<Option<Arc<zenoh::Session>>>) {
    let config = match build_config() {
        Ok(config) => config,
        Err(error) => {
            error!("Failed to build Zenoh config: {error:?}");
            return;
        }
    };

    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
    let mut first = true;
    loop {
        if first {
            first = false;
        } else {
            interval.tick().await;
        }

        debug!("Trying to open Zenoh session...");

        match zenoh::open(config.clone()).await {
            Ok(session) => {
                let zid = session.zid();

                debug!("Zenoh session successfully created with zid: {zid:#?}");

                if let Err(error) = sender.send(Some(Arc::new(session))) {
                    error!("Failed to notify the Session watcheres: {error:?}");
                }

                break;
            }
            Err(error) => {
                error!("Failed to start zenoh session: {error:?}");
            }
        }
    }
}

#[instrument(level = "debug")]
fn build_config() -> anyhow::Result<zenoh::Config> {
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
