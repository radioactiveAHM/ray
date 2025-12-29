mod pu;
mod su;

use std::{net::SocketAddr, sync::Arc};

use crate::resolver::RS;
use bytes::Bytes;
use tokio::io::{AsyncRead, AsyncWrite};

#[inline(always)]
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
	transport: &'static crate::config::Xhttp,
	outbound: &'static str,
) {
	let builder = h2_builder(transport);
	let waiters: Waiters = Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));
	let pc: PuConns = Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));
	loop {
		match crate::tls::Tc::new(acceptor.clone(), tcp.accept().await) {
			Ok(tc) => {
				let resolver = resolver.clone();
				let builder = builder.clone();
				let waiters = waiters.clone();
				let pc = pc.clone();
				tokio::spawn(async move {
					match tc.accept().await {
						Ok(tls) => {
							if let Err(e) =
								handle_h2_conn(transport, tls, outbound, resolver, builder, waiters, pc).await
							{
								log::warn!("h2: {e}");
							}
						}
						Err(e) => {
							log::warn!("tls: {e}");
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

#[allow(clippy::too_many_arguments)]
async fn handle_h2_conn(
	transport: &'static crate::config::Xhttp,
	tls: tokio_rustls::server::TlsStream<tokio::net::TcpStream>,
	outbound: &'static str,
	resolver: RS,
	builder: h2::server::Builder,
	waiters: Waiters,
	pc: PuConns,
) -> tokio::io::Result<()> {
	let peer_addr = tls.get_ref().0.peer_addr()?;
	let mut h2c: h2::server::Connection<tokio_rustls::server::TlsStream<tokio::net::TcpStream>, Bytes> =
		builder.handshake(tls).await.map_err(tokio::io::Error::other)?;
	let pu_get_streams = Arc::new(tokio::sync::Mutex::new(Vec::with_capacity(8)));

	// close all packet-up GET streams related to this h2 connection.
	let clean_up = async {
		for pu_get_streams_uuid in pu_get_streams.lock().await.iter() {
			// pu_con_res: avoid holding the lock
			let pu_con_res = pc.lock().await.get(pu_get_streams_uuid).cloned();
			if let Some(pu_con) = pu_con_res {
				pu_con.closed.store(true, std::sync::atomic::Ordering::Release);
				pu_con.notify.notify_waiters();
				if let Ok(sender) = pu_con.sender.reserve().await {
					sender.send(Bytes::new());
				}
			}
		}
	};

	loop {
		match h2c.accept().await {
			Some(Ok(mut stream)) => {
				let resolver = resolver.clone();
				let waiters = waiters.clone();
				let pc = pc.clone();
				let pu_get_streams = pu_get_streams.clone();
				tokio::spawn(async move {
					let mut path_parts: Vec<&str> = stream.0.uri().path().split("/").collect();
					if let Some(last) = path_parts.last()
						&& let Ok(sec) = last.parse::<usize>()
					{
						// packet up
						let pu_uuid = if let Some(uuid_str) = path_parts.get(path_parts.len() - 2)
							&& let Ok(pu_uuid) = uuid::Uuid::parse_str(uuid_str)
						{
							pu_uuid
						} else {
							stream.1.send_reset(h2::Reason::REFUSED_STREAM);
							return;
						};
						let content_len: usize = if let Some(header) = stream.0.headers().get("content-length")
							&& let Ok(cl_str) = header.to_str()
							&& let Ok(cl) = cl_str.parse()
						{
							cl
						} else {
							stream.1.send_reset(h2::Reason::REFUSED_STREAM);
							return;
						};
						path_parts.pop();
						path_parts.pop();
						let path = path_parts.join("/") + "/";
						if http_head_match(&path, stream.0.headers(), &transport.host, &transport.path) {
							stream.1.send_reset(h2::Reason::REFUSED_STREAM);
							return;
						}

						let mut pc_lock = pc.lock().await;
						if sec == 0 {
							let mut waiter_locked = waiters.lock().await;
							let waiter = waiter_locked.remove(&pu_uuid);
							let (sender, recver) = tokio::sync::mpsc::channel(transport.initial_channel_size);
							if let Some(waiter) = waiter {
								drop(waiter_locked);
								let _ = waiter.0.send(UpStream::PU(recver)).await;
							} else {
								let (chan_send, chan_recv) = tokio::sync::mpsc::channel(1);
								let _ = chan_send.send(UpStream::PU(recver)).await;
								waiter_locked.insert(pu_uuid, (chan_send, Some(chan_recv)));
								drop(waiter_locked);
							}
							let pu_con = Arc::new(Pu {
								closed: std::sync::atomic::AtomicBool::new(false),
								sec: std::sync::atomic::AtomicUsize::new(0),
								notify: tokio::sync::Notify::new(),
								sender,
							});
							pc_lock.insert(pu_uuid, pu_con.clone());
							drop(pc_lock);
							pu::write_to_channel(
								stream,
								pu_con,
								content_len,
								sec,
								transport.initial_channel_size,
								transport.recv_timeout,
							)
							.await;
						} else if let Some(pu_con) = pc_lock.get(&pu_uuid).cloned() {
							drop(pc_lock);
							pu::write_to_channel(
								stream,
								pu_con,
								content_len,
								sec,
								transport.initial_channel_size,
								transport.recv_timeout,
							)
							.await;
						} else {
							log::warn!("packet-up post uuid not found");
							stream.1.send_reset(h2::Reason::REFUSED_STREAM);
						}
					} else if let Some(last) = path_parts.last()
						&& last.len() == 36
						&& let Ok(su_uuid) = uuid::Uuid::parse_str(last)
					{
						// stream up
						path_parts.pop();
						let path = path_parts.join("/") + "/";
						if http_head_match(&path, stream.0.headers(), &transport.host, &transport.path) {
							log::warn!("http path/host error");
							stream.1.send_reset(h2::Reason::REFUSED_STREAM);
							return;
						}

						let mut waiter_locked = waiters.lock().await;
						let waiter = waiter_locked.remove(&su_uuid);
						if stream.0.method() == http::Method::POST {
							// POST method
							if let Some(waiter) = waiter {
								// A
								drop(waiter_locked);
								let _ = waiter.0.send(UpStream::SU(stream)).await;
							} else {
								// init B
								let (chan_send, chan_recv) = tokio::sync::mpsc::channel(1);
								let _ = chan_send.send(UpStream::SU(stream)).await;
								waiter_locked.insert(su_uuid, (chan_send, Some(chan_recv)));
								drop(waiter_locked);
							}
						} else if let Some(waiter) = waiter {
							// GET method
							// B
							drop(waiter_locked);
							if let Some(mut recver) = waiter.1 {
								if let Some(upstream) = recver.recv().await {
									match upstream {
										UpStream::SU(w_stream) => {
											if let Err(e) = su::stream_up(
												stream,
												w_stream,
												resolver,
												outbound,
												peer_addr,
												transport.stream_up_keepalive,
											)
											.await
											{
												log::warn!("{e}");
											}
										}
										UpStream::PU(r) => {
											pu_get_streams.lock().await.push(su_uuid);
											if let Err(e) =
												pu::packet_up(su_uuid, pc, stream, r, resolver, outbound, peer_addr)
													.await
											{
												log::warn!("{e}");
											}
										}
									}
								} else {
									log::warn!("failed to recv other stream");
								}
							} else {
								log::warn!("stream-up protocol error");
								stream.1.send_reset(h2::Reason::REFUSED_STREAM);
							}
						} else {
							// GET method
							// init A
							let (chan_send, mut chan_recv) = tokio::sync::mpsc::channel(1);
							waiter_locked.insert(su_uuid, (chan_send, None));
							drop(waiter_locked);
							if let Ok(Some(upstream)) =
								tokio::time::timeout(std::time::Duration::from_secs(5), chan_recv.recv()).await
							{
								match upstream {
									UpStream::SU(w_stream) => {
										if let Err(e) = su::stream_up(
											stream,
											w_stream,
											resolver,
											outbound,
											peer_addr,
											transport.stream_up_keepalive,
										)
										.await
										{
											log::warn!("{e}");
										}
									}
									UpStream::PU(r) => {
										pu_get_streams.lock().await.push(su_uuid);
										if let Err(e) =
											pu::packet_up(su_uuid, pc, stream, r, resolver, outbound, peer_addr).await
										{
											log::warn!("{e}");
										}
									}
								}
							} else {
								log::warn!("failed/timeout to recv other stream");
								waiters.lock().await.remove(&su_uuid);
							}
						}
					} else {
						// stream one
						let path = path_parts.join("/");
						if http_head_match(&path, stream.0.headers(), &transport.host, &transport.path) {
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
			Some(Err(e)) => {
				clean_up.await;
				return Err(tokio::io::Error::other(e));
			}
			None => {
				clean_up.await;
				return Err(tokio::io::Error::new(
					std::io::ErrorKind::ConnectionAborted,
					"h2 socket closed",
				));
			}
		}
	}
}

#[inline(always)]
async fn stream_one(
	(mut req, mut resp): (http::Request<h2::RecvStream>, h2::server::SendResponse<bytes::Bytes>),
	resolver: RS,
	outbound: &'static str,
	peer_addr: SocketAddr,
) -> tokio::io::Result<()> {
	let mut payload = Vec::with_capacity(1024 * 8);
	stream_recv_timeout(req.body_mut(), &mut payload).await?;
	if payload.len() < 19 {
		// minimum is mux header
		stream_recv_timeout(req.body_mut(), &mut payload).await?;
	}
	// try to read again
	try_stream_recv(req.body_mut(), &mut payload).await?;

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
	let mut w = resp.send_response(http_resp, false).map_err(tokio::io::Error::other)?;
	let mut h2t = H2t {
		stream: (req.body_mut(), &mut w),
	};

	let res = match vless.rt {
		crate::vless::RequestCommand::TCP => crate::handle_tcp(vless, payload.to_vec(), &mut h2t, outbound).await,
		crate::vless::RequestCommand::UDP => crate::handle_udp(vless, payload.to_vec(), &mut h2t, outbound).await,
		crate::vless::RequestCommand::MUX => {
			crate::mux::xudp(&mut h2t, payload.to_vec(), resolver, outbound, peer_addr.ip()).await
		}
	};

	let _ = w.send_data(bytes::Bytes::new(), true);
	let _ = tokio::time::timeout(
		std::time::Duration::from_secs(9),
		std::future::poll_fn(|cx| w.poll_reset(cx)),
	)
	.await;
	res
}

#[allow(clippy::large_enum_variant)]
enum UpStream {
	SU((http::Request<h2::RecvStream>, h2::server::SendResponse<bytes::Bytes>)),
	PU(tokio::sync::mpsc::Receiver<bytes::Bytes>),
}

type Waiters = std::sync::Arc<
	tokio::sync::Mutex<
		std::collections::HashMap<
			uuid::Uuid,
			(
				tokio::sync::mpsc::Sender<UpStream>,
				Option<tokio::sync::mpsc::Receiver<UpStream>>,
			),
		>,
	>,
>;

type PuConns = std::sync::Arc<tokio::sync::Mutex<std::collections::HashMap<uuid::Uuid, std::sync::Arc<Pu>>>>;

struct Pu {
	closed: std::sync::atomic::AtomicBool,
	sec: std::sync::atomic::AtomicUsize,
	notify: tokio::sync::Notify,
	sender: tokio::sync::mpsc::Sender<bytes::Bytes>,
}

#[inline(always)]
async fn stream_recv_timeout(rs: &mut h2::RecvStream, vec: &mut Vec<u8>) -> tokio::io::Result<()> {
	let data = tokio::time::timeout(std::time::Duration::from_secs(12), rs.data())
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

#[inline(always)]
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

#[inline(always)]
fn http_head_match(mut path: &str, headers: &http::HeaderMap, c_host: &Option<String>, c_path: &str) -> bool {
	if path.is_empty() {
		path = "/";
	}

	if c_path == path {
		if let Some(host) = &c_host {
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
struct H2t<'a> {
	stream: (&'a mut h2::RecvStream, &'a mut h2::SendStream<bytes::Bytes>),
}

impl<'a> AsyncRead for H2t<'a> {
	#[inline(always)]
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

impl<'a> AsyncWrite for H2t<'a> {
	#[inline(always)]
	fn poll_write(
		mut self: std::pin::Pin<&mut Self>,
		cx: &mut std::task::Context<'_>,
		buf: &[u8],
	) -> std::task::Poll<Result<usize, std::io::Error>> {
		let this = &mut *self;
		let buflen = buf.len();

		this.stream.1.reserve_capacity(buflen);
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
		self: std::pin::Pin<&mut Self>,
		_cx: &mut std::task::Context<'_>,
	) -> std::task::Poll<Result<(), std::io::Error>> {
		std::task::Poll::Ready(Ok(()))
	}
}
