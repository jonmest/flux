use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Benchmark that REUSES connections to test connection pooling
#[tokio::main]
async fn main() {
    let duration_secs = 30;
    let num_workers = 100;  // Each worker maintains a persistent connection
    let requests_per_conn = 100; // Send multiple requests per connection before reconnecting

    let counter = Arc::new(AtomicU64::new(0));
    let error_counter = Arc::new(AtomicU64::new(0));
    let start = Instant::now();

    let mut tasks = vec![];

    println!("Starting pooled connection benchmark...");
    println!("Workers: {}, Duration: {}s", num_workers, duration_secs);
    println!("Requests per connection: {}", requests_per_conn);

    for worker_id in 0..num_workers {
        let counter = counter.clone();
        let error_counter = error_counter.clone();
        let task = tokio::spawn(async move {
            let mut local_count = 0u64;
            let mut local_errors = 0u64;

            while start.elapsed().as_secs() < duration_secs {
                // Establish connection
                match TcpStream::connect("127.0.0.1:8080").await {
                    Ok(mut stream) => {
                        // Reuse this connection for multiple requests
                        for _ in 0..requests_per_conn {
                            if start.elapsed().as_secs() >= duration_secs {
                                break;
                            }

                            // Send request
                            if stream.write_all(b"test\n").await.is_err() {
                                local_errors += 1;
                                break;  // Connection broken, reconnect
                            }

                            // Read response
                            let mut buf = [0u8; 1024];
                            match stream.read(&mut buf).await {
                                Ok(n) if n > 0 => {
                                    local_count += 1;
                                }
                                _ => {
                                    local_errors += 1;
                                    break;  // Connection broken, reconnect
                                }
                            }
                        }
                        // Connection drops here and should be pooled by Flux
                    }
                    Err(_) => {
                        local_errors += 1;
                        // Small delay to avoid hammering on connection failures
                        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                    }
                }
            }

            if worker_id == 0 {
                println!("Worker {} completed: {} requests, {} errors",
                         worker_id, local_count, local_errors);
            }

            counter.fetch_add(local_count, Ordering::Relaxed);
            error_counter.fetch_add(local_errors, Ordering::Relaxed);
        });
        tasks.push(task);
    }

    for task in tasks {
        task.await.ok();
    }

    let total = counter.load(Ordering::Relaxed);
    let errors = error_counter.load(Ordering::Relaxed);
    let elapsed = start.elapsed().as_secs_f64();

    println!("\n=== Pooled Connection Benchmark Results ===");
    println!("Total requests: {}", total);
    println!("Total errors: {}", errors);
    println!("Duration: {:.2}s", elapsed);
    println!("Requests/sec: {:.2}", total as f64 / elapsed);
    println!("Success rate: {:.2}%", (total as f64 / (total + errors) as f64) * 100.0);
}
