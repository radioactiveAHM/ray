use tokio::io::{AsyncRead, AsyncWriteExt, ReadBuf};

#[inline(always)]
pub async fn stack_copy<R, W>(r: R, w: &mut W, size: usize) -> tokio::io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWriteExt + Unpin,
{
    let mut buf = vec![0; 1024 * size];
    copy(r, w, &mut buf).await
}

#[inline(always)]
async fn copy<R, W>(mut r: R, w: &mut W, buf: &mut [u8]) -> tokio::io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWriteExt + Unpin,
{
    let mut pinned = std::pin::Pin::new(&mut r);
    let mut wrapper = ReadBuf::new(buf);
    loop {
        std::future::poll_fn(|cx| match pinned.as_mut().poll_read(cx, &mut wrapper) {
            std::task::Poll::Pending => std::task::Poll::Pending,
            std::task::Poll::Ready(Ok(_)) => {
                if wrapper.filled().is_empty() {
                    std::task::Poll::Pending
                } else {
                    std::task::Poll::Ready(Ok(()))
                }
            }
            std::task::Poll::Ready(Err(e)) => std::task::Poll::Ready(Err(e)),
        })
        .await?;
        let _ = w.write(wrapper.filled()).await?;
        let _ = w.flush().await;
        wrapper.clear();
    }
}
