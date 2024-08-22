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

    // Endpoints creation
    {
        for endpoint in cli::file_server_endpoints() {
            debug!("Creating File Server to {endpoint:?}");
            hub.add_driver(Arc::new(
                drivers::file::server::FileServer::try_new(&endpoint).unwrap(),
            ))
            .await?;
        }
        for endpoint in cli::tcp_client_endpoints() {
            debug!("Creating TCP Client to {endpoint:?}");
            hub.add_driver(Arc::new(drivers::tcp::client::TcpClient::new(&endpoint)))
                .await?;
        }
        for endpoint in cli::tcp_server_endpoints() {
            debug!("Creating TCP Server to {endpoint:?}");
            hub.add_driver(Arc::new(drivers::tcp::server::TcpServer::new(&endpoint)))
                .await?;
        }
        for endpoint in cli::udp_client_endpoints() {
            debug!("Creating UDP Client to {endpoint:?}");
            hub.add_driver(Arc::new(drivers::udp::client::UdpClient::new(&endpoint)))
                .await?;
        }
        for endpoint in cli::udp_server_endpoints() {
            debug!("Creating UDP Server to {endpoint:?}");
            hub.add_driver(Arc::new(drivers::udp::server::UdpServer::new(&endpoint)))
                .await?;
        }
        for endpoint in cli::udp_broadcast_endpoints() {
            debug!("Creating UDP Broadcast to {endpoint:?}");

            let mut s = endpoint.split(':');
            let _ip = s.next().unwrap();
            let port = s.next().unwrap();
            let broadcast_ip = "255.255.255.255";

            let endpoint = format!("{broadcast_ip}:{port}");

            hub.add_driver(Arc::new(drivers::udp::client::UdpClient::new(&endpoint)))
                .await?;
            continue;
        }
        for _endpoint in cli::serial_endpoints() {
            error!("Serial endpoint not implemented");
            continue;
        }
    }

    wait_ctrlc().await;

    for (id, driver_info) in hub.drivers().await {
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
