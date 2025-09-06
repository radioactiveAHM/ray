#[inline(always)]
pub fn authenticate(vconn: &crate::vless::Vless, userip: std::net::SocketAddr) -> bool {
    for user in &crate::CONFIG.users {
        if let Ok(uuid) = uuid::Uuid::try_parse(&user.uuid)
            && uuid.as_bytes() == vconn.uuid.as_slice()
        {
            if let Some(target) = vconn.target {
                log::info!(
                    "User {} connected from {} commanding {} to {}",
                    user.name,
                    userip,
                    vconn.rt,
                    target.0
                )
            } else {
                log::info!(
                    "User {} connected from {} commanding {}",
                    user.name,
                    userip,
                    vconn.rt
                )
            }
            return false;
        }
    }
    true
}
