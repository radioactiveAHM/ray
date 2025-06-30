use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    pin::Pin,
    sync::Arc,
    time::Duration,
};

use tokio::{
    io::{AsyncRead, AsyncWriteExt, ReadBuf},
    sync::{Mutex, RwLock},
    time::{sleep, timeout},
};
use uuid::Uuid;

enum IoType {
    Tcp(IoTcp),
    Udp(IoUdp),
}

impl IoType {
    fn is_tcp(&self) -> bool {
        match self {
            Self::Tcp(_) => true,
            Self::Udp(_) => false,
        }
    }
    fn get_tcp(&self) -> &IoTcp {
        match self {
            IoType::Tcp(io) => io,
            _ => panic!(),
        }
    }
    fn get_udp(&self) -> &IoUdp {
        match self {
            IoType::Tcp(_) => panic!(),
            IoType::Udp(io) => io,
        }
    }
}

struct IoTcp {
    r: Mutex<tokio::io::ReadHalf<tokio::net::TcpStream>>,
    w: Mutex<tokio::io::WriteHalf<tokio::net::TcpStream>>,
}

struct IoUdp {
    udp: tokio::net::UdpSocket,
    buff: Mutex<Vec<u8>>,
}

struct XHttpConnection {
    io: IoType,
    sec: RwLock<usize>,
}

type ConnectionManager = Arc<RwLock<HashMap<Uuid, Arc<XHttpConnection>>>>;

#[inline(never)]
pub async fn xhttp(
    acceptor: tokio_rustls::TlsAcceptor,
    tcp: tokio::net::TcpListener,
    config: &'static crate::config::Config,
    resolver: &'static hickory_resolver::Resolver<
        hickory_resolver::name_server::GenericConnector<
            hickory_resolver::proto::runtime::TokioRuntimeProvider,
        >,
    >,
    sockopt: crate::config::SockOpt,
    xhttp_options: crate::config::Xhttp,
) {
    let xhttp_options: &'static crate::config::Xhttp =
        crate::utils::unsafe_staticref(&xhttp_options);
    let sockopt: &'static crate::config::SockOpt = crate::utils::unsafe_staticref(&sockopt);

    let mut h2_conf = h2::server::Builder::new();
    h2_conf.max_header_list_size(1024 * 1024);
    h2_conf.max_concurrent_reset_streams(100);
    h2_conf.max_pending_accept_reset_streams(100);

    if let Some(reset_stream_duration) = xhttp_options.reset_stream_duration {
        h2_conf.reset_stream_duration(Duration::from_millis(reset_stream_duration));
    };
    if let Some(initial_connection_window_size) = xhttp_options.initial_connection_window_size {
        h2_conf.initial_connection_window_size(initial_connection_window_size);
    }
    if let Some(initial_window_size) = xhttp_options.initial_window_size {
        h2_conf.initial_window_size(initial_window_size);
    }
    if let Some(max_concurrent_streams) = xhttp_options.max_concurrent_streams {
        h2_conf.max_concurrent_streams(max_concurrent_streams);
    }

    if let Some(max_frame_size) = xhttp_options.max_frame_size {
        h2_conf.max_frame_size(max_frame_size);
    }

    if let Some(max_send_buffer_size) = xhttp_options.max_send_buffer_size {
        h2_conf.max_send_buffer_size(max_send_buffer_size);
    }

    let connection_manager: ConnectionManager = Arc::new(RwLock::new(HashMap::new()));
    loop {
        match crate::tls::Tc::new(acceptor.clone(), tcp.accept().await) {
            Ok(tc) => {
                if let Ok(tls_stream) = tc.accept().await {
                    let cm = connection_manager.clone();
                    let h2_conf = h2_conf.clone();
                    tokio::spawn(async move {
                        // handle h2
                        let peer_addr: Result<SocketAddr, std::io::Error> =
                            tls_stream.get_ref().0.peer_addr();
                        match h2_conf.handshake(tls_stream).await {
                            Ok(h2con) => {
                                if let Err(e) = h2c(
                                    h2con,
                                    cm,
                                    config,
                                    resolver,
                                    peer_addr,
                                    sockopt,
                                    xhttp_options,
                                )
                                .await
                                {
                                    if crate::log() {
                                        println!("XHTTP-H2: {e}");
                                    }
                                }
                            }
                            Err(e) => {
                                if crate::log() {
                                    println!("XHTTP-H2: {e}");
                                }
                            }
                        }
                    });
                }
            }
            Err(e) => {
                if crate::log() {
                    println!("XHTTP-TLS: {e}")
                }
            }
        }
    }
}

