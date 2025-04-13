use std::net::SocketAddr;

use hickory_resolver::{
    Resolver, config::NameServerConfigGroup, name_server::TokioConnectionProvider,
};

use crate::verror::VError;

pub async fn resolve(domain: &str, port: u16) -> Result<SocketAddr, VError> {
    let config = crate::resolver_config();
    let c = hickory_resolver::config::ResolverConfig::from_parts(
        None,
        Vec::new(),
        NameServerConfigGroup::from_ips_clear(&[config.addr.ip()], config.addr.port(), true),
    );
    let resolver = Resolver::builder_with_config(c, TokioConnectionProvider::default()).build();

    if let Ok(resolved) = resolver.lookup_ip(domain).await {
        let lookingup = match config.mode {
            crate::config::ResolvingMode::IPv4 => {
                let v4 = resolved.iter().find(|ip| ip.is_ipv4());
                if v4.is_none() {
                    resolved.iter().next()
                } else {
                    v4
                }
            }
            crate::config::ResolvingMode::IPv6 => {
                let v6 = resolved.iter().find(|ip| ip.is_ipv6());
                if v6.is_none() {
                    resolved.iter().next()
                } else {
                    v6
                }
            }
        };

        if let Some(ip4) = lookingup {
            Ok(SocketAddr::new(ip4, port))
        } else {
            if crate::log() {
                println!("No host for {domain}");
            }
            Err(VError::NoHost)
        }
    } else {
        if crate::log() {
            println!("Failed to resolve {domain}");
        }
        Err(VError::ResolveDnsFailed)
    }
}
