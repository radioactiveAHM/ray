use std::fmt::Display;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

use crate::utils::convert_two_u8s_to_u16_be;
use crate::verror::VError;

const fn parse_socket(s: u8) -> Result<RequestCommand, VError> {
	if s == 1 {
		Ok(RequestCommand::TCP)
	} else if s == 2 {
		Ok(RequestCommand::UDP)
	} else if s == 3 {
		Ok(RequestCommand::MUX)
	} else {
		Err(VError::UnknownSocket)
	}
}
async fn parse_target(buff: &[u8], port: u16, resolver: &crate::resolver::RS) -> Result<(SocketAddr, usize), VError> {
	match buff.get(21).ok_or(VError::Unknown)? {
		1 => {
			if buff.len() < 26 {
				return Err(VError::Unknown);
			}
			Ok((
				SocketAddr::V4(SocketAddrV4::new(
					Ipv4Addr::new(buff[22], buff[23], buff[24], buff[25]),
					port,
				)),
				26,
			))
		}
		2 => {
			if buff.len() < buff[22] as usize + 23 {
				return Err(VError::Unknown);
			}
			if let Ok(s) = core::str::from_utf8(&buff[23..buff[22] as usize + 23]) {
				if let Some(bl) = &crate::CONFIG.blacklist {
					// if there is a blacklist
					crate::blacklist::containing(bl, s)?;
				}
				match crate::resolver::resolve(resolver, s, port).await {
					Ok(ip) => Ok((ip, 23 + buff[22] as usize)),
					Err(e) => Err(e),
				}
			} else {
				Err(VError::UTF8Err)
			}
		}
		3 => {
			if buff.len() < 38 {
				return Err(VError::Unknown);
			}
			Ok((
				SocketAddr::V6(SocketAddrV6::new(
					Ipv6Addr::new(
						convert_two_u8s_to_u16_be([buff[22], buff[23]]),
						convert_two_u8s_to_u16_be([buff[24], buff[25]]),
						convert_two_u8s_to_u16_be([buff[26], buff[27]]),
						convert_two_u8s_to_u16_be([buff[28], buff[29]]),
						convert_two_u8s_to_u16_be([buff[30], buff[31]]),
						convert_two_u8s_to_u16_be([buff[32], buff[33]]),
						convert_two_u8s_to_u16_be([buff[34], buff[35]]),
						convert_two_u8s_to_u16_be([buff[36], buff[37]]),
					),
					port,
					0,
					0,
				)),
				38,
			))
		}
		_ => Err(VError::TargetErr),
	}
}

#[allow(clippy::upper_case_acronyms)]
#[derive(Debug)]
pub enum RequestCommand {
	TCP,
	UDP,
	MUX,
}
impl Display for RequestCommand {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match *self {
			Self::TCP => write!(f, "TCP"),
			Self::UDP => write!(f, "UDP"),
			Self::MUX => write!(f, "XUDP"),
		}
	}
}

pub struct Vless {
	pub uuid: [u8; 16],
	pub rt: RequestCommand,
	pub target: Option<(SocketAddr, usize)>,
}

impl Vless {
	pub async fn new(buff: &[u8], resolver: &crate::resolver::RS) -> Result<Self, VError> {
		if buff.is_empty() {
			return Err(VError::Unknown);
		}
		if buff[0] != 0 {
			return Err(VError::Unknown);
		}

		if buff.len() >= 19 && *buff.get(18).ok_or(VError::Unknown)? == 3 {
			// mux
			let mut v = Self {
				uuid: [0; 16],
				rt: parse_socket(buff[18])?,
				target: None,
			};

			v.uuid.copy_from_slice(&buff[1..17]);

			return Ok(v);
		}

		let port = convert_two_u8s_to_u16_be([
			*buff.get(19).ok_or(VError::Unknown)?,
			*buff.get(20).ok_or(VError::Unknown)?,
		]);
		let mut v = Self {
			uuid: [0; 16],
			rt: parse_socket(buff[18])?,
			target: Some(parse_target(buff, port, resolver).await?),
		};

		v.uuid.copy_from_slice(&buff[1..17]);

		Ok(v)
	}
}
