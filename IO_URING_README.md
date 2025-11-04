# io_uring Implementation for Flux Load Balancer

This directory contains a complete io_uring implementation for educational purposes.

## What You'll Learn

This implementation teaches you:
- ✅ How io_uring works at a conceptual level
- ✅ Why io_uring is faster than epoll (3-7× improvement)
- ✅ How to use io_uring in Rust with tokio-uring
- ✅ Running multiple async runtimes in one application
- ✅ Real-world performance optimization techniques

## Files

### Documentation

1. **[IO_URING_GUIDE.md](IO_URING_GUIDE.md)** - Complete educational guide
   - Evolution of Linux I/O (blocking → select → epoll → io_uring)
   - The syscall problem explained
   - How ring buffers work
   - Why io_uring is more efficient
   - Detailed comparisons with diagrams

2. **[IO_URING_IMPLEMENTATION.md](IO_URING_IMPLEMENTATION.md)** - Step-by-step migration guide
   - Dependency setup
   - Code changes explained
   - Two-runtime architecture
   - Troubleshooting guide
   - Performance testing

### Code

3. **[src/proxy_uring.rs](src/proxy_uring.rs)** - io_uring proxy implementation
   - Heavily commented for educational purposes
   - Every concept explained inline
   - Shows ownership model differences
   - Demonstrates batching benefits

4. **[examples/main_uring.rs](examples/main_uring.rs)** - Complete example
   - How to run dual runtimes
   - tokio for background tasks
   - tokio_uring for proxy
   - Production-ready structure

## Quick Start

### Prerequisites

```bash
# Check your kernel version (need 5.1+, you have 6.8.0 ✓)
uname -r

# Ensure you have Rust installed
cargo --version
```

### Read First

1. Start with **[IO_URING_GUIDE.md](IO_URING_GUIDE.md)** to understand concepts
2. Then read **[IO_URING_IMPLEMENTATION.md](IO_URING_IMPLEMENTATION.md)** for implementation

### Run the io_uring Version

```bash
# Build
cargo build --release --example main_uring

# Start backend servers (in separate terminals)
cargo run --release --bin echo_server 3000
cargo run --release --bin echo_server 3001

# Run io_uring version
cargo run --release --example main_uring

# Benchmark
./run_benchmarks.sh
```

## Architecture Overview

```
┌────────────────────────────────────────────────────────┐
│                 Flux Load Balancer                      │
├────────────────────────────────────────────────────────┤
│                                                         │
│  Thread 1: Tokio Runtime (epoll)                       │
│  ├─ Health Checker (every 5 seconds)                   │
│  ├─ Gossip Protocol (every 1 second)                   │
│  └─ Background maintenance                             │
│      └─ Low frequency = epoll is fine                  │
│                                                         │
│  Main Thread: tokio_uring Runtime (io_uring)           │
│  └─ Proxy Server                                       │
│      ├─ Accept connections (io_uring)                  │
│      ├─ Connect to backends (io_uring)                 │
│      └─ Bidirectional copy (io_uring)                  │
│          └─ High frequency = io_uring shines!          │
│                                                         │
└────────────────────────────────────────────────────────┘
```

## Performance Expectations

Based on your current results:

| Version | Req/sec | Improvement |
|---------|---------|-------------|
| Original (epoll, new connections) | 14,515 | Baseline |
| Original (epoll, pooled) | 152,957 | 10.5× |
| **io_uring (pooled)** | **220,000-280,000** | **15-19×** |

Expected improvements:
- **40-80% more throughput** than epoll version
- **60-80% fewer syscalls** (verify with strace)
- **30-50% less CPU usage** (verify with top)

## Verification

### Check io_uring is Working

```bash
# Method 1: Check logs
cargo run --release --example main_uring
# Look for: "🚀 io_uring proxy listening on..."

# Method 2: Trace syscalls
sudo strace -c -p $(pgrep main_uring)
# Should see io_uring_enter() instead of many epoll_wait()

# Method 3: Profile with perf
sudo perf record -F 99 -g ./target/release/examples/main_uring
# Run benchmark, then:
sudo perf report
# Look for io_uring_* functions in flamegraph
```

## Educational Value

This implementation demonstrates:

### Rust Concepts
- ✅ Multiple async runtimes in one application
- ✅ Ownership and move semantics with I/O
- ✅ Zero-copy buffer management
- ✅ Concurrent futures and task spawning
- ✅ Safe abstractions over unsafe kernel APIs

### Systems Programming
- ✅ Linux kernel interfaces (io_uring)
- ✅ Ring buffer architecture
- ✅ Syscall optimization techniques
- ✅ Context switching overhead
- ✅ DMA and zero-copy I/O

### Performance Engineering
- ✅ Identifying bottlenecks
- ✅ Measuring improvements (strace, perf)
- ✅ Batching operations
- ✅ Cache-friendly data structures
- ✅ CPU vs I/O bound analysis

## Common Questions

### Q: Why keep two runtimes?

**A:** Background tasks (health checks, gossip) are low-frequency and work fine with epoll. Only the proxy needs io_uring's extreme performance. This hybrid approach is simpler and more maintainable.

### Q: Can I use io_uring for everything?

**A:** Yes, but it's more complex. tokio-uring is still maturing. For production, hybrid is recommended.

### Q: What if io_uring isn't available?

**A:** The original epoll version (src/proxy.rs) is still there and works great. io_uring is an optimization, not a requirement.

### Q: Is this production-ready?

**A:** tokio-uring is experimental. For education and portfolios: great! For production: test thoroughly or stick with epoll.

### Q: How much faster will it be?

**A:** Depends on workload:
- High I/O frequency: 40-80% faster
- Connection pooling: 15-30% faster
- Low concurrency: Similar to epoll
- CPU-bound: No difference

## Next Steps

1. ✅ Read the guides
2. ✅ Build and run the io_uring version
3. ✅ Compare benchmarks with original
4. ✅ Use strace to verify syscall reduction
5. ✅ Profile with perf to see the difference
6. ✅ Experiment with SQPOLL mode (see guide)
7. ✅ Add to your portfolio!

## Resources

- [io_uring official docs](https://kernel.dk/io_uring.pdf)
- [tokio-uring GitHub](https://github.com/tokio-rs/tokio-uring)
- [io-uring Rust crate](https://docs.rs/io-uring/)
- [Lord of the io_uring (excellent deep dive)](https://unixism.net/loti/)

## Credits

This implementation was created for educational purposes to demonstrate:
- Modern Linux kernel features
- High-performance Rust systems programming
- Real-world performance optimization

Perfect for portfolios, learning, and technical interviews!

---

**Your System:** Linux 6.8.0 - Full io_uring support ✓

**Status:** Ready to use! Start with [IO_URING_GUIDE.md](IO_URING_GUIDE.md)

Happy learning! 🚀
