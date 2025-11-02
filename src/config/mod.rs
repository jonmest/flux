use serde::Deserialize;
use std::net::SocketAddr;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub server: ServerConfig,
    pub gossip: GossipConfig,
    pub backends: Vec<Backend>,
    pub health_check: HealthCheckConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct GossipConfig {
    pub bind_addr: SocketAddr,
    pub gossip_interval_ms: u64,
    pub ping_timeout_ms: u64,
    pub suspect_timeout_ms: u64,
    pub seed_nodes: Vec<SocketAddr>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub listen_addr: SocketAddr,
}

#[derive(Debug, Deserialize, Clone)]
pub struct HealthCheckConfig {
    pub check_interval_seconds: u64,
    pub check_timeout_seconds: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Backend {
    pub addr: SocketAddr,
    pub weight: u32,
}

impl Config {
    pub fn from_file(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }
}
