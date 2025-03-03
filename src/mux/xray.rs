use tokio::io::AsyncWrite;

use crate::{
    utils::convert_two_u8s_to_u16_be,
    verror::VError,
};

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

        self.b.extend_from_slice(buf);
        if self.b.len() > 1024 * 16 {
            return std::task::Poll::Ready(Err(crate::verror::VError::BufferOverflow.into()));
        }

        let _ = self.ch_snd.try_send(());

        let mut deadloop = 0u8;
        loop {
            if deadloop == 20 {
                return std::task::Poll::Ready(Err(crate::verror::VError::UdpDeadLoop.into()));
            }
            deadloop += 1;

            if self.b.len() < 8 {
                break;
            }
            let head_size = {
                if self.b[..6] == [0, 4, 0, 0, 2, 1] {
                    6
                } else {
                    self.head.len()
                }
            };
            if &self.b[..head_size] != self.head && self.b[..6] != [0, 4, 0, 0, 2, 1] {
                return std::task::Poll::Ready(Err(VError::MailFormedXrayMuxPacket.into()));
            }
            let psize =
                convert_two_u8s_to_u16_be([self.b[head_size], self.b[head_size + 1]]) as usize;
            if psize==0 {
                return std::task::Poll::Ready(Err(VError::MailFormedXrayMuxPacket.into()));
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

pub async fn copy_t2u(
    udp: &tokio::net::UdpSocket,
    mut r: tokio::net::tcp::ReadHalf<'_>,
    head: &[u8],
    b1: Vec<u8>,
    ch_snd: tokio::sync::mpsc::Sender<()>,
) -> tokio::io::Result<()> {
    let mut b = Vec::with_capacity(1024 * 4);
    b.extend_from_slice(&b1);
    drop(b1);
    let mut uw = UdpWriter {
        udp,
        b,
        head,
        ch_snd,
    };
    tokio::io::copy(&mut r, &mut uw).await?;

    Ok(())
}

// ______________________________________________________________________________________
