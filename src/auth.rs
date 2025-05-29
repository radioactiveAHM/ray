#[inline(always)]
pub fn authenticate(
    c: &'static crate::config::Config,
    vconn: &crate::vless::Vless,
    userip: std::net::SocketAddr,
) -> bool {
    for user in &c.users {
        if let Ok(uuid) = uuid::Uuid::try_parse(&user.uuid) {
            if uuid.as_bytes() == vconn.uuid.as_slice() {
                if crate::log() {
                    if let Some(target) = vconn.target {
                        println!(
                            "User {} connected from {} commanding {} to {}",
                            user.name, userip, vconn.rt, target.0
                        );
                    }else {
                        println!(
                            "User {} connected from {} commanding {}",
                            user.name, userip, vconn.rt
                        );
                    }
                    
                }
                return false;
            }
        }
    }
    true
}
