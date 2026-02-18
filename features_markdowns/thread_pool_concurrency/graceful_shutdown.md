# Graceful Shutdown via Signal Handling

**Feature**: Add graceful shutdown via signal handling (SIGTERM, SIGINT) with in-flight request draining
**Category**: Thread Pool & Concurrency
**Complexity**: 5/10
**Necessity**: 7/10

---

## Overview

The server currently has no signal handling. Pressing `Ctrl+C` (SIGINT) or sending `SIGTERM` (e.g., from `kill`, Docker, systemd) immediately terminates the process. Any in-flight requests are abruptly dropped — clients receive connection resets, partial responses are lost, and file handles may not be properly closed.

A graceful shutdown should:
1. Stop accepting new connections
2. Wait for in-flight requests to complete (with a timeout)
3. Shut down worker threads cleanly
4. Exit with a clean status code

### Current State

**`src/main.rs` lines 36-43** (listener loop):
```rust
for stream in listener.incoming() {
    let routes_clone = routes.clone();
    let stream = stream.unwrap();

    pool.execute(move || {
        handle_connection(stream, routes_clone);
    });
}
```

The `for stream in listener.incoming()` loop blocks forever. The only way to stop the server is to kill the process. The `ThreadPool::drop()` implementation does attempt graceful shutdown (drops the sender, joins worker threads), but it's never reached because the listener loop never exits.

**`src/lib.rs` lines 47-57** (Drop):
```rust
impl Drop for ThreadPool {
    fn drop(&mut self) {
        drop(self.sender.take());

        for worker in self.workers.drain(..) {
            println!("Shutting down worker {}", worker.id);
            worker.thread.join().unwrap();
        }
    }
}
```

The `Drop` implementation is already correct for graceful shutdown — it drops the sender (causing workers to see `Err` on `recv()` and exit) then joins all threads. The missing piece is signaling the listener loop to stop.

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/main.rs`

- Add signal handling for SIGINT and SIGTERM
- Break out of the listener loop when a shutdown signal is received
- Allow in-flight requests to drain before process exit

---

## Step-by-Step Implementation

### Approach: Atomic Flag + Non-Blocking Accept

The standard approach without external dependencies:
1. Set an `AtomicBool` flag when a signal is received
2. Set `TcpListener` to non-blocking mode so we can check the flag between accepts
3. When the flag is set, break out of the loop
4. `ThreadPool` drop handles the rest

### Step 1: Set Up the Shutdown Flag

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

fn main() {
    let shutdown = Arc::new(AtomicBool::new(false));

    // ... existing setup ...
}
```

### Step 2: Register Signal Handlers

On Unix, signal handlers must be async-signal-safe. Setting an `AtomicBool` is one of the few safe operations. We use `libc::signal` or the platform-specific approach.

However, since rcomm has **no external dependencies**, we need to use only `std`. The standard library doesn't provide direct signal handling, but we can use `std::process::exit`-style approaches or the `ctrlc` crate pattern using a dedicated signal thread.

**Approach without external dependencies — pipe-based self-notify**:

```rust
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::os::unix::net::UnixStream;
```

Actually, the simplest zero-dependency approach on Unix is:

```rust
use std::sync::atomic::{AtomicBool, Ordering};

static SHUTDOWN: AtomicBool = AtomicBool::new(false);

extern "C" fn signal_handler(_sig: libc::c_int) {
    SHUTDOWN.store(true, Ordering::SeqCst);
}
```

But using `libc` is an external dependency. Without it, we can use `unsafe` with raw signal numbers:

**Practical zero-dependency approach**:

```rust
use std::sync::atomic::{AtomicBool, Ordering};

static SHUTDOWN: AtomicBool = AtomicBool::new(false);

fn install_signal_handlers() {
    // SIGINT = 2, SIGTERM = 15 on all Unix platforms
    unsafe {
        libc_signal(2, signal_handler as usize);  // SIGINT
        libc_signal(15, signal_handler as usize); // SIGTERM
    }
}

extern "C" fn signal_handler(_sig: i32) {
    SHUTDOWN.store(true, Ordering::SeqCst);
}

// Minimal FFI binding — just the one function we need
extern "C" {
    fn signal(sig: i32, handler: usize) -> usize;
}

fn install_signal_handlers() {
    unsafe {
        signal(2, signal_handler as usize);  // SIGINT
        signal(15, signal_handler as usize); // SIGTERM
    }
}
```

This uses a minimal inline FFI declaration for `signal()` from the C standard library, which is always available. No `libc` crate needed.

