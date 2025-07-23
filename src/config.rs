use std::net::SocketAddr;

#[derive(serde::Deserialize)]
pub struct BlackList {
    pub name: String,
    pub domains: Vec<String>,
}

#[derive(serde::Deserialize, Clone, Copy)]
pub struct TcpSocketOptions {
    pub send_buffer_size: Option<u32>,
    pub recv_buffer_size: Option<u32>,
    pub nodelay: Option<bool>,
    pub keepalive: Option<bool>,
}

#[derive(serde::Deserialize)]
pub struct Tls {
    pub enable: bool,
    pub max_fragment_size: Option<usize>,
    pub alpn: Vec<String>,
    pub certificate: String,
    pub key: String,
}

#[derive(serde::Deserialize, Clone)]
pub struct Http {
    pub path: String,
    pub method: String,
    pub host: Option<String>,
}

#[derive(serde::Deserialize, Clone)]
pub struct Ws {
    pub path: String,
    pub host: Option<String>,
    pub threshold: Option<usize>,
    pub frame_size: Option<usize>,
}

#[derive(serde::Deserialize, Clone)]
pub struct Xhttp {
    // H2
    pub initial_connection_window_size: Option<u32>,
    pub initial_window_size: Option<u32>,
    pub target_window_size: Option<u32>,
    pub max_concurrent_streams: Option<u32>,
    pub max_frame_size: Option<u32>,
    pub max_send_buffer_size: Option<usize>,
    pub reset_stream_duration: Option<u64>,

    pub recv_data_frame_timeout: u64,
    pub tcp_buffer_size: usize,

    pub wait_for_sec_timeout: u64,
    pub wait_for_sec_interval: u64,

    pub wait_for_init_timeout: u64,
    pub wait_for_init_interval: u64,

    pub get_resp_headers: Vec<(String, String)>,
    pub post_resp_headers: Vec<(String, String)>,
}

#[derive(serde::Deserialize, Clone)]
#[allow(clippy::upper_case_acronyms)]
pub enum Transporter {
    TCP,
    HTTP(Http),
    HttpUpgrade(Http),
    WS(Ws),
    XHttp(Xhttp),
}

impl Transporter {
    pub fn is_xhttp(&self) -> Option<Xhttp> {
        match self {
            Self::XHttp(xhttp) => Some(xhttp.clone()),
            _ => None,
        }
    }
}

#[derive(serde::Deserialize)]
#[allow(dead_code)]
pub struct User {
    pub name: String,
    pub uuid: String,
}

#[derive(serde::Deserialize, Clone, Copy)]
pub enum ResolvingMode {
    IPv4, // Prefer IPv4 over IPv6
    IPv6, // Prefer IPv6 over IPv4
}

#[derive(serde::Deserialize, Clone)]
pub struct Resolver {
    pub address: Option<String>,
    pub ip_port: SocketAddr,
    pub mode: ResolvingMode,
}

#[derive(serde::Deserialize, Clone, Copy)]
pub enum TcpProxyMod {
    Buffer,
    Stack,
}

#[derive(serde::Deserialize, Clone)]
#[allow(dead_code)]
pub struct SockOpt {
    pub interface: Option<String>,
    pub bind_to_device: bool,
    pub mss: Option<i32>,
    pub congestion: Option<String>,
}

#[derive(serde::Deserialize)]
pub struct Inbound {
    pub listen: SocketAddr,
    pub transporter: Transporter,
    pub tls: Tls,
    pub sockopt: SockOpt,
}

#[derive(serde::Deserialize)]
pub struct Config {
    pub log: bool,
    pub thread_stack_size: Option<usize>,
    pub tcp_proxy_buffer_size: Option<usize>,
    pub udp_proxy_buffer_size: Option<usize>,
    pub tcp_close_delay: u64,
    pub tcp_idle_timeout: u64,
    pub udp_idle_timeout: u64,
    pub users: Vec<User>,
    pub inbounds: Vec<Inbound>,
    pub resolver: Resolver,
    pub tcp_socket_options: TcpSocketOptions,
    pub blacklist: Option<Vec<BlackList>>,
}

pub fn load_config() -> Config {
    if let Ok(mut p) = std::env::current_exe() {
        if p.pop() {
            let c = p.join("config.json");
            if c.exists() {
                let config_file = std::fs::read(c).expect("Can not read config file");
                let conf: Config =
                    serde_json::from_slice(&config_file).expect("Malformed config file");
                return conf;
            }
        }
    }
    let config_file = std::fs::read("config.json").expect("Can not read config file");
    serde_json::from_slice(&config_file).expect("Malformed config file")
}
