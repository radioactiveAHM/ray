use std::net::SocketAddr;

#[derive(serde::Deserialize)]
#[allow(dead_code)]
pub struct HTTP {
    pub path: String,
    pub method: String,
    pub host: Option<String>,
}

#[derive(serde::Deserialize)]
#[allow(dead_code)]
pub enum Transporter {
    TCP,
    HTTP(HTTP),
    HttpUpgrade(HTTP),
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
    pub listen: SocketAddr,
    pub users: Vec<User>,
    pub transporter: Transporter,
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
