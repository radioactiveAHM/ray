pub type SuWaiter = std::sync::Arc<
	tokio::sync::Mutex<
		std::collections::HashMap<
			uuid::Uuid,
			(
				tokio::sync::mpsc::Sender<(http::Request<h2::RecvStream>, h2::server::SendResponse<bytes::Bytes>)>,
				Option<
					tokio::sync::mpsc::Receiver<(
						http::Request<h2::RecvStream>,
						h2::server::SendResponse<bytes::Bytes>,
					)>,
				>,
			),
		>,
	>,
>;

pub async fn stream_up(
	mut r_stream: (http::Request<h2::RecvStream>, h2::server::SendResponse<bytes::Bytes>),
	mut w_stream: (http::Request<h2::RecvStream>, h2::server::SendResponse<bytes::Bytes>),
	resolver: super::RS,
	outbound: &'static str,
	peer_addr: std::net::SocketAddr,
) -> tokio::io::Result<()> {
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
			crate::vless::RequestCommand::TCP => crate::handle_tcp(vless, payload.to_vec(), &mut h2t, outbound).await,
			crate::vless::RequestCommand::UDP => crate::handle_udp(vless, payload.to_vec(), &mut h2t, outbound).await,
			crate::vless::RequestCommand::MUX => {
				crate::mux::xudp(&mut h2t, payload.to_vec(), resolver, outbound, peer_addr.ip()).await
			}
		}
	};

	r_stream.1.send_reset(h2::Reason::CANCEL);
	w_stream.1.send_reset(h2::Reason::CANCEL);

	res
}
