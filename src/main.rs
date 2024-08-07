mod cli;
mod hub;
mod logger;
mod sinks;
mod sources;

use tracing::*;

// #[tokio::main(flavor = "multi_thread")]
fn main() -> Result<(), std::io::Error> {
    // CLI should be started before logger to allow control over verbosity
    cli::init();
    // Logger should start before everything else to register any log information
    logger::init();

    // TODO: Implement a parsing for the endpoints
    let endpoints = cli::mavlink_connections();
    dbg!(endpoints);

    wait_ctrlc();

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
