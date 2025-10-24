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

fn deserialize_uuid<'de, D>(deserializer: D) -> Result<uuid::Uuid, D::Error>
where
    D: serde::Deserializer<'de>,
{
    uuid::Uuid::try_parse(&<String as serde::Deserialize>::deserialize(deserializer)?)
        .map_err(serde::de::Error::custom)
}

#[derive(serde::Deserialize)]
pub struct User {
    pub name: String,
    #[serde(deserialize_with = "deserialize_uuid")]
    pub uuid: uuid::Uuid,
}

#[derive(serde::Deserialize, Clone, Copy)]
pub enum ResolvingMode {
    /// Only query for A (Ipv4) records
    Ipv4Only,
    /// Only query for AAAA (Ipv6) records
    Ipv6Only,
    /// Query for A and AAAA in parallel
    Ipv4AndIpv6,
    /// Query for Ipv6 if that fails, query for Ipv4
    Ipv6thenIpv4,
    /// Query for Ipv4 if that fails, query for Ipv6 (default)
    Ipv4thenIpv6,
}
impl ResolvingMode {
    pub fn convert(&self) -> hickory_resolver::config::LookupIpStrategy {
        match self {
            Self::Ipv4Only => hickory_resolver::config::LookupIpStrategy::Ipv4Only,
            Self::Ipv6Only => hickory_resolver::config::LookupIpStrategy::Ipv6Only,
            Self::Ipv4AndIpv6 => hickory_resolver::config::LookupIpStrategy::Ipv4AndIpv6,
            Self::Ipv6thenIpv4 => hickory_resolver::config::LookupIpStrategy::Ipv6thenIpv4,
            Self::Ipv4thenIpv6 => hickory_resolver::config::LookupIpStrategy::Ipv4thenIpv6,
        }
    }
}

#[derive(serde::Deserialize, Clone)]
pub struct Resolver {
    pub resolver: Option<String>,
    pub ips: Vec<std::net::IpAddr>,
    pub port: u16,
    pub trust_negative_responses: bool,
    pub ip_strategy: ResolvingMode,
    pub cache_size: usize,
    pub timeout: u64,
    pub num_concurrent_reqs: usize,
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
pub enum RuntimeMode {
    Single,
    Multi
}

#[derive(serde::Deserialize)]
pub struct Runtime {
    pub runtime_mode: RuntimeMode,
    pub worker_threads: Option<usize>,
    pub thread_stack_size: Option<usize>
}

#[derive(serde::Deserialize)]
pub struct Config {
    pub runtime: Runtime,
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
