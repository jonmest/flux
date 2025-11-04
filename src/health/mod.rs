use crate::backend::SharedBackendPool;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time;
use tracing::{debug, error};

pub struct HealthChecker {
    backend_pool: SharedBackendPool,
    check_interval: Duration,
    check_timeout: Duration,
}

impl HealthChecker {
    pub fn new(
        backend_pool: SharedBackendPool,
        check_interval_seconds: u64,
        check_timeout_seconds: u64,
    ) -> Self {
        Self {
            backend_pool,
            check_interval: Duration::from_secs(check_interval_seconds),
            check_timeout: Duration::from_secs(check_timeout_seconds),
        }
    }

    pub async fn run(&self) {
        let mut interval = time::interval(self.check_interval);
        loop {
            interval.tick().await;
            self.check_all_backends().await;
        }
    }

    async fn check_all_backends(&self) {
        let backends = {
            let pool = self.backend_pool.read().await;
            pool.get_all_backends()
        };

        let mut check_tasks = Vec::new();

        for backend in backends {
            let addr = backend.addr;
            let timeout = self.check_timeout;
            let pool = self.backend_pool.clone();

            let task = tokio::spawn(async move {
                let is_healthy = check_backend(addr, timeout).await;
                let mut pool = pool.write().await;
                pool.update_health(addr, is_healthy);
            });

            check_tasks.push(task);
        }

        for task in check_tasks {
            if let Err(e) = task.await {
                error!("Health check task failed: {}", e);
            }
        }
    }
}

async fn check_backend(addr: std::net::SocketAddr, timeout: Duration) -> bool {
    debug!("Health checking {}", addr);

    match time::timeout(timeout, TcpStream::connect(addr)).await {
        Ok(Ok(_stream)) => {
            debug!("Health check SUCCESS for {}", addr);
            true
        }
        Ok(Err(e)) => {
            debug!("Health check FAILED for {}: {}", addr, e);
            false
        }
        Err(_) => {
            debug!("Health check TIMEOUT for {}", addr);
            false
        }
    }
}
