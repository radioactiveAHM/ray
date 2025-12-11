use crate::utils::{self, convert_two_u8s_to_u16_be};
use tokio::io::AsyncWrite;

pub struct UdpWriter<'a> {
	pub udp: &'a tokio::net::UdpSocket,
	pub b: utils::DeqBuffer,
}
impl AsyncWrite for UdpWriter<'_> {
	#[inline(always)]
	fn poll_write(
		mut self: std::pin::Pin<&mut Self>,
		cx: &mut std::task::Context<'_>,
		buf: &[u8],
	) -> std::task::Poll<Result<usize, std::io::Error>> {
		if buf.is_empty() {
			return std::task::Poll::Ready(Ok(0));
		}

		self.b.write(buf);

		// we don't want to blow up the memory
		if self.b.slice().len() > 1024 * 16 {
			return std::task::Poll::Ready(Err(crate::verror::VError::BufferOverflow.into()));
		}

		let mut deadloop = 0u8;
		loop {
			if deadloop == 20 {
				return std::task::Poll::Ready(Err(crate::verror::VError::UdpDeadLoop.into()));
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
				return std::task::Poll::Ready(Err(crate::verror::VError::MailFormedUdpPacket.into()));
			}

			if psize <= size - 2 {
				// we have bytes to send
				let packet = &self.b.slice()[2..psize + 2];
				match self.udp.poll_send(cx, packet) {
					std::task::Poll::Pending => continue,
					std::task::Poll::Ready(Ok(_)) => {
						self.b.remove(psize + 2);
						continue;
					}
					std::task::Poll::Ready(Err(e)) => {
						return std::task::Poll::Ready(Err(e));
					}
				}
			} else {
				// empty or incomplete bytes
				break;
			}
		}

		std::task::Poll::Ready(Ok(buf.len()))
	}

	#[inline(always)]
	fn poll_shutdown(
		self: std::pin::Pin<&mut Self>,
		_cx: &mut std::task::Context<'_>,
	) -> std::task::Poll<Result<(), std::io::Error>> {
		std::task::Poll::Ready(Ok(()))
	}

	#[inline(always)]
	fn poll_flush(
		self: std::pin::Pin<&mut Self>,
		_cx: &mut std::task::Context<'_>,
	) -> std::task::Poll<Result<(), std::io::Error>> {
		std::task::Poll::Ready(Ok(()))
	}
}

#[inline(always)]
pub async fn udp_socket(
	serve_addrs: std::net::SocketAddr,
	_sockopt: crate::config::SockOpt,
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
