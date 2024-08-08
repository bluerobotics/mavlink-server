mod cli;
mod hub;
mod logger;
mod sinks;
mod sources;

use anyhow::*;
use sinks::Sink;
use sources::Source;
use tracing::*;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    // CLI should be started before logger to allow control over verbosity
    cli::init();
    // Logger should start before everything else to register any log information
    logger::init();

    let hub = hub::Hub::new(100).await;

    let hub_receiver = hub.get_receiver();
    let sink = Sink::new(sinks::SinkInfo::FakeSink, hub_receiver);
    hub.add_sink(sink).await?;

    let hub_sender = hub.get_sender();
    let source = Source::new(sources::SourceInfo::FakeSource, hub_sender);
    hub.add_source(source).await?;

    // TODO: Implement a parsing for the endpoints
    let endpoints = cli::mavlink_connections();
    for endpoint in endpoints {
        //
    }

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
