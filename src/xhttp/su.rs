use rand::Rng;

pub async fn stream_up(
	mut r_stream: (http::Request<h2::RecvStream>, h2::server::SendResponse<bytes::Bytes>),
	mut w_stream: (http::Request<h2::RecvStream>, h2::server::SendResponse<bytes::Bytes>),
	resolver: super::RS,
	outbound: &'static str,
	peer_addr: std::net::SocketAddr,
	stream_up_keepalive: Option<((u64, u64), (usize, usize))>,
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

	let handle = if let Some((dur, sz)) = stream_up_keepalive {
		if let Ok(mut ss) = w_stream
			.1
			.send_response(
				http::Response::builder()
					.version(http::Version::HTTP_2)
					.status(200)
					.body(())
					.unwrap(),
				false,
			)
			.map_err(tokio::io::Error::other)
		{
			Some(tokio::spawn(async move {
				let mut random_data = vec![0; sz.1];
				loop {
					let sleep_dur: u64 = rand::rng().random_range(dur.0..dur.1);
					tokio::time::sleep(std::time::Duration::from_secs(sleep_dur)).await;
					let random_data_to_send: usize = rand::rng().random_range(sz.0..sz.1);
					rand::rng().fill(&mut random_data[..random_data_to_send]);

					ss.reserve_capacity(random_data_to_send);
					match std::future::poll_fn(|cx| ss.poll_capacity(cx)).await {
						Some(Ok(_)) => {
							if ss
								.send_data(
									bytes::Bytes::copy_from_slice(&random_data[..random_data_to_send]),
									false,
								)
								.is_err()
							{
								break;
							}
						}
						_ => break,
					}
				}
			}))
		} else {
			None
		}
	} else {
		None
	};

	let mut h2t = super::H2t {
		stream: (w_stream.0.body_mut(), &mut w),
	};

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

	if let Some(handle) = handle {
		handle.abort();
	}

	res
}
