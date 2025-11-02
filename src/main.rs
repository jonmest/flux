use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::info;

mod backend;
mod config;
mod gossip;
mod health;
mod proxy;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("flux=debug,info")
        .init();

    info!("Starting Flux load balancer");
    let config = config::Config::from_file("config.toml")?;
    info!("Loaded config with {} backends", config.backends.len());

    let backends: Vec<backend::Backend> = config
        .backends
        .into_iter()
        .map(|b| backend::Backend {
            addr: b.addr,
            weight: b.weight,
        })
        .collect();

    let backend_pool = Arc::new(Mutex::new(backend::BackendPool::new(backends)));
    let health_checker = health::HealthChecker::new(
        backend_pool.clone(),
        config.health_check.check_interval_seconds,
        config.health_check.check_timeout_seconds,
    );

    tokio::spawn(async move {
        health_checker.run().await;
    });
    info!("Health checker started.");

    let gossip_addr = config.gossip.bind_addr;
    let member_id = gossip::MemberId::generate(gossip_addr);
    let suspect_timeout = Duration::from_secs(5);

    let (mut gossip_layer, member_list) =
        gossip::GossipLayer::new(member_id, gossip_addr, suspect_timeout).await?;

    tokio::spawn(async move {
        gossip_layer.run().await;
    });

    info!("Gossip layer started on {}", gossip_addr);

    let proxy = proxy::Proxy::new(config.server.listen_addr, backend_pool);
    proxy.run().await?;

    info!("Flux is running.");
    Ok(())
}
