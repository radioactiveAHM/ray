use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use tokio::net::{TcpSocket, TcpStream};

#[inline(always)]
pub fn tcpsocket(a: SocketAddr, sockopt: &crate::config::SockOpt) -> tokio::io::Result<TcpSocket> {
	let socket: TcpSocket = if a.is_ipv4() {
		tokio::net::TcpSocket::new_v4()?
	} else {
		tokio::net::TcpSocket::new_v6()?
	};

	#[cfg(target_os = "linux")]
	{
		if let Some(mss) = sockopt.mss {
			if tcp_options::set_tcp_mss(&socket, mss).is_err() {
				log::warn!("Failed to set tcp mss");
			};
		}
		if let Some(congestion) = &sockopt.congestion {
			if tcp_options::set_tcp_congestion(&socket, congestion).is_err() {
				log::warn!("Failed to set tcp congestion");
			};
		}
		if sockopt.bind_to_device {
			if let Some(interface) = &sockopt.interface {
				if tcp_options::set_tcp_bind_device(&socket, &interface).is_err() {
					log::warn!("Failed to set bind to device");
				};
			}
		}
	}

	if let Some(sbs) = sockopt.send_buffer_size {
		socket.set_send_buffer_size(sbs)?;
	}
	if let Some(rbs) = sockopt.recv_buffer_size {
		socket.set_recv_buffer_size(rbs)?;
	}
	if let Some(nodelay) = sockopt.nodelay {
		socket.set_nodelay(nodelay)?;
	}
	if let Some(keepalive) = sockopt.keepalive {
		socket.set_keepalive(keepalive)?;
	}

	socket.bind(a)?;

	Ok(socket)
}

#[inline(always)]
pub async fn stream(a: SocketAddr, sockopt: &crate::config::SockOpt) -> tokio::io::Result<TcpStream> {
	let ip = if a.is_ipv4() {
		IpAddr::V4(Ipv4Addr::UNSPECIFIED)
	} else {
		IpAddr::V6(Ipv6Addr::UNSPECIFIED)
	};

	tcpsocket(SocketAddr::new(ip, 0), sockopt)?.connect(a).await
}

#[cfg(target_os = "linux")]
pub mod tcp_options {
	pub fn set_tcp_mss(socket: &tokio::net::TcpSocket, mss: i32) -> Result<(), ()> {
		let fd = std::os::unix::io::AsRawFd::as_raw_fd(socket);

		let result = unsafe {
			libc::setsockopt(
				fd,
				libc::IPPROTO_TCP,
				libc::TCP_MAXSEG,
				&mss as *const i32 as *const libc::c_void,
				std::mem::size_of::<i32>() as libc::socklen_t,
			)
		};
		if result == -1 {
			return Err(());
		}
		Ok(())
	}
	pub fn set_tcp_congestion(socket: &tokio::net::TcpSocket, congestion: &str) -> Result<(), ()> {
		if let Ok(c) = std::ffi::CString::new(congestion) {
			let fd = std::os::unix::io::AsRawFd::as_raw_fd(socket);

			let result = unsafe {
				libc::setsockopt(
					fd,
					libc::IPPROTO_TCP,
					libc::TCP_CONGESTION,
					c.as_ptr() as *const _,
					c.to_bytes().len() as libc::socklen_t,
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
	pub fn set_tcp_bind_device(socket: &tokio::net::TcpSocket, device: &str) -> Result<(), ()> {
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
}
