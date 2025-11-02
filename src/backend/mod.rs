use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;
use tracing::{info, warn};

#[derive(Debug, Clone)]
pub struct Backend {
    pub addr: SocketAddr,
    pub weight: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HealthStatus {
    Healthy,
    Unhealthy,
}

#[derive(Debug)]
struct BackendHealth {
    backend: Backend,
    status: HealthStatus,
    consecutive_failures: u32,
    consecutive_successes: u32,
    last_check: Instant,
}

impl BackendHealth {
    fn new(backend: Backend) -> Self {
        Self {
            backend,
            status: HealthStatus::Healthy,
            consecutive_successes: 0,
            consecutive_failures: 0,
            last_check: Instant::now(),
        }
    }
}

pub struct BackendPool {
    backends: Vec<BackendHealth>,
    current_index: usize, // For round robin
}

impl BackendPool {
    pub fn new(backends: Vec<Backend>) -> Self {
        let backends = backends.into_iter().map(BackendHealth::new).collect();

        Self {
            backends,
            current_index: 0,
        }
    }

    pub fn select_backend(&mut self) -> Option<Backend> {
        if self.backends.is_empty() {
            return None;
        }

        let start_index = self.current_index;
        loop {
            let backend_health = &self.backends[self.current_index];
            self.current_index = (self.current_index + 1) % self.backends.len();

            if backend_health.status == HealthStatus::Healthy {
                return Some(backend_health.backend.clone());
            }

            // if no healhty backends
            if self.current_index == start_index {
                warn!("No healthy backends available!");
                return None;
            }
        }
    }

    pub fn update_health(&mut self, addr: SocketAddr, is_healthy: bool) {
        if let Some(backend_health) = self.backends.iter_mut().find(|b| b.backend.addr == addr) {
            backend_health.last_check = Instant::now();

            if is_healthy {
                backend_health.consecutive_successes += 1;
                backend_health.consecutive_failures = 0;

                if backend_health.consecutive_successes >= 2
                    && backend_health.status == HealthStatus::Unhealthy
                {
                    info!("Backend {} is now HEALTHY", addr);
                    backend_health.status = HealthStatus::Healthy;
                }
            } else {
                backend_health.consecutive_successes = 0;
                backend_health.consecutive_failures += 1;

                if backend_health.consecutive_failures >= 2
                    && backend_health.status == HealthStatus::Healthy
                {
                    warn!("Backend {} is now UNHEALTHY", addr);
                    backend_health.status = HealthStatus::Unhealthy;
                }
            }
        }
    }

    pub fn get_all_backends(&self) -> Vec<Backend> {
        self.backends.iter().map(|bh| bh.backend.clone()).collect()
    }
}

pub type SharedBackendPool = Arc<Mutex<BackendPool>>;
