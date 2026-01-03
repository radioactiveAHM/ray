use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

mod auth;
mod config;
mod ioutils;
mod mux;
mod pipe;
mod resolver;
mod rules;
mod tcp;
mod tls;
mod transporters;
mod udputils;
mod utils;
mod verror;
mod vless;
mod xhttp;

static CONFIG: std::sync::LazyLock<config::Config> = std::sync::LazyLock::new(config::load_config);

fn main() {
	match CONFIG.runtime.runtime_mode {
		config::RuntimeMode::Multi => {
			let mut r = tokio::runtime::Builder::new_multi_thread();

			if let Some(worker_threads) = CONFIG.runtime.worker_threads {
				r.worker_threads(worker_threads);
			}
			if let Some(thread_stack_size) = CONFIG.runtime.thread_stack_size {
				r.thread_stack_size(thread_stack_size);
			}
			if let Some(event_interval) = CONFIG.runtime.event_interval {
				r.event_interval(event_interval);
			}
			if let Some(global_queue_interval) = CONFIG.runtime.global_queue_interval {
				r.global_queue_interval(global_queue_interval);
			}
			if let Some(thread_keep_alive) = CONFIG.runtime.thread_keep_alive {
				r.thread_keep_alive(std::time::Duration::from_secs(thread_keep_alive));
			}

			r
		}
		config::RuntimeMode::Single => {
			let mut r = tokio::runtime::Builder::new_current_thread();

			if let Some(thread_stack_size) = CONFIG.runtime.thread_stack_size {
				r.thread_stack_size(thread_stack_size);
			}
			if let Some(event_interval) = CONFIG.runtime.event_interval {
				r.event_interval(event_interval);
			}
			if let Some(global_queue_interval) = CONFIG.runtime.global_queue_interval {
				r.global_queue_interval(global_queue_interval);
			}
			if let Some(thread_keep_alive) = CONFIG.runtime.thread_keep_alive {
				r.thread_keep_alive(std::time::Duration::from_secs(thread_keep_alive));
			}
			if let Some(max_io_events_per_tick) = CONFIG.runtime.max_io_events_per_tick {
				r.max_io_events_per_tick(max_io_events_per_tick);
			}

			r
		}
	}
	.enable_all()
	.build()
	.unwrap()
	.block_on(app());
}

async fn app() {
	if let Some(level) = &CONFIG.log.level {
		let mut logger = env_logger::builder();
		#[cfg(not(debug_assertions))]
		{
			if let Some(file) = &CONFIG.log.file {
				logger.target(env_logger::Target::Pipe(Box::new(
					std::fs::OpenOptions::new()
						.create(true)
						.append(true)
						.open(file)
						.unwrap(),
				)));
			}
		}
		// Level order: Error, Warn, Info, Debug, Trace
		logger.filter_level(level.into()).init();
	}

	// Log panic info
	std::panic::set_hook(Box::new(|message| {
		log::error!("{message}");
	}));

	log::set_max_level(log::LevelFilter::Trace);

	let resolver = resolver::generate_resolver(&CONFIG.resolver);

	for inbound in &CONFIG.inbounds {
		let resolver = resolver.clone();
		tokio::spawn(async move {
			let tcp = tokio::net::TcpListener::bind(inbound.listen).await.unwrap();
			if inbound.tls.enable {
				// with tls
				let acceptor = tls::tls_server(&inbound.tls);
				if let Some(xhttp) = inbound.transporter.grab_xhttp() {
					return xhttp::xhttp_server(resolver, tcp, acceptor, xhttp, &inbound.outbound).await;
				}

				loop {
					match tls::Tc::new(acceptor.clone(), tcp.accept().await) {
						Ok(tc) => {
							let resolver = resolver.clone();
							tokio::spawn(async move {
								if let Err(e) = tls_handler(tc, resolver, &inbound.transporter, &inbound.outbound).await
								{
									log::warn!("TLS: {e}")
								}
							});
						}
						Err(e) => {
							log::warn!("TLS: {e}")
						}
					}
				}
			} else {
				// no tls
				loop {
					if let Ok((stream, _)) = tcp.accept().await {
						let resolver = resolver.clone();
						tokio::spawn(async move {
							if let Ok(peer_addr) = stream.peer_addr()
								&& let Err(e) =
									stream_handler(stream, peer_addr, resolver, &inbound.transporter, &inbound.outbound)
										.await
							{
								log::warn!("NOTLS: {e}")
							}
						});
					}
				}
			}
		});
	}

	// idle
	std::future::pending::<()>().await
}

