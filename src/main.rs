use std::{
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6},
    pin::Pin,
    sync::Arc,
};

use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf},
    time::timeout,
};
use tokio_rustls::{
    TlsAcceptor,
    rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
};
use utils::{unsafe_staticref, unsafe_refmut};

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

static mut LOG: bool = false;
static mut UIT: u64 = 15;
static mut RESOLVER_MODE: config::ResolvingMode = config::ResolvingMode::IPv4;
static mut TSO: config::TcpSocketOptions = config::TcpSocketOptions {
    send_buffer_size: None,
    recv_buffer_size: None,
    nodelay: None,
    keepalive: None,
    listen_backlog: 128,
};

fn log() -> bool {
    unsafe { LOG }
}

fn uit() -> u64 {
    unsafe { UIT }
}

fn resolver_mode() -> config::ResolvingMode {
    unsafe { RESOLVER_MODE }
}

fn tso() -> config::TcpSocketOptions {
    unsafe { TSO }
}

trait PeekWraper {
    async fn peek(&self) -> tokio::io::Result<()>;
}
impl PeekWraper for tokio::net::TcpStream {
    async fn peek(&self) -> tokio::io::Result<()> {
        let mut buf = [0; 2];
        let mut wraper = ReadBuf::new(&mut buf);
        std::future::poll_fn(|cx| match self.poll_peek(cx, &mut wraper) {
            std::task::Poll::Pending => std::task::Poll::Ready(Ok(())),
            std::task::Poll::Ready(Ok(size)) => {
                if size == 0 {
                    std::task::Poll::Ready(Err(tokio::io::Error::new(
                        std::io::ErrorKind::ConnectionAborted,
                        "Peek: ConnectionAborted",
                    )))
                } else {
                    std::task::Poll::Ready(Ok(()))
                }
            }
            std::task::Poll::Ready(Err(e)) => std::task::Poll::Ready(Err(e)),
        })
        .await
    }
}

impl PeekWraper for tokio_rustls::server::TlsStream<tokio::net::TcpStream> {
    async fn peek(&self) -> tokio::io::Result<()> {
        let mut buf = [0; 2];
        let mut wraper = ReadBuf::new(&mut buf);
        std::future::poll_fn(|cx| match self.get_ref().0.poll_peek(cx, &mut wraper) {
            std::task::Poll::Pending => std::task::Poll::Ready(Ok(())),
            std::task::Poll::Ready(Ok(size)) => {
                if size == 0 {
                    std::task::Poll::Ready(Err(tokio::io::Error::new(
                        std::io::ErrorKind::ConnectionAborted,
                        "Peek: ConnectionAborted",
                    )))
                } else {
                    std::task::Poll::Ready(Ok(()))
                }
            }
            std::task::Poll::Ready(Err(e)) => std::task::Poll::Ready(Err(e)),
        })
        .await
    }
}

fn main() {
    let c = config::load_config();
    if let Some(size) = c.thread_stack_size {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_stack_size(size)
            .build()
            .unwrap()
            .block_on(async {
                async_main(c).await;
            });
    } else {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                async_main(c).await;
            });
    }
}

async fn async_main(c: config::Config) {
    tokio_rustls::rustls::crypto::ring::default_provider()
        .install_default()
        .unwrap();
    // Load config and convert to &'static
    let config: &'static config::Config = utils::unsafe_staticref(&c);

    let resolver = resolver::generate_resolver(&config.resolver);
    let cresolver = utils::unsafe_staticref(&resolver);

    unsafe {
        LOG = config.log;
        UIT = config.udp_idle_timeout;
        RESOLVER_MODE = config.resolver.mode;
        TSO = config.tcp_socket_options
    }

    let tcp = tcp::tcpsocket(config.listen, false)
        .unwrap()
        .listen(config.tcp_socket_options.listen_backlog)
        .unwrap();

    if config.tls.enable {
        // with tls
        let certs = CertificateDer::pem_file_iter(&config.tls.certificate)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let key = PrivateKeyDer::from_pem_file(&config.tls.key).unwrap();
        let mut c: tokio_rustls::rustls::ServerConfig =
            tokio_rustls::rustls::ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(certs, key)
                .unwrap();
        c.alpn_protocols = config
            .tls
            .alpn
            .iter()
            .map(|p| p.as_bytes().to_vec())
            .collect();
        c.max_fragment_size = config.tls.max_fragment_size;
        let acceptor = TlsAcceptor::from(Arc::new(c));

        loop {
            match tls::Tc::new(acceptor.clone(), tcp.accept().await) {
                Ok(tc) => {
                    tokio::spawn(async move {
                        if let Err(e) = tls_handler(tc, config, cresolver).await {
                            if log() {
                                println!("TLS: {e}")
                            }
                        }
                    });
                }
                Err(e) => {
                    if log() {
                        println!("TLS: {e}")
                    }
                }
            }
        }
    } else {
        // no tls
        loop {
            if let Ok((stream, _)) = tcp.accept().await {
                tokio::spawn(async move {
                    if let Ok(peer_addr) = stream.peer_addr() {
                        if let Err(e) = stream_handler(stream, config, peer_addr, cresolver).await {
                            if log() {
                                println!("NOTLS: {e}");
                            }
                        }
                    }
                });
            }
        }
    }
}