#[inline(always)]
async fn h2c(
    mut h2con: h2::server::Connection<
        tokio_rustls::server::TlsStream<tokio::net::TcpStream>,
        bytes::Bytes,
    >,
    cm: ConnectionManager,
    config: &'static crate::config::Config,
    resolver: &'static hickory_resolver::Resolver<
        hickory_resolver::name_server::GenericConnector<
            hickory_resolver::proto::runtime::TokioRuntimeProvider,
        >,
    >,
    peer_addr: Result<SocketAddr, std::io::Error>,
    sockopt: &'static crate::config::SockOpt,
    xhttp_options: &'static crate::config::Xhttp,
) -> Result<(), Box<dyn std::error::Error>> {
    let peer_addr = peer_addr?;
    if let Some(target_window_size) = xhttp_options.target_window_size {
        h2con.set_target_window_size(target_window_size);
    }
    while let Some(h2_stream) = h2con.accept().await {
        let h2_stream = h2_stream?;
        let cm = cm.clone();
        tokio::spawn(async move {
            match *h2_stream.0.method() {
                http::Method::GET => {
                    if let Err(e) = handle_get(h2_stream, cm, config, xhttp_options).await {
                        if crate::log() {
                            println!("H2-GET: {e}");
                        }
                    }
                }
                http::Method::POST => {
                    if let Err(e) = handle_post(
                        h2_stream,
                        cm,
                        config,
                        resolver,
                        peer_addr,
                        sockopt,
                        xhttp_options,
                    )
                    .await
                    {
                        if crate::log() {
                            println!("H2-POST: {e}");
                        }
                    }
                }
                _ => (),
            }
        });
    }
    std::future::poll_fn(|cx| h2con.poll_closed(cx)).await?;
    Ok(())
}

#[inline(always)]
fn gen_http_resp(xhttp_options: &'static crate::config::Xhttp, get: bool) -> Result<http::Response<()>, Box<dyn std::error::Error>> {
    let mut resp = http::Response::builder()
        .status(200)
        .version(http::Version::HTTP_2)
        .body(())?;
    if get {
        for head in &xhttp_options.get_resp_headers {
            resp.headers_mut().insert(head.0.as_str(), http::header::HeaderValue::from_str(head.1.as_str())?);
        }
    } else {
        for head in &xhttp_options.post_resp_headers {
            resp.headers_mut().insert(head.0.as_str(), http::header::HeaderValue::from_str(head.1.as_str())?);
        }
    };

    Ok(resp)
}

#[inline(always)]
async fn handle_get(
    mut h2_stream: (
        http::Request<h2::RecvStream>,
        h2::server::SendResponse<bytes::Bytes>,
    ),
    cm: ConnectionManager,
    _config: &'static crate::config::Config,
    xhttp_options: &'static crate::config::Xhttp,
) -> Result<(), Box<dyn std::error::Error>> {
    // TODO: Match path
    let cid = get_cid(h2_stream.0.uri().path(), true)?;
    let mut h2_w = h2_stream.1.send_response(gen_http_resp(xhttp_options, true)?, false)?;
    let conn = wait_for_init(&cm, &cid, xhttp_options).await?;
    if conn.io.is_tcp() {
        let io = conn.io.get_tcp();

        let mut buff = vec![0u8; xhttp_options.tcp_buffer_size * 1024];
        let mut wrapper = ReadBuf::new(&mut buff);
        drop_conn(
            &cm,
            &cid,
            h2_w.send_data(bytes::Bytes::copy_from_slice(&[0, 0]), false),
        )
        .await?;
        loop {
            {
                let mut reader = io.r.lock().await;
                let mut pinned = Pin::new(&mut *reader);
                let read_poll =
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
                    });
                let stat = tokio::select! {
                    data = read_poll => {
                        data
                    }
                    _ = std::future::poll_fn(|cx| h2_w.poll_reset(cx)) => {
                        Err(tokio::io::Error::new(std::io::ErrorKind::ConnectionReset, "H2 Connection Reset"))
                    }
                };
                drop_conn(&cm, &cid, stat).await?;
            }
            h2_w.reserve_capacity(wrapper.filled().len());
            println!("size of packet {}", wrapper.filled().len());
            let cap = std::future::poll_fn(|cx| h2_w.poll_capacity(cx)).await;
            println!("size of cap {:?}", cap);
            drop_conn(
                &cm,
                &cid,
                h2_w.send_data(bytes::Bytes::copy_from_slice(wrapper.filled()), false),
            )
            .await?;
            wrapper.clear();
        }
    } else {
        let io = conn.io.get_udp();
        drop_conn(
            &cm,
            &cid,
            h2_w.send_data(bytes::Bytes::copy_from_slice(&[0, 0]), false),
        )
        .await?;
        let mut buf = [0u8; 1024 * 8];
        loop {
            let stat = tokio::select! {
                size = io.udp.recv(&mut buf[2..]) => {
                    size
                }
                _ = std::future::poll_fn(|cx| h2_w.poll_reset(cx)) => {
                    Err(tokio::io::Error::new(std::io::ErrorKind::ConnectionReset, "H2 Connection Reset"))
                }
            };
            let size = drop_conn(&cm, &cid, stat).await?;
            h2_w.reserve_capacity(size + 2);
            std::future::poll_fn(|cx| h2_w.poll_capacity(cx)).await;
            [buf[0], buf[1]] = crate::utils::convert_u16_to_two_u8s_be(size as u16);
            drop_conn(
                &cm,
                &cid,
                h2_w.send_data(bytes::Bytes::copy_from_slice(&buf[..size + 2]), false),
            )
            .await?;
        }
    }
}

