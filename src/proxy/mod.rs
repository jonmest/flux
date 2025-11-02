use crate::backend::SharedBackendPool;
use anyhow::{Result, anyhow};
use std::net::SocketAddr;
use tokio::io;
use tokio::net::{TcpListener, TcpStream};
use tracing::{debug, error, info};

pub struct Proxy {
    listen_addr: SocketAddr,
    backend_pool: SharedBackendPool,
}

impl Proxy {
    pub fn new(listen_addr: SocketAddr, backend_pool: SharedBackendPool) -> Self {
        Self {
            listen_addr,
            backend_pool,
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

                    tokio::spawn(async move {
                        if let Err(e) =
                            handle_connection(client_socket, backend_pool, client_addr).await
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
    client_addr: SocketAddr,
) -> Result<()> {
    let backend = {
        let mut pool = backend_pool.lock().await;
        pool.select_backend()
            .ok_or_else(|| anyhow!("No backends available!"))?
    };
    debug!("Routing {} to backend {}", client_addr, backend.addr);
    let mut backend_socket = TcpStream::connect(backend.addr).await?;

    debug!("Connected to backend {}", backend.addr);
    let (bytes_client_to_backend, bytes_backend_to_client) =
        io::copy_bidirectional(&mut client_socket, &mut backend_socket).await?;

    debug!(
        "Connection closed. Transferred {} bytes to backend, {} bytes to client",
        bytes_client_to_backend, bytes_backend_to_client
    );

    Ok(())
}
