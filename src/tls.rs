use tokio_rustls::TlsAcceptor;

pub struct Tc {
	pub acceptor: TlsAcceptor,
	pub stream: (tokio::net::TcpStream, std::net::SocketAddr),
	pub buffer_limit: Option<usize>,
}
impl Tc {
	pub fn new(
		acceptor: TlsAcceptor,
		stream: Result<(tokio::net::TcpStream, std::net::SocketAddr), tokio::io::Error>,
		buffer_limit: Option<usize>,
	) -> Result<Self, tokio::io::Error> {
		Ok(Self {
			acceptor,
			stream: stream?,
			buffer_limit,
		})
	}
	pub async fn accept(self) -> Result<tokio_rustls::server::TlsStream<tokio::net::TcpStream>, tokio::io::Error> {
		self.acceptor.accept(self.stream.0).await.map(|mut tls| {
			tls.get_mut().1.set_buffer_limit(self.buffer_limit.map(|v| v * 1024));
			tls
		})
	}
}
