mod cli;
mod drivers;
mod hub;
mod logger;
mod protocol;
mod stats;
mod web;

use std::sync::Arc;

use anyhow::*;
use tokio::sync::RwLock;
use tracing::*;

#[tokio::main(flavor = "multi_thread", worker_threads = 10)]
async fn main() -> Result<()> {
    // CLI should be started before logger to allow control over verbosity
    cli::init();
    // Logger should start before everything else to register any log information
    logger::init();

    for driver in cli::endpoints() {
        hub::add_driver(driver).await?;
    }

    let _stats = stats::Stats::new(tokio::time::Duration::from_secs(1)).await;

    web::run("0.0.0.0:8080".parse().unwrap()).await;

    for (id, driver_info) in hub::drivers().await? {
        debug!("Removing driver id {id:?} ({driver_info:?})");
        hub::remove_driver(id).await?;
    }

    Ok(())
}
