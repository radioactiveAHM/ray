use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    pin::Pin,
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
pub fn tcpsocket(a: SocketAddr, minimize: bool) -> tokio::io::Result<TcpSocket> {
    let socket = if a.is_ipv4() {
        tokio::net::TcpSocket::new_v4()?
    } else {
        tokio::net::TcpSocket::new_v6()?
    };

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
pub async fn stream(a: SocketAddr, interface: Option<String>) -> tokio::io::Result<TcpStream> {
    let ip = if let Some(interface) = interface {
        get_interface(a.is_ipv4(), interface)
    } else {
        if a.is_ipv4() {
            IpAddr::V4(Ipv4Addr::UNSPECIFIED)
        } else {
            IpAddr::V6(Ipv6Addr::UNSPECIFIED)
        }
    };
    
    Ok(tcpsocket(
        SocketAddr::new(ip, 0),
        a.port() == 53 || a.port() == 853,
    )?
    .connect(a)
    .await?)
}

#[inline(always)]
pub fn get_interface(ipv4: bool, interface: String) -> IpAddr {
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
                "interface {} not found or interface does not provide IPv6", &interface
            );
        }
        // fallback
        if ipv4 {
            return IpAddr::V4(Ipv4Addr::UNSPECIFIED);
        } else {
            return IpAddr::V6(Ipv6Addr::UNSPECIFIED);
        }
    }

    match ip.unwrap().1 {
        IpAddr::V4(ip) => IpAddr::V4(ip),
        IpAddr::V6(ip) => IpAddr::V6(ip)
    }
}