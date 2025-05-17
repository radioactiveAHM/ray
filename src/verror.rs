use std::fmt::Display;

#[derive(Debug)]
pub enum VError {
    Unknown,
    UTF8Err,
    TargetErr,
    UnknownSocket,
    AuthenticationFailed,
    Wtf,
    TransporterError,
    MailFormedSingboxMuxPacket,
    MailFormedXrayMuxPacket,
    NoHost,
    ResolveDnsFailed,
    YameteKudasai,
    BufferOverflow,
    UdpDeadLoop,
    MailFormedUdpPacket,
    DomainInBlacklist
}
impl Display for VError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VError::Unknown => write!(f, "Unknown"),
            VError::UTF8Err => write!(f, "UTF8Err"),
            VError::TargetErr => write!(f, "TargetErr"),
            VError::UnknownSocket => write!(f, "UnknownSocket"),
            VError::AuthenticationFailed => write!(f, "AuthenticationFailed"),
            VError::Wtf => write!(f, "WTF"),
            VError::TransporterError => write!(f, "TransporterError"),
            VError::MailFormedSingboxMuxPacket => write!(f, "MailFormedSingboxMuxPacket"),
            VError::MailFormedXrayMuxPacket => write!(f, "MailFormedXrayMuxPacket"),
            VError::UdpDeadLoop => write!(f, "UdpDeadLoop"),
            VError::MailFormedUdpPacket => write!(f, "MailFormedUdpPacket"),
            VError::NoHost => write!(f, "NoHost"),
            VError::ResolveDnsFailed => write!(f, "ResolveDnsFailed"),
            VError::YameteKudasai => write!(f, "YameteKudasai"),
            VError::BufferOverflow => write!(f, "BufferOverflow"),
            VError::DomainInBlacklist => write!(f, "Domain In Blacklist")
        }
    }
}
impl std::error::Error for VError {}

impl From<VError> for tokio::io::Error {
    fn from(e: VError) -> Self {
        tokio::io::Error::other(e)
    }
}
