pub fn get_opt(tag: &str) -> &'static crate::config::Opt {
	if let Some(o) = crate::CONFIG.outbounds.get(tag) {
		&o.opt
	} else {
		// fallback to first outbound
		&crate::CONFIG.outbounds.iter().next().unwrap().1.opt
	}
}

pub fn rules(
	ip: &std::net::IpAddr,
	target_domain: Option<String>,
	default_outbound_tag: &str,
) -> tokio::io::Result<&'static crate::config::Opt> {
	let mut op = crate::config::OP::Allow;
	if let Some(rules) = &crate::CONFIG.rules {
		for rule in rules {
			if let Some(ips) = &rule.ips
				&& ips.contains(ip)
			{
				op = rule.operation.clone();
				break;
			} else if let Some(target_domain) = &target_domain
				&& let Some(domains) = &rule.domains
				&& domains.iter().any(|domain| target_domain.contains(domain.as_str()))
			{
				op = rule.operation.clone();
				break;
			}
		}
	};

	Ok(match op {
		crate::config::OP::Reject => Err(crate::verror::VError::Reject)?,
		crate::config::OP::Allow => {
			// the outbounds must have at least one element
			get_opt(default_outbound_tag)
		}
		crate::config::OP::Outbound(o_tag) => {
			if let Some(outbound) = crate::CONFIG.outbounds.get(o_tag.as_str()) {
				&outbound.opt
			} else {
				// fallback
				get_opt(default_outbound_tag)
			}
		}
	})
}
