pub mod fake;
pub mod file;
pub mod tcp;
pub mod udp;

use std::sync::Arc;

use anyhow::Result;
use regex::Regex;
use tokio::sync::broadcast;
use url::Url;

use crate::protocol::Protocol;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Type {
    FileServer,
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
pub trait Driver: Send + Sync {
    async fn run(&self, hub_sender: broadcast::Sender<Protocol>) -> Result<()>;
    fn info(&self) -> DriverInfo;
}

type OnMessageCallback<T> = Option<Box<dyn OnMessageCallbackExt<T> + Send + Sync>>;

pub trait OnMessageCallbackExt<T>: Send + Sync {
    fn call(&self, msg: T) -> futures::future::BoxFuture<'static, Result<()>>;
}

impl<F, T, Fut> OnMessageCallbackExt<T> for F
where
    F: Fn(T) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<()>> + Send + 'static,
{
    fn call(&self, msg: T) -> futures::future::BoxFuture<'static, Result<()>> {
        Box::pin(self(msg))
    }
}

#[derive(Debug, Clone)]
pub struct DriverInfo {
    name: String,
}

pub trait DriverExt: Sync + Send {
    fn valid_schemes(&self) -> Vec<String>;
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
        let mut ip = std::net::Ipv4Addr::UNSPECIFIED.to_string();
        let port: u16;
        if let Ok(p) = legacy_entry.arg1.parse::<u16>() {
            port = p;
        } else {
            ip = legacy_entry.arg1.clone();
            let Some(arg2) = legacy_entry.arg2 else {
                return Err(format!("Missing port in legacy entry: {debug_entry:?}"));
            };

            port = match arg2.parse::<u16>() {
                Ok(port) => port,
                Err(error) => {
                    return Err(format!(
                        "Failed to parse port: {error:?}, legacy entry: {debug_entry:?}"
                    ));
                }
            }
        };

        match url::Url::parse(&format!("{scheme}://{ip}:{port}")) {
            Ok(url) => Ok(url),
            Err(error) => Err(format!(
                "Failed to parse URL: {error:?}, from legacy entry: {debug_entry:?}"
            )),
        }
    }
}

impl std::fmt::Debug for dyn DriverExt {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let scheme = self.default_scheme();
        write!(f, "DriverExt ({scheme})")
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
    pub driver_ext: Box<dyn DriverExt>,
    pub typ: Type,
}

pub fn endpoints() -> Vec<ExtInfo> {
    vec![
        ExtInfo {
            driver_ext: Box::new(file::server::FileServerExt),
            typ: Type::FileServer,
        },
        ExtInfo {
            driver_ext: Box::new(tcp::client::TcpClientExt),
            typ: Type::TcpClient,
        },
        ExtInfo {
            driver_ext: Box::new(tcp::server::TcpServerExt),
            typ: Type::TcpServer,
        },
        ExtInfo {
            driver_ext: Box::new(udp::client::UdpClientExt),
            typ: Type::UdpClient,
        },
        ExtInfo {
            driver_ext: Box::new(udp::server::UdpServerExt),
            typ: Type::UdpServer,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
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
}
