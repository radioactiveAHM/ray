pub async fn stream_up(
	mut r_stream: (http::Request<h2::RecvStream>, h2::server::SendResponse<bytes::Bytes>),
	mut w_stream: (http::Request<h2::RecvStream>, h2::server::SendResponse<bytes::Bytes>),
	resolver: super::RS,
	outbound: &'static str,
	peer_addr: std::net::SocketAddr,
) -> tokio::io::Result<()> {
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
	let mut h2t = super::H2t {
		stream: (w_stream.0.body_mut(), &mut w),
	};

	// todo: write bytes with random interval to w_stream keep conn alive
	let res = match vless.rt {
		crate::vless::RequestCommand::TCP => crate::handle_tcp(vless, payload.to_vec(), &mut h2t, outbound).await,
		crate::vless::RequestCommand::UDP => crate::handle_udp(vless, payload.to_vec(), &mut h2t, outbound).await,
		crate::vless::RequestCommand::MUX => {
			crate::mux::xudp(&mut h2t, payload.to_vec(), resolver, outbound, peer_addr.ip()).await
		}
	};

	let _ = w.send_data(bytes::Bytes::new(), true).map_err(tokio::io::Error::other);
	let _ = tokio::time::timeout(
		std::time::Duration::from_secs(9),
		std::future::poll_fn(|cx| w.poll_reset(cx)),
	)
	.await;

	res
}
