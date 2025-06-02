use std::{
    cell::RefCell,
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6},
    pin::Pin,
};

use tokio::{
    io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadBuf},
    time::timeout,
};

use crate::{
    utils::{
        convert_two_u8s_to_u16_be, convert_u16_to_two_u8s_be, unsafe_refmut, unsafe_staticref,
    },
    verror::VError,
};

// 0, 20, 0, 0, 1, 1, 2, 75, 102, 1, 74, 125, 250, 129, 176, 58, 211, 123, 232, 80, 69, 83, 0, 20, 0, 1, 0, 0, 33, 18, 164, 66, 112, 85, 120, 115, 112, 54, 113, 105, 113, 48, 70, 72
// |__|   |__|  |  |  |  |____|   |  |______________|    |______________________________|   |___|  |________________________________________________________________________________|
// H-Len   ID   T Opt NT  Port    AT     Address                    Global ID              Opt-Len                                         Opt body

#[inline(always)]
async fn parse_target(
    buff: &[u8],
    resolver: &'static hickory_resolver::Resolver<
        hickory_resolver::name_server::GenericConnector<
            hickory_resolver::proto::runtime::TokioRuntimeProvider,
        >,
    >,
    blacklist: &Option<Vec<crate::config::BlackList>>,
    domain_map: RefCell<HashMap<IpAddr, String>>,
) -> Result<SocketAddr, VError> {
    let port = convert_two_u8s_to_u16_be([buff[5], buff[6]]);
    match buff[7] {
        1 => Ok(SocketAddr::V4(SocketAddrV4::new(
            Ipv4Addr::new(buff[8], buff[9], buff[10], buff[11]),
            port,
        ))),
        2 => {
            if let Ok(s) = core::str::from_utf8(&buff[9..buff[8] as usize + 9]) {
                if let Some(bl) = blacklist {
                    // if there is a blacklist
                    crate::blacklist::containing(bl, s)?;
                }
                match crate::resolver::resolve(resolver, s, port).await {
                    Ok(ip) => {
                        let _ = domain_map.borrow_mut().insert(ip.ip(), s.to_string());
                        Ok(ip)
                    }
                    Err(e) => Err(e),
                }
            } else {
                Err(VError::UTF8Err)
            }
        }
        3 => Ok(SocketAddr::V6(SocketAddrV6::new(
            Ipv6Addr::new(
                convert_two_u8s_to_u16_be([buff[8], buff[9]]),
                convert_two_u8s_to_u16_be([buff[10], buff[11]]),
                convert_two_u8s_to_u16_be([buff[12], buff[13]]),
                convert_two_u8s_to_u16_be([buff[14], buff[15]]),
                convert_two_u8s_to_u16_be([buff[16], buff[17]]),
                convert_two_u8s_to_u16_be([buff[18], buff[19]]),
                convert_two_u8s_to_u16_be([buff[20], buff[21]]),
                convert_two_u8s_to_u16_be([buff[22], buff[23]]),
            ),
            port,
            0,
            0,
        ))),
        _ => Err(VError::TargetErr),
    }
}

