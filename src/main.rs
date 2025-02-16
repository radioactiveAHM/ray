use std::{net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6}, str::FromStr};

use tokio::io::{AsyncReadExt, AsyncWriteExt};

mod  verror;
mod utils;
mod vless;
mod config;
mod auth;
mod udputils;

#[tokio::main]
async fn main() {
    // Load config and convert to &'static
    let c = config::load_config();
    let config: &'static config::Config = utils::unsafe_staticref(&c);

    let tcp = tokio::net::TcpListener::bind(config.listen).await.unwrap();
    
    loop {
        if let Ok((stream, _)) = tcp.accept().await {
            tokio::spawn(async move{
                if let Err(e) = stream_handler(stream, config).await {
                    if config.log {
                        println!("{e}");
                    }
                }
            });
        }
    }
}

async fn stream_handler(mut stream: tokio::net::TcpStream, config: &'static config::Config) -> tokio::io::Result<()> {
    let mut buff = vec![0;1024*8];
    let size = stream.read(&mut buff).await?;

    let mut vless = vless::Vless::new(&buff[..size])?;
    if auth::authenticate(config, &vless) {
        return Err(verror::VError::AuthenticationFailed.into());
    }

    match vless.target.2 {
        vless::SocketType::TCP => {
            handle_tcp(vless, buff, size, stream).await?;
        },
        vless::SocketType::UDP => {
            vless.target.1 += 2;
            handle_udp(vless, buff, size, stream).await?;
        }
    }

    Ok(())
}

async fn handle_tcp(vless: vless::Vless, buff: Vec<u8>, size: usize, mut stream: tokio::net::TcpStream) -> tokio::io::Result<()> {
    let mut target = tokio::net::TcpStream::connect(format!("{}:{}", &vless.target.0, vless.port)).await?;

    target.write(&buff[vless.target.1..size]).await?;
    target.flush().await?;
    drop(buff);

    let (mut client_read, mut client_write) = stream.split();
    let (mut target_read, mut target_write) = target.split();
    
    client_write.write(&[0, 0]).await?;
    tokio::try_join!(
        tokio::io::copy(&mut client_read, &mut target_write),
        tokio::io::copy(&mut target_read, &mut client_write),
    )?;

    Ok(())
}

async fn handle_udp(vless: vless::Vless, buff: Vec<u8>, size: usize, mut stream: tokio::net::TcpStream) -> tokio::io::Result<()> {
    let addr = std::net::SocketAddr::from_str(&format!("{}:{}", &vless.target.0, vless.port));
    if addr.is_err() {
        return Err(tokio::io::Error::other(addr.unwrap_err()));
    }
    let addrtype = {
        let a = addr.as_ref().unwrap();
        if a.is_ipv4() {
            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))
        } else if a.is_ipv6() {
            SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, 0, 0, 0))
        } else {
            return Err(tokio::io::Error::other(verror::VError::WTF));
        }
    };
    let udp = tokio::net::UdpSocket::bind(addrtype).await?;
    udp.connect(addr.unwrap()).await?;

    if vless.target.1 <= size {
        if !&buff[vless.target.1..size].is_empty(){
            udp.send(&buff[vless.target.1..size]).await?;
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