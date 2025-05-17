pub fn containing(bl: &Vec<crate::config::BlackList>, domain: &str) -> Result<(), crate::verror::VError> {
    for list in bl {
        if list.domains.iter().any(|blackdomain| blackdomain.as_str()==domain) {
            if crate::log() {
                println!("Domain {} in {} blacklist blocked", domain, list.name);
            }
            return Err(crate::verror::VError::DomainInBlacklist);
        }
    }
    Ok(())
}