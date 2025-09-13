use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};

use crate::utils::{self, convert_two_u8s_to_u16_be, convert_u16_to_two_u8s_be};

pub async fn copy_u2t<W>(
    udp: &tokio::net::UdpSocket,
    mut w: W,
    buf_size: usize,
) -> tokio::io::Result<()>
where
    W: AsyncWrite + Unpin + Send,
{
    let mut buff = vec![0; buf_size * 8];

    {
        // write first packet
        let size = udp.recv(&mut buff[4..]).await?;
        let octat = convert_u16_to_two_u8s_be(size as u16);
        buff[2..4].copy_from_slice(&octat);
        let _ = w.write(&buff[..size + 4]).await?;
        w.flush().await?;
    }

    loop {
        let size = udp.recv(&mut buff[2..]).await?;
        let octat = convert_u16_to_two_u8s_be(size as u16);
        buff[0..2].copy_from_slice(&octat);
        let _ = w.write(&buff[..size + 2]).await?;
        w.flush().await?;
    }
}

// _________________________________________________________________________________________________________ I hate this~

struct UdpWriter<'a> {
    udp: &'a tokio::net::UdpSocket,
    b: utils::DeqBuffer,
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

        self.b.write(buf);

        // we don't want to blow up the memory
        if self.b.slice().len() > 1024 * 16 {
            return std::task::Poll::Ready(Err(crate::verror::VError::BufferOverflow.into()));
        }

        let mut deadloop = 0u8;
        loop {
            if deadloop == 20 {
                return std::task::Poll::Ready(Err(crate::verror::VError::UdpDeadLoop.into()));
            }
            deadloop += 1;

            let buf = self.b.slice();
            let size = buf.len();
            // must be at least 3 bytes which 0 and 1 are len
            if size < 3 {
                break;
            }
            let psize = convert_two_u8s_to_u16_be([buf[0], buf[1]]) as usize;

            // len must not be 0
            if psize == 0 {
                return std::task::Poll::Ready(Err(
                    crate::verror::VError::MailFormedUdpPacket.into()
                ));
            }

            if psize <= size - 2 {
                // we have bytes to send
                let packet = &self.b.slice()[2..psize + 2];
                match self.udp.poll_send(cx, packet) {
                    std::task::Poll::Pending => continue,
                    std::task::Poll::Ready(Ok(_)) => {
                        self.b.remove(psize + 2);
                        continue;
                    }
                    std::task::Poll::Ready(Err(e)) => {
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

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        std::task::Poll::Ready(Ok(()))
    }
}

pub async fn copy_t2u<R>(
    udp: &tokio::net::UdpSocket,
    r: R,
    buf_size: usize,
    timeout_dur: u64,
) -> tokio::io::Result<()>
where
    R: AsyncRead + Unpin + Send,
{
    let mut uw = UdpWriter {
        udp,
        b: utils::DeqBuffer::new(buf_size * 1024), // buf_size unit is kb
    };

    crate::pipe::copy_timeout(r, &mut uw, buf_size, timeout_dur).await
}