async fn tls_handler(
	tc: tls::Tc,
	resolver: resolver::RS,
	transport: &'static config::Transporter,
	outbound: &'static str,
) -> tokio::io::Result<()> {
	let peer_addr: SocketAddr = tc.stream.0.peer_addr()?;
	let stream: tokio_rustls::server::TlsStream<tokio::net::TcpStream> = tc.accept().await?;

	stream_handler(stream, peer_addr, resolver, transport, outbound).await
}

async fn stream_handler<S>(
	mut stream: S,
	peer_addr: SocketAddr,
	resolver: resolver::RS,
	transport: &'static config::Transporter,
	outbound: &'static str,
) -> tokio::io::Result<()>
where
	S: AsyncRead + AsyncWrite + Unpin,
{
	let mut buff: Vec<u8> = vec![0; 1024 * 8];
	let mut size;

	// Handle transporters
	match &transport {
		config::Transporter::TCP => {
			size = stream.read(&mut buff).await?;
		}
		config::Transporter::HttpUpgrade(http) => {
			size = stream.read(&mut buff).await?;
			transporters::httpupgrade_transporter(http, &buff[..size], &mut stream).await?;
			size = stream.read(&mut buff).await?;
		}
		config::Transporter::HTTP(http) => {
			size = stream.read(&mut buff).await?;
			if let Some(p) = utils::catch_in_buff(b"\r\n\r\n", &buff) {
				let head = &buff[..p.1];
				transporters::http_transporter(http, head, &mut stream).await?;
				size -= buff.drain(..p.1).len();
				stream.write_all(b"HTTP/1.1 200 Ok\r\n\r\n").await?;
			} else {
				return Err(crate::verror::VError::TransporterError.into());
			}
		}
		config::Transporter::WS(ws_options) => {
			drop(buff);
			if let Ok((req, ws)) = tokio_websockets::ServerBuilder::new()
				.config(tokio_websockets::Config::default().frame_size(ws_options.frame_size.unwrap_or(1024) * 1024))
				.accept(stream)
				.await
			{
				// HTTP path match
				if req.uri().path() != ws_options.path {
					return Err(verror::VError::TransporterError.into());
				}
				// HTTP Host match if Host is not null
				if let Some(host) = &ws_options.host {
					if let Some(req_host) = req.headers().get("Host") {
						if host.as_bytes() != req_host.as_bytes() {
							return Err(verror::VError::TransporterError.into());
						}
					} else {
						return Err(verror::VError::TransporterError.into());
					}
				}
				return transporters::websocket_transport(ws, resolver, peer_addr, outbound).await;
			} else {
				return Err(verror::VError::TransporterError.into());
			}
		}
		config::Transporter::XHTTP(_) => {
			return Ok(());
		}
	}

	let vless = vless::Vless::new(&buff[..size], &resolver).await?;
	if auth::authenticate(&vless, peer_addr) {
		return Err(verror::VError::AuthenticationFailed.into());
	}

	let payload = buff[..size].to_vec();
	drop(buff);
	if let Err(e) = match vless.rt {
		vless::RequestCommand::TCP => handle_tcp(vless, payload, &mut stream, outbound).await,
		vless::RequestCommand::UDP => handle_udp(vless, payload, &mut stream, outbound).await,
		vless::RequestCommand::MUX => mux::xudp(&mut stream, payload, resolver, outbound, peer_addr.ip()).await,
	} {
		log::warn!("{peer_addr}: {e}");
	} else {
		log::warn!("{peer_addr}: closed connection")
	}

	stream.shutdown().await
}

