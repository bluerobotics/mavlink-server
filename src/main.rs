use anyhow::*;
use mavlink_server::{cli, hub, logger, web};
use tracing::*;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    // CLI should be started before logger to allow control over verbosity
    cli::init();
    // Logger should start before everything else to register any log information
    logger::init(cli::log_path(), cli::is_verbose(), cli::is_tracing());

    info!(
        "{}, version: {}-{}, build date: {}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
        env!("VERGEN_GIT_SHA"),
        env!("VERGEN_BUILD_DATE")
    );
    info!(
        "Starting at {}",
        chrono::Local::now().format("%Y-%m-%dT%H:%M:%S"),
    );
    debug!("Command line call: {}", cli::command_line_string());
    debug!("Command line input struct call: {}", cli::command_line());

    for driver in cli::endpoints() {
        hub::add_driver(driver).await?;
    }

    // This will block until the web server is stopped, with this, the application ends
    web::run(cli::web_server()).await;

    for (id, driver_info) in hub::drivers().await? {
        debug!("Removing driver id {id:?} ({driver_info:?})");
        hub::remove_driver(id).await?;
    }

    Ok(())
}
