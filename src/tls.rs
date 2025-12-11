use tokio_rustls::TlsAcceptor;

pub struct Tc {
	pub acceptor: TlsAcceptor,
	pub stream: (tokio::net::TcpStream, std::net::SocketAddr),
}
impl Tc {
	pub fn new(
		acceptor: TlsAcceptor,
		stream: Result<(tokio::net::TcpStream, std::net::SocketAddr), tokio::io::Error>,
	) -> Result<Self, tokio::io::Error> {
		Ok(Self {
			acceptor,
			stream: stream?,
		})
	}
	pub async fn accept(self) -> Result<tokio_rustls::server::TlsStream<tokio::net::TcpStream>, tokio::io::Error> {
		self.acceptor.accept(self.stream.0).await.map(|mut tls| {
			tls.get_mut().1.set_buffer_limit(Some(crate::CONFIG.tls_buffer_limit));
			tls
		})
	}
}