async fn handle_tcp<S>(
	vless: vless::Vless,
	payload: Vec<u8>,
	mut stream: S,
	outbound: &'static str,
) -> tokio::io::Result<()>
where
	S: AsyncRead + AsyncWrite + Unpin,
{
	let (target_addr, domain, body) = vless.target.unwrap();
	let opt = rules::rules(&target_addr.ip(), domain, outbound)?;
	let mut target = tcp::stream(target_addr, opt).await?;

	if !&payload[body..].is_empty() {
		target.write_all(&payload[body..]).await?;
		target.flush().await?;
	}
	drop(payload);

	let (r_tpbs, w_tpbs) = (
		CONFIG.tcp_proxy_buffer_size.0 * 1024,
		CONFIG.tcp_proxy_buffer_size.1 * 1024,
	);

	stream.write_all(&[0, 0]).await?;

	let mut client_buf = vec![0; r_tpbs];
	let mut client_buf_rb = tokio::io::ReadBuf::new(&mut client_buf);
	let mut target_buf = vec![0; w_tpbs];
	let mut target_buf_rb = tokio::io::ReadBuf::new(&mut target_buf);

	let (client_r, client_w) = ioutils::split(&mut stream);
	let (mut target_r, mut target_w) = target.split();
	let mut client_r_pin = std::pin::Pin::new(client_r);
	let mut target_r_pin = std::pin::Pin::new(&mut target_r);

	let mut up_stream_closed = false;
	let err: tokio::io::Error;
	loop {
		match tokio::time::timeout(std::time::Duration::from_secs(CONFIG.tcp_idle_timeout), async {
			if opt.tcp_read_buffered {
				tokio::select! {
					read = pipe::Read(&mut client_r_pin, &mut client_buf_rb) => {
						if read.is_err() {
							up_stream_closed = true;
						}
						read?;
						target_w.write_all(client_buf_rb.filled()).await?;
						target_w.flush().await
					},
					read = pipe::Fill(&mut target_r_pin, &mut target_buf_rb) => {
						if !target_buf_rb.filled().is_empty(){
							client_w.write_all(target_buf_rb.filled()).await?;
							client_w.flush().await?;
						}
						read
					},
				}
			} else {
				tokio::select! {
					read = pipe::Read(&mut client_r_pin, &mut client_buf_rb) => {
						if read.is_err() {
							up_stream_closed = true;
						}
						read?;
						target_w.write_all(client_buf_rb.filled()).await?;
						target_w.flush().await
					},
					read = pipe::Read(&mut target_r_pin, &mut target_buf_rb) => {
						read?;
						client_w.write_all(target_buf_rb.filled()).await?;
						client_w.flush().await
					},
				}
			}
		})
		.await
		{
			Err(_) => {
				err = tokio::io::Error::other("Timeout");
				break;
			}
			Ok(Err(e)) => {
				err = e;
				break;
			}
			_ => (),
		}
	}

	if up_stream_closed {
		loop {
			match tokio::time::timeout(
				std::time::Duration::from_secs(3),
				pipe::Read(&mut target_r_pin, &mut target_buf_rb),
			)
			.await
			{
				Ok(Err(_)) | Err(_) => break,
				_ => (),
			};
			if client_w.write_all(target_buf_rb.filled()).await.is_err() {
				break;
			}
			if client_w.flush().await.is_err() {
				break;
			}
		}
	}

	let _ = target_w.shutdown().await;
	Err(err)
}

