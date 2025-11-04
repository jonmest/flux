use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let port = std::env::args()
        .nth(1)
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);

    let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).await?;
    println!("Echo server listening on port {}", port);

    loop {
        let (socket, _) = listener.accept().await?;
        tokio::spawn(async move {
            if let Err(e) = handle_client(socket).await {
                eprintln!("Error handling client: {}", e);
            }
        });
    }
}

async fn handle_client(mut socket: TcpStream) -> Result<(), Box<dyn std::error::Error>> {
    let mut buf = vec![0u8; 1024];

    loop {
        let n = socket.read(&mut buf).await?;
        if n == 0 {
            break;
        }

        // Echo back with "Backend says: " prefix
        let response = format!("Backend says: {}", String::from_utf8_lossy(&buf[..n]));
        socket.write_all(response.as_bytes()).await?;
    }

    Ok(())
}