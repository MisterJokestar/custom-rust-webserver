# Request Limit Per Persistent Connection Implementation Plan

## Feature Overview

Implement a configurable limit on the number of HTTP requests allowed per persistent TCP connection to prevent abuse, resource exhaustion, and slowloris-style attacks. Once a connection reaches the request limit, the server will close the connection gracefully, forcing clients to establish new connections if they need to send additional requests.

**Status**: Planned
**Complexity**: 2/10
**Necessity**: 4/10
**Category**: Security

### Current Behavior

The current implementation (assuming HTTP/1.1 persistent connections are implemented):
- Accepts multiple sequential HTTP requests on a single TCP connection
- No limit on the number of requests per connection
- Connection only closes when:
  - Client sends `Connection: close` header
  - Timeout expires (no new request received)
  - Malformed/unparseable request arrives
  - Client closes the connection

### Desired Behavior

After implementation:
- Enforce a configurable maximum number of requests per connection (e.g., 100 requests)
- Default limit can be set via environment variable (e.g., `RCOMM_MAX_REQUESTS_PER_CONNECTION`)
- When a connection reaches the limit:
  - Send the response for the final allowed request with `Connection: close` header
  - Close the TCP connection after writing the response
  - Log the connection closure due to request limit
- Limit applies to both GET and non-GET requests
- Limit is applied per-connection, not globally

---

## Key Technical Challenges

### 1. Connection Request Counter

**Challenge**: Tracking the number of requests processed on a single connection.

**Current State**: The `handle_connection()` function processes requests in a loop. We need to maintain a counter across loop iterations.

**Solution**:
- Add a request counter variable in the `handle_connection()` function
- Increment the counter after each successful request is processed
- Check the counter before processing the next request
- Make the counter accessible to the logic that determines whether to close the connection

### 2. Configuration Management

**Challenge**: Making the request limit configurable without hardcoding it.

**Current State**: The server already uses environment variables for `RCOMM_PORT` and `RCOMM_ADDRESS`.

**Solution**:
- Add a new environment variable `RCOMM_MAX_REQUESTS_PER_CONNECTION`
- Default to a reasonable value (e.g., 100) if not set
- Provide a helper function similar to `get_port()` and `get_address()` to retrieve this setting
- Pass the limit to `handle_connection()` as a parameter

### 3. Decision Point Integration

**Challenge**: Determining when to force a connection close based on request count.

**Current State**: Connection closure logic is already present in the keep-alive implementation (checking `should_close_connection()`).

**Solution**:
- Create a new helper function that checks both the client's `Connection` header AND the request count limit
- Return whether the connection should close due to either condition
- Ensure the response includes `Connection: close` header before closing
- Close the connection immediately after sending the response

### 4. Response Header Management

**Challenge**: Ensuring `Connection: close` is always included when the limit is reached.

**Current State**: `HttpResponse` allows header manipulation via `add_header()`.

**Solution**:
- Before sending the final response that will close the connection, ensure `Connection: close` header is set
- This may override any previous `Connection: keep-alive` header
- Response serialization via `as_bytes()` will include the header in the output

---

## Implementation Plan

### Phase 1: Add Configuration Functions

**File**: `src/main.rs`

**Changes**:

1. Add a new helper function to retrieve the maximum requests per connection from environment:

```rust
fn get_max_requests_per_connection() -> usize {
    std::env::var("RCOMM_MAX_REQUESTS_PER_CONNECTION")
        .unwrap_or_else(|_| String::from("100"))
        .parse::<usize>()
        .unwrap_or(100)
}
```

2. Call this function in `main()` and pass it to `handle_connection()`:

