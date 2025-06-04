use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    pin::Pin,
    str::FromStr,
};

use tokio::net::{TcpSocket, TcpStream};

pub struct TcpWriterGeneric<'a, W> {
    pub hr: Pin<&'a mut W>,
    pub signal: tokio::sync::mpsc::Sender<()>,
}
impl<W> tokio::io::AsyncWrite for TcpWriterGeneric<'_, W>
where
    W: tokio::io::AsyncWrite + Unpin + Send,
{
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<Result<usize, std::io::Error>> {
        let _ = self.signal.try_send(());
        self.hr.as_mut().poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        self.hr.as_mut().poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        self.hr.as_mut().poll_shutdown(cx)
    }
}

#[inline(always)]
#[allow(unused_variables)]
pub fn tcpsocket(
    a: SocketAddr,
    minimize: bool,
    sockopt: &crate::config::SockOpt,
) -> tokio::io::Result<TcpSocket> {
    let socket: TcpSocket = if a.is_ipv4() {
        tokio::net::TcpSocket::new_v4()?
    } else {
        tokio::net::TcpSocket::new_v6()?
    };

    #[cfg(target_os = "linux")]
    {
        if let Some(mss) = sockopt.mss {
            if tcp_options::set_tcp_mss(&socket, mss).is_err() && crate::log() {
                println!("Failed to set tcp mss");
            };
        }
        if let Some(congestion) = &sockopt.congestion {
            if tcp_options::set_tcp_congestion(&socket, congestion).is_err() && crate::log() {
                println!("Failed to set tcp congestion");
            };
        }
        if sockopt.bind_to_device {
            if let Some(interface) = &sockopt.interface {
                if tcp_options::set_tcp_bind_device(&socket, &interface).is_err() && crate::log() {
                    println!("Failed to set bind to device");
                };
            }
        }
    }

    if minimize {
        // useful if socket is used for dns.
        if socket.set_send_buffer_size(1024 * 4).is_ok() {
            let _ = socket.set_recv_buffer_size(1024 * 4);
        } else if socket.set_send_buffer_size(1024 * 8).is_ok() {
            let _ = socket.set_recv_buffer_size(1024 * 8);
        } else if socket.set_send_buffer_size(1024 * 16).is_ok() {
            let _ = socket.set_recv_buffer_size(1024 * 16);
        }
    } else {
        let options = crate::tso();
        if let Some(sbs) = options.send_buffer_size {
            socket.set_send_buffer_size(sbs)?;
        }
        if let Some(rbs) = options.recv_buffer_size {
            socket.set_recv_buffer_size(rbs)?;
        }
        if let Some(nodelay) = options.nodelay {
            socket.set_nodelay(nodelay)?;
        }
        if let Some(keepalive) = options.keepalive {
            socket.set_nodelay(keepalive)?;
        }
    }

    socket.bind(a)?;

    Ok(socket)
}

#[inline(always)]
pub async fn stream(
    a: SocketAddr,
    sockopt: crate::config::SockOpt,
) -> tokio::io::Result<TcpStream> {
    let ip = if let Some(interface) = &sockopt.interface {
        get_interface(a.is_ipv4(), interface)
    } else if a.is_ipv4() {
        IpAddr::V4(Ipv4Addr::UNSPECIFIED)
    } else {
        IpAddr::V6(Ipv6Addr::UNSPECIFIED)
    };

    tcpsocket(
        SocketAddr::new(ip, 0),
        a.port() == 53 || a.port() == 853,
        &sockopt,
    )?
    .connect(a)
    .await
}

#[inline(always)]
pub fn get_interface(ipv4: bool, interface: &str) -> IpAddr {
    // if user input ip as interface
    if let Ok(ip) = IpAddr::from_str(interface) {
        return ip;
    }
    // Cause panic if it fails, informing the user that the binding interface is not available.
    let interfaces =
        local_ip_address::list_afinet_netifas().expect("binding interface is not available");

    let ip = interfaces.iter().find(|i| {
        if ipv4 {
            i.0.as_str().to_lowercase() == interface.to_lowercase() && i.1.is_ipv4()
        } else {
            i.0.as_str().to_lowercase() == interface.to_lowercase() && i.1.is_ipv6()
        }
    });

    if ip.is_none() {
        if crate::log() {
            println!(
                "interface {} not found or interface does not provide IPv6",
                &interface
            );
        }
        // fallback
        if ipv4 {
            return IpAddr::V4(Ipv4Addr::UNSPECIFIED);
        } else {
            return IpAddr::V6(Ipv6Addr::UNSPECIFIED);
        }
    }

    ip.unwrap().1
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
    pub fn set_udp_bind_device(socket: &tokio::net::UdpSocket, device: &str) -> Result<(), ()> {
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
