use std::sync::Arc;

use tokio::io::{AsyncRead, AsyncWrite};

#[inline(always)]
pub async fn recv_bytes_buffered(
	stream_recver: &mut h2::RecvStream,
	bytes_vec: &mut Vec<bytes::Bytes>,
	content_len: usize,
	recv_timeout: u64,
) -> Option<()> {
	let mut recved = 0;
	loop {
		if recved >= content_len {
			break;
		}
		match tokio::time::timeout(std::time::Duration::from_secs(recv_timeout), stream_recver.data()).await {
			Err(_) | Ok(Some(Err(_))) => return None,
			Ok(None) => break,
			Ok(Some(Ok(data))) => {
				recved += data.len();
				let _ = stream_recver.flow_control().release_capacity(data.len());
				bytes_vec.push(data);
			}
		}
	}
	Some(())
}

#[inline(always)]
pub async fn write_to_channel(
	mut stream: (http::Request<h2::RecvStream>, h2::server::SendResponse<bytes::Bytes>),
	pu: Arc<super::Pu>,
	content_len: usize,
	sec: usize,
	initial_channel_size: usize,
	recv_timeout: u64,
) {
	let recver = stream.0.body_mut();
	let sender = &pu.sender;
	let mut buffed_bytes = Vec::with_capacity(initial_channel_size);

	// buffer data
	if recv_bytes_buffered(recver, &mut buffed_bytes, content_len, recv_timeout)
		.await
		.is_none()
	{
		// data incomplete
		pu.closed.store(true, std::sync::atomic::Ordering::Release);
		pu.notify.notify_waiters();
		return;
	}

	let _ss_res = stream.1.send_response(
		http::Response::builder()
			.version(http::Version::HTTP_2)
			.status(200)
			.body(())
			.unwrap(),
		true,
	);

	// wait for sec matching
	loop {
		if pu.closed.load(std::sync::atomic::Ordering::Acquire) {
			pu.notify.notify_waiters();
			return;
		}
		if pu.sec.load(std::sync::atomic::Ordering::Acquire) == sec {
			break;
		}
		pu.notify.notified().await;
	}

	// send all buffed data
	for data in buffed_bytes {
		if let Ok(sender) = sender.reserve().await {
			sender.send(data);
		} else {
			pu.closed.store(true, std::sync::atomic::Ordering::Release);
			pu.notify.notify_waiters();
			return;
		}
	}

	pu.sec.store(sec + 1, std::sync::atomic::Ordering::Release);
	pu.notify.notify_waiters();
}

#[allow(clippy::too_many_arguments)]
#[inline(always)]
pub async fn packet_up(
	id: uuid::Uuid,
	pc: super::PuConns,
	mut r_stream: (http::Request<h2::RecvStream>, h2::server::SendResponse<bytes::Bytes>),
	mut recver: tokio::sync::mpsc::Receiver<bytes::Bytes>,
	resolver: super::RS,
	outbound: &'static str,
	peer_addr: std::net::SocketAddr,
) -> tokio::io::Result<()> {
	let res = {
		let mut payload = Vec::with_capacity(1024 * 8);
		payload.extend_from_slice(
			&recver
				.recv()
				.await
				.ok_or(tokio::io::Error::other("packet-up read channel closed"))?,
		);
		if payload.len() < 19 {
			// minimum is mux header
			payload.extend_from_slice(
				&recver
					.recv()
					.await
					.ok_or(tokio::io::Error::other("packet-up read channel closed"))?,
			);
		}
		// try recv again
		if let Ok(data) = &recver.try_recv() {
			payload.extend_from_slice(data);
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

		let mut w = r_stream
			.1
			.send_response(http_resp, false)
			.map_err(tokio::io::Error::other)?;

		let mut h2t = H2t {
			stream: (&mut recver, &mut w),
		};

		let res = match vless.rt {
			crate::vless::RequestCommand::TCP => crate::handle_tcp(vless, payload.to_vec(), &mut h2t, outbound).await,
			crate::vless::RequestCommand::UDP => crate::handle_udp(vless, payload.to_vec(), &mut h2t, outbound).await,
			crate::vless::RequestCommand::MUX => {
				crate::mux::xudp(&mut h2t, payload.to_vec(), resolver, outbound, peer_addr.ip()).await
			}
		};

		let _ = w.send_data(bytes::Bytes::new(), true).map_err(tokio::io::Error::other);
		drop(recver);
		let _ = tokio::time::timeout(
			std::time::Duration::from_secs(9),
			std::future::poll_fn(|cx| w.poll_reset(cx)),
		)
		.await;
		res
	};
	let _ = pc.lock().await.remove(&id);
	res
}

struct H2t<'a> {
	stream: (
		&'a mut tokio::sync::mpsc::Receiver<bytes::Bytes>,
		&'a mut h2::SendStream<bytes::Bytes>,
	),
}

impl<'a> AsyncRead for H2t<'a> {
	#[inline(always)]
	fn poll_read(
		mut self: std::pin::Pin<&mut Self>,
		cx: &mut std::task::Context<'_>,
		buf: &mut tokio::io::ReadBuf<'_>,
	) -> std::task::Poll<std::io::Result<()>> {
		let this = &mut *self;
		buf.put_slice(
			&std::task::ready!(this.stream.0.poll_recv(cx)).ok_or(tokio::io::Error::other("recv channel closed"))?,
		);
		std::task::Poll::Ready(Ok(()))
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
					.send_data(bytes::Bytes::copy_from_slice(chunk), false)
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
