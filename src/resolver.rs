use std::{net::SocketAddr, sync::Arc};

use hickory_resolver::{Resolver, config::NameServerConfig, net::runtime::TokioRuntimeProvider};

use crate::verror::VError;

pub type RS = Arc<Resolver<TokioRuntimeProvider>>;

pub fn generate_resolver(rc: &crate::config::Resolver) -> RS {
	let servers = rc
		.servers
		.iter()
		.map(|server| {
			if let Some(addr) = &server.proto {
				let mut parts = addr.split("://");
				if let Some(scheme) = parts.next()
					&& let Some(domain) = parts.next()
				{
					match scheme {
						"https" => NameServerConfig::https(server.ip, domain.into(), None),
						"h3" => NameServerConfig::h3(server.ip, domain.into(), None),
						"quic" => NameServerConfig::quic(server.ip, domain.into()),
						"tls" => NameServerConfig::tls(server.ip, domain.into()),
						"udp" => NameServerConfig::udp(server.ip),
						_ => panic!("Dns protocol not supported"),
					}
				} else {
					panic!("invalid resolver configuration")
				}
			} else {
				NameServerConfig::udp(server.ip)
			}
		})
		.collect();

	let mut options = hickory_resolver::config::ResolverOpts::default();
	options.cache_size = rc.cache_size;
	options.ip_strategy = rc.ip_strategy.convert();
	options.timeout = std::time::Duration::from_secs(rc.timeout);
	options.num_concurrent_reqs = rc.num_concurrent_reqs;

	Arc::new(
		hickory_resolver::TokioResolver::builder_with_config(
			hickory_resolver::config::ResolverConfig::from_parts(None, Vec::new(), servers),
			TokioRuntimeProvider::default(),
		)
		.with_options(options)
		.build()
		.unwrap(),
	)
}

#[inline(always)]
pub async fn resolve(resolver: &Resolver<TokioRuntimeProvider>, domain: &str, port: u16) -> Result<SocketAddr, VError> {
	match resolver.lookup_ip(domain).await {
		Ok(lookup) => {
			if let Some(ip) = lookup.iter().next() {
				Ok(SocketAddr::new(ip, port))
			} else {
				Err(VError::NoHost)
			}
		}
		Err(_) => Err(VError::ResolveDnsFailed),
	}
}