#[inline(never)]
pub async fn xudp<S>(
    mut stream: S,
    mut buffer: Vec<u8>,
    resolver: &'static hickory_resolver::Resolver<
        hickory_resolver::name_server::GenericConnector<
            hickory_resolver::proto::runtime::TokioRuntimeProvider,
        >,
    >,
    blacklist: &Option<Vec<crate::config::BlackList>>,
    buf_size: usize,
    interface: Option<String>,
    peer_ip: IpAddr 
) -> tokio::io::Result<()>
where
    S: AsyncRead + crate::PeekWraper + AsyncWrite + Unpin + Send + 'static,
{
    // remove vless head
    buffer.drain(..19);
    // tcp idle controller
    let (ch_snd, mut ch_rcv) = tokio::sync::mpsc::channel(10);
    let stream_ghost = unsafe_staticref(&stream);
    let timeout_handler = async move {
        let mut dur = 0;
        loop {
            if dur >= crate::uit() {
                break;
            }
            // idle mode
            match timeout(std::time::Duration::from_secs(crate::uit() / 10), async {
                ch_rcv.recv().await
            })
            .await
            {
                Err(_) => dur += crate::uit() / 10,
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
    
    let ip = if let Some(interface) = interface {
        crate::tcp::get_interface(peer_ip.is_ipv4(), interface)
    } else {
        if peer_ip.is_ipv4() {
            IpAddr::V4(Ipv4Addr::UNSPECIFIED)
        } else {
            IpAddr::V6(Ipv6Addr::UNSPECIFIED)
        }
    };
    let udp = tokio::net::UdpSocket::bind(SocketAddr::new(ip, 0)).await?;
    let domain_map: RefCell<HashMap<IpAddr, String>> = RefCell::new(HashMap::new());

    if let Err(e) = tokio::try_join!(
        timeout_handler,
        copy_t2u(
            &udp,
            unsafe_refmut(&stream),
            buffer,
            ch_snd.clone(),
            domain_map.clone(),
            buf_size,
            resolver,
            blacklist
        ),
        copy_u2t(&udp, unsafe_refmut(&stream), ch_snd, domain_map.clone())
    ) {
        let _ = stream.shutdown().await;
        return Err(e);
    };

    Ok(())
}

#[inline(always)]
pub async fn copy_u2t<W>(
    udp: &tokio::net::UdpSocket,
    w: &mut W,
    ch_snd: tokio::sync::mpsc::Sender<()>,
    domain_map: RefCell<HashMap<IpAddr, String>>,
) -> tokio::io::Result<()>
where
    W: AsyncWrite + Unpin + Send,
{
    let _ = w.write([0, 0].as_slice()).await?;
    // K: Keep Sub Connections (Keep)
    //                       H Len   ID   K Opt UDP
    //                       |___|  |--|  |  |  |
    let mut head: [u8; 7] = [0, 12, 0, 0, 2, 1, 2];
    let mut buff = [0; 1024 * 8];
    loop {
        let (packet_len, addr) = udp.recv_from(&mut buff).await?;
        let _ = ch_snd.try_send(());
        let port = convert_u16_to_two_u8s_be(addr.port());
        let addrtype_and_addr = {
            if let Some(domain) = domain_map.borrow().get(&addr.ip()) {
                let mut target_addr = vec![2, 0];
                target_addr[1] = domain.len() as u8;
                [head[0], head[1]] = convert_u16_to_two_u8s_be((domain.len() + 1 + 12) as u16);
                target_addr.extend_from_slice(domain.as_bytes());
                target_addr
            } else {
                let mut target_addr = vec![1];
                match addr {
                    SocketAddr::V4(v4) => {
                        head[1] = 12;
                        target_addr.extend_from_slice(&v4.ip().octets());
                    }
                    SocketAddr::V6(v6) => {
                        head[1] = 24;
                        target_addr[0] = 3;
                        target_addr.extend_from_slice(&v6.ip().octets());
                    }
                }
                target_addr
            }
        };
        let _ = w
            .write(
                &[
                    &head,
                    port.as_slice(),
                    &addrtype_and_addr,
                    convert_u16_to_two_u8s_be(packet_len as u16).as_slice(),
                    &buff[..packet_len],
                ]
                .concat(),
            )
            .await?;
    }
}

#[inline(always)]
pub async fn copy_t2u<R>(
    udp: &tokio::net::UdpSocket,
    mut r: R,
    b0: Vec<u8>,
    ch_snd: tokio::sync::mpsc::Sender<()>,
    domain_map: RefCell<HashMap<IpAddr, String>>,
    buf_size: usize,
    resolver: &'static hickory_resolver::Resolver<
        hickory_resolver::name_server::GenericConnector<
            hickory_resolver::proto::runtime::TokioRuntimeProvider,
        >,
    >,
    blacklist: &Option<Vec<crate::config::BlackList>>,
) -> tokio::io::Result<()>
where
    R: AsyncRead + Unpin + Send,
{
    let mut b = Vec::with_capacity(buf_size * 1024);
    b.extend_from_slice(&b0);

    handle_xudp_packets(
        Pin::new(&mut r),
        udp,
        ch_snd,
        b,
        domain_map,
        resolver,
        blacklist,
    )
    .await
}

#[inline(always)]
async fn handle_xudp_packets<R>(
    mut r: Pin<&mut R>,
    udp: &tokio::net::UdpSocket,
    ch_snd: tokio::sync::mpsc::Sender<()>,
    mut internal_buf: Vec<u8>,
    domain_map: RefCell<HashMap<IpAddr, String>>,
    resolver: &'static hickory_resolver::Resolver<
        hickory_resolver::name_server::GenericConnector<
            hickory_resolver::proto::runtime::TokioRuntimeProvider,
        >,
    >,
    blacklist: &Option<Vec<crate::config::BlackList>>,
) -> tokio::io::Result<()>
where
    R: AsyncRead + Unpin + Send,
{
    // --> handle first packet if avaliable
    if internal_buf.len() > 2 {
        let head_size = convert_two_u8s_to_u16_be([internal_buf[0], internal_buf[1]]) as usize;
        if internal_buf.len() >= head_size + 2 {
            if internal_buf[2..5] == [0, 0, 1] || internal_buf[2..5] == [0, 0, 2] {
                // Stat: New Subjoin
                let target =
                    parse_target(&internal_buf[2..], resolver, blacklist, domain_map.clone())
                        .await?;
                let opt = internal_buf[5] == 1;
                if opt {
                    let opt_len = convert_two_u8s_to_u16_be([
                        internal_buf[head_size + 2],
                        internal_buf[head_size + 3],
                    ]) as usize;
                    let opt_body = &internal_buf[2 + head_size + 2..];
                    if opt_body.len() >= opt_len {
                        // send body and clean up
                        let _ = udp.send_to(&opt_body[..opt_len], target).await?;
                        internal_buf.drain(..2 + head_size + 2 + opt_len);
                    }
                } else {
                    // no body, remove header and continue
                    internal_buf.drain(..2 + head_size);
                }
            } else if internal_buf[2..5] == [0, 0, 4] || internal_buf[2..5] == [0, 0, 3] {
                // KeepAlive
                // head len: 4
                internal_buf.drain(..2 + 4);
            } else {
                return Err(crate::verror::VError::MuxError.into());
            }
        }
    };
    // <--

    let mut buf = [0; 1024 * 8];
    let mut wrapper = ReadBuf::new(&mut buf);
    loop {
        if internal_buf.len() >= 1024 * 16 {
            return Err(crate::verror::VError::MuxBufferOverflow.into());
        }
        match std::future::poll_fn(|cx| match r.as_mut().poll_read(cx, &mut wrapper) {
            std::task::Poll::Pending => std::task::Poll::Pending,
            std::task::Poll::Ready(Ok(_)) => {
                let _ = ch_snd.try_send(());
                if wrapper.filled().is_empty() {
                    std::task::Poll::Pending
                } else {
                    std::task::Poll::Ready(Ok(()))
                }
            }
            std::task::Poll::Ready(Err(e)) => std::task::Poll::Ready(Err(e)),
        })
        .await
        {
            Err(e) => return Err(e),
            Ok(_) => {
                internal_buf.extend_from_slice(wrapper.filled());
                wrapper.clear();
                loop {
                    if internal_buf.len() < 2 {
                        break;
                    };
                    let head_size =
                        convert_two_u8s_to_u16_be([internal_buf[0], internal_buf[1]]) as usize;
                    if internal_buf.len() < head_size + 2 {
                        // incomplete head
                        break;
                    }
                    if internal_buf[2..5] == [0, 0, 1] || internal_buf[2..5] == [0, 0, 2] {
                        // Stat: New Subjoin and Keep frames
                        let target = parse_target(
                            &internal_buf[2..],
                            resolver,
                            blacklist,
                            domain_map.clone(),
                        )
                        .await?;
                        let opt = internal_buf[5] == 1;
                        if opt {
                            let opt_len = convert_two_u8s_to_u16_be([
                                internal_buf[head_size + 2],
                                internal_buf[head_size + 3],
                            ]) as usize;
                            let opt_body = &internal_buf[2 + head_size + 2..];
                            if opt_body.len() >= opt_len {
                                // send body and clean up
                                let _ = udp.send_to(&opt_body[..opt_len], target).await?;
                                internal_buf.drain(..2 + head_size + 2 + opt_len);
                            } else {
                                break;
                            }
                        } else {
                            // no body, remove header and continue
                            internal_buf.drain(..2 + head_size);
                        }
                    } else if internal_buf[2..5] == [0, 0, 4] || internal_buf[2..5] == [0, 0, 3] {
                        // KeepAlive and End frames
                        // head len: 4
                        internal_buf.drain(..2 + 4);
                    } else {
                        return Err(crate::verror::VError::MuxError.into());
                    }
                }
            }
        }
    }
}
