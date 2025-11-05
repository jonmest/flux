use crate::backend::SharedBackendPool;
use crate::connection_pool::SharedConnectionPool;
use anyhow::{Error, Result, anyhow};
use std::net::SocketAddr;
use tokio::net::{TcpListener, TcpStream};
use tracing::{debug, error, info};

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
        let listener = TcpListener::bind(self.listen_addr).await?;
        info!("Listening on {}", self.listen_addr);

        loop {
            match listener.accept().await {
                Ok((client_socket, client_addr)) => {
                    debug!("New connection from {}", client_addr);
                    let backend_pool = self.backend_pool.clone();
                    let connection_pool = self.connection_pool.clone();

                    tokio::spawn(async move {
                        if let Err(e) = handle_connection(
                            client_socket,
                            backend_pool,
                            connection_pool,
                            client_addr,
                        )
                        .await
                        {
                            error!("Error handling connection from {}: {}", client_addr, e);
                        }
                    });
                }
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                }
            }
        }
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

async fn copy_with_pooling(
    client: &mut TcpStream,
    backend: &mut TcpStream,
) -> Result<()> {
    let res = tokio::time::timeout(std::time::Duration::from_secs(30), tokio::io::copy_bidirectional(client, backend)).await;
    res.map(|s| ())
        .map_err(|err| err.into())
}
