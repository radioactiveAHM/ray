use crate::utils::{self, convert_two_u8s_to_u16_be};
pub struct UdpWriter<'a> {
	pub udp: &'a tokio::net::UdpSocket,
	pub b: utils::DeqBuffer,
}
impl<'a> UdpWriter<'a> {
	#[inline(always)]
	pub async fn send_packets(&mut self, buf: &[u8]) -> tokio::io::Result<()> {
		if buf.is_empty() {
			return Ok(());
		}
		self.b.write(buf);

		let mut deadloop = 0u8;
		loop {
			if deadloop == 20 {
				return Err(crate::verror::VError::UdpDeadLoop.into());
			}
			deadloop += 1;

			let buf = self.b.slice();
			let size = buf.len();
			// must be at least 3 bytes which 0 and 1 are len
			if size < 3 {
				break;
			}
			let psize = convert_two_u8s_to_u16_be([buf[0], buf[1]]) as usize;

			// len must not be 0
			if psize == 0 {
				return Err(crate::verror::VError::MailFormedUdpPacket.into());
			}

			if psize <= size - 2 {
				// we have bytes to send
				let packet = &self.b.slice()[2..psize + 2];
				self.udp.send(packet).await?;
				self.b.remove(psize + 2);
			} else {
				// empty or incomplete bytes
				break;
			}
		}
		Ok(())
	}
}

#[inline(always)]
pub async fn udp_socket(
	serve_addrs: std::net::SocketAddr,
	_sockopt: &crate::config::Opt,
) -> tokio::io::Result<tokio::net::UdpSocket> {
	let ipversion = if serve_addrs.is_ipv4() {
		socket2::Domain::IPV4
	} else {
		socket2::Domain::IPV6
	};

	let socket: socket2::Socket = socket2::Socket::new(ipversion, socket2::Type::DGRAM, Some(socket2::Protocol::UDP))?;

	// Set Nonblocking
	if let Err(e) = socket.set_nonblocking(true) {
		log::warn!("UDP set nonblocking: {e}")
	}

	#[cfg(target_os = "linux")]
	{
		if _sockopt.bind_to_device {
			if let Some(interface) = &_sockopt.interface {
				if set_udp_bind_device(&socket, &interface).is_err() {
					log::warn!("Failed to set bind to device")
				};
			}
		}
	}

	socket.bind(&serve_addrs.into())?;

	tokio::net::UdpSocket::from_std(socket.into())
}

#[cfg(target_os = "linux")]
pub fn set_udp_bind_device(socket: &socket2::Socket, device: &str) -> Result<(), ()> {
	if let Ok(device) = std::ffi::CString::new(device) {
		let fd = std::os::unix::io::AsRawFd::as_raw_fd(socket);

		let result = unsafe {
			libc::setsockopt(
				fd,
				libc::SOL_SOCKET,
				libc::SO_BINDTODEVICE,
				device.as_ptr() as *const _,
				device.to_bytes().len() as libc::socklen_t,
			)
		};
		if result == -1 {
			return Err(());
		}
		Ok(())
	} else {
		Err(())
	}
}
