use std::net::SocketAddr;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use super::backend::Backend;
use super::health::{BackendHealth, HealthStatus};

pub struct BackendPool {
    backends: Vec<BackendHealth>,
    current_index: usize,
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
            backend_health.last_local_check = Instant::now();

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

    pub fn get_backend_health_updates(&self) -> Vec<crate::gossip::BackendUpdate> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        self.backends
            .iter()
            .map(|backend_health| crate::gossip::BackendUpdate {
                backend_addr: backend_health.backend.addr,
                is_healthy: backend_health.status == HealthStatus::Healthy,
                from_member: crate::gossip::MemberId("local".to_string()),
                timestamp,
            })
            .collect()
    }

    pub fn apply_backend_update(&mut self, update: &crate::gossip::BackendUpdate) {
        if let Some(backend_health) = self
            .backends
            .iter_mut()
            .find(|b| b.backend.addr == update.backend_addr)
        {
            let time_since_local_check = backend_health.last_local_check.elapsed();
            let trust_local = time_since_local_check < Duration::from_secs(6);

            let should_apply = if trust_local {
                if update.is_healthy {
                    false
                } else {
                    backend_health.status == HealthStatus::Healthy
                        && backend_health.consecutive_failures == 0
                }
            } else {
                true
            };

            if !should_apply {
                debug!(
                    "Ignoring gossip about {} - we checked locally {}s ago",
                    update.backend_addr,
                    time_since_local_check.as_secs()
                );
                return;
            }

            let new_status = if update.is_healthy {
                HealthStatus::Healthy
            } else {
                HealthStatus::Unhealthy
            };

            if backend_health.status != new_status {
                info!(
                    "Gossip update: Backend {} is now {} (from {})",
                    update.backend_addr,
                    if update.is_healthy {
                        "HEALTHY"
                    } else {
                        "UNHEALTHY"
                    },
                    update.from_member.0
                );
                backend_health.status = new_status;

                if update.is_healthy {
                    backend_health.consecutive_successes = 2;
                    backend_health.consecutive_failures = 0;
                } else {
                    backend_health.consecutive_failures = 2;
                    backend_health.consecutive_successes = 0;
                }
            }

            backend_health.last_check = Instant::now();
        }
    }
}

pub type SharedBackendPool = Arc<Mutex<BackendPool>>;
