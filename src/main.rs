use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    time::timeout,
};

mod auth;
mod config;
mod mux;
mod tcp;
mod transporters;
mod udputils;
mod utils;
mod verror;
mod vless;

static mut LOG: bool = false;
static mut UIT: u64 = 15;

fn log() -> bool {
    unsafe { LOG }
}

fn uit() -> u64 {
    unsafe { UIT }
}

#[tokio::main]
async fn main() {
    // Load config and convert to &'static
    let c = config::load_config();
    let config: &'static config::Config = utils::unsafe_staticref(&c);

    unsafe {
        LOG = config.log;
        UIT = config.udp_idle_timeout;
    }

    let tcp = tokio::net::TcpListener::bind(config.listen).await.unwrap();

    loop {
        if let Ok((stream, _)) = tcp.accept().await {
            tokio::spawn(async move {
                if let Err(e) = stream_handler(stream, config).await {
                    if log() {
                        println!("{e}");
                    }
                }
            });
        }
    }
}

async fn stream_handler(
    mut stream: tokio::net::TcpStream,
    config: &'static config::Config,
) -> tokio::io::Result<()> {
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

    let mut vless = vless::Vless::new(&buff[..size]).await?;
    if auth::authenticate(config, &vless) {
        return Err(verror::VError::AuthenticationFailed.into());
    }

    let user_addr = stream.peer_addr()?;
    if let Err(e) = match vless.rt {
        vless::SocketType::TCP => handle_tcp(vless, buff, size, stream, config).await,
        vless::SocketType::UDP => {
            vless.target.as_mut().unwrap().1 += 2;
            handle_udp(vless, buff, size, stream).await
        }
        vless::SocketType::MUX => mux::mux_udp(stream, buff[..size].to_vec()).await,
    } {
        if log() {
            println!("{user_addr}: {e}")
        }
    } else {
        if log() {
            println!("{user_addr}: closed connection")
        }
    }

    Ok(())
}

async fn handle_tcp(
    vless: vless::Vless,
    buff: Vec<u8>,
    size: usize,
    mut stream: tokio::net::TcpStream,
    config: &'static config::Config,
) -> tokio::io::Result<()> {
    let (target, body) = vless.target.as_ref().unwrap();
    let mut target = tokio::net::TcpStream::connect(target).await?;

    let _ = target.write(&buff[*body..size]).await?;
    target.flush().await?;
    drop(buff);

    let (mut client_read, mut client_write) = stream.split();
    let (mut target_read, target_write) = target.split();

    let (ch_snd, mut ch_rcv) = tokio::sync::mpsc::channel(1);

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

    let mut tcpwriter = tcp::TcpWriter {
        hr: target_write,
        signal: ch_snd,
    };

    let _ = client_write.write(&[0, 0]).await?;
    tokio::try_join!(
        timeout_handler,
        tokio::io::copy(&mut client_read, &mut tcpwriter),
        tokio::io::copy(&mut target_read, &mut client_write),
    )?;

    Ok(())
}

async fn handle_udp(
    vless: vless::Vless,
    buff: Vec<u8>,
    size: usize,
    mut stream: tokio::net::TcpStream,
) -> tokio::io::Result<()> {
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

    // proxy UDP
    let (client_read, client_write) = stream.split();
    tokio::try_join!(
        udputils::copy_t2u(&udp, client_read),
        udputils::copy_u2t(&udp, client_write)
    )?;

    Ok(())
}
