use std::net::{IpAddr, SocketAddr};

use hickory_resolver::Resolver;

use crate::verror::VError;

pub async fn lookup(
    r: &Resolver<
        hickory_resolver::name_server::GenericConnector<
            hickory_resolver::proto::runtime::TokioRuntimeProvider,
        >,
    >,
    d: &str,
    v4: bool,
) -> Option<IpAddr> {
    if v4 {
        if let Ok(record) = r.ipv4_lookup(d).await {
            record.iter().next().map(|a| IpAddr::V4(a.0))
        } else {
            if crate::log() {
                println!("Failed to resolve {d}");
            }
            None
        }
    } else if let Ok(record) = r.ipv6_lookup(d).await {
        record.iter().next().map(|aaaa| IpAddr::V6(aaaa.0))
    } else {
        if crate::log() {
            println!("Failed to resolve {d}");
        }
        None
    }
}

pub async fn resolve(
    resolver: &Resolver<
        hickory_resolver::name_server::GenericConnector<
            hickory_resolver::proto::runtime::TokioRuntimeProvider,
        >,
    >,
    domain: &str,
    port: u16,
) -> Result<SocketAddr, VError> {
    match crate::resolver_mode() {
        crate::config::ResolvingMode::IPv4 => {
            if let Some(ip) = lookup(resolver, domain, true).await {
                Ok(SocketAddr::new(ip, port))
            } else if let Some(ip) = lookup(resolver, domain, false).await {
                Ok(SocketAddr::new(ip, port))
            } else {
                if crate::log() {
                    println!("No host for {domain}");
                }
                Err(VError::NoHost)
            }
        }
        crate::config::ResolvingMode::IPv6 => {
            if let Some(ip) = lookup(resolver, domain, false).await {
                Ok(SocketAddr::new(ip, port))
            } else if let Some(ip) = lookup(resolver, domain, true).await {
                Ok(SocketAddr::new(ip, port))
            } else {
                if crate::log() {
                    println!("No host for {domain}");
                }
                Err(VError::NoHost)
            }
        }
    }
}
