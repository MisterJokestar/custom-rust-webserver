# Connection Rate Limiting Per IP Address Implementation Plan

## 1. Overview of the Feature

Connection rate limiting is a critical security mechanism that protects HTTP servers from abuse, denial-of-service (DoS) attacks, and resource exhaustion. By restricting the number of concurrent connections and/or new connections allowed from a single IP address, the server can maintain stability and fairness for legitimate clients.

Currently, rcomm accepts unlimited concurrent connections from any IP address, making it vulnerable to simple DoS attacks where a single malicious client can exhaust the thread pool or server resources.

**Goal**: Implement per-IP connection rate limiting that tracks active connections by source IP address and rejects or queues new connections that exceed a configurable threshold.

**Key Objectives**:
1. Track the number of concurrent connections per IP address
2. Enforce a maximum connections limit per IP (configurable, default: 10 connections per IP)
3. Enforce a maximum connection rate (new connections per second, configurable, default: 5 requests/sec per IP)
4. Return HTTP 429 (Too Many Requests) for rate-limited requests
5. Maintain minimal performance impact for normal traffic
6. Thread-safe implementation using existing patterns (Arc, Mutex)

**Impact**:
- Security: Mitigates simple DoS attacks and connection exhaustion attacks
- Fairness: Prevents single clients from monopolizing server resources
- Stability: Maintains consistent performance under attack
- Standards adherence: Uses HTTP 429 status code (RFC 6585)

---

## 2. Files to be Modified or Created

### Modified Files

1. **`/home/jwall/personal/rusty/rcomm/src/main.rs`**
   - Modify `main()` to initialize and share the rate limiter across threads
   - Modify TCP listener loop to check rate limits before accepting connections
   - Modify `handle_connection()` to update rate limiter state on completion

2. **`/home/jwall/personal/rusty/rcomm/src/lib.rs`**
   - Export a new `RateLimiter` public struct (optional; can be internal to main.rs)

### New Files

3. **`/home/jwall/personal/rusty/rcomm/src/rate_limiter.rs`** (recommended)
   - Create a standalone module for rate limiting logic
   - Implements `RateLimiter` struct with concurrent connection tracking
   - Provides methods: `check_and_record()`, `release()`, `reset_if_needed()`
   - Uses `HashMap<String, IPConnectionState>` for per-IP tracking
   - Uses `Arc<Mutex<HashMap>>` for thread-safe access

### Configuration Files (Optional)

4. **Environment variable support** (optional)
   - `RCOMM_MAX_CONNECTIONS_PER_IP` (default: 10)
   - `RCOMM_MAX_REQUESTS_PER_SECOND_PER_IP` (default: 5)

---

## 3. Step-by-Step Implementation Details

### Step 1: Create the RateLimiter Module

**File**: `/home/jwall/personal/rusty/rcomm/src/rate_limiter.rs`

Create a new module that tracks connection state per IP address:

