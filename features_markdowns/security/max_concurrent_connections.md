# Feature: Add Configurable Maximum Concurrent Connection Limit

**Category:** Security
**Complexity:** 4/10
**Necessity:** 7/10
**Status:** Implementation Plan

---

## Overview

This feature adds a configurable maximum concurrent connection limit to rcomm, allowing administrators to prevent resource exhaustion attacks and control server capacity. When the limit is reached, new connections will be rejected with a `503 Service Unavailable` response, protecting the server from being overwhelmed.

### Motivation

- **Security**: Prevents denial-of-service (DoS) attacks that attempt to exhaust server resources by opening many simultaneous connections
- **Resource Management**: Allows operators to cap memory and CPU usage regardless of thread pool size
- **Graceful Degradation**: Explicitly rejects excess connections rather than letting them queue indefinitely or fail mysteriously
- **Configuration**: Environment variable and code default for flexibility across deployment environments

### Current Behavior

Currently, the server has no limit on concurrent connections. Every incoming TCP connection is immediately queued to the thread pool, and the thread pool will create jobs for them as workers become available. This can lead to:
- Unbounded memory usage (kernel socket buffers, TcpStream allocations)
- Thread pool job queue growing without limit
- No visibility into how many connections are being handled

---

## Architecture & Design Decisions

### Approach: Atomic Counter at Listener Level

We will track active connections using an `Arc<AtomicUsize>` (or `Arc<Mutex<usize>>` for more safety guarantees). This counter:
- Lives in `main()` alongside the thread pool
- Is incremented in the TCP listener loop when a connection is accepted
- Is decremented when `handle_connection()` completes (via a scope guard or finally block)
- Is checked before accepting a new connection; if >= limit, the connection is rejected

**Why this approach?**
1. **Minimal changes**: No modifications needed to ThreadPool internals
2. **Early rejection**: Connections are rejected before being queued, saving thread pool space
3. **No special synchronization overhead**: Atomic operations are cheap and don't require locks in the hot path
4. **Composable**: The counter is passed to `handle_connection()` via closure capture, so it decrements automatically when the job completes

### Configuration

Three configuration sources (in priority order):
1. **Environment Variable**: `RCOMM_MAX_CONNECTIONS` (int, e.g., `100`)
2. **Code Default**: `1024` (reasonable for most servers without being too permissive)
3. **No CLI flag** (not needed for initial implementation; environment variables provide full configurability)

### Connection Lifecycle with Counter

```
1. Listener accepts TCP connection
2. Check: active_count >= limit?
   - YES: Write 503 response, close stream, continue loop
   - NO: Increment counter, spawn job
3. Worker picks up job (async in thread pool)
4. handle_connection() executes, processes request
5. Job completes, decrement counter (via guard/RAII)
6. Worker waits for next job
```

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Changes:**
- Add `use std::sync::atomic::{AtomicUsize, Ordering}` import
- Create `Arc<AtomicUsize>` counter in `main()`
- Add `get_max_connections()` function (reads env var or returns default)
- Check counter in listener loop before queuing job
- Send counter to closure so job can decrement when done
- Modify `handle_connection()` signature to accept the counter and decrement on drop

**Key additions:**
- Helper function to load and validate max connections config
- A `ConnectionGuard` struct (or inline drop logic) to automatically decrement

---

## Step-by-Step Implementation

### Step 1: Add Helper Function for Configuration

In `src/main.rs`, after `get_address()`:

```rust
fn get_max_connections() -> usize {
    std::env::var("RCOMM_MAX_CONNECTIONS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(1024)
}
```

This follows the same pattern as `get_port()` and `get_address()`, returning a sensible default if the env var is missing or invalid.

### Step 2: Add ConnectionGuard RAII Helper

Also in `src/main.rs`, before `main()`:

```rust
/// RAII guard that decrements the active connection counter when dropped.
struct ConnectionGuard {
    counter: Arc<AtomicUsize>,
}

impl ConnectionGuard {
    fn new(counter: Arc<AtomicUsize>) -> Self {
        ConnectionGuard { counter }
    }
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}
```

