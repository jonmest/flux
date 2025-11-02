use std::time::Instant;

use super::Backend;

#[derive(Debug, Clone, PartialEq)]
pub(super) enum HealthStatus {
    Healthy,
    Unhealthy,
}

#[derive(Debug)]
pub(super) struct BackendHealth {
    pub(super) backend: Backend,
    pub(super) status: HealthStatus,
    pub(super) consecutive_failures: u32,
    pub(super) consecutive_successes: u32,
    pub(super) last_check: Instant,
    pub(super) last_local_check: Instant,
}

impl BackendHealth {
    pub(super) fn new(backend: Backend) -> Self {
        Self {
            backend,
            status: HealthStatus::Healthy,
            consecutive_successes: 0,
            consecutive_failures: 0,
            last_check: Instant::now(),
            last_local_check: Instant::now(),
        }
    }
}