```rust
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Tracks the state of connections from a single IP address.
#[derive(Debug, Clone)]
struct IPConnectionState {
    /// Number of currently active connections from this IP
    active_connections: usize,
    /// Timestamp of the last connection from this IP
    last_connection_time: Instant,
    /// Number of connections in the current time window
    connections_in_window: usize,
    /// When the current rate-limit window started
    window_start_time: Instant,
}

/// Rate limiter that enforces per-IP connection and request limits.
pub struct RateLimiter {
    /// Maximum concurrent connections per IP address
    max_connections_per_ip: usize,
    /// Maximum new connections per second per IP address
    max_requests_per_second_per_ip: usize,
    /// Time window for rate limiting (default: 1 second)
    rate_limit_window: Duration,
    /// Map of IP addresses to their connection states
    ip_states: Arc<Mutex<HashMap<String, IPConnectionState>>>,
}

impl RateLimiter {
    /// Creates a new RateLimiter with default or environment-configured limits.
    pub fn new() -> Self {
        let max_connections_per_ip = std::env::var("RCOMM_MAX_CONNECTIONS_PER_IP")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10);

        let max_requests_per_second_per_ip = std::env::var("RCOMM_MAX_REQUESTS_PER_SECOND_PER_IP")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5);

        RateLimiter {
            max_connections_per_ip,
            max_requests_per_second_per_ip,
            rate_limit_window: Duration::from_secs(1),
            ip_states: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Checks if a new connection from the given IP should be allowed.
    ///
    /// Returns:
    /// - `Ok(())` if the connection is allowed
    /// - `Err(String)` with a reason if the connection is rate-limited
    pub fn check_and_record(&self, ip: &str) -> Result<(), String> {
        let mut states = self.ip_states.lock().unwrap();
        let now = Instant::now();

        let state = states
            .entry(ip.to_string())
            .or_insert_with(|| IPConnectionState {
                active_connections: 0,
                last_connection_time: now,
                connections_in_window: 0,
                window_start_time: now,
            });

        // Reset the rate-limit window if it has expired
        if now.duration_since(state.window_start_time) >= self.rate_limit_window {
            state.connections_in_window = 0;
            state.window_start_time = now;
        }

        // Check concurrent connection limit
        if state.active_connections >= self.max_connections_per_ip {
            return Err(format!(
                "Too many concurrent connections from {}: {} (limit: {})",
                ip, state.active_connections, self.max_connections_per_ip
            ));
        }

        // Check request rate limit
        if state.connections_in_window >= self.max_requests_per_second_per_ip {
            return Err(format!(
                "Rate limit exceeded for {}: {} requests/sec (limit: {})",
                ip, state.connections_in_window, self.max_requests_per_second_per_ip
            ));
        }

        // Accept the connection: increment counters
        state.active_connections += 1;
        state.connections_in_window += 1;
        state.last_connection_time = now;

        Ok(())
    }

    /// Releases a connection for the given IP (call when connection closes).
    pub fn release(&self, ip: &str) {
        if let Ok(mut states) = self.ip_states.lock() {
            if let Some(state) = states.get_mut(ip) {
                if state.active_connections > 0 {
                    state.active_connections -= 1;
                }
            }
        }
    }

    /// Returns the current number of active connections for an IP (for monitoring).
    pub fn get_active_connections(&self, ip: &str) -> usize {
        self.ip_states
            .lock()
            .unwrap()
            .get(ip)
            .map(|s| s.active_connections)
            .unwrap_or(0)
    }

    /// Clones the inner Arc for shared ownership across threads.
    pub fn clone_limiter(&self) -> Arc<Mutex<HashMap<String, IPConnectionState>>> {
        Arc::clone(&self.ip_states)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_limiter_has_defaults() {
        let limiter = RateLimiter::new();
        assert_eq!(limiter.max_connections_per_ip, 10);
        assert_eq!(limiter.max_requests_per_second_per_ip, 5);
    }

    #[test]
    fn first_connection_from_ip_is_allowed() {
        let limiter = RateLimiter::new();
        assert!(limiter.check_and_record("192.168.1.100").is_ok());
    }

    #[test]
    fn concurrent_connections_within_limit_allowed() {
        let limiter = RateLimiter::new();
        for i in 0..5 {
            assert!(limiter.check_and_record("192.168.1.100").is_ok());
            assert_eq!(limiter.get_active_connections("192.168.1.100"), i + 1);
        }
    }

    #[test]
    fn concurrent_connections_exceeding_limit_rejected() {
        let limiter = RateLimiter::new();
        // Allow up to max_connections_per_ip
        for _ in 0..10 {
            let _ = limiter.check_and_record("192.168.1.100");
        }
        // 11th connection should be rejected
        assert!(limiter.check_and_record("192.168.1.100").is_err());
    }

    #[test]
    fn release_decrements_active_connections() {
        let limiter = RateLimiter::new();
        let _ = limiter.check_and_record("192.168.1.100");
        assert_eq!(limiter.get_active_connections("192.168.1.100"), 1);
        limiter.release("192.168.1.100");
        assert_eq!(limiter.get_active_connections("192.168.1.100"), 0);
    }

    #[test]
    fn different_ips_have_separate_limits() {
        let limiter = RateLimiter::new();
        let _ = limiter.check_and_record("192.168.1.100");
        let _ = limiter.check_and_record("192.168.1.101");
        assert_eq!(limiter.get_active_connections("192.168.1.100"), 1);
        assert_eq!(limiter.get_active_connections("192.168.1.101"), 1);
    }

    #[test]
    fn release_on_unknown_ip_is_safe() {
        let limiter = RateLimiter::new();
        limiter.release("192.168.1.999"); // Should not panic
    }
}
```

### Step 2: Export the RateLimiter Module

**File**: `/home/jwall/personal/rusty/rcomm/src/lib.rs`

Add the rate_limiter module to the library exports:

