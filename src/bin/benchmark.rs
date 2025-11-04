use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[tokio::main]
async fn main() {
    let duration_secs = 30;
    let num_workers = 100;

    let counter = Arc::new(AtomicU64::new(0));
    let start = Instant::now();

    let mut tasks = vec![];

    for _ in 0..num_workers {
        let counter = counter.clone();
        let task = tokio::spawn(async move {
            while start.elapsed().as_secs() < duration_secs {
                if let Ok(mut stream) = TcpStream::connect("127.0.0.1:8080").await {
                    if stream.write_all(b"test\n").await.is_ok() {
                        let mut buf = [0u8; 1024];
                        if stream.read(&mut buf).await.is_ok() {
                            counter.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            }
        });
        tasks.push(task);
    }

    for task in tasks {
        task.await.ok();
    }

    let total = counter.load(Ordering::Relaxed);
    let elapsed = start.elapsed().as_secs_f64();

    println!("Total requests: {}", total);
    println!("Duration: {:.2}s", elapsed);
    println!("Requests/sec: {:.2}", total as f64 / elapsed);
}

