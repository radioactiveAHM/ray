use std::{
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6},
    sync::Arc,
};

use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    time::timeout,
};
use tokio_rustls::{
    TlsAcceptor,
    rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
};

use hickory_resolver::{
    Resolver, config::NameServerConfigGroup, name_server::TokioConnectionProvider,
};

mod auth;
mod config;
mod mux;
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

fn log() -> bool {
    unsafe { LOG }
}

fn uit() -> u64 {
    unsafe { UIT }
}

fn resolver_mode() -> config::ResolvingMode {
    unsafe { RESOLVER_MODE }
}

#[tokio::main]
async fn main() {
    // Load config and convert to &'static
    let c = config::load_config();
    let config: &'static config::Config = utils::unsafe_staticref(&c);

    let resolver = Resolver::builder_with_config(
        hickory_resolver::config::ResolverConfig::from_parts(
            None,
            Vec::new(),
            NameServerConfigGroup::from_ips_clear(
                &[config.resolver.addr.ip()],
                config.resolver.addr.port(),
                true,
            ),
        ),
        TokioConnectionProvider::default(),
    )
    .build();

    let cresolver: &'static Resolver<
        hickory_resolver::name_server::GenericConnector<
            hickory_resolver::proto::runtime::TokioRuntimeProvider,
        >,
    > = utils::unsafe_staticref(&resolver);

    unsafe {
        LOG = config.log;
        UIT = config.udp_idle_timeout;
        RESOLVER_MODE = config.resolver.mode;
    }

    let tcp = tokio::net::TcpListener::bind(config.listen).await.unwrap();

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
        let acceptor = TlsAcceptor::from(Arc::new(c));

        loop {
            match tls::Tc::new(acceptor.clone(), tcp.accept().await) {
                Ok(tc) => {
                    tokio::spawn(async move {
                        if let Err(e) = tls_handler(tc, config, cresolver).await {
                            if log() {
                                println!("DoH server<TLS>: {e}")
                            }
                        }
                    });
                }
                Err(e) => {
                    if log() {
                        println!("DoH server<TLS>: {e}")
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
                                println!("{e}");
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
    resolver: &'static Resolver<
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
    resolver: &'static Resolver<
        hickory_resolver::name_server::GenericConnector<
            hickory_resolver::proto::runtime::TokioRuntimeProvider,
        >,
    >,
) -> tokio::io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
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

    let mut vless = vless::Vless::new(&buff[..size], resolver).await?;
    if auth::authenticate(config, &vless) {
        return Err(verror::VError::AuthenticationFailed.into());
    }

    if let Err(e) = match vless.rt {
        vless::SocketType::TCP => handle_tcp(vless, buff, size, stream, config).await,
        vless::SocketType::UDP => {
            vless.target.as_mut().unwrap().1 += 2;
            handle_udp(vless, buff, size, stream).await
        }
        vless::SocketType::MUX => mux::xudp(stream, buff[..size].to_vec(), resolver).await,
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
    stream: S,
    config: &'static config::Config,
) -> tokio::io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (target, body) = vless.target.as_ref().unwrap();
    let mut target = tokio::net::TcpStream::connect(target).await?;

    let _ = target.write(&buff[*body..size]).await?;
    target.flush().await?;
    drop(buff);

    let (mut client_read, mut client_write) = tokio::io::split(stream);
    let (mut target_read, target_write) = tokio::io::split(target);

    let (ch_snd, mut ch_rcv) = tokio::sync::mpsc::channel(10);

    let timeout_handler = async move {
        loop {
            match timeout(
                std::time::Duration::from_secs(config.tcp_idle_timeout),
                async { ch_rcv.recv().await },
            )
            .await
            {
                Err(_) => break,
                Ok(None) => break,
                _ => continue,
            };
        }

        Err::<(), tokio::io::Error>(tokio::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "Connection idle timeout",
        ))
    };

    let _ = client_write.write(&[0, 0]).await?;

    // A timeout controller listens for both upload and download activities. If there is no upload or download activity for a specified duration, the connection will be closed.
    let mut tcpwriter_client = tcp::TcpWriterGeneric {
        hr: client_write,
        signal: ch_snd.clone(),
    };

    let mut tcpwriter_target = tcp::TcpWriterGeneric {
        hr: target_write,
        signal: ch_snd,
    };

    tokio::try_join!(
        timeout_handler,
        tokio::io::copy(&mut client_read, &mut tcpwriter_target),
        tokio::io::copy(&mut target_read, &mut tcpwriter_client),
    )?;

    Ok(())
}

async fn handle_udp<S>(
    vless: vless::Vless,
    buff: Vec<u8>,
    size: usize,
    stream: S,
) -> tokio::io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
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

    let timeout_handler = async move {
        loop {
            match timeout(std::time::Duration::from_secs(uit()), async {
                ch_rcv.recv().await
            })
            .await
            {
                Err(_) => break,
                Ok(None) => break,
                _ => continue,
            };
        }

        Err::<(), tokio::io::Error>(tokio::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "Connection idle timeout",
        ))
    };

    // proxy UDP
    let (client_read, client_write) = tokio::io::split(stream);
    tokio::try_join!(
        timeout_handler,
        udputils::copy_t2u(&udp, client_read, ch_snd.clone()),
        udputils::copy_u2t(&udp, client_write, ch_snd)
    )?;

    Ok(())
}
