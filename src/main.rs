use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Arc,
};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_rustls::{
    TlsAcceptor,
    rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
};

mod auth;
mod blacklist;
mod config;
mod mux;
mod pipe;
mod resolver;
mod tcp;
mod tls;
mod transporters;
mod udputils;
mod utils;
mod verror;
mod vless;

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
    // Log panic info
    std::panic::set_hook(Box::new(|message| {
        log::error!("{message}");
    }));

    {
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
        logger.filter_level(CONFIG.log.level.convert()).init();
    }

    let resolver = Arc::new(resolver::generate_resolver(&CONFIG.resolver));

    for inbound in &CONFIG.inbounds {
        let resolver = resolver.clone();
        tokio::spawn(async move {
            let tcp = tokio::net::TcpListener::bind(inbound.listen).await.unwrap();
            if inbound.tls.enable {
                // with tls
                let certs = CertificateDer::pem_file_iter(&inbound.tls.certificate)
                    .unwrap()
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap();
                let key = PrivateKeyDer::from_pem_file(&inbound.tls.key).unwrap();
                let mut c: tokio_rustls::rustls::ServerConfig =
                    tokio_rustls::rustls::ServerConfig::builder()
                        .with_no_client_auth()
                        .with_single_cert(certs, key)
                        .unwrap();
                c.alpn_protocols = inbound
                    .tls
                    .alpn
                    .iter()
                    .map(|p| p.as_bytes().to_vec())
                    .collect();
                c.max_fragment_size = inbound.tls.max_fragment_size;
                let acceptor = TlsAcceptor::from(Arc::new(c));

                loop {
                    match tls::Tc::new(acceptor.clone(), tcp.accept().await) {
                        Ok(tc) => {
                            let resolver = resolver.clone();
                            tokio::spawn(async move {
                                if let Err(e) = tls_handler(
                                    tc,
                                    resolver,
                                    inbound.transporter.clone(),
                                    inbound.sockopt.clone(),
                                )
                                .await
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
                                && let Err(e) = stream_handler(
                                    stream,
                                    peer_addr,
                                    resolver,
                                    inbound.transporter.clone(),
                                    inbound.sockopt.clone(),
                                )
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
    resolver: Arc<
        hickory_resolver::Resolver<
            hickory_resolver::name_server::GenericConnector<
                hickory_resolver::proto::runtime::TokioRuntimeProvider,
            >,
        >,
    >,
    transport: config::Transporter,
    sockopt: config::SockOpt,
) -> tokio::io::Result<()> {
    let peer_addr: SocketAddr = tc.stream.0.peer_addr()?;
    let stream: tokio_rustls::server::TlsStream<tokio::net::TcpStream> = tc.accept().await?;

    stream_handler(stream, peer_addr, resolver, transport, sockopt).await
}

async fn stream_handler<S>(
    mut stream: S,
    peer_addr: SocketAddr,
    resolver: Arc<
        hickory_resolver::Resolver<
            hickory_resolver::name_server::GenericConnector<
                hickory_resolver::proto::runtime::TokioRuntimeProvider,
            >,
        >,
    >,
    transport: config::Transporter,
    sockopt: config::SockOpt,
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
                let _ = stream.write(b"HTTP/1.1 200 Ok\r\n\r\n").await?;
            } else {
                return Err(crate::verror::VError::TransporterError.into());
            }
        }
        config::Transporter::WS(ws_options) => {
            drop(buff);
            if let Ok((req, ws)) = tokio_websockets::ServerBuilder::new()
                .config(
                    tokio_websockets::Config::default()
                        .frame_size(ws_options.frame_size.unwrap_or(1024) * 1024),
                )
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
                return transporters::websocket_transport(ws, resolver, peer_addr, sockopt).await;
            } else {
                return Err(verror::VError::TransporterError.into());
            }
        }
    }

    let vless = vless::Vless::new(&buff[..size], &resolver).await?;
    if auth::authenticate(&vless, peer_addr) {
        return Err(verror::VError::AuthenticationFailed.into());
    }

    let payload = buff[..size].to_vec();
    drop(buff);
    if let Err(e) = match vless.rt {
        vless::RequestCommand::TCP => {
            handle_tcp(vless, payload, &mut stream, sockopt, CONFIG.tcp_fill_buffer).await
        }
        vless::RequestCommand::UDP => {
            handle_udp(vless, payload, &mut stream, sockopt, CONFIG.tcp_fill_buffer).await
        }
        vless::RequestCommand::MUX => {
            mux::xudp(
                &mut stream,
                payload,
                resolver,
                CONFIG.udp_proxy_buffer_size.unwrap_or(8),
                sockopt,
                peer_addr.ip(),
            )
            .await
        }
    } {
        log::warn!("{peer_addr}: {e}");
    } else {
        log::warn!("{peer_addr}: closed connection")
    }

    let _ = stream.shutdown().await;
    Ok(())
}

async fn handle_tcp<S>(
    vless: vless::Vless,
    payload: Vec<u8>,
    mut stream: S,
    sockopt: config::SockOpt,
    tcp_fill_buffer: bool,
) -> tokio::io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (target_addr, body) = vless.target.as_ref().unwrap();
    let mut target = tcp::stream(*target_addr, &sockopt).await?;

    if !&payload[*body..].is_empty() {
        let _ = target.write(&payload[*body..]).await?;
    }
    drop(payload);

    let tpbs = CONFIG.tcp_proxy_buffer_size.unwrap_or(8);

    let _ = stream.write(&[0, 0]).await?;

    let mut client_buf = vec![0; 1024 * tpbs];
    let mut client_buf_rb = tokio::io::ReadBuf::new(&mut client_buf);
    let mut target_buf = vec![0; 1024 * tpbs];
    let mut target_buf_rb = tokio::io::ReadBuf::new(&mut target_buf);

    let (mut client_r, mut client_w) = tokio::io::split(stream);
    let (mut target_r, mut target_w) = target.split();
    let mut client_r_pin = std::pin::Pin::new(&mut client_r);
    let mut client_w_pin = std::pin::Pin::new(&mut client_w);
    let mut target_r_pin = std::pin::Pin::new(&mut target_r);
    let mut target_w_pin = std::pin::Pin::new(&mut target_w);

    loop {
        let operation = tokio::time::timeout(
            std::time::Duration::from_secs(CONFIG.tcp_idle_timeout),
            async {
                tokio::select! {
                    piping = pipe::copy(&mut client_r_pin, &mut target_w_pin, &mut client_buf_rb, tcp_fill_buffer) => {
                        piping
                    },
                    piping = pipe::copy(&mut target_r_pin, &mut client_w_pin, &mut target_buf_rb, CONFIG.tcp_fill_buffer) => {
                        piping
                    },
                }
            },
        )
        .await;

        match operation {
            Err(_) => {
                let _ = target.shutdown().await;
                return Err(tokio::io::Error::other("Timeout"));
            }
            Ok(Err(e)) => {
                let _ = target.shutdown().await;
                return Err(e);
            }
            _ => (),
        }
    }
}

async fn handle_udp<S>(
    vless: vless::Vless,
    payload: Vec<u8>,
    stream: S,
    sockopt: config::SockOpt,
    tcp_fill_buffer: bool,
) -> tokio::io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (target, body) = vless.target.as_ref().unwrap();
    let ip = if let Some(interface) = &sockopt.interface {
        tcp::get_interface(target.is_ipv4(), interface)
    } else if target.is_ipv4() {
        IpAddr::V4(Ipv4Addr::UNSPECIFIED)
    } else {
        IpAddr::V6(Ipv6Addr::UNSPECIFIED)
    };
    let udp = tokio::net::UdpSocket::bind(SocketAddr::new(ip, 0)).await?;
    #[cfg(target_os = "linux")]
    {
        if sockopt.bind_to_device {
            if let Some(interface) = &sockopt.interface {
                if tcp::tcp_options::set_udp_bind_device(&udp, &interface).is_err() {
                    log::warn!("Failed to set bind to device")
                };
            }
        }
    }
    udp.connect(target).await?;

    // first packet might not be complete
    if !&payload[*body..].is_empty() {
        udp.send(&payload[*body + 2..]).await?;
    }
    drop(payload);

    let mut client_buf = vec![0; 1024 * CONFIG.udp_proxy_buffer_size.unwrap_or(8)];
    let mut client_buf_rb = tokio::io::ReadBuf::new(&mut client_buf);

    let (mut client_r, mut client_w) = tokio::io::split(stream);
    let mut client_r_pin = std::pin::Pin::new(&mut client_r);
    let mut client_w_pin = std::pin::Pin::new(&mut client_w);

    let mut uw = udputils::UdpWriter {
        udp: &udp,
        b: utils::DeqBuffer::new(CONFIG.udp_proxy_buffer_size.unwrap_or(8) * 1024), // buf_size unit is kb
    };
    let mut uw_pin = std::pin::Pin::new(&mut uw);

    let mut ur = udputils::UdpReader {
        udp: &udp,
        buf: vec![0; CONFIG.udp_proxy_buffer_size.unwrap_or(8) * 1024],
    };

    let _ = client_w_pin.write(&[0, 0]).await?;

    loop {
        let operation = tokio::time::timeout(
            std::time::Duration::from_secs(CONFIG.udp_idle_timeout),
            async {
                tokio::select! {
                    piping = pipe::copy(&mut client_r_pin, &mut uw_pin, &mut client_buf_rb, tcp_fill_buffer) => {
                        piping
                    },
                    piping = ur.copy(&mut client_w_pin) => {
                        piping
                    },
                }
            },
        )
        .await;

        match operation {
            Err(_) => {
                return Err(tokio::io::Error::other("Timeout"));
            }
            Ok(Err(e)) => {
                return Err(e);
            }
            _ => (),
        }
    }
}
