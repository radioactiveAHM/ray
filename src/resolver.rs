use std::net::{IpAddr, SocketAddr};

use hickory_resolver::{
    Resolver, config::NameServerConfigGroup, name_server::TokioConnectionProvider,
};

use crate::verror::VError;

pub fn generate_resolver(
    rc: &crate::config::Resolver,
) -> Resolver<
    hickory_resolver::name_server::GenericConnector<
        hickory_resolver::proto::runtime::TokioRuntimeProvider,
    >,
> {
    let protocol = if let Some(addr) = &rc.resolver {
        let mut parts = addr.split("://");
        if let Some(scheme) = parts.next()
            && let Some(domain) = parts.next()
        {
            match scheme {
                "https" => NameServerConfigGroup::from_ips_https(
                    &[rc.remote.ip()],
                    rc.remote.port(),
                    domain.to_string(),
                    true,
                ),
                "h3" => NameServerConfigGroup::from_ips_h3(
                    &[rc.remote.ip()],
                    rc.remote.port(),
                    domain.to_string(),
                    true,
                ),
                "quic" => NameServerConfigGroup::from_ips_quic(
                    &[rc.remote.ip()],
                    rc.remote.port(),
                    domain.to_string(),
                    true,
                ),
                "tls" => NameServerConfigGroup::from_ips_tls(
                    &[rc.remote.ip()],
                    rc.remote.port(),
                    domain.to_string(),
                    true,
                ),
                "udp" => {
                    NameServerConfigGroup::from_ips_clear(&[rc.remote.ip()], rc.remote.port(), true)
                }
                _ => panic!("Dns protocol not supported"),
            }
        } else {
            panic!("invalid resolver configuration")
        }
    } else {
        NameServerConfigGroup::from_ips_clear(&[rc.remote.ip()], rc.remote.port(), true)
    };

    Resolver::builder_with_config(
        hickory_resolver::config::ResolverConfig::from_parts(None, Vec::new(), protocol),
        TokioConnectionProvider::default(),
    )
    .build()
}

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
            log::error!("Failed to resolve {d}");
            None
        }
    } else if let Ok(record) = r.ipv6_lookup(d).await {
        record.iter().next().map(|aaaa| IpAddr::V6(aaaa.0))
    } else {
        log::error!("Failed to resolve {d}");
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
                log::error!("No host for {domain}");
                Err(VError::NoHost)
            }
        }
        crate::config::ResolvingMode::IPv6 => {
            if let Some(ip) = lookup(resolver, domain, false).await {
                Ok(SocketAddr::new(ip, port))
            } else if let Some(ip) = lookup(resolver, domain, true).await {
                Ok(SocketAddr::new(ip, port))
            } else {
                log::error!("No host for {domain}");
                Err(VError::NoHost)
            }
        }
    }
}
