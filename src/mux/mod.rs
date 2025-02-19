mod singbox;
mod xray;

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

use tokio::io::AsyncReadExt;

use crate::{
    utils::convert_two_u8s_to_u16_be,
    verror::VError,
};

async fn parse_target(buff: &[u8], port: u16) -> Result<(SocketAddr, usize, usize), VError> {
    match buff[9] {
        1 => Ok((
            SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::new(buff[10], buff[11], buff[12], buff[13]),
                port
            )),
            convert_two_u8s_to_u16_be([buff[14], buff[15]]) as usize,
            16,
        )),
        2 => {
            if let Ok(s) = core::str::from_utf8(&buff[11..buff[10] as usize + 11]) {
                let resolve = tokio::net::lookup_host(format!("{s}:{port}")).await;
                if resolve.is_err() {
                    return Err(VError::ResolveDnsFailed);
                }
                let ip = resolve.unwrap().collect::<Vec<SocketAddr>>();
                if ip.is_empty() {
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
            SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::new(
                convert_two_u8s_to_u16_be([buff[10], buff[11]]),
                convert_two_u8s_to_u16_be([buff[12], buff[13]]),
                convert_two_u8s_to_u16_be([buff[14], buff[15]]),
                convert_two_u8s_to_u16_be([buff[16], buff[17]]),
                convert_two_u8s_to_u16_be([buff[18], buff[19]]),
                convert_two_u8s_to_u16_be([buff[20], buff[21]]),
                convert_two_u8s_to_u16_be([buff[22], buff[23]]),
                convert_two_u8s_to_u16_be([buff[24], buff[25]]),
            ), port, 0, 0)),
            convert_two_u8s_to_u16_be([buff[26], buff[27]]) as usize,
            28,
        )),
        _ => Err(VError::TargetErr),
    }
}

pub async fn mux_udp(mut stream: tokio::net::TcpStream, mut buffer: Vec<u8>) -> tokio::io::Result<()> {
    let (mut client_read, client_write) = stream.split();
    if buffer.len()==19 {
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
                    return Err(tokio::io::Error::other(VError::WTF));
                }
            };
        
            udp = tokio::net::UdpSocket::bind(addrtype).await?;
            udp.connect(target.0).await?;
            
            buff[4] = 2;

            head = buff[..target.2 - 2].to_vec();
        }
    
        tokio::try_join!(
            singbox::copy_t2u(&udp, client_read, &head),
            singbox::copy_u2t(&udp, client_write, &head)
        )?;
    } else if buffer.len() > 19{
        // xray way
        buffer.drain(..19);

        let udp: tokio::net::UdpSocket;
        let head: Vec<u8>;
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
                    return Err(tokio::io::Error::other(VError::WTF));
                }
            };

            udp = tokio::net::UdpSocket::bind(addrtype).await?;
            udp.connect(target.0).await?;

            buffer[4] = 2;
            head = buffer[..target.2 - 2].to_vec();
        }

        tokio::try_join!(
            xray::copy_t2u(&udp, client_read, &head, buffer),
            xray::copy_u2t(&udp, client_write, &head)
        )?;
    }

    Ok(())
}
