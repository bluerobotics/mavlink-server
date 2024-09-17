pub mod fake;
pub mod serial;
pub mod tcp;
pub mod tlog;
pub mod udp;

use std::sync::Arc;

use anyhow::Result;
use regex::Regex;
use tokio::sync::broadcast;
use tracing::*;
use url::Url;

use crate::{protocol::Protocol, stats::accumulated::driver::AccumulatedDriverStatsProvider};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Type {
    FakeClient,
    FakeSource,
    Serial,
    TlogWriter,
    TlogReader,
    TcpClient,
    TcpServer,
    UdpClient,
    UdpServer,
}

// This legacy description is based on others tools like mavp2p, mavproxy and mavlink-router
#[derive(Debug, Clone)]
pub struct DriverDescriptionLegacy {
    typ: Type,
    arg1: String,
    arg2: Option<String>,
}

#[async_trait::async_trait]
pub trait Driver: Send + Sync + AccumulatedDriverStatsProvider {
    async fn run(&self, hub_sender: broadcast::Sender<Arc<Protocol>>) -> Result<()>;
    fn info(&self) -> Box<dyn DriverInfo>;
}

pub trait DriverInfo: Sync + Send {
    fn name(&self) -> &str;

    fn valid_schemes(&self) -> Vec<String>;
    // CLI helpers
    fn cli_example_legacy(&self) -> Vec<String>;
    fn cli_example_url(&self) -> Vec<String>;

    fn create_endpoint_from_url(&self, url: &Url) -> Option<Arc<dyn Driver>>;

    fn default_scheme(&self) -> String {
        self.valid_schemes().first().unwrap().clone()
    }

    // This is mostly used by network based endpoints, other endpoints can overwrite it
    fn url_from_legacy(
        &self,
        legacy_entry: crate::drivers::DriverDescriptionLegacy,
    ) -> Result<url::Url, String> {
        let debug_entry = legacy_entry.clone();
        let scheme = self.default_scheme();
        let mut host = std::net::Ipv4Addr::UNSPECIFIED.to_string();

        // For cases like serial baudrate, the number may not be under u16
        // For such legacy cases, we are going to use "arg2" as ab optional parameter field
        let mut port: Option<u16> = None;
        let mut argument = None;

        if let Ok(number) = legacy_entry.arg1.parse::<u16>() {
            port = Some(number);
        } else {
            host = legacy_entry.arg1.clone();

            if let Some(arg2) = legacy_entry.arg2 {
                match arg2.parse::<u16>() {
                    Ok(number) => port = Some(number),
                    Err(error) => {
                        debug!("{error} for arg2: {arg2}, using url argument");
                        argument = Some(arg2);
                    }
                };
            } else {
                debug!("Missing port in legacy entry: {debug_entry:?}");
            };
        };

        match url::Url::parse(&format!("{scheme}://{host}")) {
            Ok(mut url) => {
                if let Some(port) = port {
                    if url.set_port(Some(port)).is_err() {
                        debug!("Failed to set port {port} in URL: {url}, moving to argument");
                        if argument.is_none() {
                            argument = Some(port.to_string());
                        }
                    };
                }

                if let Some(argument) = argument {
                    url.query_pairs_mut().append_pair("arg2", &argument);
                }
                Ok(url)
            }
            Err(error) => Err(format!(
                "Failed to parse URL: {error:?}, from legacy entry: {debug_entry:?}"
            )),
        }
    }
}

impl std::fmt::Debug for dyn DriverInfo {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let scheme = self.default_scheme();
        write!(f, "DriverInfo ({scheme})")
    }
}

fn process_old_format(entry: &str) -> Option<DriverDescriptionLegacy> {
    let captures = Regex::new(r"^(?P<scheme>\w+):(?P<arg1>[^:]+)(:(?P<arg2>\d+))?$")
        .unwrap()
        .captures(entry)?;

    let prefix = captures.name("scheme").unwrap().as_str();
    let arg1 = captures.name("arg1").unwrap().as_str();
    let arg2 = captures.name("arg2").map(|m| m.as_str());

    let endpoints = endpoints();
    let endpoint = endpoints.iter().find(|endpoint| {
        endpoint
            .driver_ext
            .valid_schemes()
            .contains(&prefix.to_string())
    })?;

    Some(DriverDescriptionLegacy {
        typ: endpoint.typ.clone(),
        arg1: arg1.to_string(),
        arg2: arg2.map(|a| a.to_string()),
    })
}

pub fn create_driver_from_entry(entry: &str) -> Result<Arc<dyn Driver>, String> {
    endpoints()
        .iter()
        .filter_map(|endpoint| {
            let driver_ext = &endpoint.driver_ext;
            if let Some(legacy_entry) = process_old_format(entry) {
                if legacy_entry.typ != endpoint.typ {
                    return None;
                }
                return driver_ext
                    .create_endpoint_from_url(&driver_ext.url_from_legacy(legacy_entry).unwrap());
            }

            let url = Url::parse(entry).map_err(|e| e.to_string()).ok()?;
            if driver_ext
                .valid_schemes()
                .contains(&url.scheme().to_string())
            {
                return driver_ext.create_endpoint_from_url(&url);
            }
            None
        })
        .next()
        .ok_or_else(|| format!("Found no driver for entry: {entry}"))
}

#[derive(Debug)]
pub struct ExtInfo {
    pub driver_ext: Box<dyn DriverInfo>,
    pub typ: Type,
}