async fn handle_tcp_bytes<S>(
	vless: vless::Vless,
	payload: Vec<u8>,
	mut stream: S,
	outbound: &'static str,
) -> tokio::io::Result<()>
where
	S: ioutils::AsyncRecvBytes + AsyncWrite + Unpin,
{
	let (target_addr, domain, body) = vless.target.unwrap();
	let opt = rules::rules(&target_addr.ip(), domain, outbound)?;
	let mut target = tcp::stream(target_addr, opt).await?;

	if !&payload[body..].is_empty() {
		target.write_all(&payload[body..]).await?;
		target.flush().await?;
	}
	drop(payload);

	let w_tpbs = CONFIG.tcp_proxy_buffer_size.1 * 1024;

	stream.write_all(&[0, 0]).await?;

	let mut target_buf = vec![0; w_tpbs];
	let mut target_buf_rb = tokio::io::ReadBuf::new(&mut target_buf);

	let (client_r, client_w) = ioutils::split(&mut stream);
	let (mut target_r, mut target_w) = target.split();
	let mut client_r_pin = std::pin::Pin::new(client_r);
	let mut target_r_pin = std::pin::Pin::new(&mut target_r);

	let mut up_stream_closed = false;
	let err: tokio::io::Error;
	loop {
		match tokio::time::timeout(std::time::Duration::from_secs(CONFIG.tcp_idle_timeout), async {
			if opt.tcp_read_buffered {
				tokio::select! {
					read = pipe::RecvBytes(&mut client_r_pin) => {
						if read.is_err() {
							up_stream_closed = true;
						}
						target_w.write_all(&read?).await?;
						target_w.flush().await
					},
					read = pipe::Fill(&mut target_r_pin, &mut target_buf_rb) => {
						if !target_buf_rb.filled().is_empty(){
							client_w.write_all(target_buf_rb.filled()).await?;
							client_w.flush().await?;
						}
						read
					},
				}
			} else {
				tokio::select! {
					read = pipe::RecvBytes(&mut client_r_pin) => {
						if read.is_err() {
							up_stream_closed = true;
						}
						target_w.write_all(&read?).await?;
						target_w.flush().await
					},
					read = pipe::Read(&mut target_r_pin, &mut target_buf_rb) => {
						read?;
						client_w.write_all(target_buf_rb.filled()).await?;
						client_w.flush().await
					},
				}
			}
		})
		.await
		{
			Err(_) => {
				err = tokio::io::Error::other("Timeout");
				break;
			}
			Ok(Err(e)) => {
				err = e;
				break;
			}
			_ => (),
		}
	}

	if up_stream_closed {
		loop {
			match tokio::time::timeout(
				std::time::Duration::from_secs(3),
				pipe::Read(&mut target_r_pin, &mut target_buf_rb),
			)
			.await
			{
				Ok(Err(_)) | Err(_) => break,
				_ => (),
			};
			if client_w.write_all(target_buf_rb.filled()).await.is_err() {
				break;
			}
			if client_w.flush().await.is_err() {
				break;
			}
		}
	}

	let _ = target_w.shutdown().await;
	Err(err)
}

async fn handle_udp<S>(
	vless: vless::Vless,
	payload: Vec<u8>,
	mut stream: S,
	outbound: &'static str,
) -> tokio::io::Result<()>
where
	S: AsyncRead + AsyncWrite + Unpin,
{
	let (target, domain, body) = vless.target.unwrap();
	let ip = if target.is_ipv4() {
		IpAddr::V4(Ipv4Addr::UNSPECIFIED)
	} else {
		IpAddr::V6(Ipv6Addr::UNSPECIFIED)
	};
	let opt = rules::rules(&target.ip(), domain, outbound)?;
	let udp = udputils::udp_socket(SocketAddr::new(ip, 0), opt).await?;
	udp.connect(target).await?;

	let (r_upbs, w_upbs) = (
		CONFIG.udp_proxy_buffer_size.0 * 1024,
		CONFIG.udp_proxy_buffer_size.1 * 1024,
	);

	let mut client_buf = vec![0; w_upbs];
	let mut client_buf_rb = tokio::io::ReadBuf::new(&mut client_buf);
	let mut udp_buf = vec![0; r_upbs];

	let (client_r, client_w) = ioutils::split(&mut stream);
	let mut client_r_pin = std::pin::Pin::new(client_r);

	let mut uw = udputils::UdpWriter {
		udp: &udp,
		b: utils::DeqBuffer::new(w_upbs),
	};

	if !&payload[body..].is_empty() {
		uw.send_packets(&payload[body..]).await?;
	}
	drop(payload);

	client_w.write_all(&[0, 0]).await?;

	let mut up_stream_closed = false;
	let err: tokio::io::Error;
	loop {
		match tokio::time::timeout(std::time::Duration::from_secs(CONFIG.udp_idle_timeout), async {
			tokio::select! {
				read = pipe::Read(&mut client_r_pin, &mut client_buf_rb) => {
					if read.is_err() {
						up_stream_closed = true;
					}
					read?;
					uw.send_packets(client_buf_rb.filled()).await?;
					Ok(())
				},
				size = udp.recv(&mut udp_buf[2..]) => {
					let size = size?;
					udp_buf[..2].copy_from_slice(&utils::convert_u16_to_two_u8s_be(size as u16));
					client_w.write_all(&udp_buf[..size + 2]).await?;
					client_w.flush().await
				},
			}
		})
		.await
		{
			Err(_) => {
				err = tokio::io::Error::other("Timeout");
				break;
			}
			Ok(Err(e)) => {
				err = e;
				break;
			}
			_ => (),
		}
	}

	if up_stream_closed {
		loop {
			match tokio::time::timeout(std::time::Duration::from_secs(3), udp.recv(&mut udp_buf[2..])).await {
				Ok(Err(_)) | Err(_) => break,
				Ok(Ok(size)) => {
					udp_buf[..2].copy_from_slice(&utils::convert_u16_to_two_u8s_be(size as u16));
					if client_w.write_all(&udp_buf[..size + 2]).await.is_err() {
						break;
					}
					if client_w.flush().await.is_err() {
						break;
					}
				}
			}
		}
	}

	Err(err)
}

