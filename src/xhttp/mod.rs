mod su;

use std::{net::SocketAddr, sync::Arc};

use crate::resolver::RS;
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
	transport: crate::config::Xhttp,
	outbound: &'static str,
) {
	let builder = h2_builder(&transport);
	let http_head: Arc<(Option<String>, String)> = Arc::new((transport.host, transport.path));
	let su_waiter: su::SuWaiter = Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));
	loop {
		match crate::tls::Tc::new(acceptor.clone(), tcp.accept().await) {
			Ok(tc) => {
				let resolver = resolver.clone();
				let builder = builder.clone();
				let http_head = http_head.clone();
				let su_waiter = su_waiter.clone();
				tokio::spawn(async move {
					match tc.accept().await {
						Ok(tls) => {
							if let Err(e) = handle_h2_conn(tls, resolver, builder, outbound, http_head, su_waiter).await
							{
								log::warn!("{e}");
							}
						}
						Err(e) => {
							log::warn!("tls {e}");
						}
					}
				});
			}
			Err(e) => {
				log::warn!("socket: {e}")
			}
		}
	}
}

async fn handle_h2_conn(
	tls: tokio_rustls::server::TlsStream<tokio::net::TcpStream>,
	resolver: RS,
	builder: h2::server::Builder,
	outbound: &'static str,
	http_head: Arc<(Option<String>, String)>,
	su_waiter: su::SuWaiter,
) -> tokio::io::Result<()> {
	let peer_addr = tls.get_ref().0.peer_addr()?;
	let mut h2c: h2::server::Connection<tokio_rustls::server::TlsStream<tokio::net::TcpStream>, Bytes> =
		builder.handshake(tls).await.map_err(tokio::io::Error::other)?;
	loop {
		match h2c.accept().await {
			Some(stream_res) => {
				let mut stream = stream_res.map_err(tokio::io::Error::other)?;
				let resolver = resolver.clone();
				let http_head = http_head.clone();
				let su_waiter = su_waiter.clone();
				tokio::spawn(async move {
					let mut path_parts: Vec<&str> = stream.0.uri().path().split("/").collect();
					if let Some(last) = path_parts.last()
						&& last.len() == 36
						&& let Ok(su_uuid) = uuid::Uuid::parse_str(last)
					{
						// stream up
						path_parts.pop();
						let path = path_parts.join("/") + "/";
						if http_head_match(&path, stream.0.headers(), http_head) {
							log::warn!("http path/host error");
							stream.1.send_reset(h2::Reason::REFUSED_STREAM);
							return;
						}

						let mut su_waiter_locked = su_waiter.lock().await;
						let stat = su_waiter_locked.remove(&su_uuid);
						if let Some(waiter) = stat {
							// send stream
							let _ = waiter.send(stream).await;
						} else {
							let (chan_send, chan_recv) = tokio::sync::mpsc::channel(1);
							let _ = su_waiter_locked.insert(su_uuid, chan_send);
							drop(su_waiter_locked);
							if let Err(e) =
								su::stream_up(su_uuid, su_waiter, stream, chan_recv, resolver, outbound, peer_addr)
									.await
							{
								log::warn!("{e}")
							}
						}
					} else {
						// stream one
						let path = path_parts.join("/");
						if http_head_match(&path, stream.0.headers(), http_head) {
							log::warn!("http path/host error");
							stream.1.send_reset(h2::Reason::REFUSED_STREAM);
							return;
						}

						if let Err(e) = stream_one(stream, resolver, outbound, peer_addr).await {
							log::warn!("{e}")
						}
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
	outbound: &'static str,
	peer_addr: SocketAddr,
) -> tokio::io::Result<()> {
	let res = {
		let mut payload = Vec::new();
		stream_recv_timeout(req.body_mut(), &mut payload).await?;
		if payload.len() < 19 {
			// minimum is mux header
			stream_recv_timeout(req.body_mut(), &mut payload).await?;
		} else {
			// try to read again if exist to complete
			try_stream_recv(req.body_mut(), &mut payload).await?;
		}

		let vless = crate::vless::Vless::new(&payload, &resolver).await?;
		if crate::auth::authenticate(&vless, peer_addr) {
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
			crate::vless::RequestCommand::TCP => crate::handle_tcp(vless, payload.to_vec(), &mut h2t, outbound).await,
			crate::vless::RequestCommand::UDP => crate::handle_udp(vless, payload.to_vec(), &mut h2t, outbound).await,
			crate::vless::RequestCommand::MUX => {
				crate::mux::xudp(&mut h2t, payload.to_vec(), resolver, outbound, peer_addr.ip()).await
			}
		}
	};
	resp.send_reset(h2::Reason::CANCEL);
	res
}

// utils

async fn stream_recv_timeout(rs: &mut h2::RecvStream, vec: &mut Vec<u8>) -> tokio::io::Result<()> {
	let data = tokio::time::timeout(std::time::Duration::from_secs(5), rs.data())
		.await?
		.ok_or(tokio::io::Error::new(
			std::io::ErrorKind::ConnectionAborted,
			"h2 stream read closed",
		))?
		.map_err(tokio::io::Error::other)?;

	vec.extend_from_slice(&data);
	rs.flow_control()
		.release_capacity(data.len())
		.map_err(tokio::io::Error::other)
}

async fn try_stream_recv(rs: &mut h2::RecvStream, vec: &mut Vec<u8>) -> tokio::io::Result<()> {
	std::future::poll_fn(|cx| match rs.poll_data(cx) {
		std::task::Poll::Ready(Some(Ok(data))) => {
			vec.extend_from_slice(&data);
			rs.flow_control()
				.release_capacity(data.len())
				.map_err(tokio::io::Error::other)?;
			std::task::Poll::Ready(Ok(()))
		}
		_ => std::task::Poll::Ready(Ok(())),
	})
	.await
}

fn http_head_match(mut path: &str, headers: &http::HeaderMap, http_head: Arc<(Option<String>, String)>) -> bool {
	if path.is_empty() {
		path = "/";
	}

	if http_head.1 == path {
		if let Some(host) = &http_head.0 {
			if let Some(value) = headers.get("host")
				&& let Ok(req_host) = value.to_str()
			{
				host != req_host
			} else if let Some(value) = headers.get("referer")
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

impl AsyncRead for H2t {
	fn poll_read(
		mut self: std::pin::Pin<&mut Self>,
		cx: &mut std::task::Context<'_>,
		buf: &mut tokio::io::ReadBuf<'_>,
	) -> std::task::Poll<std::io::Result<()>> {
		let this = &mut *self;
		match std::task::ready!(this.stream.0.poll_data(cx)) {
			None => std::task::Poll::Ready(Err(tokio::io::Error::new(
				std::io::ErrorKind::ConnectionAborted,
				"h2 stream read closed",
			))),
			Some(data_res) => {
				let data = data_res.map_err(tokio::io::Error::other)?;
				buf.put_slice(&data);
				std::task::Poll::Ready(
					this.stream
						.0
						.flow_control()
						.release_capacity(data.len())
						.map_err(tokio::io::Error::other),
				)
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
