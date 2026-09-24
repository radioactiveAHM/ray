use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};

pub trait AsyncRecvBytes {
	fn poll_recv_bytes(&mut self, cx: &mut std::task::Context<'_>) -> std::task::Poll<tokio::io::Result<bytes::Bytes>>;
}

pub const fn split<T>(xref: &mut T) -> (&mut T, &mut T) {
	unsafe {
		(
			core::mem::transmute::<&mut T, &mut T>(xref),
			core::mem::transmute::<&mut T, &mut T>(xref),
		)
	}
}

pub async fn simple_read_with_eof<S>(stream: &mut S, buf: &mut [u8]) -> tokio::io::Result<usize>
where
	S: AsyncRead + AsyncWrite + Unpin,
{
	let size = stream.read(buf).await?;
	if size == 0 {
		return Err(tokio::io::Error::new(
			std::io::ErrorKind::UnexpectedEof,
			"UnexpectedEof",
		));
	}
	Ok(size)
}

pub async fn simple_try_read_with_eof<S>(stream: &mut S, buf: &mut [u8]) -> tokio::io::Result<usize>
where
	S: AsyncRead + AsyncWrite + Unpin,
{
	let mut pin = std::pin::pin!(stream);
	let mut buf = tokio::io::ReadBuf::new(buf);
	std::future::poll_fn(|cx| match pin.as_mut().poll_read(cx, &mut buf) {
		std::task::Poll::Pending => std::task::Poll::Ready(Ok(0)),
		std::task::Poll::Ready(Ok(())) => {
			let len = buf.filled().len();
			if len == 0 {
				std::task::Poll::Ready(Err(tokio::io::Error::new(
					std::io::ErrorKind::UnexpectedEof,
					"UnexpectedEof",
				)))
			} else {
				std::task::Poll::Ready(Ok(len))
			}
		}
		std::task::Poll::Ready(Err(e)) => std::task::Poll::Ready(Err(e)),
	})
	.await
}