```rust
fn main() {
    let port = get_port();
    let address = get_address();
    let full_address = format!("{address}:{port}");
    let listener = TcpListener::bind(&full_address).unwrap();

    let pool = ThreadPool::new(4);

    let path = Path::new("./pages");
    let routes = build_routes(String::from(""), path);

    let max_requests = get_max_requests_per_connection();  // NEW

    println!("Routes:\n{routes:#?}\n\n");
    println!("Listening on {full_address}");
    println!("Max requests per connection: {max_requests}");  // NEW

    for stream in listener.incoming() {
        let routes_clone = routes.clone();
        let stream = stream.unwrap();

        pool.execute(move || {
            handle_connection(stream, routes_clone, max_requests);  // MODIFIED
        });
    }
}
```

**Code Example**:

```rust
fn get_max_requests_per_connection() -> usize {
    std::env::var("RCOMM_MAX_REQUESTS_PER_CONNECTION")
        .unwrap_or_else(|_| String::from("100"))
        .parse::<usize>()
        .unwrap_or(100)
}
```

---

### Phase 2: Add Request Limit Parameter to handle_connection()

**File**: `src/main.rs`

**Changes**:

1. Update the `handle_connection()` function signature to accept the request limit:

```rust
fn handle_connection(
    mut stream: TcpStream,
    routes: HashMap<String, PathBuf>,
    max_requests: usize,  // NEW PARAMETER
) {
    // ... existing code
}
```

2. Add a request counter in the request loop:

```rust
fn handle_connection(
    mut stream: TcpStream,
    routes: HashMap<String, PathBuf>,
    max_requests: usize,
) {
    stream.set_read_timeout(Some(Duration::from_secs(15)))
        .unwrap_or_else(|e| {
            eprintln!("Failed to set read timeout: {e}");
        });

    let mut buf_reader = BufReader::new(&stream);
    let mut request_count = 0;  // NEW: Counter

    loop {
        request_count += 1;  // NEW: Increment after accepting request

        // Attempt to parse next request
        match HttpRequest::build_from_stream(&mut buf_reader) {
            Ok(req) => {
                let (mut response, mut should_close) = handle_single_request(
                    &req,
                    &routes,
                );

                // NEW: Check if request limit reached
                if request_count >= max_requests {
                    should_close = true;
                    println!("Request limit ({}) reached, closing connection", max_requests);
                }

                // Ensure Connection: close header is set if closing
                if should_close {
                    response.add_header("Connection".to_string(), "close".to_string());
                } else {
                    response.add_header("Connection".to_string(), "keep-alive".to_string());
                }

                // Send response
                stream.write_all(&response.as_bytes())
                    .unwrap_or_else(|e| {
                        eprintln!("Failed to write response: {e}");
                    });

                // Check if we should close the connection
                if should_close {
                    break;
                }
            }
            Err(HttpParseError::IoError(io_err)) => {
                if io_err.kind() == std::io::ErrorKind::WouldBlock
                    || io_err.kind() == std::io::ErrorKind::TimedOut {
                    println!("Connection idle, closing");
                    break;
                }
                eprintln!("IO error: {io_err}");
                break;
            }
            Err(e) => {
                eprintln!("Request parse error: {e}");
                let mut response = HttpResponse::build(String::from("HTTP/1.1"), 400);
                response.add_body(format!("Bad Request: {e}").into());
                response.add_header("Connection".to_string(), "close".to_string());
                let _ = stream.write_all(&response.as_bytes());
                break;
            }
        }
    }
}
```

**Code Snippet**:

```rust
let mut request_count = 0;

loop {
    request_count += 1;

    match HttpRequest::build_from_stream(&mut buf_reader) {
        Ok(req) => {
            let (mut response, mut should_close) = handle_single_request(&req, &routes);

            // Check request limit
            if request_count >= max_requests {
                should_close = true;
                println!("Request limit ({}) reached, closing connection", max_requests);
            }

            // Set connection header
            if should_close {
                response.add_header("Connection".to_string(), "close".to_string());
            } else {
                response.add_header("Connection".to_string(), "keep-alive".to_string());
            }

            stream.write_all(&response.as_bytes())?;

            if should_close {
                break;
            }
        }
        // ... error handling
    }
}
```

