use tokio::io::{AsyncRead, ReadBuf};

pub struct Read<'a, 'b, R>(pub std::pin::Pin<&'a mut R>, pub &'a mut ReadBuf<'b>);
impl<'a, 'b, R> Future for Read<'a, 'b, R>
where
	R: AsyncRead + Unpin,
{
	type Output = tokio::io::Result<()>;

	fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
		let this = &mut *self;
		this.1.clear();
		std::task::ready!(this.0.as_mut().poll_read(cx, this.1)).map(|_| {
			if this.1.filled().is_empty() {
				std::task::Poll::Ready(Err(tokio::io::Error::new(std::io::ErrorKind::UnexpectedEof, "eof")))
			} else {
				std::task::Poll::Ready(Ok(()))
			}
		})?
	}
}

pub struct RecvBytes<'a, R>(pub &'a mut R);
impl<'a, R> Future for RecvBytes<'a, R>
where
	R: crate::ioutils::AsyncRecvBytes + Unpin,
{
	type Output = tokio::io::Result<bytes::Bytes>;

	fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
		std::task::ready!(self.0.poll_recv_bytes(cx).map_ok(|b| {
			if b.is_empty() {
				std::task::Poll::Ready(Err(tokio::io::Error::new(std::io::ErrorKind::UnexpectedEof, "eof")))
			} else {
				std::task::Poll::Ready(Ok(b))
			}
		}))?
	}
}
