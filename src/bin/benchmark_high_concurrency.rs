use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// High-concurrency benchmark to stress-test the RwLock fix
/// This uses MANY concurrent workers to expose contention issues
#[tokio::main]
async fn main() {
    let duration_secs = 30;
    let num_workers = 1000;  // Much higher concurrency!

    let counter = Arc::new(AtomicU64::new(0));
    let error_counter = Arc::new(AtomicU64::new(0));
    let start = Instant::now();

    println!("Starting HIGH CONCURRENCY benchmark...");
    println!("Workers: {}, Duration: {}s", num_workers, duration_secs);
    println!("This tests RwLock contention under heavy concurrent load");

    let mut tasks = vec![];

    for _ in 0..num_workers {
        let counter = counter.clone();
        let error_counter = error_counter.clone();
        let task = tokio::spawn(async move {
            while start.elapsed().as_secs() < duration_secs {
                match TcpStream::connect("127.0.0.1:8080").await {
                    Ok(mut stream) => {
                        // Send request
                        if stream.write_all(b"test\n").await.is_ok() {
                            // Read response
                            let mut buf = [0u8; 1024];
                            if stream.read(&mut buf).await.is_ok() {
                                counter.fetch_add(1, Ordering::Relaxed);
                            } else {
                                error_counter.fetch_add(1, Ordering::Relaxed);
                            }
                        } else {
                            error_counter.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    Err(_) => {
                        error_counter.fetch_add(1, Ordering::Relaxed);
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
    let errors = error_counter.load(Ordering::Relaxed);
    let elapsed = start.elapsed().as_secs_f64();

    println!("\n=== High Concurrency Benchmark Results ===");
    println!("Total requests: {}", total);
    println!("Total errors: {}", errors);
    println!("Duration: {:.2}s", elapsed);
    println!("Requests/sec: {:.2}", total as f64 / elapsed);
    println!("Success rate: {:.2}%", (total as f64 / (total + errors) as f64) * 100.0);
}
