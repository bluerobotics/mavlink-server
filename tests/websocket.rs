mod common;

use anyhow::*;
use clap::Parser;
use mavlink_server::{cli, hub, stats};
use tracing::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 10)]
async fn test_wsserver_receive_only() -> Result<()> {
    let port = common::pick_free_port();
    let bind_addr = format!("0.0.0.0:{port}");

    cli::init_with(cli::Args::parse_from(vec![
        &std::env::args().next().unwrap_or_default(), // Required dummy argv[0] (program name)
        &format!("wsclient://{bind_addr}"),
        &format!("wsserver://{bind_addr}?direction=receiver"),
        "--mavlink-heartbeat-frequency",
        "10",
    ]));

    for driver in cli::endpoints() {
        hub::add_driver(driver).await?;
    }

    stats::set_period(tokio::time::Duration::from_millis(100)).await?;
    common::wait_for_stats_collection().await;
    for (_uuid, driver) in stats::drivers_stats().await? {
        if *driver.name == "WsServer" {
            assert!(driver.stats.output.is_none());
            assert!(driver.stats.input.is_some());
        }

        if *driver.name == "WsClient" {
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
async fn test_wsserver_send_only() -> Result<()> {
    let port = common::pick_free_port();
    let bind_addr = format!("0.0.0.0:{port}");

    cli::init_with(cli::Args::parse_from(vec![
        &std::env::args().next().unwrap_or_default(), // Required dummy argv[0] (program name)
        &format!("wsclient://{bind_addr}"),
        &format!("wsserver://{bind_addr}?direction=sender"),
        "--mavlink-heartbeat-frequency",
        "10",
    ]));

    for driver in cli::endpoints() {
        hub::add_driver(driver).await?;
    }

    stats::set_period(tokio::time::Duration::from_millis(100)).await?;
    common::wait_for_stats_collection().await;
    for (_uuid, driver) in stats::drivers_stats().await? {
        if *driver.name == "WsServer" {
            assert!(driver.stats.output.is_some());
            assert!(driver.stats.input.is_none());
        }

        if *driver.name == "WsClient" {
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
async fn test_wsclient_send_only() -> Result<()> {
    let port = common::pick_free_port();
    let bind_addr = format!("0.0.0.0:{port}");

    cli::init_with(cli::Args::parse_from(vec![
        &std::env::args().next().unwrap_or_default(), // Required dummy argv[0] (program name)
        &format!("wsclient://{bind_addr}?direction=sender"),
        &format!("wsserver://{bind_addr}"),
        "--mavlink-heartbeat-frequency",
        "10",
    ]));

    for driver in cli::endpoints() {
        hub::add_driver(driver).await?;
    }

    stats::set_period(tokio::time::Duration::from_millis(100)).await?;
    common::wait_for_stats_collection().await;
    for (_uuid, driver) in stats::drivers_stats().await? {
        if *driver.name == "WsServer" {
            assert!(driver.stats.output.is_some());
            assert!(driver.stats.input.is_some());
        }

        if *driver.name == "WsClient" {
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
async fn test_wsclient_receive_only() -> Result<()> {
    let port = common::pick_free_port();
    let bind_addr = format!("0.0.0.0:{port}");

    cli::init_with(cli::Args::parse_from(vec![
        &std::env::args().next().unwrap_or_default(), // Required dummy argv[0] (program name)
        &format!("wsclient://{bind_addr}?direction=receiver"),
        &format!("wsserver://{bind_addr}"),
        "--mavlink-heartbeat-frequency",
        "10",
    ]));

    for driver in cli::endpoints() {
        hub::add_driver(driver).await?;
    }

    stats::set_period(tokio::time::Duration::from_millis(100)).await?;
    common::wait_for_stats_collection().await;
    for (_uuid, driver) in stats::drivers_stats().await? {
        if *driver.name == "WsServer" {
            assert!(driver.stats.output.is_some());
            assert!(driver.stats.input.is_none());
        }

        if *driver.name == "WsClient" {
            assert!(driver.stats.output.is_none());
            assert!(driver.stats.input.is_some());
        }
    }

    for (id, driver_info) in hub::drivers().await? {
        debug!("Removing driver id {id:?} ({driver_info:?})");
        hub::remove_driver(id).await?;
    }

    Ok(())
}
