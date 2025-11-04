use anyhow::Result;
use dashmap::DashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpStream;
use tracing::debug;
use socket2::{Socket, TcpKeepalive};
use std::time::Duration;

pub struct ConnectionPool {
    pools: Arc<DashMap<SocketAddr, Vec<TcpStream>>>,
    max_size_per_backend: usize,
}

impl ConnectionPool {
    pub fn new(max_size_per_backend: usize) -> Self {
        Self {
            pools: Arc::new(DashMap::new()),
            max_size_per_backend,
        }
    }

    pub async fn get(&self, backend: SocketAddr) -> Result<TcpStream> {
        // try to get from pool first
        {
            if let Some(mut pool) = self.pools.get_mut(&backend) {
                while let Some(stream) = pool.pop() {
                    if is_connection_alive(&stream).await {
                        debug!("Reusing pooled connection to {}", backend);
                        return Ok(stream);
                    } else {
                        debug!("Discarding dead pooled connection to {}", backend);
                        continue;
                    }
                }
            }
        }
        debug!("Creating new connection to {}", backend);
        let stream = TcpStream::connect(backend).await?;
        
        configure_keepalive(&stream)?;
        
        Ok(stream)
    }

    pub async fn return_connection(&self, backend: SocketAddr, stream: TcpStream) {
        let mut pool = self.pools.entry(backend).or_insert_with(Vec::new);

        if pool.len() < self.max_size_per_backend {
            pool.push(stream);
            debug!(
                "Returned connection to pool for {} (pool size: {})",
                backend,
                pool.len()
            );
        } else {
            debug!("Pool full for {}, dropping connection", backend);
            drop(stream);
        }
    }

   
}

async fn is_connection_alive(stream: &TcpStream) -> bool {
    let mut buf = [0u8; 1];
    match stream.try_read(&mut buf) {
        Ok(0) => false,  // EOF = connection closed
        Ok(_) => true,   // data available = connection alive (shouldn't happen for pooled connections)
        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
            // no data available but connection is still open
            // this is the expected case for idle pooled connections
            stream.peer_addr().is_ok()
        }
        Err(_) => false, // connection is dead
    }
}

fn configure_keepalive(stream: &TcpStream) -> Result<()> {
    let sock_ref = socket2::SockRef::from(stream);
    
    let keepalive = TcpKeepalive::new()
        .with_time(Duration::from_secs(30)) // probe after 30 seconds of idle
        .with_interval(Duration::from_secs(10)); // probe every 10 seconds
    
    sock_ref.set_tcp_keepalive(&keepalive)?;
    
    // enable TCP_NODELAY to reduce latency
    stream.set_nodelay(true)?;
    
    Ok(())
}

pub type SharedConnectionPool = Arc<ConnectionPool>;
