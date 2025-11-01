use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub struct Backend {
    pub addr: SocketAddr,
    pub weight: u32,
}

pub struct BackendPool {
    backends: Vec<Backend>,
    current_index: usize, // For round robin
}

impl BackendPool {
    pub fn new(backends: Vec<Backend>) -> Self {
        Self {
            backends,
            current_index: 0,
        }
    }

    pub fn select_backend(&mut self) -> Option<Backend> {
        if self.backends.is_empty() {
            return None;
        }

        let backend = self.backends[self.current_index].clone();
        self.current_index = (self.current_index + 1) % self.backends.len();
        Some(backend)
    }
}

pub type SharedBackendPool = Arc<Mutex<BackendPool>>;
