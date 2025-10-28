use tokio::{
    io::{AsyncRead, AsyncWriteExt, ReadBuf},
    time::timeout,
};

#[inline(always)]
pub async fn copy<R, W>(mut r: R, w: &mut W, buf: &mut ReadBuf<'_>) -> tokio::io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWriteExt + Unpin,
{
    let mut pinned = std::pin::Pin::new(&mut r);
    read(&mut pinned, buf).await?;
    let _ = w.write(buf.filled()).await?;
    w.flush().await?;
    buf.clear();
    Ok(())
}

#[inline(always)]
pub async fn read<R>(
    pinned: &mut std::pin::Pin<&mut R>,
    buf: &mut ReadBuf<'_>,
) -> tokio::io::Result<()>
where
    R: AsyncRead + Unpin,
{
    std::future::poll_fn(|cx| match pinned.as_mut().poll_read(cx, buf) {
        std::task::Poll::Pending => std::task::Poll::Pending,
        std::task::Poll::Ready(Ok(_)) => {
            if buf.filled().is_empty() {
                std::task::Poll::Ready(Err(tokio::io::Error::other("Pipe read EOF")))
            } else {
                std::task::Poll::Ready(Ok(()))
            }
        }
        std::task::Poll::Ready(Err(e)) => std::task::Poll::Ready(Err(e)),
    })
    .await
}

#[inline(always)]
pub async fn read_timeout<R>(
    pinned: &mut std::pin::Pin<&mut R>,
    wrapper: &mut ReadBuf<'_>,
    timeout_dur: u64,
) -> tokio::io::Result<()>
where
    R: AsyncRead + Unpin,
{
    match timeout(std::time::Duration::from_secs(timeout_dur), async {
        std::future::poll_fn(|cx| match pinned.as_mut().poll_read(cx, wrapper) {
            std::task::Poll::Pending => std::task::Poll::Pending,
            std::task::Poll::Ready(Ok(_)) => {
                if wrapper.filled().is_empty() {
                    std::task::Poll::Ready(Err(tokio::io::Error::other("Pipe read EOF")))
                } else {
                    std::task::Poll::Ready(Ok(()))
                }
            }
            std::task::Poll::Ready(Err(e)) => std::task::Poll::Ready(Err(e)),
        })
        .await
    })
    .await
    {
        Ok(v) => v,
        Err(_) => Err(tokio::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "Pipe read timeout",
        )),
    }
}
