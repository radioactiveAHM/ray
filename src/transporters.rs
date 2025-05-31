use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};

pub async fn httpupgrade_transporter<S>(
    chttp: &crate::config::Http,
    buff: &[u8],
    stream: &mut S,
) -> tokio::io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    if let Ok(http) = core::str::from_utf8(buff) {
        // if there is no host
        // i'm too lazy to parse http headers :D
        if let Some(host) = &chttp.host {
            if !http.contains(host.as_str()) {
                return Err(crate::verror::VError::TransporterError.into());
            }
        }

        if let Some(head) = http.lines().next() {
            if head != format!("{} {} HTTP/1.1", chttp.method, chttp.path) {
                let _ = stream
                    .write(b"HTTP/1.1 404 Not Found\r\nconnection: close\r\n\r\n")
                    .await?;
                return Err(crate::verror::VError::TransporterError.into());
            }
        } else {
            return Err(crate::verror::VError::TransporterError.into());
        }
    } else {
        return Err(crate::verror::VError::UTF8Err.into());
    }

    let _ = stream.write(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n").await?;
    Ok(())
}

pub async fn http_transporter<S>(
    chttp: &crate::config::Http,
    buff: &[u8],
    stream: &mut S,
) -> tokio::io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    if let Ok(http) = core::str::from_utf8(buff) {
        // if there is no host
        // i'm too lazy to parse http headers :D
        if let Some(host) = &chttp.host {
            if !http.contains(host.as_str()) {
                return Err(crate::verror::VError::TransporterError.into());
            }
        }
        if let Some(head) = http.lines().next() {
            if head != format!("{} {} HTTP/1.1", chttp.method, chttp.path) {
                let _ = stream
                    .write(b"HTTP/1.1 404 Not Found\r\nconnection: close\r\n\r\n")
                    .await?;
                return Err(crate::verror::VError::TransporterError.into());
            }
        } else {
            return Err(crate::verror::VError::TransporterError.into());
        }
    } else {
        return Err(crate::verror::VError::UTF8Err.into());
    }

    Ok(())
}

struct WST<S>(pub tokio_websockets::WebSocketStream<S>)
where
    S: AsyncRead + crate::PeekWraper + AsyncWrite + Unpin + Send + 'static;

impl<S> AsyncRead for WST<S>
where
    S: AsyncRead + crate::PeekWraper + AsyncWrite + Unpin + Send,
{
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.0.poll_next_unpin(cx) {
            std::task::Poll::Pending => std::task::Poll::Pending,
            std::task::Poll::Ready(None) => std::task::Poll::Pending,
            std::task::Poll::Ready(Some(Err(e))) => {
                std::task::Poll::Ready(Err(tokio::io::Error::other(e)))
            }
            std::task::Poll::Ready(Some(Ok(message))) => {
                buf.put_slice(message.as_payload());
                std::task::Poll::Ready(Ok(()))
            }
        }
    }
}

impl<S> AsyncWrite for WST<S>
where
    S: AsyncRead + crate::PeekWraper + AsyncWrite + Unpin + Send,
{
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        match self.0.poll_flush_unpin(cx) {
            std::task::Poll::Pending => std::task::Poll::Pending,
            std::task::Poll::Ready(Ok(_)) => std::task::Poll::Ready(Ok(())),
            std::task::Poll::Ready(Err(e)) => {
                std::task::Poll::Ready(Err(tokio::io::Error::other(e)))
            }
        }
    }
    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        match self.0.poll_close_unpin(cx) {
            std::task::Poll::Pending => std::task::Poll::Pending,
            std::task::Poll::Ready(Ok(_)) => std::task::Poll::Ready(Ok(())),
            std::task::Poll::Ready(Err(e)) => {
                std::task::Poll::Ready(Err(tokio::io::Error::other(e)))
            }
        }
    }
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<Result<usize, std::io::Error>> {
        let bin_message = tokio_websockets::Message::binary(bytes::Bytes::copy_from_slice(buf));
        match self.0.start_send_unpin(bin_message) {
            Ok(_) => std::task::Poll::Ready(Ok(buf.len())),
            Err(e) => std::task::Poll::Ready(Err(tokio::io::Error::other(e))),
        }
    }
}

impl<S> crate::PeekWraper for WST<S>
where
    S: AsyncRead + crate::PeekWraper + AsyncWrite + Unpin + Send,
{
    async fn peek(&self) -> tokio::io::Result<()> {
        self.0.get_ref().peek().await
    }
}

#[inline(never)]
pub async fn websocket_transport<S>(
    mut ws: tokio_websockets::WebSocketStream<S>,
    config: &'static crate::config::Config,
    resolver: &'static hickory_resolver::Resolver<
        hickory_resolver::name_server::GenericConnector<
            hickory_resolver::proto::runtime::TokioRuntimeProvider,
        >,
    >,
    peer_addr: std::net::SocketAddr,
) -> tokio::io::Result<()>
where
    S: AsyncRead + crate::PeekWraper + AsyncWrite + Unpin + Send + 'static,
{
    let vless: crate::vless::Vless;
    let mut payload = Vec::new();
    if let Some(Ok(vless_header_message)) = ws.next().await {
        vless = crate::vless::Vless::new(
            vless_header_message.as_payload(),
            resolver,
            &config.blacklist,
        )
        .await?;
        if crate::auth::authenticate(config, &vless, peer_addr) {
            return Err(crate::verror::VError::AuthenticationFailed.into());
        }
        payload.extend_from_slice(vless_header_message.as_payload());
    } else {
        return Err(crate::verror::VError::TransporterError.into());
    };

    let wst = WST(ws);
    if let Err(e) = match vless.rt {
        crate::vless::SocketType::TCP => crate::handle_tcp(vless, payload, wst, config).await,
        crate::vless::SocketType::UDP => crate::handle_udp(vless, payload, wst, config).await,
        crate::vless::SocketType::MUX => {
            crate::mux::xudp(
                wst,
                payload,
                resolver,
                &config.blacklist,
                config.udp_proxy_buffer_size.unwrap_or(8),
            )
            .await
        }
    } {
        if crate::log() {
            println!("{peer_addr}: {e}")
        }
    } else if crate::log() {
        println!("{peer_addr}: closed connection")
    }

    Ok(())
}
