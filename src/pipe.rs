use tokio::io::{AsyncRead, ReadBuf};

pub struct Read<'a, 'b, 'c, R>(pub &'a mut std::pin::Pin<&'b mut R>, pub &'a mut ReadBuf<'c>);
impl<'a, 'b, 'c, R> Future for Read<'a, 'b, 'c, R>
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

pub struct RecvBytes<'a, 'b, R>(pub &'a mut std::pin::Pin<&'b mut R>);
impl<'a, 'b, R> Future for RecvBytes<'a, 'b, R>
where
	R: crate::ioutils::AsyncRecvBytes + Unpin,
{
	type Output = tokio::io::Result<bytes::Bytes>;

	fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
		let poll = std::task::ready!(self.0.as_mut().poll_recv_bytes(cx).map_ok(|b| {
			if b.is_empty() {
				std::task::Poll::Ready(Err(tokio::io::Error::new(std::io::ErrorKind::UnexpectedEof, "eof")))
			} else {
				std::task::Poll::Ready(Ok(b))
			}
		}));
		poll?
	}
}