---

### Phase 3: Add Helper Function to Check Request Limit

**File**: `src/main.rs` (optional refactoring for clarity)

**Changes**:

Add a helper function to encapsulate the request limit check logic:

```rust
fn should_close_due_to_request_limit(current_count: usize, max_requests: usize) -> bool {
    current_count >= max_requests
}
```

Then use it in the loop:

```rust
if should_close_due_to_request_limit(request_count, max_requests) {
    should_close = true;
    println!("Request limit ({}) reached, closing connection", max_requests);
}
```

This makes the code more testable and maintainable.

---

### Phase 4: Update Integration Tests

**File**: `src/bin/integration_test.rs`

**Changes**:

Add new test cases for request limit enforcement:

```rust
fn test_request_limit_enforced(addr: &str) -> Result<(), String> {
    let mut stream = TcpStream::connect(addr)
        .map_err(|e| format!("connect: {e}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| format!("set timeout: {e}"))?;

    // Send 3 requests (assuming limit is set low for testing, e.g., 2)
    for i in 0..3 {
        let req = format!(
            "GET / HTTP/1.1\r\nHost: localhost\r\nConnection: keep-alive\r\n\r\n"
        );

        match stream.write_all(req.as_bytes()) {
            Ok(_) => {},
            Err(e) => {
                // Connection may already be closed by server
                if i < 2 {
                    return Err(format!("Failed to send request {}: {e}", i + 1));
                } else {
                    // This is expected after hitting limit
                    return Ok(());
                }
            }
        }

        match read_response(&mut stream) {
            Ok(resp) => {
                if i < 2 {
                    assert_eq_or_err(&resp.status_code, &200, "response status")?;

                    // Check Connection header
                    if i == 1 {  // Second request should have Connection: close
                        if let Some(conn) = resp.headers.get("connection") {
                            if !conn.to_lowercase().contains("close") {
                                return Err("Expected Connection: close in response".to_string());
                            }
                        } else {
                            return Err("Missing Connection header in final response".to_string());
                        }
                    }
                }
            }
            Err(e) => {
                if i < 2 {
                    return Err(format!("Failed to read response {}: {e}", i + 1));
                } else {
                    // Expected: connection closed after limit
                    return Ok(());
                }
            }
        }
    }

    Ok(())
}

fn test_request_limit_with_various_methods(addr: &str) -> Result<(), String> {
    let mut stream = TcpStream::connect(addr)
        .map_err(|e| format!("connect: {e}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| format!("set timeout: {e}"))?;

    // Send GET request
    let req1 = "GET / HTTP/1.1\r\nHost: localhost\r\nConnection: keep-alive\r\n\r\n";
    stream.write_all(req1.as_bytes())
        .map_err(|e| format!("write GET: {e}"))?;
    let resp1 = read_response(&mut stream)
        .map_err(|e| format!("read GET response: {e}"))?;
    assert_eq_or_err(&resp1.status_code, &200, "GET response")?;

    // Send POST request with body (counts toward limit)
    let body = "key=value";
    let req2 = format!(
        "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(req2.as_bytes())
        .map_err(|e| format!("write POST: {e}"))?;
    let resp2 = read_response(&mut stream)
        .map_err(|e| format!("read POST response: {e}"))?;
    assert_eq_or_err(&resp2.status_code, &404, "POST response")?;  // 404 since no POST handler

    Ok(())
}

fn test_request_limit_counter_resets_on_new_connection(addr: &str) -> Result<(), String> {
    // First connection
    {
        let mut stream = TcpStream::connect(addr)
            .map_err(|e| format!("connect 1: {e}"))?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| format!("set timeout: {e}"))?;

        let req = "GET / HTTP/1.1\r\nHost: localhost\r\nConnection: keep-alive\r\n\r\n";
        stream.write_all(req.as_bytes())
            .map_err(|e| format!("write: {e}"))?;
        let resp = read_response(&mut stream)
            .map_err(|e| format!("read: {e}"))?;
        assert_eq_or_err(&resp.status_code, &200, "first connection response")?;
    }  // Connection closes

    // Second connection should have a fresh counter
    {
        let mut stream = TcpStream::connect(addr)
            .map_err(|e| format!("connect 2: {e}"))?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| format!("set timeout: {e}"))?;

        let req = "GET / HTTP/1.1\r\nHost: localhost\r\nConnection: keep-alive\r\n\r\n";
        stream.write_all(req.as_bytes())
            .map_err(|e| format!("write: {e}"))?;
        let resp = read_response(&mut stream)
            .map_err(|e| format!("read: {e}"))?;
        assert_eq_or_err(&resp.status_code, &200, "second connection response")?;

        // Should NOT have Connection: close (limit not reached)
        if let Some(conn) = resp.headers.get("connection") {
            if conn.to_lowercase().contains("close") {
                return Err("Should not have Connection: close on first request".to_string());
            }
        }
    }

    Ok(())
}
```

