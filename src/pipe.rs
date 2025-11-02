use tokio::{
    io::{AsyncRead, AsyncWriteExt, ReadBuf},
    time::timeout,
};

#[inline(always)]
pub async fn copy<R, W>(
    r: &mut std::pin::Pin<&mut R>,
    w: &mut std::pin::Pin<&mut W>,
    buf: &mut ReadBuf<'_>,
    fill_buf: bool
) -> tokio::io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWriteExt + Unpin,
{
    if fill_buf {
        fill(r, buf).await?;
    } else {
        read(r, buf).await?;
    }
    let _ = Write(w, buf.filled()).await;
    Flush(w).await?;
    buf.clear();
    Ok(())
}

#[inline(always)]
pub async fn read<R>(r: &mut std::pin::Pin<&mut R>, buf: &mut ReadBuf<'_>) -> tokio::io::Result<()>
where
    R: AsyncRead + Unpin,
{
    std::future::poll_fn(|cx| match r.as_mut().poll_read(cx, buf) {
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
pub async fn fill<R>(r: &mut std::pin::Pin<&mut R>, buf: &mut ReadBuf<'_>) -> tokio::io::Result<()>
where
    R: AsyncRead + Unpin,
{
    loop {
        if std::future::poll_fn(|cx| match r.as_mut().poll_read(cx, buf) {
            std::task::Poll::Pending => {
                if buf.filled().is_empty() {
                    std::task::Poll::Pending
                } else {
                    // nothing to read anymore
                    std::task::Poll::Ready(Ok(true))
                }
            }
            std::task::Poll::Ready(Ok(_)) => {
                if buf.filled().is_empty() {
                    std::task::Poll::Ready(Err(tokio::io::Error::other("EOF")))
                } else if buf.remaining() == 0 {
                    // buf full
                    std::task::Poll::Ready(Ok(true))
                } else {
                    // continue reading
                    std::task::Poll::Ready(Ok(false))
                }
            }
            std::task::Poll::Ready(Err(e)) => std::task::Poll::Ready(Err(e)),
        })
        .await?
        {
            break;
        }
    }
    Ok(())
}


pub struct Write<'a, 'b, W>(pub &'a mut std::pin::Pin<&'b mut W>, pub &'a [u8]);
impl<'a, 'b, W> Future for Write<'a, 'b, W>
where
    W: AsyncWriteExt + Unpin,
{
    type Output = tokio::io::Result<usize>;
    #[inline(always)]
    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        this.0.as_mut().poll_write(cx, this.1)
    }
}

pub struct Flush<'a, 'b, W>(pub &'a mut std::pin::Pin<&'b mut W>);
impl<'a, 'b, W> Future for Flush<'a, 'b, W>
where
    W: AsyncWriteExt + Unpin,
{
    type Output = tokio::io::Result<()>;
    #[inline(always)]
    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        self.0.as_mut().poll_flush(cx)
    }
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
