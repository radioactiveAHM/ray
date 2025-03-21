use std::net::SocketAddr;

#[derive(serde::Deserialize)]
pub struct Tls {
    pub enable: bool,
    pub alpn: Vec<String>,
    pub certificate: String,
    pub key: String,
}

#[derive(serde::Deserialize)]
#[allow(dead_code)]
pub struct Http {
    pub path: String,
    pub method: String,
    pub host: Option<String>,
}

#[derive(serde::Deserialize)]
#[allow(dead_code)]
#[allow(clippy::upper_case_acronyms)]
pub enum Transporter {
    TCP,
    HTTP(Http),
    HttpUpgrade(Http),
}

#[derive(serde::Deserialize)]
#[allow(dead_code)]
pub struct User {
    pub name: String,
    pub uuid: String,
}

#[derive(serde::Deserialize)]
pub struct Config {
    pub log: bool,
    pub tcp_idle_timeout: u64,
    pub udp_idle_timeout: u64,
    pub listen: SocketAddr,
    pub users: Vec<User>,
    pub transporter: Transporter,
    pub tls: Tls,
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
