use std::{net::SocketAddr, sync::Arc};

use hickory_resolver::{Resolver, config::NameServerConfigGroup, name_server::TokioConnectionProvider};

use crate::verror::VError;

pub type RS = Arc<
	Resolver<hickory_resolver::name_server::GenericConnector<hickory_resolver::proto::runtime::TokioRuntimeProvider>>,
>;

pub fn generate_resolver(rc: &crate::config::Resolver) -> RS {
	let protocol = if let Some(addr) = &rc.resolver {
		let mut parts = addr.split("://");
		if let Some(scheme) = parts.next()
			&& let Some(domain) = parts.next()
		{
			match scheme {
				"https" => NameServerConfigGroup::from_ips_https(
					&rc.ips,
					rc.port,
					domain.to_string(),
					rc.trust_negative_responses,
				),
				"h3" => NameServerConfigGroup::from_ips_h3(
					&rc.ips,
					rc.port,
					domain.to_string(),
					rc.trust_negative_responses,
				),
				"quic" => NameServerConfigGroup::from_ips_quic(
					&rc.ips,
					rc.port,
					domain.to_string(),
					rc.trust_negative_responses,
				),
				"tls" => NameServerConfigGroup::from_ips_tls(
					&rc.ips,
					rc.port,
					domain.to_string(),
					rc.trust_negative_responses,
				),
				"udp" => NameServerConfigGroup::from_ips_clear(&rc.ips, rc.port, rc.trust_negative_responses),
				_ => panic!("Dns protocol not supported"),
			}
		} else {
			panic!("invalid resolver configuration")
		}
	} else {
		NameServerConfigGroup::from_ips_clear(&rc.ips, rc.port, rc.trust_negative_responses)
	};

	let mut options = hickory_resolver::config::ResolverOpts::default();
	options.cache_size = rc.cache_size;
	options.ip_strategy = rc.ip_strategy.convert();
	options.timeout = std::time::Duration::from_secs(rc.timeout);
	options.num_concurrent_reqs = rc.num_concurrent_reqs;

	Arc::new(
		Resolver::builder_with_config(
			hickory_resolver::config::ResolverConfig::from_parts(None, Vec::new(), protocol),
			TokioConnectionProvider::default(),
		)
		.with_options(options)
		.build(),
	)
}

#[inline(always)]
pub async fn resolve(
	resolver: &Resolver<
		hickory_resolver::name_server::GenericConnector<hickory_resolver::proto::runtime::TokioRuntimeProvider>,
	>,
	domain: &str,
	port: u16,
) -> Result<SocketAddr, VError> {
	match resolver.lookup_ip(domain).await {
		Ok(lookup) => {
			if let Some(ip) = lookup.iter().next() {
				Ok(SocketAddr::new(ip, port))
			} else {
				Err(VError::NoHost)
			}
		}
		Err(e) => Err(VError::ResolveDnsFailed(e)),
	}
}