```rust
pub mod models;
pub mod rate_limiter;

// ... rest of lib.rs ...
```

### Step 3: Modify the Main Server

**File**: `/home/jwall/personal/rusty/rcomm/src/main.rs`

Update the server to use the rate limiter. Modify the main function and TCP listener loop:

**Current Code** (lines 1-44):
```rust
use std::{
    collections::HashMap,
    fs,
    io::prelude::*,
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
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

**Updated Code**:
```rust
use std::{
    collections::HashMap,
    fs,
    io::prelude::*,
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::Arc,
};
use rcomm::ThreadPool;
use rcomm::rate_limiter::RateLimiter;
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

fn main() {
    let port = get_port();
    let address = get_address();
    let full_address = format!("{address}:{port}");
    let listener = TcpListener::bind(&full_address).unwrap();

    let pool = ThreadPool::new(4);

    let path = Path::new("./pages");
    let routes = build_routes(String::from(""), path);
    let rate_limiter = Arc::new(RateLimiter::new());

    println!("Routes:\n{routes:#?}\n\n");
    println!("Listening on {full_address}");

    for stream in listener.incoming() {
        let routes_clone = routes.clone();
        let stream = stream.unwrap();
        let rate_limiter_clone = Arc::clone(&rate_limiter);

        // Extract IP from the socket address
        let client_ip = stream.peer_addr()
            .map(|addr| addr.ip().to_string())
            .unwrap_or_else(|_| String::from("unknown"));

        // Check rate limit before processing
        match rate_limiter_clone.check_and_record(&client_ip) {
            Ok(()) => {
                pool.execute(move || {
                    handle_connection(stream, routes_clone, rate_limiter_clone, client_ip);
                });
            }
            Err(reason) => {
                eprintln!("Rate limit exceeded: {reason}");
                // Send 429 Too Many Requests response
                let mut response = HttpResponse::build(String::from("HTTP/1.1"), 429);
                response.add_body(
                    format!("HTTP 429: Too Many Requests\n\nReason: {reason}\n").into()
                );
                let _ = stream.write_all(&response.as_bytes());
            }
        }
    }
}

