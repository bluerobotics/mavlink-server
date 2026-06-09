use std::{path::PathBuf, time::Duration};

use anyhow::{Context, Result};
use zenoh;

pub async fn spawn_zenoh_router(port: u16, topic_prefix: &str) -> tokio::task::JoinHandle<()> {
    let topic_prefix = topic_prefix.to_string();

    tokio::spawn(async move {
        let mut config = zenoh::Config::default();
        config
            .insert_json5("mode", r#""router""#)
            .expect("Failed to insert router mode");
        config
            .insert_json5("listen/endpoints", &format!(r#"["tcp/127.0.0.1:{port}"]"#))
            .expect("Failed to insert listen endpoints");

        let session = zenoh::open(config)
            .await
            .expect("Failed to start zenoh router for tests");

        spawn_zenoh_loopback(session, &topic_prefix).await;

        std::future::pending::<()>().await
    })
}

async fn spawn_zenoh_loopback(session: zenoh::Session, topic_prefix: &str) {
    let out_topic = format!("{topic_prefix}/out");
    let in_topic = format!("{topic_prefix}/in");

    tokio::spawn(async move {
        let subscriber = session
            .declare_subscriber(&out_topic)
            .await
            .unwrap_or_else(|error| panic!("Failed to subscribe to {out_topic}: {error:?}"));
        let publisher = session
            .declare_publisher(&in_topic)
            .await
            .unwrap_or_else(|error| panic!("Failed to publish on {in_topic}: {error:?}"));

        while let Ok(sample) = subscriber.recv_async().await {
            if let Err(error) = publisher.put(sample.payload().to_bytes()).await {
                panic!("Failed to loop back {out_topic} -> {in_topic}: {error:?}");
            }
        }
    });
}

pub fn write_zenoh_client_config(port: u16) -> Result<PathBuf> {
    let path = std::env::temp_dir().join(format!(
        "mavlink-server-zenoh-test-{}-{port}.json5",
        std::process::id()
    ));

    std::fs::write(
        &path,
        format!(
            r#"{{
  mode: "client",
  connect: {{ endpoints: ["tcp/127.0.0.1:{port}"] }}
}}"#
        ),
    )
    .with_context(|| format!("Failed to write zenoh config to {}", path.display()))?;

    Ok(path)
}

pub async fn wait_for_router() {
    tokio::time::sleep(Duration::from_millis(500)).await;
}
