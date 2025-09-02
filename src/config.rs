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
#[allow(clippy::upper_case_acronyms)]
pub enum Transporter {
    TCP,
    HTTP(Http),
    HttpUpgrade(Http),
    WS(Ws),
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
    pub resolver: Option<String>,
    pub ips: Vec<std::net::IpAddr>,
    pub port: u16,
    pub trust_negative_responses: bool,
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
#[allow(non_camel_case_types)]
pub enum LevelFilter {
    off,
    error,
    warn,
    info,
    debug,
    trace,
}

impl LevelFilter {
    pub fn convert(&self) -> log::LevelFilter {
        match self {
            Self::off => log::LevelFilter::Off,
            Self::error => log::LevelFilter::Error,
            Self::warn => log::LevelFilter::Warn,
            Self::info => log::LevelFilter::Info,
            Self::debug => log::LevelFilter::Debug,
            Self::trace => log::LevelFilter::Trace,
        }
    }
}

#[derive(serde::Deserialize)]
#[allow(dead_code)]
pub struct Log {
    pub level: LevelFilter,
    pub file: Option<std::path::PathBuf>,
}

#[derive(serde::Deserialize)]
pub struct Config {
    pub log: Log,
    pub tcp_proxy_buffer_size: Option<usize>,
    pub udp_proxy_buffer_size: Option<usize>,
    pub tcp_idle_timeout: u64,
    pub udp_idle_timeout: u64,
    pub users: Vec<User>,
    pub inbounds: Vec<Inbound>,
    pub resolver: Resolver,
    pub tcp_socket_options: TcpSocketOptions,
    pub blacklist: Option<Vec<BlackList>>,
}

pub fn load_config() -> Config {
    if let Ok(mut p) = std::env::current_exe()
        && p.pop()
    {
        let c = p.join("config.json");
        if c.exists() {
            let config_file = std::fs::read(c).expect("Can not read config file");
            let conf: Config = serde_json::from_slice(&config_file).expect("Malformed config file");
            return conf;
        }
    }
    let config_file = std::fs::read("config.json").expect("Can not read config file");
    serde_json::from_slice(&config_file).expect("Malformed config file")
}
