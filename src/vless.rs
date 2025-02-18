use std::net::IpAddr;

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
async fn parse_target(buff: &[u8]) -> Result<(IpAddr, usize), VError> {
    match buff[21] {
        1 => Ok((
            std::net::IpAddr::V4(std::net::Ipv4Addr::new(
                buff[22], buff[23], buff[24], buff[25],
            )),
            26,
        )),
        2 => {
            if let Ok(s) = core::str::from_utf8(&buff[23..buff[22] as usize + 23]) {
                let resolve = tokio::net::lookup_host(format!("{s}:443")).await;
                if resolve.is_err() {
                    return Err(VError::NoHost);
                }
                let ip = resolve.unwrap().next();
                if ip.is_none() {
                    return Err(VError::NoHost);
                }
                Ok((ip.unwrap().ip(), 23 + buff[22] as usize))
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
            38,
        )),
        _ => Err(VError::TargetErr),
    }
}

#[derive(Debug)]
pub enum SocketType {
    TCP,
    UDP,
    MUX,
}
#[derive(Debug)]
pub struct Vless {
    pub uuid: [u8; 16],
    pub port: u16,
    pub rt: SocketType,
    pub target: Option<(IpAddr, usize)>,
}

impl Vless {
    pub async fn new(buff: &[u8]) -> Result<Self, VError> {
        if buff.is_empty() {
            return Err(VError::Unknown);
        }
        if buff[0] != 0 {
            return Err(VError::Unknown);
        }

        if buff.len() == 19 && buff[buff.len() - 1] == 3 {
            // mux
            let mut v = Self {
                uuid: [0; 16],
                port: 0,
                rt: parse_socket(buff[18])?,
                target: None,
            };

            v.uuid.copy_from_slice(&buff[1..17]);

            return Ok(v);
        }

        if buff.len() < 20 {
            return Err(VError::Unknown);
        }

        let mut v = Self {
            uuid: [0; 16],
            port: convert_two_u8s_to_u16_be([buff[19], buff[20]]),
            rt: parse_socket(buff[18])?,
            target: Some(parse_target(buff).await?),
        };

        v.uuid.copy_from_slice(&buff[1..17]);

        Ok(v)
    }
}
