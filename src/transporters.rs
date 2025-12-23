use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};

pub async fn httpupgrade_transporter<S>(
	chttp: &crate::config::Http,
	buff: &[u8],
	stream: &mut S,
) -> tokio::io::Result<()>
where
	S: AsyncRead + AsyncWrite + Unpin,
{
	if let Ok(http) = core::str::from_utf8(buff) {
		if let Some(host) = &chttp.host
			&& !http.contains(host.as_str())
		{
			return Err(crate::verror::VError::TransporterError.into());
		}

		if let Some(head) = http.lines().next() {
			if head != format!("{} {} HTTP/1.1", chttp.method, chttp.path) {
				stream
					.write_all(b"HTTP/1.1 404 Not Found\r\nconnection: close\r\n\r\n")
					.await?;
				return Err(crate::verror::VError::TransporterError.into());
			}
		} else {
			return Err(crate::verror::VError::TransporterError.into());
		}
	} else {
		return Err(crate::verror::VError::UTF8Err.into());
	}

	stream
		.write_all(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n")
		.await
}

pub async fn http_transporter<S>(chttp: &crate::config::Http, buff: &[u8], stream: &mut S) -> tokio::io::Result<()>
where
	S: AsyncRead + AsyncWrite + Unpin,
{
	if let Ok(http) = core::str::from_utf8(buff) {
		// if there is no host
		// i'm too lazy to parse http headers :D
		if let Some(host) = &chttp.host
			&& !http.contains(host.as_str())
		{
			return Err(crate::verror::VError::TransporterError.into());
		}
		if let Some(head) = http.lines().next() {
			if head != format!("{} {} HTTP/1.1", chttp.method, chttp.path) {
				stream
					.write_all(b"HTTP/1.1 404 Not Found\r\nconnection: close\r\n\r\n")
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

struct Wst<S: AsyncRead + AsyncWrite + Unpin> {
	pub ws: tokio_websockets::WebSocketStream<S>,
	closed: bool,
}

impl<S> AsyncRead for Wst<S>
where
	S: AsyncRead + AsyncWrite + Unpin,
{
	fn poll_read(
		mut self: std::pin::Pin<&mut Self>,
		cx: &mut std::task::Context<'_>,
		buf: &mut tokio::io::ReadBuf<'_>,
	) -> std::task::Poll<std::io::Result<()>> {
		if self.closed {
			return std::task::Poll::Ready(Err(crate::verror::VError::WsClosed.into()));
		}
		match self.ws.poll_next_unpin(cx) {
			std::task::Poll::Pending => std::task::Poll::Pending,
			std::task::Poll::Ready(None) => std::task::Poll::Ready(Err(crate::verror::VError::WsClosed.into())),
			std::task::Poll::Ready(Some(Err(e))) => std::task::Poll::Ready(Err(tokio::io::Error::other(e))),
			std::task::Poll::Ready(Some(Ok(message))) => {
				if message.is_ping() {
					let _ = self.ws.start_send_unpin(tokio_websockets::Message::pong(Vec::new()));
					std::task::Poll::Pending
				} else {
					if message.is_close() {
						self.closed = true;
					}
					if !message.as_payload().is_empty() {
						buf.put_slice(message.as_payload());
						std::task::Poll::Ready(Ok(()))
					} else {
						std::task::Poll::Pending
					}
				}
			}
		}
	}
}

impl<S> AsyncWrite for Wst<S>
where
	S: AsyncRead + AsyncWrite + Unpin,
{
	fn poll_flush(
		mut self: std::pin::Pin<&mut Self>,
		cx: &mut std::task::Context<'_>,
	) -> std::task::Poll<Result<(), std::io::Error>> {
		self.ws.poll_flush_unpin(cx).map_err(tokio::io::Error::other)
	}
	fn poll_shutdown(
		self: std::pin::Pin<&mut Self>,
		_cx: &mut std::task::Context<'_>,
	) -> std::task::Poll<Result<(), std::io::Error>> {
		std::task::Poll::Ready(Ok(()))
	}
	fn poll_write(
		mut self: std::pin::Pin<&mut Self>,
		_cx: &mut std::task::Context<'_>,
		buf: &[u8],
	) -> std::task::Poll<Result<usize, std::io::Error>> {
		match self
			.ws
			.start_send_unpin(tokio_websockets::Message::binary(buf.to_vec()))
		{
			Ok(_) => std::task::Poll::Ready(Ok(buf.len())),
			Err(e) => std::task::Poll::Ready(Err(tokio::io::Error::other(e))),
		}
	}
}

pub async fn websocket_transport<S>(
	mut ws: tokio_websockets::WebSocketStream<S>,
	resolver: crate::resolver::RS,
	peer_addr: std::net::SocketAddr,
	outbound: &'static str,
) -> tokio::io::Result<()>
where
	S: AsyncRead + AsyncWrite + Unpin,
{
	let mut payload = Vec::with_capacity(1024 * 8);
	payload.extend_from_slice(
		ws.next()
			.await
			.ok_or(crate::verror::VError::WsClosed)?
			.map_err(tokio::io::Error::other)?
			.as_payload(),
	);

	if payload.len() < 19 {
		payload.extend_from_slice(
			tokio::time::timeout(std::time::Duration::from_secs(12), ws.next())
				.await?
				.ok_or(crate::verror::VError::WsClosed)?
				.map_err(tokio::io::Error::other)?
				.as_payload(),
		);
	}
	try_recv(&mut ws, &mut payload).await?;

	let vless = crate::vless::Vless::new(&payload, &resolver).await?;
	if crate::auth::authenticate(&vless, peer_addr) {
		return Err(crate::verror::VError::AuthenticationFailed.into());
	}

	let mut wst = Wst { ws, closed: false };
	if let Err(e) = match vless.rt {
		crate::vless::RequestCommand::TCP => crate::handle_tcp(vless, payload, &mut wst, outbound).await,
		crate::vless::RequestCommand::UDP => crate::handle_udp(vless, payload, &mut wst, outbound).await,
		crate::vless::RequestCommand::MUX => {
			crate::mux::xudp(&mut wst, payload, resolver, outbound, peer_addr.ip()).await
		}
	} {
		log::warn!("{peer_addr}: {e}");
	} else {
		log::warn!("{peer_addr}: closed connection");
	}

	let _ = wst
		.ws
		.send(tokio_websockets::Message::close(
			Some(tokio_websockets::CloseCode::NORMAL_CLOSURE),
			"pipe closed",
		))
		.await;
	Ok(())
}

async fn try_recv<S: AsyncRead + AsyncWrite + Unpin>(
	ws: &mut tokio_websockets::WebSocketStream<S>,
	buf: &mut Vec<u8>,
) -> tokio::io::Result<()> {
	std::future::poll_fn(|cx| match ws.poll_next_unpin(cx) {
		std::task::Poll::Pending => std::task::Poll::Ready(Ok(())),
		std::task::Poll::Ready(Some(Ok(message))) => {
			buf.extend_from_slice(message.as_payload());
			std::task::Poll::Ready(Ok(()))
		}
		std::task::Poll::Ready(Some(Err(e))) => std::task::Poll::Ready(Err(tokio::io::Error::other(e))),
		std::task::Poll::Ready(None) => std::task::Poll::Ready(Err(crate::verror::VError::WsClosed.into())),
	})
	.await
}
