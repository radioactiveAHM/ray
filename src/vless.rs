use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

use crate::utils::convert_two_u8s_to_u16_be;
use crate::verror::VError;

// UUID 1-17 | 17 - (22|23) details
// uuid::uuid!("a18b0775-2669-5cfc-b5e8-99bd5fd70884").as_bytes()

// 17 - 23 details
// 17 & 18: [0, 1]
// 19 & 20: port as two u8
// 21: type of target (1 = ipv4 [22..26]) (2 = domian [23..x] which 22 is len) (3 = ipv6 [22..38])

fn parse_socket(s: u8) -> Result<SocketType, VError> {
    if s == 1 {
        Ok(SocketType::TCP)
    } else if s == 2 {
        Ok(SocketType::UDP)
    } else if s == 3 {
        Ok(SocketType::MUX)
    } else {
        Err(VError::UnknownSocket)
    }
}
async fn parse_target(
    buff: &[u8],
    port: u16,
    resolver: &'static hickory_resolver::Resolver<
        hickory_resolver::name_server::GenericConnector<
            hickory_resolver::proto::runtime::TokioRuntimeProvider,
        >,
    >,
) -> Result<(SocketAddr, usize), VError> {
    match buff[21] {
        1 => Ok((
            SocketAddr::V4(SocketAddrV4::new(
                Ipv4Addr::new(buff[22], buff[23], buff[24], buff[25]),
                port,
            )),
            26,
        )),
        2 => {
            if let Ok(s) = core::str::from_utf8(&buff[23..buff[22] as usize + 23]) {
                match crate::resolver::resolve(resolver, s, port).await {
                    Ok(ip) => Ok((ip, 23 + buff[22] as usize)),
                    Err(e) => Err(e),
                }
            } else {
                Err(VError::UTF8Err)
            }
        }
        3 => Ok((
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
        )),
        _ => Err(VError::TargetErr),
    }
}

#[derive(Debug)]
#[allow(clippy::upper_case_acronyms)]
pub enum SocketType {
    TCP,
    UDP,
    MUX,
}
#[derive(Debug)]
pub struct Vless {
    pub uuid: [u8; 16],
    pub rt: SocketType,
    pub target: Option<(SocketAddr, usize)>,
}

impl Vless {
    pub async fn new(
        buff: &[u8],
        resolver: &'static hickory_resolver::Resolver<
            hickory_resolver::name_server::GenericConnector<
                hickory_resolver::proto::runtime::TokioRuntimeProvider,
            >,
        >,
    ) -> Result<Self, VError> {
        if buff.is_empty() {
            return Err(VError::Unknown);
        }
        if buff[0] != 0 {
            return Err(VError::Unknown);
        }

        if buff.len() >= 19 && buff[18] == 3 {
            // mux
            let mut v = Self {
                uuid: [0; 16],
                rt: parse_socket(buff[18])?,
                target: None,
            };

            v.uuid.copy_from_slice(&buff[1..17]);

            return Ok(v);
        }

        if buff.len() < 20 {
            return Err(VError::Unknown);
        }
        let port = convert_two_u8s_to_u16_be([buff[19], buff[20]]);
        let mut v = Self {
            uuid: [0; 16],
            rt: parse_socket(buff[18])?,
            target: Some(parse_target(buff, port, resolver).await?),
        };

        v.uuid.copy_from_slice(&buff[1..17]);

        Ok(v)
    }
}