#[inline(always)]
async fn drop_conn<T, E>(cm: &ConnectionManager, cid: &Uuid, result: Result<T, E>) -> Result<T, E> {
    if result.is_err() {
        cm.write().await.remove(cid);
    }
    result
}

#[inline(always)]
async fn handle_post(
    mut h2_stream: (
        http::Request<h2::RecvStream>,
        h2::server::SendResponse<bytes::Bytes>,
    ),
    cm: ConnectionManager,
    config: &'static crate::config::Config,
    resolver: &'static hickory_resolver::Resolver<
        hickory_resolver::name_server::GenericConnector<
            hickory_resolver::proto::runtime::TokioRuntimeProvider,
        >,
    >,
    peer_addr: SocketAddr,
    sockopt: &'static crate::config::SockOpt,
    xhttp_options: &'static crate::config::Xhttp,
) -> Result<(), Box<dyn std::error::Error>> {
    let cid = get_cid(h2_stream.0.uri().path(), false)?;
    let body_len = {
        if let Some(clen) = h2_stream.0.headers().get("content-length") {
            clen.to_str()?.parse::<usize>()?
        } else {
            h2_stream.1.send_reset(h2::Reason::CANCEL);
            return Err(Box::new(crate::verror::VError::NoContentLength));
        }
    };

    let sec = if let Some(sec_str) = h2_stream.0.uri().path().split("/").last() {
        sec_str.parse::<usize>()?
    } else {
        h2_stream.1.send_reset(h2::Reason::CANCEL);
        return Err(Box::new(crate::verror::VError::ParseSecError));
    };

    if sec == 0 {
        // Init new XhttpConn and upload payload
        if let Some(payload) = h2_stream.0.body_mut().data().await {
            let payload = payload?;
            let vless = crate::vless::Vless::new(&payload, resolver, &config.blacklist).await?;
            if crate::auth::authenticate(config, &vless, peer_addr) {
                h2_stream.1.send_reset(h2::Reason::CANCEL);
                return Err(Box::new(crate::verror::VError::AuthenticationFailed));
            };
            let target = vless.target.unwrap();
            match vless.rt {
                crate::vless::SocketType::MUX => {
                    h2_stream.1.send_reset(h2::Reason::CANCEL);
                    return Ok(());
                }
                crate::vless::SocketType::UDP => {
                    let ip = if let Some(interface) = &sockopt.interface {
                        crate::tcp::get_interface(target.0.is_ipv4(), interface)
                    } else if target.0.is_ipv4() {
                        IpAddr::V4(Ipv4Addr::UNSPECIFIED)
                    } else {
                        IpAddr::V6(Ipv6Addr::UNSPECIFIED)
                    };
                    let udp = tokio::net::UdpSocket::bind(SocketAddr::new(ip, 0)).await?;
                    #[cfg(target_os = "linux")]
                    {
                        if sockopt.bind_to_device {
                            if let Some(interface) = &sockopt.interface {
                                if crate::tcp::tcp_options::set_udp_bind_device(&udp, &interface)
                                    .is_err()
                                    && crate::log()
                                {
                                    if crate::log() {
                                        println!("Failed to set bind to device");
                                    }
                                };
                            }
                        }
                    }
                    udp.connect(target.0).await?;
                    if !&payload[target.1..].is_empty() {
                        let _ = udp.send(&payload[target.1 + 2..]).await;
                    }
                    cm.write().await.insert(
                        cid,
                        Arc::new(
                            XHttpConnection {
                                io: IoType::Udp(IoUdp {
                                    udp,
                                    buff: Mutex::new(Vec::with_capacity(1024 * 8)),
                                }),
                                sec: RwLock::new(1),
                            }
                        )
                    );
                }
                crate::vless::SocketType::TCP => {
                    let mut tcp = crate::tcp::stream(target.0, sockopt).await?;
                    if !&payload[target.1..].is_empty() {
                        let _ = tcp.write(&payload[target.1..]).await?;
                    }
                    let (r, w) = tokio::io::split(tcp);
                    cm.write().await.insert(
                        cid,
                        Arc::new(
                            XHttpConnection {
                                io: IoType::Tcp(IoTcp {
                                    r: Mutex::new(r),
                                    w: Mutex::new(w),
                                }),
                                sec: RwLock::new(1),
                            }
                        )
                    );
                }
            };
        }
    } else {
        let mut buffed_data = Vec::with_capacity(body_len);
        // let mut buffed_data = Vec::with_capacity(1024 * 8);
        loop {
            if buffed_data.len() >= body_len {
                break;
            }

            if let Some(frame) = timeout(
                Duration::from_millis(xhttp_options.recv_data_frame_timeout),
                async { h2_stream.0.body_mut().data().await },
            )
            .await?
            {
                // buffed_data.extend_from_slice(&frame?);
                buffed_data.extend_from_slice(&frame?);
            } else {
                h2_stream.1.send_reset(h2::Reason::CANCEL);
                return Err(Box::new(crate::verror::VError::NoDataFrame));
            }
        }
        let conn: Arc<XHttpConnection> = if let Some(c) = cm.read().await.get(&cid) {
            c.clone()
        } else {
            h2_stream.1.send_reset(h2::Reason::CANCEL);
            return Err(Box::new(crate::verror::VError::NotInitiated));
        };
        wait_for_sec(&conn, sec, xhttp_options).await?;
        match &conn.io {
            IoType::Tcp(io) => {
                let _ = io.w.lock().await.write(&buffed_data).await?;
                let mut sec_lock = conn.sec.write().await;
                *sec_lock = sec + 1;
            }
            IoType::Udp(io) => {
                let mut udp_buff = io.buff.lock().await;
                if udp_buff.len() > 1024 * 16 {
                    h2_stream.1.send_reset(h2::Reason::CANCEL);
                    return Err(Box::new(crate::verror::VError::BufferOverflow));
                }
                udp_buff.extend_from_slice(&buffed_data);
                // process udp
                loop {
                    if udp_buff.len() < 3 {
                        break;
                    }
                    let psize =
                        crate::utils::convert_two_u8s_to_u16_be([udp_buff[0], udp_buff[1]])
                            as usize;
                    if psize == 0 || psize > udp_buff.len() - 2 {
                        break;
                    }
                    let _ = io.udp.send(&udp_buff[2..psize + 2]).await;
                    udp_buff.drain(0..psize + 2);
                }
            }
        }
    }
    h2_stream.1.send_response(gen_http_resp(xhttp_options, false)?, true)?;
    Ok(())
}

