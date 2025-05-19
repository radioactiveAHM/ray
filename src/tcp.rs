use std::{
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6},
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

        let _ = socket.set_nodelay(true);
        let _ = socket.set_keepalive(true);
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
pub async fn stream(a: SocketAddr) -> tokio::io::Result<TcpStream> {
    if a.is_ipv4() {
        Ok(tcpsocket(
            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)),
            a.port() == 53 || a.port() == 853,
        )?
        .connect(a)
        .await?)
    } else {
        Ok(tcpsocket(
            SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, 0, 0, 0)),
            a.port() == 53 || a.port() == 853,
        )?
        .connect(a)
        .await?)
    }
}
