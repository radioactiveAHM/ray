use tokio::io::{AsyncWrite, AsyncWriteExt};

use crate::utils::{convert_two_u8s_to_u16_be, convert_u16_to_two_u8s_be, Buffering};

pub async fn copy_u2t(udp: &tokio::net::UdpSocket, mut w: tokio::net::tcp::WriteHalf<'_>) -> tokio::io::Result<()> {
    let mut buff = [0; 1024*8];
    let mut buff2 = [0; 1024*8];
    let mut b = Buffering(&mut buff2, 0);
    let mut first = true;
    loop {
        let size = udp.recv(&mut buff).await?;
        let octat = convert_u16_to_two_u8s_be(size as u16);
        if first {
            w.write(b.reset().write(&[0, 0, octat[0], octat[1]]).write(&buff[..size]).get()).await?;
            first = false;
        } else {
            w.write(b.reset().write(&octat).write(&buff[..size]).get()).await?;
        }
        w.flush().await?;
    }
}

// _________________________________________________________________________________________________________ I hate this~

struct UdpWriter<'a> {
    udp: &'a tokio::net::UdpSocket,
    b: Vec<u8>
}
impl<'a> AsyncWrite for UdpWriter <'a> {
    fn poll_write(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &[u8],
        ) -> std::task::Poll<Result<usize, std::io::Error>> {

        if buf.len()==0 {
            return std::task::Poll::Ready(Ok(0))
        }

        // write buff into vec
        self.b.extend_from_slice(buf);

        loop {
            if self.b.len() < 3 {
                break;
            }
            let psize = convert_two_u8s_to_u16_be([self.b[0], self.b[1]]) as usize;
            if  psize <= self.b.len() - 2{
                // we have bytes to send
                let packet = &self.b[2..psize + 2];
                match self.udp.poll_send(cx, packet) {
                    std::task::Poll::Pending => continue,
                    std::task::Poll::Ready(Ok(_))=> {
                        self.b.drain(0..psize + 2);
                        continue
                    },
                    std::task::Poll::Ready(Err(e))=> {
                        self.b.clear();
                        return std::task::Poll::Ready(Err(e))
                    },
                }
            } else {
                // empty or incomplete bytes
                break;
            }
        }

        std::task::Poll::Ready(Ok(buf.len()))
    }

    #[allow(unused_variables)]
    fn poll_shutdown(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<(), std::io::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    #[allow(unused_variables)]
    fn poll_flush(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<(), std::io::Error>> {
        std::task::Poll::Ready(Ok(()))
    }
}

pub async fn copy_t2u(udp: &tokio::net::UdpSocket, mut r: tokio::net::tcp::ReadHalf<'_>) -> tokio::io::Result<()> {
    let mut uw = UdpWriter {
        udp,
        b: Vec::with_capacity(128)
    };
    tokio::io::copy(&mut r, &mut uw).await?;

    Ok(())
}