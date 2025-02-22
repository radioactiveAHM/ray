use tokio::io::{AsyncWrite, AsyncWriteExt};

use crate::{
    utils::{Buffering, convert_two_u8s_to_u16_be, convert_u16_to_two_u8s_be},
    verror::VError,
};

// ______________________________________________________________________________________

pub async fn copy_u2t(
    udp: &tokio::net::UdpSocket,
    mut w: tokio::net::tcp::WriteHalf<'_>,
    head: &[u8],
    ch_snd: tokio::sync::mpsc::Sender<()>,
) -> tokio::io::Result<()> {
    let mut buff = [0; 1024 * 8];
    let mut buff2 = [0; 1024 * 8];
    let mut b = Buffering(&mut buff2, 0);
    let mut first = true;

    loop {
        let size = udp.recv(&mut buff).await?;
        let _ = ch_snd.try_send(());
        let octat = convert_u16_to_two_u8s_be(size as u16);
        if first {
            let _ = w
                .write(
                    b.reset()
                        .write(&[0, 0])
                        .write(head)
                        .write(&octat)
                        .write(&buff[..size])
                        .get(),
                )
                .await?;
            first = false;
        } else {
            let _ = w
                .write(
                    b.reset()
                        .write(head)
                        .write(&octat)
                        .write(&buff[..size])
                        .get(),
                )
                .await?;
        }
        w.flush().await?;
    }
}

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

        let _ = self.ch_snd.try_send(());
        loop {
            if self.b.len() < 6 {
                break;
            }
            let head_size = {
                if self.b[..6] == [0, 4, 0, 0, 2, 1] {
                    6
                } else {
                    self.head.len()
                }
            };
            if &self.b[..head_size] != self.head && self.b[..head_size] != [0, 4, 0, 0, 2, 1] {
                return std::task::Poll::Ready(Err(VError::MailFormedMuxPacket.into()));
            }
            let psize =
                convert_two_u8s_to_u16_be([self.b[head_size], self.b[head_size + 1]]) as usize;
            if psize <= self.b.len() - head_size {
                // we have bytes to send
                let packet = &self.b[head_size + 2..psize + head_size + 2];
                match self.udp.poll_send(cx, packet) {
                    std::task::Poll::Pending => continue,
                    std::task::Poll::Ready(Ok(_)) => {
                        self.b.drain(0..psize + head_size + 2);
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
