use tokio::io::{AsyncWrite, AsyncWriteExt};

use crate::utils::{self, convert_two_u8s_to_u16_be, convert_u16_to_two_u8s_be};

pub struct UdpReader<'a> {
    pub udp: &'a tokio::net::UdpSocket,
    pub buf: Vec<u8>,
}

impl UdpReader<'_> {
    #[inline(always)]
    pub async fn copy<W: AsyncWrite + Unpin>(
        &mut self,
        w: &mut std::pin::Pin<&mut W>,
    ) -> tokio::io::Result<()> {
        let size = self.udp.recv(&mut self.buf[2..]).await?;
        self.buf[0..2].copy_from_slice(&convert_u16_to_two_u8s_be(size as u16));
        w.write_all(&self.buf[..size + 2]).await
    }
}

pub struct UdpWriter<'a> {
    pub udp: &'a tokio::net::UdpSocket,
    pub b: utils::DeqBuffer,
}
impl AsyncWrite for UdpWriter<'_> {
    #[inline(always)]
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

    #[inline(always)]
    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    #[inline(always)]
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        std::task::Poll::Ready(Ok(()))
    }
}
