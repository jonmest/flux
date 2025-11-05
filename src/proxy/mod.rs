use crate::backend::SharedBackendPool;
use crate::connection_pool::SharedConnectionPool;
use anyhow::{Error, Result, anyhow};
use socket2::{Socket, Domain, Type, Protocol};
use std::net::SocketAddr;
use tokio::net::{TcpListener, TcpStream};
use tracing::{debug, error, info};
use std::{net::TcpListener as StdTcpListener};

pub struct Proxy {
    listen_addr: SocketAddr,
    backend_pool: SharedBackendPool,
    connection_pool: SharedConnectionPool,
}

impl Proxy {
    pub fn new(
        listen_addr: SocketAddr,
        backend_pool: SharedBackendPool,
        connection_pool: SharedConnectionPool,
    ) -> Self {
        Self {
            listen_addr,
            backend_pool,
            connection_pool,
        }
    }

    pub async fn run(&self) -> Result<()> {
        let mut listeners = Vec::new();
        for _ in 0..8 {
            let std = bind_reuseport(&self.listen_addr)?;
            listeners.push(TcpListener::from_std(std)?);
        }

        for lst in listeners {
            let backend_pool = self.backend_pool.clone();
            let connection_pool = self.connection_pool.clone();

            tokio::spawn(async move {
                loop {
                    match lst.accept().await {
                        Ok((client_socket, client_addr)) => {
                            let backend_pool = backend_pool.clone();
                            let connection_pool = connection_pool.clone();

                            tokio::spawn(async move {
                                if let Err(e) = handle_connection(
                                    client_socket,
                                    backend_pool,
                                    connection_pool,
                                    client_addr,
                                ).await {
                                    error!("Error handling {client_addr}: {e:#}");
                                }
                            });
                        }
                        Err(e) => error!("accept error: {e:#}"),
                    }
                }
            });
        }

        futures::future::pending::<()>().await;
        Ok(())
    }
}

async fn handle_connection(
    mut client_socket: TcpStream,
    backend_pool: SharedBackendPool,
    connection_pool: SharedConnectionPool,
    client_addr: SocketAddr,
) -> Result<()> {
    let backend = {
        let pool = backend_pool.read().await;
        pool.select_backend()
            .ok_or_else(|| anyhow!("No backends available!"))?
    };
    debug!("Routing {} to backend {}", client_addr, backend.addr);
    let mut backend_socket = connection_pool.get(backend.addr).await?;

    debug!("Connected to backend {}", backend.addr);
    let result = copy_with_pooling(&mut client_socket, &mut backend_socket).await;
    if result.is_ok() {
        connection_pool
            .return_connection(backend.addr, backend_socket)
            .await;
    } else {
        debug!("Not returning connection to pool due to error");
    }
    result
}

async fn copy_with_pooling(client: &mut TcpStream, backend: &mut TcpStream) -> Result<()> {
    tokio::io::copy_bidirectional(client, backend).await?;
    Ok(())
}


fn bind_reuseport(addr: &SocketAddr) -> Result<StdTcpListener> {
    let addr: std::net::SocketAddr = (*addr).into();
    let domain = match addr {
        std::net::SocketAddr::V4(_) => Domain::IPV4,
        std::net::SocketAddr::V6(_) => Domain::IPV6,
    };

    let sock = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;
    sock.set_reuse_address(true)?;

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
    sock.set_reuse_port(true)?;
    sock.bind(&addr.into())?;
    sock.listen(4096)?;
    sock.set_nonblocking(true)?;
    Ok(sock.into())
}