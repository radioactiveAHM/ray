use tokio::{
	io::{AsyncRead, ReadBuf},
	time::timeout,
};

pub struct Read<'a, 'b, 'c, R>(pub &'a mut std::pin::Pin<&'b mut R>, pub &'a mut ReadBuf<'c>);
impl<'a, 'b, 'c, R> Future for Read<'a, 'b, 'c, R>
where
	R: AsyncRead + Unpin,
{
	type Output = tokio::io::Result<()>;
	#[inline(always)]
	fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
		let this = &mut *self;
		this.1.clear();
		let poll = std::task::ready!(this.0.as_mut().poll_read(cx, this.1)).map(|_| {
			if this.1.filled().is_empty() {
				std::task::Poll::Ready(Err(tokio::io::Error::other("Pipe read EOF")))
			} else {
				std::task::Poll::Ready(Ok(()))
			}
		});
		poll?
	}
}

// struct Fill<'a, 'b, 'c, R>(&'a mut std::pin::Pin<&'b mut R>, &'a mut ReadBuf<'c>);
// impl<'a, 'b, 'c, R> Future for Fill<'a, 'b, 'c, R>
// where
// 	R: AsyncRead + Unpin,
// {
// 	type Output = tokio::io::Result<()>;
// 	#[inline(always)]
// 	fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
// 		let coop = std::task::ready!(tokio::task::coop::poll_proceed(cx));
// 		let this = &mut *self;
// 		let mut filled = 0;
// 		loop {
// 			match this.0.as_mut().poll_read(cx, this.1) {
// 				std::task::Poll::Pending => {
// 					if filled == 0 {
// 						return std::task::Poll::Pending;
// 					} else {
// 						coop.made_progress();
// 						return std::task::Poll::Ready(Ok(()));
// 					}
// 				}
// 				std::task::Poll::Ready(Ok(_)) => {
// 					coop.made_progress();
// 					let fill = this.1.filled().len();
// 					if fill == 0 || filled == fill {
// 						return std::task::Poll::Ready(Err(tokio::io::Error::other("Pipe read EOF")));
// 					} else if this.1.remaining() == 0 {
// 						return std::task::Poll::Ready(Ok(()));
// 					}
// 					filled = fill;
// 				}
// 				std::task::Poll::Ready(Err(e)) => {
// 					coop.made_progress();
// 					return std::task::Poll::Ready(Err(e));
// 				}
// 			};
// 		}
// 	}
// }

#[inline(always)]
pub async fn read_timeout<R>(
	r: &mut std::pin::Pin<&mut R>,
	buf: &mut ReadBuf<'_>,
	timeout_dur: u64,
) -> tokio::io::Result<()>
where
	R: AsyncRead + Unpin,
{
	timeout(std::time::Duration::from_secs(timeout_dur), Read(r, buf))
		.await
		.map_err(tokio::io::Error::other)?
}
