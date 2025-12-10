use std::{net::SocketAddr, sync::Arc};

use crate::{CONFIG, resolver::RS};
use bytes::Bytes;
use tokio::io::{AsyncRead, AsyncWrite};

fn h2_builder(c: &crate::config::Xhttp) -> h2::server::Builder {
	let mut h2_builder = h2::server::Builder::new();
	if let Some(initial_connection_window_size) = c.initial_connection_window_size {
		h2_builder.initial_connection_window_size(initial_connection_window_size * 1024);
	}
	if let Some(initial_window_size) = c.initial_window_size {
		h2_builder.initial_window_size(initial_window_size * 1024);
	}
	if let Some(max_send_buffer_size) = c.max_send_buffer_size {
		h2_builder.max_send_buffer_size(max_send_buffer_size * 1024);
	}
	h2_builder.max_frame_size(c.max_frame_size * 1024);

	h2_builder
}

pub async fn xhttp_server(
	resolver: RS,
	tcp: tokio::net::TcpListener,
	acceptor: tokio_rustls::TlsAcceptor,
	buffer_limit: Option<usize>,
	transport: crate::config::Xhttp,
	sockopt: crate::config::SockOpt,
) {
	let builder = h2_builder(&transport);
	let http_head: Arc<(Option<String>, String)> = Arc::new((transport.host, transport.path));
	loop {
		match crate::tls::Tc::new(acceptor.clone(), tcp.accept().await, buffer_limit) {
			Ok(tc) => {
				let resolver = resolver.clone();
				let builder = builder.clone();
				let sockopt = sockopt.clone();
				let http_head = http_head.clone();
				tokio::spawn(async move {
					match tc.accept().await {
						Ok(tls) => {
							if let Err(e) = handle_h2_conn(tls, resolver, builder, sockopt, http_head).await {
								log::warn!("{e}");
							}
						}
						Err(e) => {
							log::warn!("{e}");
						}
					}
				});
			}
			Err(e) => {
				log::warn!("TLS: {e}")
			}
		}
	}
}

async fn handle_h2_conn(
	tls: tokio_rustls::server::TlsStream<tokio::net::TcpStream>,
	resolver: RS,
	builder: h2::server::Builder,
	sockopt: crate::config::SockOpt,
	http_head: Arc<(Option<String>, String)>,
) -> tokio::io::Result<()> {
	let peer_addr = tls.get_ref().0.peer_addr()?;
	let mut h2c = builder.handshake(tls).await.map_err(tokio::io::Error::other)?;
	loop {
		match h2c.accept().await {
			Some(stream_res) => {
				let stream = stream_res.map_err(tokio::io::Error::other)?;
				let resolver = resolver.clone();
				let sockopt = sockopt.clone();
				let http_head = http_head.clone();
				tokio::spawn(async move {
					if let Err(e) = stream_one(stream, resolver, sockopt, http_head, peer_addr).await {
						log::warn!("{e}")
					}
				});
			}
			None => {
				return Err(tokio::io::Error::new(
					std::io::ErrorKind::ConnectionAborted,
					"h2 socket closed",
				));
			}
		}
	}
}

async fn stream_one(
	(mut req, mut resp): (http::Request<h2::RecvStream>, h2::server::SendResponse<bytes::Bytes>),
	resolver: RS,
	sockopt: crate::config::SockOpt,
	http_head: Arc<(Option<String>, String)>,
	peer_addr: SocketAddr,
) -> tokio::io::Result<()> {
	if http_head_match(&req, http_head) {
		resp.send_reset(h2::Reason::REFUSED_STREAM);
		return Err(crate::verror::VError::TransporterError.into());
	}
	let payload = if let Some(data_res) = req.body_mut().data().await {
		data_res.map_err(tokio::io::Error::other)?
	} else {
		return Err(tokio::io::Error::new(
			std::io::ErrorKind::ConnectionAborted,
			"h2 stream read closed",
		));
	};

	let vless = crate::vless::Vless::new(&payload, &resolver).await?;
	if crate::auth::authenticate(&vless, peer_addr) {
		resp.send_reset(h2::Reason::REFUSED_STREAM);
		return Err(crate::verror::VError::AuthenticationFailed.into());
	}

	let http_resp = http::Response::builder()
		.header("X-Accel-Buffering", "no")
		.header("Cache-Control", "no-store")
		.header("Content-Type", "text/event-stream")
		.version(http::Version::HTTP_2)
		.status(http::StatusCode::OK)
		.body(())
		.unwrap();
	let w = resp.send_response(http_resp, false).map_err(tokio::io::Error::other)?;
	let mut h2t = H2t {
		stream: (req.into_body(), w),
	};
	match vless.rt {
		crate::vless::RequestCommand::TCP => crate::handle_tcp(vless, payload.to_vec(), &mut h2t, sockopt, false).await,
		crate::vless::RequestCommand::UDP => crate::handle_udp(vless, payload.to_vec(), &mut h2t, sockopt, false).await,
		crate::vless::RequestCommand::MUX => {
			crate::mux::xudp(
				&mut h2t,
				payload.to_vec(),
				resolver,
				CONFIG.udp_proxy_buffer_size.unwrap_or(8),
				sockopt,
				peer_addr.ip(),
			)
			.await
		}
	}
}

