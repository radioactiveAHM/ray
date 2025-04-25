use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

use tokio::net::{TcpSocket, TcpStream};

pub struct TcpWriterGeneric<W> {
    pub hr: tokio::io::WriteHalf<W>,
    pub signal: tokio::sync::mpsc::Sender<()>,
}
impl<W> tokio::io::AsyncWrite for TcpWriterGeneric<W>
where
    W: tokio::io::AsyncWrite + Unpin + Send,
{
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<Result<usize, std::io::Error>> {
        let _ = self.signal.try_send(());
        std::pin::Pin::new(&mut self.hr).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        std::pin::Pin::new(&mut self.hr).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        std::pin::Pin::new(&mut self.hr).poll_shutdown(cx)
    }
}

pub fn tcpsocket(a: SocketAddr) -> tokio::io::Result<TcpSocket> {
    let socket = if a.is_ipv4() {
        tokio::net::TcpSocket::new_v4()?
    } else {
        tokio::net::TcpSocket::new_v6()?
    };

    socket.bind(a)?;

    let options = crate::tso();

    if let Some(sbs) = options.send_buffer_size {
        socket.set_send_buffer_size(sbs)?;
    }
    if let Some(rbs) = options.recv_buffer_size {
        socket.set_recv_buffer_size(rbs)?;
    }
    socket.set_nodelay(options.nodelay)?;
    socket.set_keepalive(options.keepalive)?;

    Ok(socket)
}

pub async fn stream(a: SocketAddr) -> tokio::io::Result<TcpStream> {
    if a.is_ipv4() {
        Ok(
            tcpsocket(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)))?
                .connect(a)
                .await?,
        )
    } else {
        Ok(tcpsocket(SocketAddr::V6(SocketAddrV6::new(
            Ipv6Addr::UNSPECIFIED,
            0,
            0,
            0,
        )))?
        .connect(a)
        .await?)
    }
}
