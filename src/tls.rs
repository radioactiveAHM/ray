use std::sync::Arc;

use tokio_rustls::{
	TlsAcceptor,
	rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
};

pub struct Tc {
	pub acceptor: TlsAcceptor,
	pub stream: (tokio::net::TcpStream, std::net::SocketAddr),
}
impl Tc {
	#[inline(always)]
	pub fn new(
		acceptor: TlsAcceptor,
		stream: Result<(tokio::net::TcpStream, std::net::SocketAddr), tokio::io::Error>,
	) -> Result<Self, tokio::io::Error> {
		Ok(Self {
			acceptor,
			stream: stream?,
		})
	}

	#[inline(always)]
	pub async fn accept(self) -> Result<tokio_rustls::server::TlsStream<tokio::net::TcpStream>, tokio::io::Error> {
		self.acceptor.accept(self.stream.0).await.map(|mut tls| {
			tls.get_mut()
				.1
				.set_buffer_limit(Some(crate::CONFIG.tls_buffer_limit * 1024));
			tls
		})
	}
}

pub fn tls_server(tls_conf: &crate::config::Tls) -> TlsAcceptor {
	let certs = CertificateDer::pem_file_iter(&tls_conf.certificate)
		.unwrap()
		.collect::<Result<Vec<_>, _>>()
		.unwrap();
	let key = PrivateKeyDer::from_pem_file(&tls_conf.key).unwrap();
	let mut c: tokio_rustls::rustls::ServerConfig = tokio_rustls::rustls::ServerConfig::builder()
		.with_no_client_auth()
		.with_single_cert(certs, key)
		.unwrap();
	c.alpn_protocols = tls_conf.alpn.iter().map(|p| p.as_bytes().to_vec()).collect();
	c.max_fragment_size = tls_conf.max_fragment_size;
	c.send_tls13_tickets = tls_conf.tls13_tickets;
	TlsAcceptor::from(Arc::new(c))
}
