use tokio::io::{AsyncRead, AsyncWrite};

use crate::{utils::convert_two_u8s_to_u16_be, verror::VError};

// ______________________________________________________________________________________

struct UdpWriter<'a> {
    udp: &'a tokio::net::UdpSocket,
    b: Vec<u8>,
    head: &'a [u8],
    ch_snd: tokio::sync::mpsc::Sender<()>,
}
impl AsyncWrite for UdpWriter<'_> {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<Result<usize, std::io::Error>> {
        if buf.is_empty() {
            return std::task::Poll::Ready(Ok(0));
        }
        // write buff into vec
        self.b.extend_from_slice(buf);
        if self.b.len() > 1024 * 16 {
            return std::task::Poll::Ready(Err(crate::verror::VError::BufferOverflow.into()));
        }

        let head_size = self.head.len();

        let _ = self.ch_snd.try_send(());

        let mut deadloop = 0u8;
        loop {
            if deadloop == 20 {
                return std::task::Poll::Ready(Err(crate::verror::VError::UdpDeadLoop.into()));
            }
            deadloop += 1;

            if self.b.len() < head_size + 2 {
                break;
            } else if &self.b[..head_size] != self.head {
                return std::task::Poll::Ready(Err(VError::MailFormedSingboxMuxPacket.into()));
            }

            let psize =
                convert_two_u8s_to_u16_be([self.b[head_size], self.b[head_size + 1]]) as usize;
            if psize == 0 {
                return std::task::Poll::Ready(Err(VError::MailFormedSingboxMuxPacket.into()));
            }

            if psize <= self.b.len() - head_size {
                // we have bytes to send
                let packet = &self.b[head_size + 2..head_size + 2 + psize];
                match self.udp.poll_send(cx, packet) {
                    std::task::Poll::Pending => continue,
                    std::task::Poll::Ready(Ok(_)) => {
                        self.b.drain(0..head_size + 2 + psize);
                        continue;
                    }
                    std::task::Poll::Ready(Err(e)) => {
                        self.b.clear();
                        return std::task::Poll::Ready(Err(e));
                    }
                }
            } else {
                // empty or incomplete bytes
                break;
            }
        }

        std::task::Poll::Ready(Ok(buf.len()))
    }

    #[allow(unused_variables)]
    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    #[allow(unused_variables)]
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        std::task::Poll::Ready(Ok(()))
    }
}

pub async fn copy_t2u<R>(
    udp: &tokio::net::UdpSocket,
    r: tokio::io::ReadHalf<R>,
    head: &[u8],
    ch_snd: tokio::sync::mpsc::Sender<()>,
    buf_size: usize,
) -> tokio::io::Result<()>
where
    R: AsyncRead + Unpin + Send,
{
    let mut uw = UdpWriter {
        udp,
        b: Vec::with_capacity(buf_size),
        head,
        ch_snd,
    };
    let mut buf_wraper = tokio::io::BufReader::with_capacity(buf_size, r);
    tokio::io::copy_buf(&mut buf_wraper, &mut uw).await?;

    Ok(())
}

// ______________________________________________________________________________________