async fn tls_handler(
    tc: tls::Tc,
    config: &'static config::Config,
    resolver: &'static hickory_resolver::Resolver<
        hickory_resolver::name_server::GenericConnector<
            hickory_resolver::proto::runtime::TokioRuntimeProvider,
        >,
    >,
) -> tokio::io::Result<()> {
    let peer_addr: SocketAddr = tc.stream.0.peer_addr()?;
    let stream: tokio_rustls::server::TlsStream<tokio::net::TcpStream> = tc.accept().await?;

    stream_handler(stream, config, peer_addr, resolver).await
}

async fn stream_handler<S>(
    mut stream: S,
    config: &'static config::Config,
    peer_addr: SocketAddr,
    resolver: &'static hickory_resolver::Resolver<
        hickory_resolver::name_server::GenericConnector<
            hickory_resolver::proto::runtime::TokioRuntimeProvider,
        >,
    >,
) -> tokio::io::Result<()>
where
    S: AsyncRead + PeekWraper + AsyncWrite + Unpin + Send + 'static,
{
    let mut buff: Vec<u8> = vec![0; 1024 * 8];
    let mut size = stream.read(&mut buff).await?;

    // Handle transporters
    match &config.transporter {
        config::Transporter::TCP => (),
        config::Transporter::HttpUpgrade(http) => {
            transporters::httpupgrade_transporter(http, &buff[..size], &mut stream).await?;
            size = stream.read(&mut buff).await?;
        }
        config::Transporter::HTTP(http) => {
            if let Some(p) = utils::catch_in_buff(b"\r\n\r\n", &buff) {
                let head = &buff[..p.1];
                transporters::http_transporter(http, head, &mut stream).await?;
                size -= buff.drain(..p.1).len();
                let _ = stream.write(b"HTTP/1.1 200 Ok\r\n\r\n").await?;
            } else {
                return Err(crate::verror::VError::TransporterError.into());
            }
        }
    }

    let mut vless = vless::Vless::new(&buff[..size], resolver, &config.blacklist).await?;
    if auth::authenticate(config, &vless, peer_addr) {
        return Err(verror::VError::AuthenticationFailed.into());
    }

    if let Err(e) = match vless.rt {
        vless::SocketType::TCP => handle_tcp(vless, buff, size, stream, config).await,
        vless::SocketType::UDP => {
            vless.target.as_mut().unwrap().1 += 2;
            handle_udp(vless, buff, size, stream, config).await
        }
        vless::SocketType::MUX => {
            mux::xudp(
                stream,
                buff[..size].to_vec(),
                resolver,
                &config.blacklist,
                config.udp_proxy_buffer_size.unwrap_or(8),
            )
            .await
        }
    } {
        if log() {
            println!("{peer_addr}: {e}")
        }
    } else if log() {
        println!("{peer_addr}: closed connection")
    }

    Ok(())
}

