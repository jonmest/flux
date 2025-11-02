use std::net::SocketAddr;

#[derive(Debug, Clone)]
pub struct Backend {
    pub addr: SocketAddr,
    pub weight: u32,
}
