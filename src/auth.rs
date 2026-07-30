use crate::CONFIG;

pub fn authenticate(vconn: &crate::vless::Vless, userip: std::net::SocketAddr) -> bool {
	if let Some(username) = CONFIG.users.get(&vconn.uuid) {
		if let Some(target) = &vconn.target {
			log::info!(
				"User {} connected from {} commanding {} to {}",
				username,
				userip,
				vconn.rt,
				target.0
			);
		} else {
			log::info!(
				"User {} connected from {} commanding {} to Null",
				username,
				userip,
				vconn.rt
			);
		}
		false
	} else {
		true
	}
}