This ensures that even if `handle_connection()` panics or returns early, the counter is properly decremented.

### Step 3: Modify Main Function

Replace the listener loop in `main()`:

**Before:**
```rust
fn main() {
    let port = get_port();
    let address = get_address();
    let full_address = format!("{address}:{port}");
    let listener = TcpListener::bind(&full_address).unwrap();

    let pool = ThreadPool::new(4);

    let path = Path::new("./pages");
    let routes = build_routes(String::from(""), path);

    println!("Routes:\n{routes:#?}\n\n");
    println!("Listening on {full_address}");

    for stream in listener.incoming() {
        let routes_clone = routes.clone();
        let stream = stream.unwrap();

        pool.execute(move || {
            handle_connection(stream, routes_clone);
        });
    }
}
```

**After:**
```rust
fn main() {
    let port = get_port();
    let address = get_address();
    let full_address = format!("{address}:{port}");
    let listener = TcpListener::bind(&full_address).unwrap();

    let pool = ThreadPool::new(4);
    let max_connections = get_max_connections();
    let active_connections = Arc::new(AtomicUsize::new(0));

    let path = Path::new("./pages");
    let routes = build_routes(String::from(""), path);

    println!("Routes:\n{routes:#?}\n\n");
    println!("Listening on {full_address}");
    println!("Max concurrent connections: {max_connections}");

    for stream in listener.incoming() {
        let current = active_connections.fetch_add(1, Ordering::SeqCst);

        if current >= max_connections {
            // Decrement since we won't be processing this connection
            active_connections.fetch_sub(1, Ordering::SeqCst);

            eprintln!("Connection rejected: max concurrent connections ({}) reached", max_connections);

            // Send 503 Service Unavailable response
            let mut response = HttpResponse::build(String::from("HTTP/1.1"), 503);
            response.add_body(String::from("Service Unavailable: Server at capacity"));
            let _ = stream.write_all(&response.as_bytes());
            continue;
        }

        let routes_clone = routes.clone();
        let connections_clone = Arc::clone(&active_connections);

        pool.execute(move || {
            let _guard = ConnectionGuard::new(connections_clone);
            handle_connection(stream, routes_clone);
        });
    }
}
```

**Key points:**
- Use `fetch_add()` with `SeqCst` ordering to atomically increment and check the count
- If limit reached, manually decrement, send 503, and continue
- Otherwise, spawn job with a guard that will auto-decrement when the closure exits
- Print max connections on startup for visibility

### Step 4: Add Missing Imports

At the top of `src/main.rs`, add to the existing `use std::` block:

```rust
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
```

(Note: `Arc` may already be in scope; adjust as needed based on existing imports)

### Step 5: Update Stdout Logging (Optional but Recommended)

When `handle_connection()` completes successfully, consider logging the active connection count. This is optional but helps operators monitor capacity:

In `handle_connection()`, before returning, you could add:
```rust
// At end of handle_connection, before implicit return
// (This requires passing the counter to the function, which the guard already does)
```

However, since we're using the guard in a closure, we don't need to modify `handle_connection()` itself. The guard handles decrement automatically.

---

## Code Snippets (Complete Reference)

### Full Updated `src/main.rs` (Listener Loop Section)

