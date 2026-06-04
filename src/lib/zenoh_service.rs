use std::sync::Arc;

use anyhow::Result;
use serde::Serialize;
use tokio::sync::OnceCell;
use tracing::*;
use zenoh::bytes::Encoding;

use crate::cli::zenoh_config_file;

#[derive(Clone, Serialize)]
pub struct FoxgloveTimestamp {
    pub sec: i64,
    pub nsec: i64,
}

#[derive(Clone, Serialize)]
pub struct FoxgloveLog {
    pub timestamp: FoxgloveTimestamp,
    pub level: u8,
    pub message: String,
    pub name: String,
    pub file: String,
    pub line: u32,
}

static SESSION: OnceCell<Arc<zenoh::Session>> = OnceCell::const_new();

#[instrument(level = "debug", skip_all)]
pub async fn get() -> Result<Arc<zenoh::Session>> {
    SESSION.get_or_try_init(init_session).await.map(Arc::clone)
}


#[instrument(level = "debug")]
async fn init_session() -> Result<Arc<zenoh::Session>> {
    let config = if let Some(config_file) = zenoh_config_file() {
        info!("Loading Zenoh config from: {config_file}");
        zenoh::Config::from_file(&config_file)
            .map_err(|e| anyhow::anyhow!("Failed to load Zenoh config file '{config_file}': {e}"))?
    } else {
        debug!("Using default Zenoh configuration");
        let mut config = zenoh::Config::default();
        config
            .insert_json5("mode", r#""client""#)
            .map_err(|e| anyhow::anyhow!("Failed to set Zenoh mode: {e}"))?;
        config
            .insert_json5("connect/endpoints", r#"["tcp/127.0.0.1:7447"]"#)
            .map_err(|e| anyhow::anyhow!("Failed to set Zenoh connect endpoints: {e}"))?;
        config
    };

    let session = zenoh::open(config)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to open Zenoh session: {e}"))?;

    info!("Zenoh session initialized successfully");
    Ok(Arc::new(session))
}

#[instrument(level = "trace", skip(payload))]
pub async fn publish(key_expr: &str, payload: &str) -> Result<()> {
    let session = get().await?;
    session
        .put(key_expr, payload)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to publish to '{key_expr}': {e}"))?;
    Ok(())
}

#[instrument(level = "trace", skip(payload))]
pub async fn publish_json(key_expr: &str, payload: &str) -> Result<()> {
    let session = get().await?;
    session
        .put(key_expr, payload)
        .encoding(Encoding::APPLICATION_JSON)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to publish JSON to '{key_expr}': {e}"))?;
    Ok(())
}

#[instrument(level = "trace", skip(log))]
pub async fn publish_foxglove_log(key_expr: &str, log: &FoxgloveLog) -> Result<()> {
    let session = get().await?;
    let payload = serde_json::to_string(log)
        .map_err(|e| anyhow::anyhow!("Failed to serialize FoxgloveLog: {e}"))?;
    session
        .put(key_expr, payload)
        .encoding(Encoding::APPLICATION_JSON.with_schema("foxglove.Log"))
        .await
        .map_err(|e| anyhow::anyhow!("Failed to publish FoxgloveLog to '{key_expr}': {e}"))?;
    Ok(())
}
