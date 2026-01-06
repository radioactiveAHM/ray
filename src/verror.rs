use std::fmt::Display;

#[derive(Debug)]
pub enum VError {
	Unknown,
	Reject,
	UTF8Err,
	TargetErr,
	UnknownSocket,
	AuthenticationFailed,
	TransporterError,
	MuxRejected,
	MuxError,
	MuxCloseConnection,
	MuxBufferOverflow,
	NoHost,
	ResolveDnsFailed(hickory_resolver::ResolveError),
	BufferOverflow,
	UdpDeadLoop,
	MailFormedUdpPacket,
	WsClosed,
}
impl Display for VError {
	#[inline(always)]
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			VError::Unknown => write!(f, "Unknown"),
			VError::Reject => write!(f, "ip/domain rejected"),
			VError::UTF8Err => write!(f, "UTF8Err"),
			VError::TargetErr => write!(f, "TargetErr"),
			VError::UnknownSocket => write!(f, "UnknownSocket"),
			VError::AuthenticationFailed => write!(f, "AuthenticationFailed"),
			VError::TransporterError => write!(f, "TransporterError"),
			VError::MuxRejected => write!(f, "MuxRejected"),
			VError::MuxError => write!(f, "MuxError"),
			VError::MuxCloseConnection => write!(f, "MuxCloseConnection"),
			VError::MuxBufferOverflow => write!(f, "MuxBufferOverflow"),
			VError::UdpDeadLoop => write!(f, "UdpDeadLoop"),
			VError::MailFormedUdpPacket => write!(f, "MailFormedUdpPacket"),
			VError::NoHost => write!(f, "NoHost"),
			VError::ResolveDnsFailed(e) => write!(f, "ResolveDnsFailed: {e}"),
			VError::BufferOverflow => write!(f, "BufferOverflow"),
			VError::WsClosed => write!(f, "WS Close Frame Received"),
		}
	}
}
impl std::error::Error for VError {}

impl From<VError> for tokio::io::Error {
	#[inline(always)]
	fn from(e: VError) -> Self {
		tokio::io::Error::other(e)
	}
}