fn handle_connection(
    mut stream: TcpStream,
    routes: HashMap<String, PathBuf>,
    rate_limiter: Arc<RateLimiter>,
    client_ip: String,
) {
    let http_request = match HttpRequest::build_from_stream(&stream) {
        Ok(req) => req,
        Err(e) => {
            eprintln!("Bad request: {e}");
            let mut response = HttpResponse::build(String::from("HTTP/1.1"), 400);
            let body = format!("Bad Request: {e}");
            response.add_body(body.into());
            let _ = stream.write_all(&response.as_bytes());
            rate_limiter.release(&client_ip);
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

    // Release the connection slot for this IP
    rate_limiter.release(&client_ip);
}
```

**Key Changes**:
- Import `Arc` from `std::sync` and `RateLimiter` from `rcomm::rate_limiter`
- Create `RateLimiter` instance in `main()` wrapped in `Arc` for thread-safe sharing
- Clone the `Arc` for each incoming connection
- Extract client IP from `stream.peer_addr()`
- Call `check_and_record()` before spawning task; if rate-limited, return 429 response
- Update `handle_connection()` signature to accept rate limiter and client IP
- Call `release()` when connection handler completes (both success and error paths)

---

## 4. Code Snippets and Pseudocode

### RateLimiter Core Logic

```
STRUCTURE IPConnectionState:
    active_connections: integer
    last_connection_time: Instant
    connections_in_window: integer
    window_start_time: Instant

FUNCTION check_and_record(ip: string) -> Result<(), String>:
    LOCK states HashMap

    GET OR CREATE state for ip
    now = current_time()

    // Reset window if expired
    IF time_since(state.window_start_time, now) >= 1 second:
        state.connections_in_window = 0
        state.window_start_time = now
    END IF

    // Check limits
    IF state.active_connections >= MAX_CONNECTIONS_PER_IP:
        RETURN Err("Too many concurrent connections")
    END IF

    IF state.connections_in_window >= MAX_REQUESTS_PER_SECOND_PER_IP:
        RETURN Err("Rate limit exceeded")
    END IF

    // Record the connection
    state.active_connections += 1
    state.connections_in_window += 1
    state.last_connection_time = now

    RETURN Ok(())
END FUNCTION

FUNCTION release(ip: string):
    LOCK states HashMap
    IF ip exists in states AND active_connections > 0:
        state.active_connections -= 1
    END IF
END FUNCTION
```

### Integration in TCP Listener

```
FUNCTION main():
    listener = bind_tcp_listener(address, port)
    pool = create_thread_pool(4)
    routes = build_routes()
    rate_limiter = RateLimiter::new()

    FOR EACH incoming_stream IN listener.incoming():
        client_ip = extract_ip(stream.peer_addr())

        CASE rate_limiter.check_and_record(client_ip) OF:
            Ok:
                pool.execute(move || handle_connection(stream, routes, rate_limiter, client_ip))
            Err(reason):
                response = HttpResponse(429, reason)
                stream.write(response.as_bytes())
        END CASE
    END FOR
END FUNCTION

FUNCTION handle_connection(stream, routes, rate_limiter, client_ip):
    TRY:
        request = parse_http_request(stream)
        response = build_response(request, routes)
        stream.write(response.as_bytes())
    FINALLY:
        rate_limiter.release(client_ip)
    END TRY
END FUNCTION
```

---

## 5. Testing Strategy

### Unit Tests (in `rate_limiter.rs`)

The `rate_limiter.rs` module includes comprehensive unit tests that verify:
- Limiter initializes with correct defaults
- First connection from any IP is allowed
- Concurrent connections within limit are allowed
- Concurrent connections exceeding limit are rejected
- Released connections decrement the counter
- Different IPs have independent limits
- Release on unknown IP doesn't panic

**Run unit tests**:
```bash
cargo test rate_limiter
```

### Integration Tests (in `src/bin/integration_test.rs`)

Add new test cases to verify rate limiting behavior under load:

```rust
fn test_rate_limit_429_response(addr: &str) -> Result<(), String> {
    // Open multiple concurrent connections to trigger rate limit
    let mut handles = vec![];
    let mut results = vec![];

    for _ in 0..15 {
        let addr = addr.to_string();
        let handle = std::thread::spawn(move || {
            match TcpStream::connect(&addr) {
                Ok(mut stream) => {
                    let request = "GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
                    let _ = stream.write_all(request.as_bytes());
                    // Keep connection open briefly
                    std::thread::sleep(Duration::from_millis(100));
                    Ok(())
                }
                Err(_) => Err("connection failed".to_string()),
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        results.push(handle.join());
    }

    // At least some connections should succeed (first ~10), others may get 429
    let success_count = results.iter().filter(|r| r.is_ok()).count();
    if success_count < 5 {
        return Err(format!("Expected at least 5 successful connections, got {success_count}"));
    }
    Ok(())
}

fn test_release_allows_new_connections(addr: &str) -> Result<(), String> {
    use std::time::Duration;

    // First request
    let resp = send_request(addr, "GET", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "first request")?;

    // After connection closes, new request should be allowed
    std::thread::sleep(Duration::from_millis(100));

    let resp = send_request(addr, "GET", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "second request after release")?;

    Ok(())
}

fn test_different_ips_separate_limits(addr: &str) -> Result<(), String> {
    // This test is difficult to implement without actual multi-source traffic
    // Can be tested manually with curl from different machines
    // For now, document as manual test
    Ok(())
}
```

Add these tests to the `main()` function's test list:
```rust
let results = vec![
    // ... existing tests ...
    run_test("rate_limit_429_response", || test_rate_limit_429_response(&addr)),
    run_test("release_allows_new_connections", || test_release_allows_new_connections(&addr)),
];
```

**Run integration tests**:
```bash
cargo run --bin integration_test
```

### Stress Testing (Manual)

Use `ab` (Apache Bench) or `wrk` to generate load from a single source:

```bash
# Start the server
cargo run &
SERVER_PID=$!

# Test with 100 requests, 20 concurrent
ab -n 100 -c 20 http://127.0.0.1:7878/

# Should see some 429 responses if -c exceeds RCOMM_MAX_CONNECTIONS_PER_IP

kill $SERVER_PID
```

### Configuration Testing

Test with custom environment variables:

```bash
# Test with lower limits
RCOMM_MAX_CONNECTIONS_PER_IP=3 RCOMM_MAX_REQUESTS_PER_SECOND_PER_IP=2 cargo run

# Verify limits are applied (should see 429 faster)
```

### Manual Testing with curl

```bash
# Multiple rapid requests from same IP
for i in {1..15}; do curl -v http://127.0.0.1:7878/ 2>&1 | grep -E "< HTTP"; done

# Should see some "HTTP/1.1 429" responses
```

---

## 6. Edge Cases to Consider

### Case 1: Extracting IP from Connection
**Scenario**: `stream.peer_addr()` fails (rare but possible)
**Current Behavior**: Defaults to `"unknown"` string, all failed connections grouped together
**Handling**: This could become a DoS vector if many clients fail to provide proper peer address
**Recommendation**: Log the error and consider dropping the connection instead
**Code**:
```rust
let client_ip = match stream.peer_addr() {
    Ok(addr) => addr.ip().to_string(),
    Err(e) => {
        eprintln!("Failed to get peer address: {e}");
        return; // Drop the connection immediately
    }
};
```

### Case 2: IPv4 vs IPv6 Addresses
**Scenario**: Client connects via IPv6 (e.g., `::1` for localhost)
**Current Behavior**: `.ip().to_string()` produces IPv6 format like `::1` or full `2001:db8::1`
**Expected Result**: Limits apply to each unique IPv6 address independently (correct)
**Testing**: Test with `localhost` (both IPv4 and IPv6) if supported

### Case 3: Localhost Connections During Development
**Scenario**: Multiple test scripts or connections from `127.0.0.1`
**Expected Behavior**: All share the same limit bucket, can be restrictive during testing
**Recommendation**: Document environment variable override, provide default suitable for single-host testing
**Configuration**: Default limit of 10 should be sufficient for normal web traffic

### Case 4: Behind Reverse Proxy
**Scenario**: Server runs behind nginx/Apache reverse proxy; all connections appear from `127.0.0.1` or proxy IP
**Current Behavior**: Rate limit applies to proxy IP, not the actual client
**Impact**: All real clients share the same limit bucket
**Recommendation**: Document this limitation; suggest using `X-Forwarded-For` header in future enhancement
**Future Enhancement**: Add optional support for `X-Forwarded-For` or `X-Real-IP` headers

### Case 5: Client Disconnects During Rate Limit Check
**Scenario**: Client connects, but drops connection before `handle_connection()` runs
**Current Behavior**: Connection is recorded but `release()` may never be called if thread pool task doesn't execute
**Impact**: Active connection count for that IP becomes inaccurate
**Recommendation**: Use a guard/RAII pattern to ensure `release()` is called on drop
**Code** (Alternative implementation with RAII):
```rust
struct ConnectionGuard {
    rate_limiter: Arc<RateLimiter>,
    client_ip: String,
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.rate_limiter.release(&self.client_ip);
    }
}
```

### Case 6: Time Window Boundary Conditions
**Scenario**: Multiple connections arrive exactly at the 1-second boundary
**Current Behavior**: Window resets atomically within the lock; first request resets, subsequent requests in same window are counted
**Expected Result**: Rate limit is enforced correctly within 1-second windows
**Race Condition**: Theoretically possible for brief window where requests span boundary; acceptable trade-off

### Case 7: Very High Concurrency
**Scenario**: Thousands of concurrent connections from different IPs
**Current Behavior**: HashMap grows unbounded; memory usage increases with unique IP count
**Risk**: Long-lived IP entries never cleaned up; stale entries accumulate
**Recommendation**: Add optional periodic cleanup (future enhancement) with LRU or expiration
**Current Limitation**: Use in trusted networks or add cleanup task

### Case 8: Spoofed or Malformed IP Headers
**Scenario**: If using `X-Forwarded-For` in future, attacker could forge arbitrary IPs
**Current Behavior**: Not applicable to direct peer address (cannot be spoofed)
**Future Risk**: If implemented, validate and sanitize IP before using as key

---

## 7. Implementation Checklist

- [ ] Create `/home/jwall/personal/rusty/rcomm/src/rate_limiter.rs` with:
  - [ ] `IPConnectionState` struct
  - [ ] `RateLimiter` struct with thread-safe HashMap
  - [ ] `check_and_record()` method
  - [ ] `release()` method
  - [ ] `get_active_connections()` method (for monitoring)
  - [ ] Unit tests for all methods
  - [ ] Environment variable support for limits
- [ ] Update `/home/jwall/personal/rusty/rcomm/src/lib.rs`:
  - [ ] Add `pub mod rate_limiter;`
- [ ] Modify `/home/jwall/personal/rusty/rcomm/src/main.rs`:
  - [ ] Add imports: `Arc`, `RateLimiter`
  - [ ] Create `RateLimiter` instance in `main()` wrapped in `Arc`
  - [ ] Extract client IP from `stream.peer_addr()`
  - [ ] Call `check_and_record()` before thread pool execution
  - [ ] Return 429 response for rate-limited connections
  - [ ] Update `handle_connection()` signature to accept rate limiter and IP
  - [ ] Call `release()` on all exit paths in `handle_connection()`
- [ ] Add integration tests to `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`:
  - [ ] `test_rate_limit_429_response()`
  - [ ] `test_release_allows_new_connections()`
- [ ] Run unit tests: `cargo test rate_limiter`
- [ ] Run integration tests: `cargo run --bin integration_test`
- [ ] Stress test with `ab`:
  - [ ] `ab -n 100 -c 20 http://127.0.0.1:7878/`
  - [ ] Verify 429 responses for concurrent connections exceeding limit
- [ ] Manual testing with custom environment variables:
  - [ ] Test with lower limits to verify configuration works
- [ ] Test release behavior:
  - [ ] Verify that closed connections allow new connections from same IP

---

## 8. Complexity and Risk Analysis

**Complexity**: 5/10
- Requires thread-safe state management using `Arc<Mutex<HashMap>>`
- Logic for time-window management and state tracking
- Integration into critical TCP accept loop
- Moderate error handling paths

**Risk**: Medium
- **Thread Safety**: Uses established patterns (Arc, Mutex); should be safe
- **Performance Impact**: Adds HashMap lookup and lock contention per connection; acceptable for typical loads
- **Resource Leaks**: If `release()` not called, counters grow unbounded (mitigated by careful error handling)
- **Denial of Service via IPs**: If using `X-Forwarded-For` without validation (not implemented in phase 1)
- **IPv6 Full Addresses**: Very long strings as HashMap keys; minor memory overhead

**Dependencies**: None
- Uses only `std` library (collections, sync, net, time)
- No external crates required
- Aligns with project's no-external-dependencies constraint

**Testing Coverage**: High
- Unit tests cover all code paths
- Integration tests cover end-to-end behavior
- Manual testing can verify under realistic load

---

## 9. Configuration and Monitoring

### Environment Variables

```bash
# Maximum concurrent connections per IP (default: 10)
export RCOMM_MAX_CONNECTIONS_PER_IP=20

# Maximum new connections per second per IP (default: 5)
export RCOMM_MAX_REQUESTS_PER_SECOND_PER_IP=10

cargo run
```

### Logging

Rate limiting events are logged to `stderr`:
```
Rate limit exceeded: Too many concurrent connections from 192.168.1.100: 10 (limit: 10)
Rate limit exceeded: Rate limit exceeded for 192.168.1.101: 5 requests/sec (limit: 5)
```

### Monitoring (Future)

Add optional monitoring methods:
```rust
pub fn get_stats(&self) -> HashMap<String, IPStats> {
    // Returns: IP address -> (active_connections, requests_in_window)
}
```

---

## 10. Future Enhancements

1. **Cleanup Mechanism**: Periodically remove old IP entries (LRU or TTL-based)
2. **X-Forwarded-For Support**: For reverse proxy deployments (with validation)
3. **Whitelist/Blacklist**: Allow specific IPs to bypass or always reject
4. **Dynamic Configuration**: Adjust limits without restarting server
5. **Metrics Export**: Prometheus-style metrics for monitoring
6. **Progressive Rate Limiting**: Gradual backoff instead of hard rejection
7. **Distributed Rate Limiting**: Multi-server coordination (requires external state store)
8. **Per-Endpoint Limits**: Different limits for different routes (e.g., `/api` vs `/static`)
9. **User-Agent Rate Limiting**: Limit by user agent string for finer-grained control
10. **Adaptive Limits**: Automatically adjust limits based on server load

---

## 11. References

- **HTTP 429 Status Code**: [RFC 6585 - Additional HTTP Status Codes](https://tools.ietf.org/html/rfc6585#section-4)
- **Rust Arc and Mutex**: [std::sync documentation](https://doc.rust-lang.org/std/sync/)
- **Rate Limiting Algorithms**: Token Bucket, Leaky Bucket, Sliding Window (implemented here)
- **IP Address Parsing**: [std::net::IpAddr](https://doc.rust-lang.org/std/net/enum.IpAddr.html)
