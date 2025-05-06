use anyhow::*;
use clap::Parser;
use mavlink_server::{cli, hub, stats};
use tracing::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 10)]
async fn test_udpserver_receive_only() -> Result<()> {
    cli::init_with(cli::Args::parse_from(vec![
        &std::env::args().next().unwrap_or_default(), // Required dummy argv[0] (program name)
        "udpclient:0.0.0.0:3333",
        "udpserver://0.0.0.0:3333?direction=receiver",
        "--mavlink-heartbeat-frequency",
        "10",
    ]));

    for driver in cli::endpoints() {
        hub::add_driver(driver).await?;
    }

    stats::set_period(tokio::time::Duration::from_millis(100)).await?;
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    for (_uuid, driver) in stats::drivers_stats().await? {
        if *driver.name == "UdpServer" {
            assert!(driver.stats.output.is_none());
            assert!(driver.stats.input.is_some());
        }

        if *driver.name == "UdpClient" {
            assert!(driver.stats.output.is_some());
            assert!(driver.stats.input.is_none());
        }
    }

    for (id, driver_info) in hub::drivers().await? {
        debug!("Removing driver id {id:?} ({driver_info:?})");
        hub::remove_driver(id).await?;
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 10)]
#[ignore]
async fn test_udpserver_send_only() -> Result<()> {
    cli::init_with(cli::Args::parse_from(vec![
        &std::env::args().next().unwrap_or_default(), // Required dummy argv[0] (program name)
        "udpclient:0.0.0.0:3333",
        "udpserver://0.0.0.0:3333?direction=sender",
        "--mavlink-heartbeat-frequency",
        "10",
    ]));

    for driver in cli::endpoints() {
        hub::add_driver(driver).await?;
    }

    stats::set_period(tokio::time::Duration::from_millis(100)).await?;
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    for (_uuid, driver) in stats::drivers_stats().await? {
        if *driver.name == "UdpServer" {
            assert!(driver.stats.output.is_some());
            assert!(driver.stats.input.is_none());
        }

        if *driver.name == "UdpClient" {
            assert!(driver.stats.output.is_some());
            assert!(driver.stats.input.is_some());
        }
    }

    for (id, driver_info) in hub::drivers().await? {
        debug!("Removing driver id {id:?} ({driver_info:?})");
        hub::remove_driver(id).await?;
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 10)]
#[ignore]
async fn test_udpclient_send_only() -> Result<()> {
    cli::init_with(cli::Args::parse_from(vec![
        &std::env::args().next().unwrap_or_default(), // Required dummy argv[0] (program name)
        "udpclient://0.0.0.0:3333?direction=sender",
        "udpserver://0.0.0.0:3333",
        "--mavlink-heartbeat-frequency",
        "10",
    ]));

    for driver in cli::endpoints() {
        hub::add_driver(driver).await?;
    }

    stats::set_period(tokio::time::Duration::from_millis(100)).await?;
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    for (_uuid, driver) in stats::drivers_stats().await? {
        if *driver.name == "UdpServer" {
            assert!(driver.stats.output.is_some());
            assert!(driver.stats.input.is_some());
        }

        if *driver.name == "UdpClient" {
            assert!(driver.stats.output.is_some());
            assert!(driver.stats.input.is_none());
        }
    }

    for (id, driver_info) in hub::drivers().await? {
        debug!("Removing driver id {id:?} ({driver_info:?})");
        hub::remove_driver(id).await?;
    }

    Ok(())
}