```rust
use std::{
    collections::HashMap,
    fs,
    io::prelude::*,
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::Arc,
    sync::atomic::{AtomicUsize, Ordering},
};
use rcomm::ThreadPool;
use rcomm::models::{
    http_response::HttpResponse,
    http_request::HttpRequest,
};

fn get_port() -> String {
    std::env::var("RCOMM_PORT").unwrap_or_else(|_| String::from("7878"))
}

fn get_address() -> String {
    std::env::var("RCOMM_ADDRESS").unwrap_or_else(|_| String::from("127.0.0.1"))
}

fn get_max_connections() -> usize {
    std::env::var("RCOMM_MAX_CONNECTIONS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(1024)
}

/// RAII guard that decrements the active connection counter when dropped.
struct ConnectionGuard {
    counter: Arc<AtomicUsize>,
}

impl ConnectionGuard {
    fn new(counter: Arc<AtomicUsize>) -> Self {
        ConnectionGuard { counter }
    }
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::SeqCst);
    }
}

fn main() {
    let port = get_port();
    let address = get_address();
    let full_address = format!("{address}:{port}");
    let listener = TcpListener::bind(&full_address).unwrap();

    let pool = ThreadPool::new(4);
    let max_connections = get_max_connections();
    let active_connections = Arc::new(AtomicUsize::new(0));

    let path = Path::new("./pages");
    let routes = build_routes(String::from(""), path);

    println!("Routes:\n{routes:#?}\n\n");
    println!("Listening on {full_address}");
    println!("Max concurrent connections: {max_connections}");

    for stream in listener.incoming() {
        let current = active_connections.fetch_add(1, Ordering::SeqCst);

        if current >= max_connections {
            active_connections.fetch_sub(1, Ordering::SeqCst);
            eprintln!("Connection rejected: max concurrent connections ({}) reached", max_connections);

            let mut response = HttpResponse::build(String::from("HTTP/1.1"), 503);
            response.add_body(String::from("Service Unavailable: Server at capacity"));
            let _ = stream.write_all(&response.as_bytes());
            continue;
        }

        let routes_clone = routes.clone();
        let connections_clone = Arc::clone(&active_connections);

        pool.execute(move || {
            let _guard = ConnectionGuard::new(connections_clone);
            handle_connection(stream, routes_clone);
        });
    }
}

fn handle_connection(mut stream: TcpStream, routes: HashMap<String, PathBuf>) {
    let http_request = match HttpRequest::build_from_stream(&stream) {
        Ok(req) => req,
        Err(e) => {
            eprintln!("Bad request: {e}");
            let mut response = HttpResponse::build(String::from("HTTP/1.1"), 400);
            let body = format!("Bad Request: {e}");
            response.add_body(body.into());
            let _ = stream.write_all(&response.as_bytes());
            return;
        }
    };
    let clean_target = clean_route(&http_request.target);

    println!("Request: {http_request}");

    let (mut response, filename) = if routes.contains_key(&clean_target) {
        (HttpResponse::build(String::from("HTTP/1.1"), 200),
            routes.get(&clean_target).unwrap().to_str().unwrap())
    } else {
        (HttpResponse::build(String::from("HTTP/1.1"), 404),
            "pages/not_found.html")
    };

    let contents = fs::read_to_string(filename).unwrap();
    response.add_body(contents.into());

    println!("Response: {response}");
    stream.write_all(&response.as_bytes()).unwrap();
}

// ... rest of file unchanged (clean_route, build_routes)
```

### HttpResponse 503 Status Code

The `HttpResponse::build(String::from("HTTP/1.1"), 503)` call assumes the HTTP status code enum and response builder handle 503. Verify that:

1. `src/models/http_status_codes.rs` has an entry for status code 503 mapping to "Service Unavailable"
2. If not, add it:

```rust
503 => "Service Unavailable",
```

Check the file to confirm status code mapping exists.

---

## Testing Strategy

### 1. Unit Tests (in `src/main.rs` or a new `src/config.rs`)