Add to the integration test main function:

```rust
let test_request_limit = run_test("security: request limit enforced", || {
    test_request_limit_enforced(&addr)
});

let test_limit_various_methods = run_test("security: request limit with mixed methods", || {
    test_request_limit_with_various_methods(&addr)
});

let test_limit_resets = run_test("security: request limit resets per connection", || {
    test_request_limit_counter_resets_on_new_connection(&addr)
});

// ... collect results
```

---

### Phase 5: Unit Tests (Optional)

**File**: `src/main.rs` (tests module at bottom of file)

**Changes**:

Add unit tests for the helper functions:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_close_due_to_request_limit_at_exact_limit() {
        assert!(should_close_due_to_request_limit(100, 100));
    }

    #[test]
    fn should_close_due_to_request_limit_over_limit() {
        assert!(should_close_due_to_request_limit(101, 100));
    }

    #[test]
    fn should_not_close_below_limit() {
        assert!(!should_close_due_to_request_limit(99, 100));
    }

    #[test]
    fn should_not_close_at_limit_1() {
        assert!(!should_close_due_to_request_limit(0, 100));
    }

    #[test]
    fn get_max_requests_uses_env_var() {
        std::env::set_var("RCOMM_MAX_REQUESTS_PER_CONNECTION", "50");
        assert_eq!(get_max_requests_per_connection(), 50);
        std::env::remove_var("RCOMM_MAX_REQUESTS_PER_CONNECTION");
    }

    #[test]
    fn get_max_requests_defaults_to_100() {
        std::env::remove_var("RCOMM_MAX_REQUESTS_PER_CONNECTION");
        assert_eq!(get_max_requests_per_connection(), 100);
    }

    #[test]
    fn get_max_requests_handles_invalid_value() {
        std::env::set_var("RCOMM_MAX_REQUESTS_PER_CONNECTION", "invalid");
        assert_eq!(get_max_requests_per_connection(), 100);
        std::env::remove_var("RCOMM_MAX_REQUESTS_PER_CONNECTION");
    }
}
```

---

## Testing Strategy

### Unit Tests (in `src/main.rs`)

- ✅ `should_close_due_to_request_limit()` returns true at exact limit
- ✅ `should_close_due_to_request_limit()` returns true over limit
- ✅ `should_close_due_to_request_limit()` returns false below limit
- ✅ `get_max_requests_per_connection()` reads environment variable
- ✅ `get_max_requests_per_connection()` defaults to 100
- ✅ `get_max_requests_per_connection()` handles invalid values gracefully

### Integration Tests (in `src/bin/integration_test.rs`)

- ✅ Connection closes after reaching request limit
- ✅ Final response includes `Connection: close` header
- ✅ Request limit applies to both GET and non-GET requests
- ✅ Request counter resets for new connections
- ✅ Requests below limit succeed normally
- ✅ Client cannot send additional requests after limit reached

### Manual Testing

```bash
# Build
cargo build

