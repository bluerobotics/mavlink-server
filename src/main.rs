mod cli;
mod drivers;
mod hub;
mod logger;

use std::sync::Arc;

use anyhow::*;
use tracing::*;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    // CLI should be started before logger to allow control over verbosity
    cli::init();
    // Logger should start before everything else to register any log information
    logger::init();

    let hub = hub::Hub::new(100).await;

    hub.add_driver(Arc::new(drivers::FakeSink {})).await?;
    hub.add_driver(Arc::new(drivers::FakeSource {})).await?;

    wait_ctrlc();

    for (id, driver_info) in hub.drivers().await {
        debug!("Removing driver id {id:?} ({driver_info:?})");
        hub.remove_driver(id).await?;
    }

    Ok(())
}

fn wait_ctrlc() {
    let running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        r.store(false, std::sync::atomic::Ordering::SeqCst);
    })
    .expect("Error setting Ctrl-C handler");

    info!("Waiting for Ctrl-C...");
    while running.load(std::sync::atomic::Ordering::SeqCst) {}
    info!("Received Ctrl-C! Exiting...");
}
