use anyhow::Result;
use std::net::SocketAddr;
use tokio::io;
use tokio::net::{TcpListener, TcpStream};
use tracing::{debug, error, info};

pub struct Proxy {
    listen_addr: SocketAddr,
    backend_addr: SocketAddr,
}

impl Proxy {
    pub fn new(listen_addr: SocketAddr, backend_addr: SocketAddr) -> Self {
        Self {
            listen_addr,
            backend_addr,
        }
    }

    pub async fn run(&self) -> Result<()> {
        let listener = TcpListener::bind(self.listen_addr).await?;
        info!("Listening on {}", self.listen_addr);
        info!("Forwarding to {}", self.backend_addr);

        loop {
            match listener.accept().await {
                Ok((client_socket, client_addr)) => {
                    debug!("New connection from {}", client_addr);
                    let backend_addr = self.backend_addr;

                    tokio::spawn(async move {
                        if let Err(e) = handle_connection(client_socket, backend_addr).await {
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

async fn handle_connection(mut client_socket: TcpStream, backend_addr: SocketAddr) -> Result<()> {
    let mut backend_socket = TcpStream::connect(backend_addr).await?;

    debug!("Connected to backend {}", backend_addr);
    let (bytes_client_to_backend, bytes_backend_to_client) =
        io::copy_bidirectional(&mut client_socket, &mut backend_socket).await?;

    debug!(
        "Connection closed. Transferred {} bytes to backend, {} bytes to client",
        bytes_client_to_backend, bytes_backend_to_client
    );
    Ok(())
}