#[inline(always)]
fn get_cid(path: &str, get: bool) -> Result<Uuid, crate::verror::VError> {
    let uuid = if get {
        path.split("/").last()
    } else {
        // post uuid is before sec
        let parts = path.split("/").collect::<Vec<&str>>();
        if parts.len() < 3 {
            None
        } else {
            Some(parts[parts.len() - 2])
        }
    };

    if uuid.is_none() {
        return Err(crate::verror::VError::NoUUID);
    }
    if let Ok(uuid) = uuid::Uuid::parse_str(uuid.unwrap()) {
        Ok(uuid)
    } else {
        Err(crate::verror::VError::NoUUID)
    }
}

#[inline(always)]
async fn wait_for_sec(
    c: &Arc<XHttpConnection>,
    sec: usize,
    xhttp_options: &'static crate::config::Xhttp,
) -> Result<(), crate::verror::VError> {
    let mut elapsed = Duration::from_millis(0);
    loop {
        if elapsed >= Duration::from_millis(xhttp_options.wait_for_sec_timeout) {
            return Err(crate::verror::VError::WaitForSecTimeout);
        }
        {
            if *c.sec.read().await == sec {
                return Ok(());
            }
        }
        sleep(Duration::from_millis(xhttp_options.wait_for_sec_interval)).await;
        elapsed += Duration::from_millis(xhttp_options.wait_for_sec_interval);
    }
}

#[inline(always)]
async fn wait_for_init(
    cm: &ConnectionManager,
    cid: &Uuid,
    xhttp_options: &'static crate::config::Xhttp,
) -> Result<Arc<XHttpConnection>, crate::verror::VError> {
    let mut elapsed = Duration::from_millis(0);
    loop {
        if elapsed >= Duration::from_millis(xhttp_options.wait_for_init_timeout) {
            return Err(crate::verror::VError::WaitForInitTimeout);
        }
        {
            if let Some(conn) = cm.read().await.get(cid) {
                return Ok(conn.clone());
            }
        }
        sleep(Duration::from_millis(xhttp_options.wait_for_init_interval)).await;
        elapsed += Duration::from_millis(xhttp_options.wait_for_init_interval);
    }
}