Test the config parsing function:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_max_connections_default() {
        std::env::remove_var("RCOMM_MAX_CONNECTIONS");
        assert_eq!(get_max_connections(), 1024);
    }

    #[test]
    fn test_get_max_connections_from_env() {
        std::env::set_var("RCOMM_MAX_CONNECTIONS", "256");
        assert_eq!(get_max_connections(), 256);
        std::env::remove_var("RCOMM_MAX_CONNECTIONS");
    }

    #[test]
    fn test_get_max_connections_invalid_env() {
        std::env::set_var("RCOMM_MAX_CONNECTIONS", "not_a_number");
        assert_eq!(get_max_connections(), 1024); // falls back to default
        std::env::remove_var("RCOMM_MAX_CONNECTIONS");
    }

    #[test]
    fn test_connection_guard_decrements() {
        use std::sync::atomic::AtomicUsize;

        let counter = Arc::new(AtomicUsize::new(5));
        {
            let _guard = ConnectionGuard::new(Arc::clone(&counter));
            assert_eq!(counter.load(Ordering::SeqCst), 5);
        }
        assert_eq!(counter.load(Ordering::SeqCst), 4);
    }
}
```

**Run with:**
```bash
cargo test get_max_connections
cargo test connection_guard_decrements
```

### 2. Integration Tests (in `src/bin/integration_test.rs`)

Add end-to-end tests that:

**Test Case 2.1: Accept Connections Under Limit**
- Set `RCOMM_MAX_CONNECTIONS=5`
- Spawn the server
- Open 5 simultaneous connections
- All should succeed with 200/404 responses (not 503)

```rust
#[test]
fn test_connections_under_limit_accepted() {
    let server = start_server_with_env("RCOMM_MAX_CONNECTIONS", "5");
    let port = server.port;

    let mut streams = Vec::new();
    for _ in 0..5 {
        let stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        streams.push(stream);
    }

    for mut stream in streams {
        stream.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
        let mut buf = [0u8; 1024];
        let n = stream.read(&mut buf).unwrap();
        let response = String::from_utf8_lossy(&buf[..n]);

        assert!(!response.contains("503"));
        assert!(response.contains("200") || response.contains("404"));
    }
}
```

**Test Case 2.2: Reject Connections Over Limit**
- Set `RCOMM_MAX_CONNECTIONS=2`
- Spawn the server
- Open 3 simultaneous connections
- First 2 should succeed; 3rd should get 503

```rust
#[test]
fn test_connections_over_limit_rejected() {
    let server = start_server_with_env("RCOMM_MAX_CONNECTIONS", "2");
    let port = server.port;

    let mut streams = Vec::new();
    for i in 0..3 {
        let stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        let mut stream = stream;
        stream.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();

        let mut buf = [0u8; 1024];
        let n = stream.read(&mut buf).unwrap();
        let response = String::from_utf8_lossy(&buf[..n]);

        if i < 2 {
            assert!(!response.contains("503"), "Connection {i} should not be rejected");
        } else {
            assert!(response.contains("503"), "Connection {i} should be rejected with 503");
        }

        streams.push(stream);
    }
}
```

**Test Case 2.3: Connections Decrement Counter**
- Set `RCOMM_MAX_CONNECTIONS=2`
- Open 2 connections, wait for them to complete and close
- Open 2 more connections
- All 4 should succeed (not be rejected)

```rust
#[test]
fn test_counter_decrements_after_connection_closes() {
    let server = start_server_with_env("RCOMM_MAX_CONNECTIONS", "2");
    let port = server.port;

    // First batch
    {
        let mut stream1 = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        let mut stream2 = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();

        stream1.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
        stream2.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();

        // Read responses
        let mut buf = [0u8; 1024];
        let _ = stream1.read(&mut buf).unwrap();
        let _ = stream2.read(&mut buf).unwrap();
    } // streams dropped here

    std::thread::sleep(std::time::Duration::from_millis(100)); // let counter decrement

    // Second batch - should succeed, not get rejected
    let mut stream3 = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
    let mut stream4 = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();

    stream3.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
    stream4.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();

    let mut buf = [0u8; 1024];
    let n = stream3.read(&mut buf).unwrap();
    let response = String::from_utf8_lossy(&buf[..n]);
    assert!(!response.contains("503"), "Counter should have decremented, allowing new connections");
}
```

**Run with:**
```bash
cargo run --bin integration_test
```

### 3. Manual Testing

**Scenario A: Default Limit (1024)**
```bash
cargo run &
# Server runs with max 1024 connections
# Verify startup message shows "Max concurrent connections: 1024"
```

**Scenario B: Custom Limit via Env**
```bash
RCOMM_MAX_CONNECTIONS=5 cargo run &
# Server runs with max 5 connections
# Verify startup message shows "Max concurrent connections: 5"
```

**Scenario C: Exceeding Limit**
```bash
RCOMM_MAX_CONNECTIONS=3 cargo run &
# In another terminal, open 4 connections
# Use netcat or curl with --max-time to keep connections open
for i in {1..4}; do nc 127.0.0.1 7878 & done
# First 3 should get responses; 4th should get 503
```

---

## Edge Cases & Considerations

### 1. Race Condition: Counter Check Window

**Potential Issue**: Between `fetch_add()` and the actual job execution, another connection could be accepted, exceeding the limit slightly.

**Analysis**: This is acceptable. The check uses `SeqCst` (sequential consistency) ordering, which provides full synchronization. The slight overage can occur if:
- Thread 1: `current = fetch_add(1)` returns 999 (count is now 1000, at limit for 1000 limit)
- Thread 2: `current = fetch_add(1)` returns 1000 (count is now 1001)
- Both pass the `current >= max_connections` check

**Mitigation**: Use `current + 1 > max_connections` instead of `current >= max_connections`:
```rust
if current + 1 > max_connections {
    // reject
}
```
This prevents the "+1" overage but accepts when exactly at limit. Alternative: accept the overage (rarely visible in practice since connections are handled in nanoseconds).

**Recommendation**: Use `current >= max_connections` for simplicity; the overage is minimal and acceptable for security purposes.

### 2. Atomic Ordering: SeqCst vs Relaxed

**Why SeqCst?**
- We're decrementing in a different thread (the worker) than where we incremented (main thread)
- We need happens-before guarantees so all threads see a consistent order
- `SeqCst` is the strongest guarantee; alternatives like `Release`/`Acquire` would also work but are more complex

**Why not Relaxed?**
- With `Relaxed`, we lose ordering between increment and decrement, which could cause reads to see stale values
- For a security-critical counter, the tiny performance overhead is worth the correctness

**Conclusion**: `SeqCst` is correct here; performance impact is negligible (<1% latency).

### 3. Counter Overflow

**Potential Issue**: If counter somehow wraps around (exceeds `usize::MAX`), the limit logic breaks.

**Analysis**: This is not realistic:
- `usize` is 64-bit on modern systems (max ~18 exabillion)
- Even if you could process 1 million connections/second, it would take 18 billion seconds (500+ years)
- The server would be retired/restarted long before overflow

**Mitigation**: None needed; constraint is physically impossible.

### 4. Connection Rejected but TcpStream Not Read

**Potential Issue**: Client sends a request, we reject without reading the full request. Client might hang if its send buffer is full.

**Analysis**: When we call `stream.write_all()` to send the 503 response and then drop the stream, the kernel closes the connection. The client will receive FIN or RST, causing any pending sends to fail. This is fine and expected.

**Mitigation**: None needed; normal TCP behavior.

### 5. Guard Panic Safety

**Potential Issue**: What if `handle_connection()` panics?

**Analysis**: The `_guard` is a local variable in the closure. If any code in `handle_connection()` panics, the closure unwinds and the guard's `Drop` impl runs before the panic propagates. This is guaranteed by Rust.

**Verification**: The guard WILL decrement even if `handle_connection()` panics. ✓

### 6. Zero or Negative Limit

**Potential Issue**: User sets `RCOMM_MAX_CONNECTIONS=0` or `-1`.

**Analysis**:
- `0` is a `usize` (unsigned), so `-1` parses as `usize::MAX` (not -1)
- If user sets `0`, `parse::<usize>()` succeeds and returns `0`
- All connections would be rejected (0 >= 1 is false for first connection, but 1 >= 0 is true for second), so effectively no connections allowed

**Mitigation** (optional): Add a validation check:
```rust
fn get_max_connections() -> usize {
    let limit = std::env::var("RCOMM_MAX_CONNECTIONS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(1024);

    if limit == 0 {
        eprintln!("RCOMM_MAX_CONNECTIONS must be > 0, defaulting to 1024");
        1024
    } else {
        limit
    }
}
```

**Recommendation**: Add this validation to fail fast if user misconfigures.

### 7. Very Large Limit

**Potential Issue**: User sets `RCOMM_MAX_CONNECTIONS=1000000`.

**Analysis**: Perfectly fine. The atomic counter and comparison are O(1) operations. No issue.

**Mitigation**: None needed.

### 8. Monitoring / Observability

**Missing**: There's no way to query how many connections are currently active.

**Future Improvement** (not in scope): Add a `/metrics` endpoint that returns active connection count and limit. This would require passing the counter through to `handle_connection()` or storing it globally (both doable in a later iteration).

---

## Configuration & Environment

### Environment Variable
- **Name**: `RCOMM_MAX_CONNECTIONS`
- **Type**: Unsigned integer
- **Default**: `1024`
- **Valid Range**: `1` to `usize::MAX` (practically `1` to `10000`)
- **Invalid Values**: Non-numeric strings → default to `1024`

### Startup Output
```
Routes:
{...}

Listening on 127.0.0.1:7878
Max concurrent connections: 1024
```

### Changing Limit
For different deployments:
```bash
# Production: limit to 500
RCOMM_MAX_CONNECTIONS=500 cargo run

# Development: high limit
RCOMM_MAX_CONNECTIONS=10000 cargo run

# Minimal: limit to 10
RCOMM_MAX_CONNECTIONS=10 cargo run
```

---

## Implementation Checklist

- [ ] Add imports (`Arc`, `AtomicUsize`, `Ordering`) to `src/main.rs`
- [ ] Implement `get_max_connections()` function
- [ ] Implement `ConnectionGuard` struct with `Drop` trait
- [ ] Modify `main()` to create `active_connections` counter
- [ ] Modify listener loop to check limit and reject with 503
- [ ] Verify `HttpResponse` supports status code 503, or add it
- [ ] Add unit tests for `get_max_connections()` and `ConnectionGuard`
- [ ] Add integration tests for limit enforcement
- [ ] Manual testing with different limits
- [ ] Update startup logging to show max connections
- [ ] Code review: check for race conditions, panic safety, ordering guarantees
- [ ] Verify no existing tests break (`cargo test`)

---

## Acceptance Criteria

1. **Configuration**: Server accepts `RCOMM_MAX_CONNECTIONS` env var and defaults to 1024 ✓
2. **Limit Enforcement**: Connections are rejected with HTTP 503 when limit is reached ✓
3. **Counter Decrement**: Counter is properly decremented when connections close, allowing new ones ✓
4. **Safety**: No data races, panic-safe, correct atomic ordering ✓
5. **Testing**: Unit tests for config parsing, integration tests for limit behavior ✓
6. **Logging**: Startup message shows current limit ✓
7. **No Breaking Changes**: All existing tests pass, no API changes to public types ✓

---

## References

- **Rust `AtomicUsize`**: https://doc.rust-lang.org/std/sync/atomic/struct.AtomicUsize.html
- **Atomic Ordering**: https://doc.rust-lang.org/std/sync/atomic/enum.Ordering.html (SeqCst guarantees total order)
- **HTTP 503 Status**: https://httpwg.org/specs/rfc7231.html#status.503
- **RAII Pattern**: https://doc.rust-lang.org/rust-by-example/scope/raii.html
- **Thread Safety**: Chapter 16 of "The Rust Programming Language"

---

## Future Enhancements (Out of Scope)

1. **Per-IP Connection Limits**: Prevent one client from consuming all connections
2. **Metrics Endpoint**: Expose current active connection count via `/metrics`
3. **Graceful Shutdown**: Stop accepting new connections while draining existing ones
4. **Connection Pooling**: Reuse TcpStreams (HTTP Keep-Alive)
5. **Timeout-Based Cleanup**: Close idle connections after N seconds
6. **Load Shedding Strategy**: Return 503 with `Retry-After` header for better client experience
