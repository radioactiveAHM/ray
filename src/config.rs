use std::net::SocketAddr;

#[derive(serde::Deserialize)]
#[allow(dead_code)]
pub struct User{
    pub name: String,
    pub uuid: String
}

#[derive(serde::Deserialize)]
pub struct Config {
    pub log: bool,
    pub listen: SocketAddr,
    pub users: Vec<User>
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
    let conf: Config = serde_json::from_slice(&config_file).expect("Malformed config file");
    conf
}
