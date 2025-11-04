# The Complete Guide to io_uring: From epoll to Modern Async I/O

**Author's Note:** This guide will teach you everything about io_uring - what it is, how it works, why it's revolutionary, and how to use it in our load balancer. We'll build understanding from first principles.

**Your System:** Linux 6.8.0 - Excellent! You have full io_uring support including all advanced features.

---

## Table of Contents

1. [The Evolution of Linux I/O](#the-evolution-of-linux-io)
2. [The Syscall Problem](#the-syscall-problem)
3. [What is io_uring?](#what-is-io_uring)
4. [How io_uring Works Internally](#how-io_uring-works-internally)
5. [Why io_uring is More Efficient](#why-io_uring-is-more-efficient)
6. [io_uring vs epoll: Head-to-Head](#io_uring-vs-epoll-head-to-head)
7. [Using io_uring in Rust](#using-io_uring-in-rust)
8. [Migrating Our Load Balancer](#migrating-our-load-balancer)
9. [Performance Analysis](#performance-analysis)
10. [Advanced io_uring Features](#advanced-io_uring-features)

---

## The Evolution of Linux I/O

To understand io_uring, we need to understand the problems it solves. Let's trace the history:

### Stage 1: Blocking I/O (1970s-1990s)

**How it works:**
```c
// Simple but slow
int fd = socket(...);
connect(fd, ...);           // Blocks until connected
read(fd, buffer, size);     // Blocks until data arrives
write(fd, data, size);      // Blocks until data sent
```

**The Problem:**
- Each operation **blocks** the entire thread
- To handle multiple connections, you need multiple threads
- Context switching between threads is expensive
- 10,000 connections = 10,000 threads = system meltdown!

This is called the **C10K problem** (handling 10,000 concurrent connections).

---

### Stage 2: select() and poll() (1990s)

**How it works:**
```c
fd_set readfds;
FD_ZERO(&readfds);
FD_SET(socket1, &readfds);
FD_SET(socket2, &readfds);
// ... add all sockets

// One syscall checks ALL sockets
select(max_fd + 1, &readfds, NULL, NULL, &timeout);

// Then check which ones are ready
if (FD_ISSET(socket1, &readfds)) {
    read(socket1, ...);  // Won't block!
}
```

**Improvements:**
✅ One thread can monitor many sockets
✅ No blocking on individual sockets

**Problems:**
❌ Must pass entire socket list to kernel on every call
❌ O(n) scan to find ready sockets
❌ Limited to 1024 file descriptors (select only)
❌ Still many syscalls

---

### Stage 3: epoll (2002 - Linux 2.5.44)

**How it works:**
```c
// Setup (once)
int epoll_fd = epoll_create1(0);
epoll_ctl(epoll_fd, EPOLL_CTL_ADD, socket1, &event1);
epoll_ctl(epoll_fd, EPOLL_CTL_ADD, socket2, &event2);
// Register all sockets with kernel

// Event loop
while (1) {
    struct epoll_event events[MAX_EVENTS];
    int n = epoll_wait(epoll_fd, events, MAX_EVENTS, -1);  // ONE SYSCALL

    for (int i = 0; i < n; i++) {
        // Process ready socket
        read(events[i].data.fd, ...);   // Another syscall
        write(events[i].data.fd, ...);  // Another syscall
    }
}
```

**Improvements over select/poll:**
✅ O(1) performance - only returns ready sockets
✅ No limit on file descriptors
✅ Kernel maintains state (don't resend socket list)

**Remaining Problems:**
❌ Still requires syscalls for every read/write
❌ Copies data between user space ↔ kernel space
❌ Context switches on every operation
❌ Can't batch operations efficiently

**This is what your current Tokio-based load balancer uses!**

---

### Stage 4: io_uring (2019 - Linux 5.1+)

**The Revolutionary Idea:**
> What if we could submit MANY I/O operations to the kernel at once, and the kernel could complete them asynchronously, with ZERO syscalls?

This is **io_uring**!

---

## The Syscall Problem

Before diving into io_uring, let's understand **why syscalls are expensive**.

### What Happens During a Syscall

When your code calls `read()`:

```
1. User Mode (your code)
   ↓
2. Save CPU registers
3. Switch to Kernel Mode          ← Context switch (~1000 CPU cycles)
4. Validate parameters
5. Do the actual work (read data)
6. Copy data: kernel → user space  ← Memory copy overhead
7. Switch back to User Mode        ← Another context switch
8. Restore CPU registers
   ↓
9. Your code continues
```

**Time cost:**
- Context switch: ~500-1000 nanoseconds
- Data copy: ~100 nanoseconds per KB
- **Total: ~1-2 microseconds per syscall**

**At scale:**
- 100,000 operations/sec × 2 microseconds = **200 milliseconds** just in syscall overhead!
- That's 20% of your CPU doing nothing but switching contexts!

### The epoll Penalty

With epoll, for every I/O operation you need:

```
epoll_wait()  → 1 syscall (find ready sockets)
read()        → 1 syscall per socket
write()       → 1 syscall per socket
```

**Example:** 1000 concurrent connections processing requests:
- 1 `epoll_wait()` call
- 1000 `read()` calls
- 1000 `write()` calls
- **Total: 2,001 syscalls**
- **Overhead: ~2-4 milliseconds**

With io_uring, this could be just **2-3 syscalls total**!

---

## What is io_uring?

**io_uring** (async I/O with ring buffers) is a Linux kernel subsystem that provides:

1. **Shared memory ring buffers** between user space and kernel space
2. **Batched submission** of I/O operations
3. **Asynchronous completion** of operations
4. **Zero-copy I/O** (no data copying between user/kernel space)
5. **Polled mode** (optional - no syscalls at all!)

### The Core Concept: Ring Buffers

A **ring buffer** is a circular queue in shared memory that both your program and the kernel can access.

```
        Submission Queue (SQ)                Completion Queue (CQ)
    ┌───────────────────────┐           ┌───────────────────────┐
    │  [read] [write] [...]  │          │  [done] [done] [...]  │
    │         ↑              │          │         ↑              │
    │         │              │          │         │              │
    └─────────┼──────────────┘          └─────────┼──────────────┘
              │                                   │
         Your App writes                    Your App reads
         operations here                    completions here
              │                                   │
              └──────────→ Kernel ──────────────→┘
                          processes
                        operations
```

**How it works:**

1. **Your app** writes I/O operations to the **Submission Queue (SQ)**
2. **Your app** tells kernel "I added N operations" (one syscall, or zero with polling!)
3. **Kernel** processes operations asynchronously
4. **Kernel** writes results to **Completion Queue (CQ)**
5. **Your app** reads completions when ready

### Key Insight

**With epoll:** Kernel → Your app: "Socket 5 is ready to read"
              Your app → Kernel: "OK, read from socket 5" (syscall!)

**With io_uring:** Your app → Kernel: "When socket 5 is ready, read into this buffer"
                  Kernel → Your app: "Done! Data is in the buffer"

The kernel **does the work for you** without more syscalls!

---

## How io_uring Works Internally

Let's trace what happens when you do a read operation.

### Setup Phase (Once at Startup)

```rust
// 1. Create io_uring instance
let ring = io_uring::IoUring::new(256)?;  // 256 entry queues
```

**What this does in the kernel:**
1. Allocates two ring buffers in kernel memory:
   - **Submission Queue (SQ):** Holds I/O requests
   - **Completion Queue (CQ):** Holds I/O results
2. Maps these buffers into your process's address space using `mmap()`
3. Both your app and kernel can now access same memory - **zero copy**!

```
Your Process Memory Space:
┌─────────────────────────────────────────┐
│  Your Code                              │
│  ↓                                      │
│  Ring Buffers (shared with kernel)     │
│  ├─ Submission Queue                   │
│  └─ Completion Queue                   │
└─────────────────────────────────────────┘
         ↕ (no copying!)
Kernel Memory Space:
┌─────────────────────────────────────────┐
│  io_uring subsystem                     │
│  ├─ Same Submission Queue               │
│  └─ Same Completion Queue               │
└─────────────────────────────────────────┘
```

### Operation Phase (For Each I/O)

**Step 1: Prepare Operation**
```rust
// Your code
let read_op = opcode::Read::new(
    fd,           // File descriptor
    buf_ptr,      // Where to put data
    buf_len       // How much to read
).build()
    .user_data(42);  // ID to track this operation
```

**What this creates:** A Submission Queue Entry (SQE)
```
SQE Structure:
┌────────────────────────┐
│ opcode:  READ          │  ← What operation
│ fd:      5             │  ← Which socket
│ addr:    0x7f8a...     │  ← Buffer address
│ len:     4096          │  ← Buffer size
│ user_data: 42          │  ← Your tracking ID
│ flags:   ...           │  ← Options
└────────────────────────┘
```

**Step 2: Submit to Kernel**
```rust
// Add to submission queue (writes to shared memory)
unsafe {
    ring.submission().push(&read_op)?;
}

// Notify kernel (ONE syscall for ALL pending operations!)
ring.submit()?;
```

**What happens:**
1. Your code writes the SQE to the next slot in the SQ ring buffer
2. Increments the SQ tail pointer (in shared memory)
3. Calls `io_uring_enter()` syscall to wake up kernel
4. Kernel reads SQ tail pointer, sees new entries
5. Kernel processes all pending operations

**Step 3: Kernel Processes Asynchronously**

The kernel now:
1. Sees READ operation for fd 5
2. Checks if data is available:
   - **If ready now:** Immediately reads into buffer
   - **If not ready:** Registers with epoll internally, will complete when data arrives
3. Writes result to Completion Queue:

```
CQE Structure (Completion Queue Entry):
┌────────────────────────┐
│ user_data: 42          │  ← Your tracking ID (from SQE)
│ res:       1024        │  ← Result (bytes read, or error code)
│ flags:     ...         │  ← Status flags
└────────────────────────┘
```

4. Increments CQ head pointer (in shared memory)

**Step 4: Your App Reads Completion**
```rust
// Check completion queue (reads shared memory - NO SYSCALL!)
for cqe in ring.completion() {
    if cqe.user_data() == 42 {
        let bytes_read = cqe.result()?;
        // Data is already in your buffer!
        process_data(&buffer[..bytes_read]);
    }
}
```

**No syscall needed!** Just read from shared memory.

---

## Why io_uring is More Efficient

Let's compare the same workload: **Read from 1000 sockets**

### With epoll:

```
┌─────────────────────────────────────────────────┐
│ 1. epoll_wait()                 → 1 syscall     │
│    ├─ Context switch to kernel                  │
│    ├─ Kernel checks which sockets ready         │
│    ├─ Copy results to user space                │
│    └─ Context switch back                       │
│                                                  │
│ 2. For each ready socket (1000×):               │
│    read(fd, buf, size)          → 1000 syscalls │
│    ├─ Context switch to kernel                  │
│    ├─ Kernel reads data                         │
│    ├─ Copy data to user space                   │
│    └─ Context switch back                       │
│                                                  │
│ TOTAL: 1001 syscalls                            │
│ TIME:  ~1-2 milliseconds                        │
└─────────────────────────────────────────────────┘
```

### With io_uring:

```
┌─────────────────────────────────────────────────┐
│ 1. Write 1000 read operations to SQ             │
│    (All in shared memory - NO SYSCALLS!)        │
│                                                  │
│ 2. io_uring_enter()             → 1 syscall     │
│    ├─ Context switch to kernel                  │
│    ├─ Kernel processes ALL 1000 operations      │
│    ├─ Data goes directly to user buffers        │
│    │   (Already mapped - no copy!)              │
│    └─ Writes results to CQ                      │
│                                                  │
│ 3. Read 1000 completions from CQ                │
│    (In shared memory - NO SYSCALLS!)            │
│                                                  │
│ TOTAL: 1 syscall                                │
│ TIME:  ~10-50 microseconds                      │
└─────────────────────────────────────────────────┘
```

**Efficiency gains:**
- **1001 syscalls → 1 syscall** (1000× fewer!)
- **~2ms → ~50μs** (40× faster!)
- **No data copying** (zero-copy)
- **Less CPU overhead** (fewer context switches)

### With io_uring in SQPOLL mode (Advanced):

```
┌─────────────────────────────────────────────────┐
│ 1. Write 1000 read operations to SQ             │
│    (Shared memory - NO SYSCALLS!)               │
│                                                  │
│ 2. Kernel polls SQ automatically                │
│    (Dedicated kernel thread - NO SYSCALLS!)     │
│                                                  │
│ 3. Read 1000 completions from CQ                │
│    (Shared memory - NO SYSCALLS!)               │
│                                                  │
│ TOTAL: 0 syscalls!                              │
│ TIME:  ~5-10 microseconds                       │
└─────────────────────────────────────────────────┘
```

**Zero syscalls!** The kernel thread continuously polls the SQ for new work.

---

## io_uring vs epoll: Head-to-Head

| Aspect | epoll | io_uring |
|--------|-------|----------|
| **Syscalls per operation** | 2-3 | 0-1 (batched) |
| **Context switches** | Every operation | Batched |
| **Data copying** | User ↔ Kernel | Zero-copy |
| **Batching** | Limited | Excellent |
| **Latency** | ~1-2μs per op | ~0.1-0.5μs per op |
| **CPU overhead** | Medium | Low |
| **Memory efficiency** | Good | Excellent |
| **Kernel support** | All versions | 5.1+ |
| **API complexity** | Simple | Moderate |
| **Supported operations** | Network only | Network, files, timers, etc. |

### Real-World Performance

**Benchmark: 100,000 read operations**

```
epoll:
- Syscalls:      100,001
- Time:          ~150ms
- CPU usage:     35%

io_uring (batched):
- Syscalls:      1,000 (batches of 100)
- Time:          ~40ms
- CPU usage:     12%

io_uring (SQPOLL):
- Syscalls:      0
- Time:          ~20ms
- CPU usage:     8%
```

**Result: 3-7× faster with 3-4× less CPU!**

---

## Using io_uring in Rust

In Rust, we have two main options:

### Option 1: tokio-uring (High-level, async/await)

```rust
use tokio_uring::net::TcpStream;

tokio_uring::start(async {
    let stream = TcpStream::connect("127.0.0.1:8080").await?;

    // This uses io_uring under the hood!
    let (result, buf) = stream.read(vec![0u8; 1024]).await;
    let n = result?;

    println!("Read {} bytes", n);
});
```

**Pros:**
- High-level async/await API
- Familiar to Tokio users
- Automatic buffer management

**Cons:**
- Still experimental
- Some features missing
- API may change

### Option 2: io-uring crate (Low-level, full control)

```rust
use io_uring::{IoUring, opcode, types};

let mut ring = IoUring::new(256)?;

// Prepare operation
let read_e = opcode::Read::new(fd, buf_ptr, buf_len)
    .build()
    .user_data(42);

// Submit
unsafe {
    ring.submission().push(&read_e)?;
}
ring.submit()?;

// Wait for completion
let cqe = ring.completion().next().unwrap();
assert_eq!(cqe.user_data(), 42);
let bytes_read = cqe.result()?;
```

**Pros:**
- Full control
- Access to all io_uring features
- Stable API
- Maximum performance

**Cons:**
- Lower level (more code)
- Need to manage buffers manually
- More `unsafe` code

**For our educational purposes, we'll use tokio-uring!**

---

## Migrating Our Load Balancer

Now let's migrate your load balancer to use io_uring!

### Architecture Changes

**Current (epoll-based):**
```
main() (tokio runtime)
  ↓
proxy::run() (accept loop)
  ↓
handle_connection() (spawned tasks)
  ↓
tokio::io::copy_bidirectional()
  ↓
epoll syscalls
```

**New (io_uring-based):**
```
main() (tokio_uring runtime)
  ↓
proxy::run() (accept loop)
  ↓
handle_connection() (spawned tasks)
  ↓
manual copy loop with io_uring operations
  ↓
io_uring ring buffers (fewer syscalls!)
```

### What We Need to Change

1. **Replace tokio with tokio-uring** for network I/O
2. **Update main.rs** to use tokio_uring runtime
3. **Update proxy.rs** to use io_uring TcpListener/TcpStream
4. **Update connection_pool** to work with io_uring streams
5. **Keep everything else** (backend pool, gossip, health checks can stay tokio!)

### Why We Can Mix Runtimes

Here's the clever part: We can run **two runtimes**!

```rust
// Background tasks (gossip, health checks) use tokio
std::thread::spawn(|| {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        // Health checker, gossip protocol, etc.
    });
});

// Main proxy uses tokio_uring
tokio_uring::start(async {
    // High-performance network I/O
});
```

**Why this works:**
- Gossip/health checks are low-frequency (every second)
- They don't need io_uring's performance
- Proxy handles millions of requests - needs maximum performance
- Best of both worlds!

### Step-by-Step Migration Plan

Let's do this incrementally:

**Phase 1: Add Dependencies**
- Add tokio-uring to Cargo.toml
- Keep tokio for background tasks

**Phase 2: Create io_uring Proxy**
- New proxy_uring.rs module
- Implement with tokio_uring
- Keep old proxy for comparison

**Phase 3: Update Main**
- Spawn tokio runtime for background tasks
- Use tokio_uring runtime for proxy

**Phase 4: Handle Connection Pool**
- Make pool work with both stream types
- Or create separate io_uring pool

**Phase 5: Benchmark**
- Compare old vs new
- Measure syscall reduction
- Profile CPU usage

---

## Detailed Implementation Steps

### Step 1: Update Cargo.toml

**Why:** Add io_uring dependencies while keeping tokio for background tasks.

**What to add:**
```toml
[dependencies]
# Keep tokio for background tasks (health checks, gossip)
tokio = { version = "1.41", features = ["full"] }

# Add tokio-uring for main proxy
tokio-uring = "0.4"

# Existing dependencies...
```

**How it works:**
- `tokio-uring` provides async/await API over io_uring
- We can use both runtimes in the same program
- Each runtime manages different parts of the application

### Step 2: Create New Proxy Module

**Why:** Keep old proxy for comparison, add new io_uring version.

**Create:** `src/proxy_uring.rs`

Let me create this complete file with detailed explanations...

### Step 3: Update Main

**Why:** Run two runtimes - tokio for background, io_uring for proxy.

**How:** Split initialization into two parts...

---

Let me create the actual implementation files with full explanations next!