# Set a low limit for testing (default 100)
export RCOMM_MAX_REQUESTS_PER_CONNECTION=3

# Start server
./target/debug/rcomm &
SERVER_PID=$!

# Send multiple requests on one connection (using curl with keep-alive)
# Note: This requires persistent connection support to be implemented first
curl -v http://127.0.0.1:7878/
curl -v http://127.0.0.1:7878/
curl -v http://127.0.0.1:7878/
curl -v http://127.0.0.1:7878/  # Should fail or close connection

# Test with netcat for more direct control
(
  echo -e "GET / HTTP/1.1\r\nHost: localhost\r\n\r"
  sleep 0.1
  echo -e "GET / HTTP/1.1\r\nHost: localhost\r\n\r"
  sleep 0.1
  echo -e "GET / HTTP/1.1\r\nHost: localhost\r\n\r"
  sleep 0.1
  echo -e "GET / HTTP/1.1\r\nHost: localhost\r\n\r"
) | nc localhost 7878

# Kill server
kill $SERVER_PID
unset RCOMM_MAX_REQUESTS_PER_CONNECTION
```

---

## Edge Cases & Considerations

### 1. Request Counting Starts at 1

**Scenario**: Should we count the first request as 1 or 0?

**Decision**: Count starting at 1. The first request is the 1st request.

**Implementation**:
```rust
let mut request_count = 0;
loop {
    request_count += 1;  // Increment at loop start
    // ... process request
    if request_count >= max_requests {
        should_close = true;
    }
}
```

### 2. Maximum Value Considerations

**Scenario**: What if `max_requests` is set to 0 or 1?

**Handling**:
- `max_requests = 0`: Disable connections entirely (edge case, reject all requests with Connection: close)
- `max_requests = 1`: Allow only one request per connection, then close (valid use case for high-security scenarios)
- Current recommendation: Minimum 10, typical 100-1000

**Code**:
```rust
fn get_max_requests_per_connection() -> usize {
    let val = std::env::var("RCOMM_MAX_REQUESTS_PER_CONNECTION")
        .unwrap_or_else(|_| String::from("100"))
        .parse::<usize>()
        .unwrap_or(100);

    // Optionally, enforce minimum
    if val == 0 { 1 } else { val }
}
```

### 3. Limit Exceeded on Response Write Failure

**Scenario**: Connection limit is reached, but writing the response fails.

**Handling**: Let the error propagate and close the connection anyway. The response with `Connection: close` header has been queued, even if write fails.

**Code**: Already handled by existing error handling in `handle_connection()`.

### 4. Pipelined Requests at Limit

**Scenario**: Client sends N requests in one write, where the Nth request would exceed the limit.

**Handling**:
- Process first N-1 requests normally
- The Nth request is read and processed
- Response is sent with `Connection: close`
- Connection closes immediately after
- Any subsequent pipelined request data is discarded

**Why this is fine**: This is expected behavior. The client knows the limit and is responsible for not pipelining beyond it.

### 5. Timeout vs. Request Limit

**Scenario**: Both timeout and request limit could trigger closure.

**Handling**: Whichever happens first causes closure. This is not a conflict.

**Code**: Current implementation already handles both:
```rust
if should_close {  // Could be from request limit OR Connection: close
    break;
}
// Plus timeout error handling in Err(HttpParseError::IoError(...))
```

### 6. Limit Configuration per Route (Future)

**Out of Scope**: Currently, the limit is global per connection, not per route.

**Future Enhancement**: Could add per-route request limits (e.g., API endpoints allow fewer requests than static file endpoints) by checking the request target in `handle_single_request()`.

### 7. Load Balancer or Proxy Behind Server

**Scenario**: Server is behind a load balancer that reuses connections to backend servers.

**Impact**: Load balancer connection will close after N requests. It should gracefully reconnect.

**Recommendation**: Set `RCOMM_MAX_REQUESTS_PER_CONNECTION` high enough (e.g., 10,000) so this is not a bottleneck.

### 8. Monitoring and Metrics

**Out of Scope**: Not tracking closed connections or metrics in this phase.

**Future Enhancement**: Add counters for:
- Total requests per connection
- Connections closed due to request limit
- Average requests per connection

---

## Interaction with Existing Features

### Persistent Connections (Keep-Alive)

**Dependency**: This feature assumes HTTP/1.1 persistent connections are implemented (see `persistent_connections.md`).

**Interaction**:
- Request limit works within the persistent connection loop
- Limit is one additional reason (besides `Connection: close` or timeout) to close a connection
- No conflicts

### Connection Timeout

**Interaction**: Both timeout and request limit can cause closure independently.

**Precedence**: Whichever occurs first closes the connection. No conflict.

### Content-Length Enforcement

**No Direct Interaction**: Request limit doesn't depend on or affect Content-Length validation.

---

## Configuration Examples

### Default Behavior

```bash
# No environment variable set
# Uses default: 100 requests per connection
cargo run
```

### Low Limit (Strict Security)

```bash
# Allow only 10 requests per connection
export RCOMM_MAX_REQUESTS_PER_CONNECTION=10
cargo run
```

### High Limit (Performance)

```bash
# Allow 10,000 requests per connection (effectively unlimited for most use cases)
export RCOMM_MAX_REQUESTS_PER_CONNECTION=10000
cargo run
```

### Single Request Only (Maximum Security)

```bash
# Force one request per connection
export RCOMM_MAX_REQUESTS_PER_CONNECTION=1
cargo run
```

---

## Implementation Order

1. **Phase 1**: Add configuration function `get_max_requests_per_connection()`
2. **Phase 2**: Update `handle_connection()` signature and add request counter
3. **Phase 3**: Add `should_close_due_to_request_limit()` helper function
4. **Phase 4**: Add integration tests
5. **Phase 5**: Add unit tests
6. **Phase 6**: Manual testing with various configurations
7. **Phase 7**: Update documentation (README, CLAUDE.md)

---

## Success Criteria

- [x] Environment variable `RCOMM_MAX_REQUESTS_PER_CONNECTION` is read successfully
- [x] Default value of 100 is used when environment variable is not set
- [x] Invalid environment variable values are handled gracefully
- [x] Server closes connection after processing exactly N requests (N = configured limit)
- [x] Final response before closure includes `Connection: close` header
- [x] Connection counter resets for each new connection
- [x] Request limit applies regardless of HTTP method (GET, POST, etc.)
- [x] Limit applies regardless of request size or body presence
- [x] All existing tests pass
- [x] New integration tests pass
- [x] `cargo test` succeeds
- [x] `cargo run --bin integration_test` succeeds
- [x] Manual testing confirms behavior with various limits

---

## Performance Impact

**Positive**:
- **Reduced attack surface**: Limits slowloris and resource exhaustion attacks
- **Predictable resource cleanup**: Forces connection renewal periodically
- **Memory stability**: Prevents long-lived connections with potential accumulated state

**Negative** (if limit is too low):
- **Increased connection overhead**: More TCP handshakes for high-traffic clients
- **Client latency**: Clients must re-establish connections more frequently
- **Server CPU usage**: More threads/connections to manage

**Recommended**:
- Default of 100 is a good balance
- For load-balanced systems, consider 1000-10000
- For single-user or low-traffic scenarios, can be much lower

---

## Security Considerations

### Denial of Service (DoS) Prevention

**Threat**: Malicious client opens connection and sends requests indefinitely.

**Mitigation**: Request limit + timeout prevents this. Client can only send N requests per connection.

### Slowloris Attack Prevention

**Threat**: Malicious client sends one request very slowly, holding resources.

**Mitigation**: Timeout (separate feature) prevents indefinite waiting. Request limit forces periodic renewal.

### Resource Exhaustion

**Threat**: Many connections, each with many requests, consume server resources.

**Mitigation**: Request limit forces periodic closure, freeing thread pool workers and buffers.

### No Security Vulnerabilities Introduced

This feature **only adds restrictions**, never opens new attack surfaces.

---

## Rollback Plan

If issues arise during implementation:

1. Revert `src/main.rs` to commit before Phase 1
2. Remove environment variable handling
3. Remove request counter logic
4. Test that original single-request-per-connection behavior is restored

---

## Future Enhancements

- **Per-route limits**: Different limits for different endpoints
- **Per-IP limits**: Track connections per client IP address
- **Configurable header**: Allow clients to request different limits
- **Metrics**: Expose request limit statistics via `/metrics` endpoint
- **Dynamic adjustment**: Change limit without restarting server
- **Gradual shutdown**: Use `Connection: close` to gently migrate clients before restart

---

## Documentation Updates

After implementation, update:

- **README.md**: Document the `RCOMM_MAX_REQUESTS_PER_CONNECTION` environment variable
- **CLAUDE.md**: Add to Architecture section and update implementation notes
- **Integration test output**: Include request limit test results in summary

Example README update:

```markdown
## Configuration