async fn handle_udp_bytes<S>(
	vless: vless::Vless,
	payload: Vec<u8>,
	mut stream: S,
	outbound: &'static str,
) -> tokio::io::Result<()>
where
	S: ioutils::AsyncRecvBytes + AsyncWrite + Unpin,
{
	let (target, domain, body) = vless.target.unwrap();
	let ip = if target.is_ipv4() {
		IpAddr::V4(Ipv4Addr::UNSPECIFIED)
	} else {
		IpAddr::V6(Ipv6Addr::UNSPECIFIED)
	};
	let opt = rules::rules(&target.ip(), domain, outbound)?;
	let udp = udputils::udp_socket(SocketAddr::new(ip, 0), opt).await?;
	udp.connect(target).await?;

	let (r_upbs, w_upbs) = (
		CONFIG.udp_proxy_buffer_size.0 * 1024,
		CONFIG.udp_proxy_buffer_size.1 * 1024,
	);

	let mut udp_buf = vec![0; r_upbs];

	let (client_r, client_w) = ioutils::split(&mut stream);
	let mut client_r_pin = std::pin::Pin::new(client_r);

	let mut uw = udputils::UdpWriter {
		udp: &udp,
		b: utils::DeqBuffer::new(w_upbs),
	};

	if !&payload[body..].is_empty() {
		uw.send_packets(&payload[body..]).await?;
	}
	drop(payload);

	client_w.write_all(&[0, 0]).await?;

	let mut up_stream_closed = false;
	let err: tokio::io::Error;
	loop {
		match tokio::time::timeout(std::time::Duration::from_secs(CONFIG.udp_idle_timeout), async {
			tokio::select! {
				read = pipe::RecvBytes(&mut client_r_pin) => {
					if read.is_err() {
						up_stream_closed = true;
					}
					uw.send_packets(&read?).await?;
					Ok(())
				},
				size = udp.recv(&mut udp_buf[2..]) => {
					let size = size?;
					udp_buf[..2].copy_from_slice(&utils::convert_u16_to_two_u8s_be(size as u16));
					client_w.write_all(&udp_buf[..size + 2]).await?;
					client_w.flush().await
				},
			}
		})
		.await
		{
			Err(_) => {
				err = tokio::io::Error::other("Timeout");
				break;
			}
			Ok(Err(e)) => {
				err = e;
				break;
			}
			_ => (),
		}
	}

	if up_stream_closed {
		loop {
			match tokio::time::timeout(std::time::Duration::from_secs(3), udp.recv(&mut udp_buf[2..])).await {
				Ok(Err(_)) | Err(_) => break,
				Ok(Ok(size)) => {
					udp_buf[..2].copy_from_slice(&utils::convert_u16_to_two_u8s_be(size as u16));
					if client_w.write_all(&udp_buf[..size + 2]).await.is_err() {
						break;
					}
					if client_w.flush().await.is_err() {
						break;
					}
				}
			}
		}
	}

	Err(err)
}