### Step 3: Switch Listener to Non-Blocking with Polling

**Current**:
```rust
for stream in listener.incoming() {
```

This blocks indefinitely on each `accept()` call, so the shutdown flag is never checked.

**New — non-blocking accept with poll interval**:

```rust
use std::time::Duration;

listener.set_nonblocking(true).expect("Cannot set non-blocking");

loop {
    if SHUTDOWN.load(Ordering::SeqCst) {
        println!("Shutdown signal received, stopping listener...");
        break;
    }

    match listener.accept() {
        Ok((stream, _addr)) => {
            stream.set_nonblocking(false).expect("Cannot set blocking");
            let routes_clone = routes.clone();

            pool.execute(move || {
                handle_connection(stream, routes_clone);
            });
        }
        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
            // No pending connection — sleep briefly and check shutdown flag
            std::thread::sleep(Duration::from_millis(50));
        }
        Err(e) => {
            eprintln!("Accept error: {e}");
        }
    }
}
```

**Key details**:
- `set_nonblocking(true)` on the listener makes `accept()` return `WouldBlock` immediately when no connection is pending
- We sleep 50ms between polls to avoid busy-spinning (this adds at most 50ms of latency to accepting connections, which is negligible)
- Accepted streams are set back to blocking mode since `handle_connection()` expects blocking I/O
- When the shutdown flag is set, the loop exits, and `pool` is dropped, triggering graceful worker shutdown

### Step 4: Add Drain Timeout

The `ThreadPool::drop()` joins all workers, waiting indefinitely. If a worker is stuck on a slow response, shutdown hangs forever. Add a timeout:

**Update `src/lib.rs` Drop implementation**:

```rust
impl Drop for ThreadPool {
    fn drop(&mut self) {
        drop(self.sender.take());

        println!("Waiting for workers to finish...");

        for worker in self.workers.drain(..) {
            println!("Shutting down worker {}", worker.id);
            worker.thread.join().unwrap();
        }

        println!("All workers shut down.");
    }
}
```

The basic join is already correct. Adding a join timeout is complex in Rust's standard library (there's no `JoinHandle::join_timeout`). A practical approach is to set read/write timeouts on TCP streams (separate feature: request timeout), which ensures workers don't block indefinitely.

For now, the existing `Drop` is sufficient. Workers exit when:
1. The sender is dropped → `recv()` returns `Err` → worker breaks out of loop
2. If a worker is executing a job when the sender is dropped, it finishes that job, then loops back to `recv()`, gets `Err`, and exits

### Step 5: Print Shutdown Messages

```rust
// After the listener loop breaks:
println!("No longer accepting connections. Draining in-flight requests...");
drop(pool); // This blocks until all workers finish
println!("Server shut down gracefully.");
```

### Complete Updated `main()`

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

static SHUTDOWN: AtomicBool = AtomicBool::new(false);

extern "C" fn signal_handler(_sig: i32) {
    SHUTDOWN.store(true, Ordering::SeqCst);
}

extern "C" {
    fn signal(sig: i32, handler: usize) -> usize;
}

fn install_signal_handlers() {
    unsafe {
        signal(2, signal_handler as usize);  // SIGINT
        signal(15, signal_handler as usize); // SIGTERM
    }
}

