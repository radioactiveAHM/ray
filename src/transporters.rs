use tokio::io::AsyncWriteExt;

pub async fn httpupgrade_transporter(
    chttp: &crate::config::Http,
    buff: &[u8],
    stream: &mut tokio::net::TcpStream,
) -> tokio::io::Result<()> {
    if let Ok(http) = core::str::from_utf8(buff) {
        // if there is no host
        // i'm too lazy to parse http headers :D
        if let Some(host) = &chttp.host {
            if !http.contains(host.as_str()) {
                return Err(crate::verror::VError::TransporterError.into());
            }
        }

        if let Some(head) = http.lines().next() {
            if head != format!("{} {} HTTP/1.1", chttp.method, chttp.path) {
                let _ = stream
                    .write(b"HTTP/1.1 404 Not Found\r\nconnection: close\r\n\r\n")
                    .await?;
                return Err(crate::verror::VError::TransporterError.into());
            }
        } else {
            return Err(crate::verror::VError::TransporterError.into());
        }
    } else {
        return Err(crate::verror::VError::UTF8Err.into());
    }

    let _ = stream.write(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n").await?;
    Ok(())
}

pub async fn http_transporter(
    chttp: &crate::config::Http,
    buff: &[u8],
    stream: &mut tokio::net::TcpStream,
) -> tokio::io::Result<()> {
    if let Ok(http) = core::str::from_utf8(buff) {
        // if there is no host
        // i'm too lazy to parse http headers :D
        if let Some(host) = &chttp.host {
            if !http.contains(host.as_str()) {
                return Err(crate::verror::VError::TransporterError.into());
            }
        }
        if let Some(head) = http.lines().next() {
            if head != format!("{} {} HTTP/1.1", chttp.method, chttp.path) {
                let _ = stream
                    .write(b"HTTP/1.1 404 Not Found\r\nconnection: close\r\n\r\n")
                    .await?;
                return Err(crate::verror::VError::TransporterError.into());
            }
        } else {
            return Err(crate::verror::VError::TransporterError.into());
        }
    } else {
        return Err(crate::verror::VError::UTF8Err.into());
    }

    Ok(())
}
