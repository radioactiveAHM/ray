pub type SuWaiter = std::sync::Arc<
	tokio::sync::Mutex<
		std::collections::HashMap<
			uuid::Uuid,
			tokio::sync::mpsc::Sender<(http::Request<h2::RecvStream>, h2::server::SendResponse<bytes::Bytes>)>,
		>,
	>,
>;

pub async fn stream_up(
	id: uuid::Uuid,
	su_waiter: SuWaiter,
	stream: (http::Request<h2::RecvStream>, h2::server::SendResponse<bytes::Bytes>),
	mut chan_recv: tokio::sync::mpsc::Receiver<(http::Request<h2::RecvStream>, h2::server::SendResponse<bytes::Bytes>)>,
	resolver: super::RS,
	sockopt: crate::config::SockOpt,
	peer_addr: std::net::SocketAddr,
) -> tokio::io::Result<()> {
	let stream2 = match tokio::time::timeout(std::time::Duration::from_secs(5), chan_recv.recv()).await {
		Ok(Some(stream)) => stream,
		// if timeout or channel closed drop su from waiter
		Ok(None) => {
			let _ = su_waiter.lock().await.remove(&id);
			return Err(tokio::io::Error::other("failed to recv other stream"));
		}
		_ => {
			let _ = su_waiter.lock().await.remove(&id);
			return Err(tokio::io::Error::other("timeout recv other stream"));
		}
	};

	let (mut r_stream, mut w_stream) = if stream.0.method() == http::method::Method::GET {
		(stream, stream2)
	} else {
		(stream2, stream)
	};

	let res = {
		let mut payload = Vec::new();
		super::stream_recv_timeout(w_stream.0.body_mut(), &mut payload).await?;
		if payload.len() < 19 {
			// minimum is mux header
			super::stream_recv_timeout(w_stream.0.body_mut(), &mut payload).await?;
		} else {
			// try to read again if exist to complete
			super::try_stream_recv(w_stream.0.body_mut(), &mut payload).await?;
		}

		let vless = crate::vless::Vless::new(&payload, &resolver).await?;
		if crate::auth::authenticate(&vless, peer_addr) {
			r_stream.1.send_reset(h2::Reason::REFUSED_STREAM);
			w_stream.1.send_reset(h2::Reason::REFUSED_STREAM);
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

		let w = r_stream
			.1
			.send_response(http_resp, false)
			.map_err(tokio::io::Error::other)?;
		let mut h2t = super::H2t {
			stream: (w_stream.0.into_body(), w),
		};

		match vless.rt {
			crate::vless::RequestCommand::TCP => crate::handle_tcp(vless, payload.to_vec(), &mut h2t, sockopt).await,
			crate::vless::RequestCommand::UDP => crate::handle_udp(vless, payload.to_vec(), &mut h2t, sockopt).await,
			crate::vless::RequestCommand::MUX => {
				crate::mux::xudp(&mut h2t, payload.to_vec(), resolver, sockopt, peer_addr.ip()).await
			}
		}
	};

	r_stream.1.send_reset(h2::Reason::CANCEL);
	w_stream.1.send_reset(h2::Reason::CANCEL);

	res
}