fn main() {
    install_signal_handlers();

    let port = get_port();
    let address = get_address();
    let full_address = format!("{address}:{port}");
    let listener = TcpListener::bind(&full_address).unwrap();

    let pool = ThreadPool::new(4);

    let path = Path::new("./pages");
    let routes = build_routes(String::from(""), path);

    println!("Routes:\n{routes:#?}\n\n");
    println!("Listening on {full_address}");

    listener.set_nonblocking(true).expect("Cannot set non-blocking");

    loop {
        if SHUTDOWN.load(Ordering::SeqCst) {
            println!("\nShutdown signal received, stopping listener...");
            break;
        }

        match listener.accept() {
            Ok((stream, _addr)) => {
                stream.set_nonblocking(false).expect("Cannot set blocking");
                let routes_clone = routes.clone();

                pool.execute(move || {
                    handle_connection(stream, routes_clone);
                });
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                eprintln!("Accept error: {e}");
            }
        }
    }

    println!("No longer accepting connections. Draining in-flight requests...");
    drop(pool);
    println!("Server shut down gracefully.");
}
```

---

## Edge Cases & Handling

### 1. Signal During Job Execution
A worker is in the middle of `handle_connection()` when SIGTERM arrives. The listener stops accepting, but the worker finishes its current request normally. The client gets a complete response. This is the core benefit of graceful shutdown.

### 2. Multiple Signals
If the user presses `Ctrl+C` twice, the second signal just sets the already-true flag. No additional effect. If the user wants to force-kill, they use `Ctrl+\` (SIGQUIT) or `kill -9`, which are unblockable.

### 3. Queued but Unprocessed Jobs
Jobs in the channel queue when shutdown starts will still be processed — dropping the sender doesn't remove pending messages. Workers drain the remaining queue before seeing `Err` from `recv()`. This means all accepted connections get served.

### 4. Very Long In-Flight Requests
If a worker is stuck on a slow `write_all()` or `read_to_string()`, shutdown blocks indefinitely. The request timeout feature (separate) would bound this. Without it, operators can `kill -9` as a last resort.

### 5. Shutdown During Startup
If SIGTERM arrives before the listener loop starts, `SHUTDOWN` is set to `true` and the loop exits immediately on the first iteration. Clean shutdown with no requests processed.

### 6. Platform Compatibility
The `signal()` FFI function is POSIX-standard and available on all Unix-like systems (Linux, macOS, BSDs). On Windows, `SIGINT` is supported but `SIGTERM` is not meaningful. For Windows support, `SetConsoleCtrlHandler` would be needed (a future enhancement).

---

## Testing Strategy

### Manual Testing

```bash
cargo run &
SERVER_PID=$!

# Send some requests
curl http://127.0.0.1:7878/ &
curl http://127.0.0.1:7878/ &

# Send SIGTERM
kill $SERVER_PID

# Expected output:
# Shutdown signal received, stopping listener...
# No longer accepting connections. Draining in-flight requests...
# Shutting down worker 0
# Shutting down worker 1
# Shutting down worker 2
# Shutting down worker 3
# All workers shut down.
# Server shut down gracefully.
```

```bash
# Test Ctrl+C
cargo run
# Press Ctrl+C
# Same graceful shutdown output expected
```

### Integration Test

```rust
fn test_graceful_shutdown(addr: &str, server_pid: u32) -> Result<(), String> {
    // Verify server is running
    let resp = send_request(addr, "GET", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "server should be running")?;

    // Send SIGTERM
    unsafe { libc::kill(server_pid as i32, 15); }

    // Brief pause for shutdown
    std::thread::sleep(std::time::Duration::from_secs(2));

    // Verify server is stopped (connection should be refused)
    match TcpStream::connect(addr) {
        Err(_) => Ok(()), // Expected: connection refused
        Ok(_) => Err("Server should have stopped".into()),
    }
}
```

Note: This test is harder to integrate into the existing test framework since it kills the server. It may be better as a standalone script.

---

## Implementation Checklist

- [ ] Add `static SHUTDOWN: AtomicBool` flag
- [ ] Add `signal_handler` extern "C" function
- [ ] Add minimal FFI declaration for `signal()`
- [ ] Add `install_signal_handlers()` function
- [ ] Call `install_signal_handlers()` at start of `main()`
- [ ] Set listener to non-blocking mode
- [ ] Replace `for stream in listener.incoming()` with polling loop
- [ ] Check `SHUTDOWN` flag each iteration
- [ ] Set accepted streams back to blocking mode
- [ ] Handle `WouldBlock` with 50ms sleep
- [ ] Add shutdown log messages
- [ ] Run `cargo build` — verify compilation
- [ ] Run `cargo test` — all tests pass
- [ ] Run `cargo run --bin integration_test` — all integration tests pass
- [ ] Manually test `Ctrl+C` and `kill` graceful shutdown

---

## Backward Compatibility

The server now shuts down gracefully instead of abruptly on SIGINT/SIGTERM. This is purely additive behavior. The accept loop has a 50ms polling interval which adds negligible latency. All existing tests pass unchanged.

---

## Related Features

- **Security > Request Timeout**: Bounds how long workers can be stuck on a single request, ensuring shutdown completes in bounded time
- **Thread Pool > Worker Thread Panic Recovery**: Panicking workers can delay shutdown if `join()` is called on a dead thread (it returns immediately though)
- **Configuration > Command-Line Arguments**: A `--shutdown-timeout` flag could set the maximum drain time
- **Logging & Observability > Structured Access Logging**: Shutdown events should be logged with timestamps
- **Connection Handling > Arc Route Sharing**: Independent change, no interaction with shutdown
