use anyhow::*;
use tracing::*;

use mavlink_server::{cli, hub, logger, web};

#[tokio::main(flavor = "multi_thread", worker_threads = 10)]
async fn main() -> Result<()> {
    // CLI should be started before logger to allow control over verbosity
    cli::init();
    // Logger should start before everything else to register any log information
    logger::init();

    for driver in cli::endpoints() {
        hub::add_driver(driver).await?;
    }

    web::run(cli::web_server()).await;

    for (id, driver_info) in hub::drivers().await? {
        debug!("Removing driver id {id:?} ({driver_info:?})");
        hub::remove_driver(id).await?;
    }

    Ok(())
}
