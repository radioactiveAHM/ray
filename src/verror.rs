use std::fmt::Display;

#[derive(Debug)]
pub enum VError {
    Unknown,
    UTF8Err,
    TargetErr,
    UnknownSocket,
    AuthenticationFailed,
    WTF
}
impl Display for VError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VError::Unknown => write!(f, "Unknown"),
            VError::UTF8Err => write!(f, "UTF8Err"),
            VError::TargetErr => write!(f, "TargetErr"),
            VError::UnknownSocket => write!(f, "UnknownSocket"),
            VError::AuthenticationFailed => write!(f, "AuthenticationFailed"),
            VError::WTF => write!(f, "WTF"),
        }
    }
}
impl std::error::Error for VError {}

impl From<VError> for tokio::io::Error {
    fn from(e: VError) -> Self {
        tokio::io::Error::other(e)
    }
}