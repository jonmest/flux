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

    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config.toml".to_string());
    info!("Loading config from {}", config_path);

    let config = config::Config::from_file(&config_path)?;
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
    let suspect_timeout = Duration::from_millis(config.gossip.suspect_timeout_ms);

    let (mut gossip_layer, member_list) = gossip::GossipLayer::new(
        member_id,
        gossip_addr,
        suspect_timeout,
        backend_pool.clone(),
    )
    .await?;

    let seed_nodes = config.gossip.seed_nodes.clone();
    gossip_layer.join_cluster(seed_nodes).await;

    let socket_clone = gossip_layer.socket();
    let pending_pings_clone = gossip_layer.pending_pings();
    let pending_indirect_pings_clone = gossip_layer.pending_indirect_pings();
    let member_list_clone = member_list.clone();

    // msg receive loop
    tokio::spawn(async move {
        gossip_layer.run().await;
    });

    let gossip_interval = Duration::from_millis(config.gossip.gossip_interval_ms);
    let ping_timeout = Duration::from_millis(config.gossip.ping_timeout_ms);
    let backend_pool_for_gossip = backend_pool.clone();

    tokio::spawn(async move {
        gossip::GossipLayer::start_gossip_loop(
            member_list_clone,
            socket_clone,
            pending_pings_clone,
            pending_indirect_pings_clone,
            backend_pool_for_gossip,
            gossip_interval,
            ping_timeout,
        )
        .await;
    });

    info!("Gossip layer started on {}", gossip_addr);

    let proxy = proxy::Proxy::new(config.server.listen_addr, backend_pool);
    proxy.run().await?;

    info!("Flux is running.");
    Ok(())
}