pub fn endpoints() -> Vec<ExtInfo> {
    vec![
        ExtInfo {
            driver_ext: Box::new(tlog::writer::TlogWriterInfo),
            typ: Type::TlogWriter,
        },
        ExtInfo {
            driver_ext: Box::new(tlog::reader::TlogReaderInfo),
            typ: Type::TlogReader,
        },
        ExtInfo {
            driver_ext: Box::new(serial::SerialInfo),
            typ: Type::Serial,
        },
        ExtInfo {
            driver_ext: Box::new(tcp::client::TcpClientInfo),
            typ: Type::TcpClient,
        },
        ExtInfo {
            driver_ext: Box::new(tcp::server::TcpServerInfo),
            typ: Type::TcpServer,
        },
        ExtInfo {
            driver_ext: Box::new(udp::client::UdpClientInfo),
            typ: Type::UdpClient,
        },
        ExtInfo {
            driver_ext: Box::new(udp::server::UdpServerInfo),
            typ: Type::UdpServer,
        },
        ExtInfo {
            driver_ext: Box::new(fake::FakeSourceInfo),
            typ: Type::FakeClient,
        },
        ExtInfo {
            driver_ext: Box::new(fake::FakeSinkInfo),
            typ: Type::FakeSource,
        },
    ]
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, sync::Arc};

    use anyhow::{anyhow, Result};
    use mavlink::MAVLinkV2MessageRaw;
    use mavlink_server::callbacks::{Callbacks, MessageCallback};
    use tokio::sync::RwLock;
    use tracing::*;

    use crate::stats::accumulated::driver::AccumulatedDriverStats;

    use super::*;

    #[test]
    fn test_unique_endpoints() {
        let mut unique_types = HashSet::new();
        for endpoint in endpoints() {
            assert!(
                unique_types.insert(endpoint.typ.clone()),
                "Duplicate endpoint type found: {:?}",
                endpoint.typ
            );
        }
    }

    // Example struct implementing Driver
    pub struct ExampleDriver {
        on_message_input: Callbacks<Arc<Protocol>>,
        on_message_output: Callbacks<Arc<Protocol>>,
        stats: Arc<RwLock<AccumulatedDriverStats>>,
    }

    impl ExampleDriver {
        pub fn new() -> ExampleDriverBuilder {
            ExampleDriverBuilder(Self {
                on_message_input: Callbacks::new(),
                on_message_output: Callbacks::new(),
                stats: Arc::new(RwLock::new(AccumulatedDriverStats::default())),
            })
        }
    }

    struct ExampleDriverBuilder(ExampleDriver);

    impl ExampleDriverBuilder {
        pub fn build(self) -> ExampleDriver {
            self.0
        }

        pub fn on_message_input<C>(self, callback: C) -> Self
        where
            C: MessageCallback<Arc<Protocol>>,
        {
            self.0.on_message_input.add_callback(callback.into_boxed());
            self
        }
    }

    #[async_trait::async_trait]
    impl Driver for ExampleDriver {
        async fn run(&self, hub_sender: broadcast::Sender<Arc<Protocol>>) -> Result<()> {
            let mut hub_receiver = hub_sender.subscribe();

            while let Ok(message) = hub_receiver.recv().await {
                self.stats.write().await.update_input(&message).await;

                for future in self.on_message_input.call_all(message.clone()) {
                    if let Err(error) = future.await {
                        debug!(
                            "Dropping message: on_message_input callback returned error: {error:?}"
                        );
                        continue;
                    }
                }

                trace!("Message received: {message:?}");
            }

            Ok(())
        }

        fn info(&self) -> Box<dyn DriverInfo> {
            Box::new(ExampleDriverInfo)
        }
    }

    #[async_trait::async_trait]
    impl AccumulatedDriverStatsProvider for ExampleDriver {
        async fn stats(&self) -> AccumulatedDriverStats {
            self.stats.read().await.clone()
        }

        async fn reset_stats(&self) {
            *self.stats.write().await = AccumulatedDriverStats {
                input: None,
                output: None,
            }
        }
    }

    pub struct ExampleDriverInfo;
    impl DriverInfo for ExampleDriverInfo {
        fn name(&self) -> &str {
            "ExampleDriver"
        }

        fn valid_schemes(&self) -> Vec<String> {
            vec!["exampledriver".to_string()]
        }

        fn cli_example_legacy(&self) -> Vec<String> {
            vec![]
        }

        fn cli_example_url(&self) -> Vec<String> {
            vec![]
        }

        fn create_endpoint_from_url(&self, url: &url::Url) -> Option<Arc<dyn Driver>> {
            None
        }
    }

    #[tokio::test]
    async fn on_message_input_callback_test() -> Result<()> {
        let (sender, _receiver) = tokio::sync::broadcast::channel(1);

        let called = Arc::new(RwLock::new(false));
        let driver = ExampleDriver::new()
            .on_message_input({
                let called = called.clone();
                move |_msg| {
                    let called = called.clone();

                    async move {
                        *called.write().await = true;

                        Err(anyhow!("Finished from callback"))
                    }
                }
            })
            .build();

        let receiver_task_handle = tokio::spawn({
            let sender = sender.clone();

            async move { driver.run(sender).await }
        });

        let sender_task_handle = tokio::spawn({
            let sender = sender.clone();

            async move {
                sender
                    .send(Arc::new(Protocol::new("test", MAVLinkV2MessageRaw::new())))
                    .unwrap();
            }
        });

        tokio::time::timeout(tokio::time::Duration::from_millis(1), async {
            loop {
                if *called.read().await {
                    break;
                }
                tokio::time::sleep(tokio::time::Duration::from_micros(1)).await;
            }
        })
        .await
        .unwrap();

        receiver_task_handle.abort();
        sender_task_handle.abort();

        Ok(())
    }
}
