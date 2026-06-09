use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result};
use clap::Parser;
use mavlink_server::{cli, drivers::zenoh::raw::ZenohRaw, hub, stats};
use tracing::*;

use crate::common::{pick_free_port, wait_for_stats_collection};
use crate::zenoh::{spawn_zenoh_router, wait_for_router, write_zenoh_client_config};

#[tokio::test(flavor = "multi_thread", worker_threads = 10)]
async fn test_zenohraw_bidirectional() -> Result<()> {
    let driver_name = "ZenohRaw";
    let port = pick_free_port();
    let _router = spawn_zenoh_router(port, "mavlink_raw").await;
    wait_for_router().await;

    let zenoh_config = write_zenoh_client_config(port)?;
    let zenoh_config = zenoh_config
        .to_str()
        .context("Zenoh config path is not valid UTF-8")?;

    cli::init_with(cli::Args::parse_from(vec![
        &std::env::args().next().unwrap_or_default(),
        "--allow-no-endpoints",
        "--zenoh-config-file",
        zenoh_config,
        "--mavlink-heartbeat-frequency",
        "10",
    ]));

    hub::add_driver(Arc::new(ZenohRaw::builder(driver_name).build())).await?;

    stats::set_period(Duration::from_millis(100)).await?;
    wait_for_stats_collection().await;

    let mut matching_drivers = 0;
    for (_uuid, driver) in stats::drivers_stats().await? {
        if *driver.name != driver_name {
            continue;
        }

        matching_drivers += 1;
        assert!(
            driver.stats.input.is_some(),
            "{driver_name} should receive mavlink over zenoh"
        );
        assert!(
            driver.stats.output.is_some(),
            "{driver_name} should publish mavlink over zenoh"
        );
    }

    assert_eq!(
        matching_drivers, 1,
        "expected one {driver_name} driver in stats"
    );

    for (id, driver_info) in hub::drivers().await? {
        debug!("Removing driver id {id:?} ({driver_info:?})");
        hub::remove_driver(id).await?;
    }

    Ok(())
}
