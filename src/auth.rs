pub fn authenticate(c: &'static crate::config::Config, vconn: &crate::vless::Vless) -> bool {
    for user in &c.users {
        if let Ok(uuid) = uuid::Uuid::try_parse(&user.uuid) {
            if uuid.as_bytes() == vconn.uuid.as_slice() {
                return false;
            }
        }
    }
    true
}