//

fn http_head_match(req: &http::Request<h2::RecvStream>, http_head: Arc<(Option<String>, String)>) -> bool {
	if req.uri().path() == http_head.1 {
		if let Some(host) = &http_head.0 {
			if let Some(value) = req.headers().get("host")
				&& let Ok(req_host) = value.to_str()
			{
				host != req_host
			} else if let Some(value) = req.headers().get("referer")
				&& let Ok(req_referer) = value.to_str()
			{
				!req_referer.contains(host)
			} else {
				true
			}
		} else {
			false
		}
	} else {
		true
	}
}

// utils
struct H2t {
	stream: (h2::RecvStream, h2::SendStream<bytes::Bytes>),
}

impl Drop for H2t {
	fn drop(&mut self) {
		self.stream.1.send_reset(h2::Reason::CANCEL);
	}
}

impl AsyncRead for H2t {
	fn poll_read(
		mut self: std::pin::Pin<&mut Self>,
		cx: &mut std::task::Context<'_>,
		buf: &mut tokio::io::ReadBuf<'_>,
	) -> std::task::Poll<std::io::Result<()>> {
		let this = &mut *self;
		if this.stream.0.flow_control().available_capacity() == 0 {
			let used = this.stream.0.flow_control().used_capacity();
			this.stream
				.0
				.flow_control()
				.release_capacity(used)
				.map_err(tokio::io::Error::other)?;
		}
		match std::task::ready!(this.stream.0.poll_data(cx)) {
			None => std::task::Poll::Ready(Err(tokio::io::Error::new(
				std::io::ErrorKind::ConnectionAborted,
				"h2 stream read closed",
			))),
			Some(data_res) => {
				let data = data_res.map_err(tokio::io::Error::other)?;
				buf.put_slice(&data);
				std::task::Poll::Ready(Ok(()))
			}
		}
	}
}

impl AsyncWrite for H2t {
	fn poll_write(
		mut self: std::pin::Pin<&mut Self>,
		cx: &mut std::task::Context<'_>,
		buf: &[u8],
	) -> std::task::Poll<Result<usize, std::io::Error>> {
		let this = &mut *self;
		let buflen = buf.len();

		this.stream.1.reserve_capacity(buflen);

		if this.stream.1.capacity() >= buflen {
			this.stream
				.1
				.send_data(Bytes::copy_from_slice(buf), false)
				.map_err(tokio::io::Error::other)?;
			return std::task::Poll::Ready(Ok(buflen));
		}

		match std::task::ready!(this.stream.1.poll_capacity(cx)) {
			None => std::task::Poll::Ready(Err(tokio::io::Error::new(
				std::io::ErrorKind::ConnectionAborted,
				"h2 stream closed",
			))),
			Some(Ok(available)) => {
				if available == 0 {
					return std::task::Poll::Pending;
				}

				let to_send = std::cmp::min(available, buflen);
				let chunk = &buf[..to_send];

				this.stream
					.1
					.send_data(Bytes::copy_from_slice(chunk), false)
					.map_err(tokio::io::Error::other)?;

				std::task::Poll::Ready(Ok(to_send))
			}
			Some(Err(e)) => std::task::Poll::Ready(Err(tokio::io::Error::other(e))),
		}
	}

	fn poll_flush(
		self: std::pin::Pin<&mut Self>,
		_cx: &mut std::task::Context<'_>,
	) -> std::task::Poll<Result<(), std::io::Error>> {
		std::task::Poll::Ready(Ok(()))
	}

	fn poll_shutdown(
		mut self: std::pin::Pin<&mut Self>,
		_cx: &mut std::task::Context<'_>,
	) -> std::task::Poll<Result<(), std::io::Error>> {
		self.stream.1.send_reset(h2::Reason::CANCEL);
		std::task::Poll::Ready(Ok(()))
	}
}
