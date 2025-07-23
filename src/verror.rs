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
    MuxError,
    MuxCloseConnection,
    MuxBufferOverflow,
    NoHost,
    ResolveDnsFailed,
    YameteKudasai,
    BufferOverflow,
    UdpDeadLoop,
    MailFormedUdpPacket,
    DomainInBlacklist,
    WsClosed,
    // XHttp Errors
    NoUUID,
    NoContentLength,
    ParseSecError,
    NotInitiated,
    WaitForSecTimeout,
    WaitForInitTimeout,
    WaitForFrameTimeout,
    NoDataFrame,
}
impl Display for VError {
    #[cold]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VError::Unknown => write!(f, "Unknown"),
            VError::UTF8Err => write!(f, "UTF8Err"),
            VError::TargetErr => write!(f, "TargetErr"),
            VError::UnknownSocket => write!(f, "UnknownSocket"),
            VError::AuthenticationFailed => write!(f, "AuthenticationFailed"),
            VError::Wtf => write!(f, "WTF"),
            VError::TransporterError => write!(f, "TransporterError"),
            VError::MuxError => write!(f, "MuxError"),
            VError::MuxCloseConnection => write!(f, "MuxCloseConnection"),
            VError::MuxBufferOverflow => write!(f, "MuxBufferOverflow"),
            VError::UdpDeadLoop => write!(f, "UdpDeadLoop"),
            VError::MailFormedUdpPacket => write!(f, "MailFormedUdpPacket"),
            VError::NoHost => write!(f, "NoHost"),
            VError::ResolveDnsFailed => write!(f, "ResolveDnsFailed"),
            VError::YameteKudasai => write!(f, "YameteKudasai"),
            VError::BufferOverflow => write!(f, "BufferOverflow"),
            VError::DomainInBlacklist => write!(f, "Domain In Blacklist"),
            VError::WsClosed => write!(f, "WS Close Frame Received"),
            // XHttp Errors
            VError::NoUUID => write!(f, "NoUUID"),
            VError::NoContentLength => write!(f, "NoContentLength"),
            VError::ParseSecError => write!(f, "ParseSecError"),
            VError::NotInitiated => write!(f, "NoInitiated"),
            VError::WaitForSecTimeout => write!(f, "WaitForSecTimeout"),
            VError::WaitForInitTimeout => write!(f, "WaitForInitTimeout"),
            VError::WaitForFrameTimeout => write!(f, "WaitForInitTimeout"),
            VError::NoDataFrame => write!(f, "NoDataFrame"),
        }
    }
}
impl std::error::Error for VError {}

impl From<VError> for tokio::io::Error {
    #[cold]
    fn from(e: VError) -> Self {
        tokio::io::Error::other(e)
    }
}