async fn handle_tcp<S>(
    vless: vless::Vless,
    buff: Vec<u8>,
    size: usize,
    mut stream: S,
    config: &'static config::Config,
) -> tokio::io::Result<()>
where
    S: AsyncRead + PeekWraper + AsyncWrite + Unpin + Send + 'static,
{
    let (target_addr, body) = vless.target.as_ref().unwrap();
    let mut target = tcp::stream(*target_addr).await?;

    let _ = target.write(&buff[*body..size]).await?;
    target.flush().await?;
    drop(buff);

    let (ch_snd, mut ch_rcv) = tokio::sync::mpsc::channel(10);
    let stream_ghost = unsafe_staticref(&stream);
    // A timeout controller listens for both upload and download activities. If there is no upload or download activity for a specified duration, the connection will be closed.
    let timeout_handler = async move {
        let mut dur = 0;
        loop {
            if dur >= config.tcp_idle_timeout {
                break;
            }
            // idle mode
            match timeout(
                std::time::Duration::from_secs(config.tcp_idle_timeout / 10),
                async { ch_rcv.recv().await },
            )
            .await
            {
                Err(_) => dur += config.tcp_idle_timeout / 10,
                Ok(None) => break,
                _ => {
                    dur = 0;
                    continue;
                }
            };
            // check if connection is alive using peek
            stream_ghost.peek().await?;
        }

        Err::<(), tokio::io::Error>(tokio::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "Connection idle timeout",
        ))
    };

    let mut tpbs = config.tcp_proxy_buffer_size.unwrap_or(8);
    if target_addr.port() == 53 || target_addr.port() == 853 {
        tpbs = 8;
    }

    // Reason for using `unsafe_refmut`:
    // The `try_join!` macro executes tasks concurrently but does not run them in true parallel.
    // This means the tasks may interleave execution but do not utilize separate cores simultaneously.
    // Consequently, using `tokio::io::split`, which is designed for safely splitting read/write halves
    // in parallel execution, is unnecessary in this case.
    match config.tcp_proxy_mod {
        config::TcpProxyMod::Buffer => {
            let _ = stream.write(&[0, 0]).await?;

            let mut tcpwriter_client = tcp::TcpWriterGeneric {
                hr: Pin::new(unsafe_refmut(&stream)),
                signal: ch_snd.clone(),
            };

            let mut tcpwriter_target = tcp::TcpWriterGeneric {
                hr: Pin::new(unsafe_refmut(&target)),
                signal: ch_snd,
            };

            let mut bufwraper_client = tokio::io::BufReader::with_capacity(tpbs*1024, unsafe_refmut(&stream));
            let mut bufwraper_target = tokio::io::BufReader::with_capacity(tpbs*1024, unsafe_refmut(&target));

            if let Err(e) = tokio::try_join!(
                tokio::io::copy_buf(&mut bufwraper_client, &mut tcpwriter_target),
                tokio::io::copy_buf(&mut bufwraper_target, &mut tcpwriter_client),
                timeout_handler,
            ) {
                let _ = tcpwriter_target.shutdown().await;
                let _ = tcpwriter_client.shutdown().await;
                return Err(e);
            }
        }
        config::TcpProxyMod::Stack => {
            let _ = stream.write(&[0, 0]).await?;

            let mut tcpwriter_client = tcp::TcpWriterGeneric {
                hr: Pin::new(unsafe_refmut(&stream)),
                signal: ch_snd.clone(),
            };

            let mut tcpwriter_target = tcp::TcpWriterGeneric {
                hr: Pin::new(unsafe_refmut(&target)),
                signal: ch_snd,
            };

            if let Err(e) = tokio::try_join!(
                pipe::stack_copy(unsafe_refmut(&stream), &mut tcpwriter_target, tpbs),
                pipe::stack_copy(unsafe_refmut(&target), &mut tcpwriter_client, tpbs),
                timeout_handler,
            ) {
                let _ = tcpwriter_target.shutdown().await;
                let _ = tcpwriter_client.shutdown().await;
                return Err(e);
            }
        }
    }
    Ok(())
}

async fn handle_udp<S>(
    vless: vless::Vless,
    buff: Vec<u8>,
    size: usize,
    mut stream: S,
    config: &'static config::Config,
) -> tokio::io::Result<()>
where
    S: AsyncRead + PeekWraper + AsyncWrite + Unpin + Send + 'static,
{
    let (target, body) = vless.target.as_ref().unwrap();
    let addrtype = {
        if target.is_ipv4() {
            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))
        } else if target.is_ipv6() {
            SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, 0, 0, 0))
        } else {
            return Err(tokio::io::Error::other(verror::VError::Wtf));
        }
    };
    let udp = tokio::net::UdpSocket::bind(addrtype).await?;
    udp.connect(target).await?;

    if *body <= size {
        if !&buff[*body..size].is_empty() {
            udp.send(&buff[*body..size]).await?;
        }
    }
    drop(buff);

    let (ch_snd, mut ch_rcv) = tokio::sync::mpsc::channel(10);
    let stream_ghost = unsafe_staticref(&stream);
    let timeout_handler = async move {
        let mut dur = 0;
        loop {
            if dur >= uit() {
                break;
            }
            // idle mode
            match timeout(std::time::Duration::from_secs(uit() / 10), async {
                ch_rcv.recv().await
            })
            .await
            {
                Err(_) => dur += uit() / 10,
                Ok(None) => break,
                _ => {
                    dur = 0;
                    continue;
                }
            };
            // check if connection is alive using peek
            stream_ghost.peek().await?;
        }

        Err::<(), tokio::io::Error>(tokio::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "Connection idle timeout",
        ))
    };

    let buf_size = if target.port() == 53 || target.port() == 853 {
        // DNS does not require big buffer size
        8
    } else {
        config.udp_proxy_buffer_size.unwrap_or(8)
    };

    // proxy UDP
    if let Err(e) = tokio::try_join!(
        timeout_handler,
        udputils::copy_t2u(&udp, unsafe_refmut(&stream), ch_snd.clone(), buf_size),
        udputils::copy_u2t(&udp, unsafe_refmut(&stream), ch_snd)
    ) {
        let _ = stream.shutdown().await;
        return Err(e);
    };

    Ok(())
}
