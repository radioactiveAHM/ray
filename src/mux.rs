use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::{
    utils::{convert_two_u8s_to_u16_be, convert_u16_to_two_u8s_be, Buffering},
    verror::VError,
};

// ______________________________________________________________________________________

pub async fn copy_u2t(
    udp: &tokio::net::UdpSocket,
    mut w: tokio::net::tcp::WriteHalf<'_>,
    head: &[u8],
) -> tokio::io::Result<()> {
    let mut buff = [0; 1024 * 8];
    let mut buff2 = [0; 1024 * 8];
    let mut b = Buffering(&mut buff2, 0);
    let mut first = true;

    loop {
        let size = udp.recv(&mut buff).await?;
        let octat = convert_u16_to_two_u8s_be(size as u16);
        if first {
            w.write(
                b.reset()
                    .write(&[0, 0])
                    .write(head)
                    .write(&octat)
                    .write(&buff[..size])
                    .get(),
            )
            .await?;
            first = false;
        } else {
            w.write(
                b.reset()
                    .write(head)
                    .write(&octat)
                    .write(&buff[..size])
                    .get(),
            )
            .await?;
        }
        w.flush().await?;
    }
}

struct UdpWriter<'a> {
    udp: &'a tokio::net::UdpSocket,
    b: Vec<u8>,
    head: &'a [u8],
}
impl AsyncWrite for UdpWriter<'_> {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<Result<usize, std::io::Error>> {
        if buf.is_empty() {
            return std::task::Poll::Ready(Ok(0));
        }
        // write buff into vec
        self.b.extend_from_slice(buf);

        let head_size = self.head.len();

        loop {
            if self.b.len() < head_size {
                break;
            } else if &buf[..head_size] != self.head {
                return std::task::Poll::Ready(Err(VError::MuxError.into()));
            }
            let psize =
                convert_two_u8s_to_u16_be([self.b[head_size], self.b[head_size + 1]]) as usize;
            if psize <= self.b.len() - head_size {
                // we have bytes to send
                let packet = &self.b[head_size + 2..psize + head_size + 2];
                match self.udp.poll_send(cx, packet) {
                    std::task::Poll::Pending => continue,
                    std::task::Poll::Ready(Ok(_)) => {
                        self.b.drain(0..psize + head_size + 2);
                        continue;
                    }
                    std::task::Poll::Ready(Err(e)) => {
                        self.b.clear();
                        return std::task::Poll::Ready(Err(e));
                    }
                }
            } else {
                // empty or incomplete bytes
                break;
            }
        }

        std::task::Poll::Ready(Ok(buf.len()))
    }

    #[allow(unused_variables)]
    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    #[allow(unused_variables)]
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        std::task::Poll::Ready(Ok(()))
    }
}

pub async fn copy_t2u(
    udp: &tokio::net::UdpSocket,
    mut r: tokio::net::tcp::ReadHalf<'_>,
    head: &[u8],
) -> tokio::io::Result<()> {
    let mut uw = UdpWriter {
        udp,
        b: Vec::with_capacity(128),
        head,
    };
    tokio::io::copy(&mut r, &mut uw).await?;

    Ok(())
}

// ______________________________________________________________________________________

async fn parse_target(buff: &[u8]) -> Result<(IpAddr, usize, usize), VError> {
    match buff[9] {
        1 => Ok((
            std::net::IpAddr::V4(std::net::Ipv4Addr::new(
                buff[10], buff[11], buff[12], buff[13],
            )),
            convert_two_u8s_to_u16_be([buff[14], buff[15]]) as usize,
            16,
        )),
        2 => {
            if let Ok(s) = core::str::from_utf8(&buff[11..buff[10] as usize + 11]) {
                let resolve = tokio::net::lookup_host(format!("{s}:443")).await;
                if resolve.is_err() {
                    return Err(VError::NoHost);
                }
                let ip = resolve.unwrap().next();
                if ip.is_none() {
                    return Err(VError::NoHost);
                }

                Ok((
                    ip.unwrap().ip(),
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
            std::net::IpAddr::V6(std::net::Ipv6Addr::new(
                convert_two_u8s_to_u16_be([buff[10], buff[11]]),
                convert_two_u8s_to_u16_be([buff[12], buff[13]]),
                convert_two_u8s_to_u16_be([buff[14], buff[15]]),
                convert_two_u8s_to_u16_be([buff[16], buff[17]]),
                convert_two_u8s_to_u16_be([buff[18], buff[19]]),
                convert_two_u8s_to_u16_be([buff[20], buff[21]]),
                convert_two_u8s_to_u16_be([buff[22], buff[23]]),
                convert_two_u8s_to_u16_be([buff[24], buff[25]]),
            )),
            convert_two_u8s_to_u16_be([buff[26], buff[27]]) as usize,
            28,
        )),
        _ => Err(VError::TargetErr),
    }
}

pub async fn mux_udp(mut stream: tokio::net::TcpStream) -> tokio::io::Result<()> {
    let (mut client_read, client_write) = stream.split();
    let mut buff = [0; 1024 * 8];
    let size = client_read.read(&mut buff).await?;

    if buff[0] != 0 {
        return Err(VError::Unknown.into());
    }

    let mut mux_id = [0; 6];
    mux_id.copy_from_slice(&buff[1..7]);
    let port = convert_two_u8s_to_u16_be([buff[7], buff[8]]);
    let target = parse_target(&buff[..size]).await?;

    if target.1 > buff.len() - buff[..target.2].len() {
        return Err(VError::Unknown.into());
    }

    let addrtype = {
        if target.0.is_ipv4() {
            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))
        } else if target.0.is_ipv6() {
            SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, 0, 0, 0))
        } else {
            return Err(tokio::io::Error::other(VError::WTF));
        }
    };

    let udp = tokio::net::UdpSocket::bind(addrtype).await?;
    udp.connect(SocketAddr::new(target.0, port)).await?;

    udp.send(&buff[target.2..size]).await?;

    buff[4] = 2;

    tokio::try_join!(
        copy_t2u(&udp, client_read, &buff[..target.2 - 2]),
        copy_u2t(&udp, client_write, &buff[..target.2 - 2])
    )?;

    Ok(())
}
