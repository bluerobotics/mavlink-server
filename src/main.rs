mod cli;
mod drivers;
mod hub;
mod logger;
mod protocol;

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

    let hub = hub::Hub::new(
        10000,
        Arc::new(RwLock::new(
            mavlink::ardupilotmega::MavComponent::MAV_COMP_ID_ONBOARD_COMPUTER as u8,
        )),
        Arc::new(RwLock::new(1)),
        Arc::new(RwLock::new(1.)),
    )
    .await;

    for driver in cli::endpoints() {
        hub.add_driver(driver).await?;
    }

    wait_ctrlc().await;

    for (id, driver_info) in hub.drivers().await? {
        debug!("Removing driver id {id:?} ({driver_info:?})");
        hub.remove_driver(id).await?;
    }

    Ok(())
}

async fn wait_ctrlc() {
    let running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        r.store(false, std::sync::atomic::Ordering::SeqCst);
    })
    .expect("Error setting Ctrl-C handler");

    info!("Waiting for Ctrl-C...");
    while running.load(std::sync::atomic::Ordering::SeqCst) {
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await
    }
    info!("Received Ctrl-C! Exiting...");
}
