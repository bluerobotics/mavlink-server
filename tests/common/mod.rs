use std::time::Duration;

pub fn pick_free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("Failed to bind ephemeral port")
        .local_addr()
        .expect("Failed to read ephemeral port")
        .port()
}

pub async fn wait_for_stats_collection() {
    tokio::time::sleep(Duration::from_secs(2)).await;
}
