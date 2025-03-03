mod singbox;
mod xray;

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

use tokio::{io::{AsyncReadExt, AsyncWriteExt}, time::timeout};

use crate::{utils::{convert_two_u8s_to_u16_be, convert_u16_to_two_u8s_be}, verror::VError};

async fn parse_target(buff: &[u8], port: u16) -> Result<(SocketAddr, usize, usize), VError> {
    match buff[9] {
        1 => Ok((
            SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::new(buff[10], buff[11], buff[12], buff[13]),
                port,
            )),
            convert_two_u8s_to_u16_be([buff[14], buff[15]]) as usize,
            16,
        )),
        2 => {
            if let Ok(s) = core::str::from_utf8(&buff[11..buff[10] as usize + 11]) {
                let resolve = tokio::net::lookup_host(format!("{s}:{port}")).await;
                if resolve.is_err() {
                    if crate::log() {
                        println!("ResolveDnsFailed for {}", s);
                    }
                    return Err(VError::ResolveDnsFailed);
                }
                let ip = resolve.unwrap().collect::<Vec<SocketAddr>>();
                if ip.is_empty() {
                    if crate::log() {
                        println!("NoHost for {}", s);
                    }
                    return Err(VError::NoHost);
                }

                Ok((
                    ip[0],
                    convert_two_u8s_to_u16_be([
                        buff[11 + buff[10] as usize],
                        buff[12 + buff[10] as usize],
                    ]) as usize,
                    buff[10] as usize + 13,
                ))
            } else {
                Err(VError::UTF8Err)
            }
        }
        3 => Ok((
            SocketAddr::V6(SocketAddrV6::new(
                Ipv6Addr::new(
                    convert_two_u8s_to_u16_be([buff[10], buff[11]]),
                    convert_two_u8s_to_u16_be([buff[12], buff[13]]),
                    convert_two_u8s_to_u16_be([buff[14], buff[15]]),
                    convert_two_u8s_to_u16_be([buff[16], buff[17]]),
                    convert_two_u8s_to_u16_be([buff[18], buff[19]]),
                    convert_two_u8s_to_u16_be([buff[20], buff[21]]),
                    convert_two_u8s_to_u16_be([buff[22], buff[23]]),
                    convert_two_u8s_to_u16_be([buff[24], buff[25]]),
                ),
                port,
                0,
                0,
            )),
            convert_two_u8s_to_u16_be([buff[26], buff[27]]) as usize,
            28,
        )),
        _ => Err(VError::TargetErr),
    }
}

pub async fn xudp(
    mut stream: tokio::net::TcpStream,
    mut buffer: Vec<u8>,
) -> tokio::io::Result<()> {
    let (mut client_read, client_write) = stream.split();

    let (ch_snd, mut ch_rcv) = tokio::sync::mpsc::channel(1);

    let timeout_handler = async move {
        loop {
            match timeout(std::time::Duration::from_secs(crate::uit()), async {
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

    if buffer.len() == 19 {
        drop(buffer);
        // singbox way
        let udp: tokio::net::UdpSocket;
        let head: Vec<u8>;
        {
            let mut buff = [0; 1024 * 8];
            let size = client_read.read(&mut buff).await?;

            let mut mux_id = [0; 6];
            mux_id.copy_from_slice(&buff[1..7]);
            let port = convert_two_u8s_to_u16_be([buff[7], buff[8]]);
            let target = parse_target(&buff[..size], port).await?;

            let addrtype = {
                if target.0.is_ipv4() {
                    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))
                } else if target.0.is_ipv6() {
                    SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, 0, 0, 0))
                } else {
                    return Err(tokio::io::Error::other(VError::Wtf));
                }
            };

            udp = tokio::net::UdpSocket::bind(addrtype).await?;
            udp.connect(target.0).await?;

            buff[4] = 2;

            head = buff[..target.2 - 2].to_vec();
        }

        tokio::try_join!(
            timeout_handler,
            singbox::copy_t2u(&udp, client_read, &head, ch_snd.clone()),
            copy_u2t(&udp, client_write, &head, ch_snd)
        )?;
    } else if buffer.len() > 19 {
        // xray way
        buffer.drain(..19);

        let udp: tokio::net::UdpSocket;
        let mut head: Vec<u8>;
        {
            let mut mux_id = [0; 6];
            mux_id.copy_from_slice(&buffer[1..7]);
            let port = convert_two_u8s_to_u16_be([buffer[7], buffer[8]]);
            let target = parse_target(&buffer, port).await?;

            let addrtype = {
                if target.0.is_ipv4() {
                    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))
                } else if target.0.is_ipv6() {
                    SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, 0, 0, 0))
                } else {
                    return Err(tokio::io::Error::other(VError::Wtf));
                }
            };

            udp = tokio::net::UdpSocket::bind(addrtype).await?;
            udp.connect(target.0).await?;

            buffer[4] = 2;
            head = buffer[..target.2 - 2].to_vec();
            head[1] = 12;

            // packet with no size (I HATE XUDP)
            if buffer[1] == 20 {
                udp.send(&buffer[target.2..]).await?;
                buffer.clear();
            }
        }

        tokio::try_join!(
            timeout_handler,
            xray::copy_t2u(&udp, client_read, &head, buffer, ch_snd.clone()),
            copy_u2t(&udp, client_write, &head, ch_snd)
        )?;
    }

    Ok(())
}

pub async fn copy_u2t(
    udp: &tokio::net::UdpSocket,
    mut w: tokio::net::tcp::WriteHalf<'_>,
    head: &[u8],
    ch_snd: tokio::sync::mpsc::Sender<()>,
) -> tokio::io::Result<()> {
    let mut buff = [0; 1024 * 8];

    {
        // write first packet
        let size = udp.recv(&mut buff[2+head.len()+2..]).await?;
        let _ = ch_snd.try_send(());
        let octat = convert_u16_to_two_u8s_be(size as u16);
        buff[2..head.len()+2].copy_from_slice(head);
        buff[2+head.len()..2+head.len()+2].copy_from_slice(&octat);
        let _ = w.write(&buff[..size+2+head.len()+2]).await?;
        w.flush().await?;
    }

    buff[..head.len()].copy_from_slice(head);
    loop {
        let size = udp.recv(&mut buff[head.len()+2..]).await?;
        let _ = ch_snd.try_send(());
        let octat = convert_u16_to_two_u8s_be(size as u16);
        buff[head.len()..head.len()+2].copy_from_slice(&octat);
        let _ = w.write(&buff[..size+head.len()+2]).await?;
        w.flush().await?;
    }
}