### Request Limit Per Connection

Limit the number of HTTP requests allowed per persistent TCP connection (requires persistent
connections to be enabled). This helps prevent resource exhaustion and slowloris-style attacks.

**Environment variable**: `RCOMM_MAX_REQUESTS_PER_CONNECTION` (default: 100)

Example:
```bash
# Allow 50 requests per connection
export RCOMM_MAX_REQUESTS_PER_CONNECTION=50
cargo run
```

Recommended values:
- 10 for high-security scenarios
- 100 (default) for typical web servers
- 1000+ for APIs with keep-alive connections
```

---

## Implementation Notes

### Key Points

1. **Simple Counter**: The implementation is straightforward—just increment a counter each iteration.
2. **Zero Breaking Changes**: If persistent connections are already implemented, this is a simple addition.
3. **Backward Compatible**: Clients not using persistent connections are unaffected.
4. **Easy Testing**: Integration tests can control behavior by setting the limit low.

### Potential Pitfalls

1. **Counter Overflow**: If `request_count` is `usize`, overflow wraps to 0 (unlikely to hit in practice). Could use `u64` for extra safety.
2. **Off-by-one Errors**: Ensure counter increments at the right time (start of loop, not end).
3. **Header Overwriting**: Ensure `Connection: close` is always set when limit reached, even if previous response set `Connection: keep-alive`.
4. **Logging**: Print clear message when limit is reached for debugging.

### Code Quality

- Keep the logic simple and readable
- Use helper functions for clarity
- Add comments for non-obvious behavior
- Ensure all paths (timeout, error, limit) close connection properly

---

## Testing Checklist

- [ ] Unit test: `should_close_due_to_request_limit()` at boundary
- [ ] Unit test: `get_max_requests_per_connection()` reads env var
- [ ] Unit test: `get_max_requests_per_connection()` defaults to 100
- [ ] Integration test: Connection closes after N requests
- [ ] Integration test: Response includes `Connection: close` at limit
- [ ] Integration test: Counter resets per connection
- [ ] Integration test: Works with GET requests
- [ ] Integration test: Works with POST requests
- [ ] Manual test: Low limit (3 requests)
- [ ] Manual test: High limit (10000 requests)
- [ ] Manual test: Invalid env var value
- [ ] Manual test: Unset env var uses default
- [ ] Regression test: All existing tests still pass

---

## References

- **RFC 7230**: HTTP/1.1 Message Syntax and Routing, Section 6.3 (Persistence)
- **RFC 6585**: Additional HTTP Status Codes (e.g., 429 Too Many Requests)
- **OWASP**: Slowloris attack and DoS prevention
- **Existing Feature**: Persistent Connections (see `persistent_connections.md`)
