use std::{
	collections::HashMap,
	net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6},
	pin::Pin,
};

use tokio::{
	io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadBuf},
	sync::Mutex,
};

use crate::{
	utils::{convert_two_u8s_to_u16_be, convert_u16_to_two_u8s_be},
	verror::VError,
};

// 0, 20, 0, 0, 1, 1, 2, 75, 102, 1, 74, 125, 250, 129, 176, 58, 211, 123, 232, 80, 69, 83, 0, 20, 0, 1, 0, 0, 33, 18, 164, 66, 112, 85, 120, 115, 112, 54, 113, 105, 113, 48, 70, 72
// |__|   |__|  |  |  |  |____|   |  |______________|    |______________________________|   |___|  |________________________________________________________________________________|
// H-Len   ID   T Opt NT  Port    AT     Address                    Global ID              Opt-Len                                         Opt body

#[inline(always)]
async fn parse_target(
	buff: &[u8],
	resolver: &crate::resolver::RS,
	domain_map: &Mutex<HashMap<IpAddr, String>>,
) -> Result<SocketAddr, VError> {
	let port = convert_two_u8s_to_u16_be([buff[5], buff[6]]);
	match buff[7] {
		1 => Ok(SocketAddr::V4(SocketAddrV4::new(
			Ipv4Addr::new(buff[8], buff[9], buff[10], buff[11]),
			port,
		))),
		2 => {
			if let Ok(s) = core::str::from_utf8(&buff[9..buff[8] as usize + 9]) {
				match crate::resolver::resolve(resolver, s, port).await {
					Ok(ip) => {
						let _ = domain_map.lock().await.insert(ip.ip(), s.to_string());
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

pub async fn xudp<S>(
	stream: S,
	payload: Vec<u8>,
	resolver: crate::resolver::RS,
	outbound: &'static str,
	peer_ip: IpAddr,
) -> tokio::io::Result<()>
where
	S: AsyncRead + AsyncWrite + Unpin,
{
	let ip = if peer_ip.is_ipv4() {
		IpAddr::V4(Ipv4Addr::UNSPECIFIED)
	} else {
		IpAddr::V6(Ipv6Addr::UNSPECIFIED)
	};
	let opt = crate::rules::get_opt(outbound);
	let udp = crate::udputils::udp_socket(SocketAddr::new(ip, 0), opt).await?;
	let domain_map: Mutex<HashMap<IpAddr, String>> = Mutex::new(HashMap::new());

	let (r_upbs, w_upbs) = (
		crate::CONFIG.udp_proxy_buffer_size.0 * 1024,
		crate::CONFIG.udp_proxy_buffer_size.1 * 1024,
	);

	let (mut client_r, mut client_w) = tokio::io::split(stream);
	client_w.write_all(&[0, 0]).await?;

	let mut internal_buffer: Vec<u8> = Vec::with_capacity(w_upbs);
	internal_buffer.extend_from_slice(&payload[19..]);
	drop(payload);

	handle_first_packet(&udp, &mut internal_buffer, &domain_map, &resolver).await?;

	let mut r_buf: Vec<u8> = vec![0; r_upbs];
	let mut w_buf = vec![0; w_upbs];
	let mut w_buf = ReadBuf::new(&mut w_buf);

	let mut client_r_pin = std::pin::Pin::new(&mut client_r);

	let err: tokio::io::Error;
	loop {
		match tokio::time::timeout(std::time::Duration::from_secs(crate::CONFIG.udp_idle_timeout), async {
			tokio::select! {
				res = copy_u2t(&udp, &mut client_w, &domain_map, &mut r_buf) => {
					res
				}
				res = handle_xudp_packets(&mut client_r_pin, &udp, &mut internal_buffer, &domain_map, &resolver, &mut w_buf) => {
					res
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

	Err(err)
}

#[inline(always)]
pub async fn copy_u2t<W>(
	udp: &tokio::net::UdpSocket,
	w: &mut W,
	domain_map: &Mutex<HashMap<IpAddr, String>>,
	buf: &mut [u8],
) -> tokio::io::Result<()>
where
	W: AsyncWrite + Unpin,
{
	let (packet_len, addr) = udp.recv_from(buf).await?;
	// K: Keep Sub Connections (Keep)
	//                       H Len   ID   K Opt UDP
	//                       |___|  |--|  |  |  |
	let mut head: [u8; 7] = [0, 12, 0, 0, 2, 1, 2];
	let port = convert_u16_to_two_u8s_be(addr.port());
	let addrtype_and_addr = {
		if let Some(domain) = domain_map.lock().await.get(&addr.ip()) {
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
	w.write_all(
		&[
			&head,
			port.as_slice(),
			&addrtype_and_addr,
			convert_u16_to_two_u8s_be(packet_len as u16).as_slice(),
			&buf[..packet_len],
		]
		.concat(),
	)
	.await?;
	w.flush().await
}

async fn handle_first_packet(
	udp: &tokio::net::UdpSocket,
	internal_buf: &mut Vec<u8>,
	domain_map: &Mutex<HashMap<IpAddr, String>>,
	resolver: &crate::resolver::RS,
) -> tokio::io::Result<()> {
	if internal_buf.len() > 2 {
		let head_size = convert_two_u8s_to_u16_be([internal_buf[0], internal_buf[1]]) as usize;
		if internal_buf.len() >= head_size + 2 {
			if internal_buf[2..5] == [0, 0, 1] || internal_buf[2..5] == [0, 0, 2] {
				// Stat: New Subjoin
				let target = parse_target(&internal_buf[2..], resolver, domain_map).await?;
				if internal_buf[5] == 1 {
					if let Some(opt_len_octet1) = internal_buf.get(head_size + 2)
						&& let Some(opt_len_octet2) = internal_buf.get(head_size + 3)
					{
						let opt_len = convert_two_u8s_to_u16_be([*opt_len_octet1, *opt_len_octet2]) as usize;
						let opt_body = &internal_buf[2 + head_size + 2..];
						if opt_body.len() >= opt_len {
							// send body and clean up
							let _ = udp.send_to(&opt_body[..opt_len], target).await?;
							internal_buf.drain(..2 + head_size + 2 + opt_len);
						} else {
							// opt body incomplete
							return Ok(());
						}
					} else {
						// opt first two bytes is not in buffer
						return Ok(());
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
	Ok(())
}

#[inline(always)]
async fn handle_xudp_packets<R>(
	r: &mut Pin<&mut R>,
	udp: &tokio::net::UdpSocket,
	internal_buf: &mut Vec<u8>,
	domain_map: &Mutex<HashMap<IpAddr, String>>,
	resolver: &crate::resolver::RS,
	buf: &mut ReadBuf<'_>,
) -> tokio::io::Result<()>
where
	R: AsyncRead + Unpin,
{
	if internal_buf.len() >= 1024 * 16 {
		return Err(crate::verror::VError::MuxBufferOverflow.into());
	}
	crate::pipe::Read(r, buf).await?;
	internal_buf.extend_from_slice(buf.filled());
	loop {
		if internal_buf.len() < 2 {
			break;
		};
		let head_size = convert_two_u8s_to_u16_be([internal_buf[0], internal_buf[1]]) as usize;
		if internal_buf.len() < head_size + 2 {
			// incomplete head
			break;
		}
		if internal_buf[2..5] == [0, 0, 1] || internal_buf[2..5] == [0, 0, 2] {
			// Stat: New Subjoin and Keep frames
			let target = parse_target(&internal_buf[2..], resolver, domain_map).await?;
			if internal_buf[5] == 1 {
				if let Some(opt_len_octet1) = internal_buf.get(head_size + 2)
					&& let Some(opt_len_octet2) = internal_buf.get(head_size + 3)
				{
					let opt_len = convert_two_u8s_to_u16_be([*opt_len_octet1, *opt_len_octet2]) as usize;
					let opt_body = &internal_buf[2 + head_size + 2..];
					if opt_body.len() >= opt_len {
						// send body and clean up
						let _ = udp.send_to(&opt_body[..opt_len], target).await?;
						internal_buf.drain(..2 + head_size + 2 + opt_len);
					} else {
						// opt body incomplete
						break;
					}
				} else {
					// opt first two bytes is not in buffer
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
	Ok(())
}
