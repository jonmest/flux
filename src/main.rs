use anyhow::Result;
use tracing::info;

mod config;
mod proxy;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("flux=debug,info")
        .init();

    info!("Starting Flux load balancer");
    // Todo
    let config = config::Config::from_file("config.toml")?;
    info!("Loaded config with {} backends", config.backends.len());

    // just use first one for now
    let backend = &config.backends[0];

    // create and run proxy
    let proxy = proxy::Proxy::new(config.server.listen_addr, backend.addr);
    proxy.run().await?;

    info!("Flux is running.");
    Ok(())
}
