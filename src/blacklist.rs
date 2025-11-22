#[inline(always)]
pub fn containing(bl: &Vec<crate::config::BlackList>, domain: &str) -> Result<(), crate::verror::VError> {
	for list in bl {
		if list.domains.iter().any(|blackdomain| domain.contains(blackdomain)) {
			log::info!("Domain {} in {} blacklist blocked", domain, list.name);
			return Err(crate::verror::VError::DomainInBlacklist);
		}
	}
	Ok(())
}
