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
    } else {
        Err(VError::UnknownSocket)
    }
}
fn parse_target(buff: &[u8]) -> Result<(String, usize, SocketType), VError> {
    match buff[21] {
        1 => Ok((
            std::net::IpAddr::V4(std::net::Ipv4Addr::new(
                buff[22], buff[23], buff[24], buff[25],
            ))
            .to_string(),
            26,
            parse_socket(buff[18])?,
        )),
        2 => {
            if let Ok(s) = core::str::from_utf8(&buff[23..buff[22] as usize + 23]) {
                Ok((
                    s.to_string(),
                    23 + buff[22] as usize,
                    parse_socket(buff[18])?,
                ))
            } else {
                Err(VError::UTF8Err)
            }
        }
        3 => Ok((
            std::net::IpAddr::V6(std::net::Ipv6Addr::new(
                convert_two_u8s_to_u16_be([buff[22], buff[23]]),
                convert_two_u8s_to_u16_be([buff[24], buff[25]]),
                convert_two_u8s_to_u16_be([buff[26], buff[27]]),
                convert_two_u8s_to_u16_be([buff[28], buff[29]]),
                convert_two_u8s_to_u16_be([buff[30], buff[31]]),
                convert_two_u8s_to_u16_be([buff[32], buff[33]]),
                convert_two_u8s_to_u16_be([buff[34], buff[35]]),
                convert_two_u8s_to_u16_be([buff[36], buff[37]]),
            ))
            .to_string(),
            38,
            parse_socket(buff[18])?,
        )),
        _ => Err(VError::TargetErr),
    }
}

#[derive(Debug)]
pub enum SocketType {
    TCP,
    UDP,
}
#[derive(Debug)]
pub struct Vless {
    pub uuid: [u8; 16],
    pub port: u16,
    pub target: (String, usize, SocketType),
}

impl Vless {
    pub fn new(buff: &[u8]) -> Result<Self, VError> {
        if buff.is_empty() || buff.len() < 20 {
            return Err(VError::Unknown);
        }
        if buff[0] != 0 {
            return Err(VError::Unknown);
        }

        let mut v = Self {
            uuid: [0; 16],
            port: convert_two_u8s_to_u16_be([buff[19], buff[20]]),
            target: parse_target(buff)?,
        };

        v.uuid.copy_from_slice(&buff[1..17]);

        Ok(v)
    }
}
