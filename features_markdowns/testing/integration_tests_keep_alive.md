# Integration Tests for Connection: keep-alive / Persistent Connections

**Category:** Testing
**Complexity:** 3/10
**Necessity:** 5/10
**Status:** Planning

---

## Overview

Add integration tests to `src/bin/integration_test.rs` that verify HTTP/1.1 persistent connection behavior. In HTTP/1.1, connections are persistent by default (`Connection: keep-alive`). The client can send multiple requests on a single TCP connection without reconnecting. The server should process each request and keep the connection open until the client sends `Connection: close` or the connection times out.

**Goal:** Document the current behavior (single request per connection) and provide acceptance tests for when persistent connection support is implemented.

**Note:** The server currently does NOT support persistent connections. After processing one request, the connection is effectively closed because `handle_connection()` returns without reading further requests.

---

## Current State

### Connection Handling (src/main.rs, lines 46-74)

```rust
fn handle_connection(mut stream: TcpStream, routes: HashMap<String, PathBuf>) {
    // Parse one request
    let http_request = match HttpRequest::build_from_stream(&stream) { ... };
    // Build one response
    // Write one response
    stream.write_all(&response.as_bytes()).unwrap();
    // Function returns — connection implicitly closed when stream is dropped
}
```

After `handle_connection()` returns, the `TcpStream` is dropped, closing the TCP connection. The second request on the same connection would never be read.

### Test Client (integration_test.rs, line 163)

The test `send_request()` helper explicitly sends `Connection: close`:

```rust
let request = format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
```

This means the existing tests always close after one request.

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`

**Add:** Keep-alive test functions with a multi-request-per-connection pattern.
**Modify:** `main()` function to register new tests.

---

## Step-by-Step Implementation

### Step 1: Add Keep-Alive Request Helper

Send a request without `Connection: close` and reuse the stream:

```rust
fn send_keepalive_request(
    stream: &mut TcpStream,
    method: &str,
    path: &str,
    host: &str,
) -> Result<TestResponse, String> {
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: keep-alive\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("write: {e}"))?;
    read_response(stream)
}
```

### Step 2: Add Test Functions

```rust
fn test_keepalive_two_requests_same_connection(addr: &str) -> Result<(), String> {
    // Send two requests on the same TCP connection
    let mut stream = TcpStream::connect(addr)
        .map_err(|e| format!("connect: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| format!("set timeout: {e}"))?;

    // First request
    let resp1 = send_keepalive_request(&mut stream, "GET", "/", addr)?;
    assert_eq_or_err(&resp1.status_code, &200, "first request status")?;
    assert_contains_or_err(&resp1.body, "Hello!", "first request body")?;

    // Second request on same connection
    let resp2 = send_keepalive_request(&mut stream, "GET", "/howdy", addr)?;
    assert_eq_or_err(&resp2.status_code, &200, "second request status")?;
    assert_contains_or_err(&resp2.body, "Howdy!", "second request body")?;

    Ok(())
}

fn test_keepalive_multiple_requests(addr: &str) -> Result<(), String> {
    // Send 5 requests on the same connection
    let mut stream = TcpStream::connect(addr)
        .map_err(|e| format!("connect: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| format!("set timeout: {e}"))?;

    let routes = vec!["/", "/howdy", "/index.css", "/", "/howdy"];

    for (i, path) in routes.iter().enumerate() {
        let resp = send_keepalive_request(&mut stream, "GET", path, addr)?;
        assert_eq_or_err(&resp.status_code, &200, &format!("request {i} status"))?;
    }
    Ok(())
}

fn test_connection_close_header_respected(addr: &str) -> Result<(), String> {
    // Send a request with Connection: close, verify the connection is closed
    let mut stream = TcpStream::connect(addr)
        .map_err(|e| format!("connect: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| format!("set timeout: {e}"))?;

    let request = format!(
        "GET / HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("write: {e}"))?;
    let resp = read_response(&mut stream)?;
    assert_eq_or_err(&resp.status_code, &200, "close request status")?;

    // Try to send another request — should fail or get no response
    let second_request = format!(
        "GET /howdy HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
    );
    match stream.write_all(second_request.as_bytes()) {
        Err(_) => Ok(()), // Write failed — connection closed as expected
        Ok(()) => {
            // Write succeeded (data may be buffered). Try reading.
            let mut buf = [0u8; 1];
            match stream.read(&mut buf) {
                Ok(0) => Ok(()), // EOF — connection closed
                Err(_) => Ok(()), // Error — connection closed
                Ok(_) => Err("Connection was not closed after Connection: close".to_string()),
            }
        }
    }
}

fn test_keepalive_mixed_routes(addr: &str) -> Result<(), String> {
    // Verify different routes return correct content on the same connection
    let mut stream = TcpStream::connect(addr)
        .map_err(|e| format!("connect: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| format!("set timeout: {e}"))?;

    // Request index
    let resp = send_keepalive_request(&mut stream, "GET", "/", addr)?;
    assert_eq_or_err(&resp.status_code, &200, "/ status")?;
    assert_contains_or_err(&resp.body, "Hello!", "/ body")?;

    // Request 404 page
    let resp = send_keepalive_request(&mut stream, "GET", "/nope", addr)?;
    assert_eq_or_err(&resp.status_code, &404, "/nope status")?;

    // Request valid route again
    let resp = send_keepalive_request(&mut stream, "GET", "/howdy", addr)?;
    assert_eq_or_err(&resp.status_code, &200, "/howdy status")?;
    assert_contains_or_err(&resp.body, "Howdy!", "/howdy body")?;

    Ok(())
}
```

### Step 3: Register Tests in `main()`

```rust
// Keep-alive tests
// NOTE: These tests will fail until persistent connection support is implemented.
// The first request will succeed but the second will fail with a read error.
// TODO: Enable once keep-alive is implemented.
// run_test("keepalive_two_requests", || test_keepalive_two_requests_same_connection(&addr)),
// run_test("keepalive_multiple_requests", || test_keepalive_multiple_requests(&addr)),
// run_test("connection_close_respected", || test_connection_close_header_respected(&addr)),
// run_test("keepalive_mixed_routes", || test_keepalive_mixed_routes(&addr)),
```

---

## Edge Cases & Considerations

### 1. Current Behavior: Connection Drops After First Response

**Scenario:** The server returns one response and the `TcpStream` is dropped.

**Impact on tests:** The second `send_keepalive_request()` will fail with a read error because the server closed the connection. Tests are disabled by default with TODO comments.

### 2. Response Framing

**Scenario:** For persistent connections, the client must know where one response ends and the next begins.

**Requirement:** `Content-Length` must be accurate for every response, or `Transfer-Encoding: chunked` must be used. The current server sets `Content-Length` via `add_body()`, which is correct.

### 3. read_response() Behavior

**Scenario:** The `read_response()` helper reads exactly `Content-Length` bytes for the body. On a persistent connection, this is correct — it reads one response without consuming bytes from the next.

**Key:** `BufReader` buffers data internally. If the `BufReader` reads ahead into the next response's data, subsequent calls may lose bytes. The test should use a single `BufReader` per connection rather than creating a new one per response.

**Potential Issue:** The current `read_response()` creates a `BufReader` from `stream.try_clone()`. On persistent connections, each clone shares the same underlying socket but not the buffer. This could lose data.

**Fix for keep-alive tests:** Create one `BufReader` per connection and pass it to response reading:

```rust
fn read_response_from_reader(reader: &mut BufReader<TcpStream>) -> Result<TestResponse, String> {
    // Same logic as read_response() but takes an existing BufReader
    // ...
}
```

### 4. Server-Side Keep-Alive Loop

When persistent connections are implemented, `handle_connection()` will need an inner loop:

```rust
fn handle_connection(mut stream: TcpStream, routes: HashMap<String, PathBuf>) {
    loop {
        let http_request = match HttpRequest::build_from_stream(&stream) {
            Ok(req) => req,
            Err(_) => break, // Connection closed or error
        };
        // ... process request ...
        // Check Connection: close header
        if should_close { break; }
    }
}
```

### 5. Idle Timeout

Persistent connections need an idle timeout to prevent resource leaks. This overlaps with the Request Timeout feature.

---

## Testing Strategy

### Running the Tests

```bash
cargo build && cargo run --bin integration_test
```

### Expected Results (Before Keep-Alive Feature)

All keep-alive tests are commented out / disabled. Existing tests pass unchanged.

### Expected Results (After Keep-Alive Feature)

All keep-alive tests pass. Multiple requests succeed on single connections. `Connection: close` properly terminates the connection.

---

## Implementation Checklist

- [ ] Add `send_keepalive_request()` helper function
- [ ] Add `test_keepalive_two_requests_same_connection()` test
- [ ] Add `test_keepalive_multiple_requests()` test
- [ ] Add `test_connection_close_header_respected()` test
- [ ] Add `test_keepalive_mixed_routes()` test
- [ ] Address `BufReader` sharing issue for persistent connections
- [ ] Register tests in `main()` (disabled by default)
- [ ] Run `cargo build` — no compiler errors
- [ ] Enable and verify tests once keep-alive feature is implemented

---

## Dependencies

- **HTTP Protocol > Persistent Connections**: Must be implemented before these tests can pass
- **HTTP Protocol > Connection: close Header Handling**: Server must respect the `Connection` header

---

## Related Features

- **HTTP Protocol > HTTP/1.1 Persistent Connections**: The primary feature these tests support
- **Connection Handling > HTTP Pipelining**: Pipelining builds on persistent connections
- **Security > Request Timeout**: Idle timeout for persistent connections
- **Security > Request Limit Per Connection**: Limits requests per persistent connection

---

## References

- [RFC 7230 Section 6.3 - Persistence](https://tools.ietf.org/html/rfc7230#section-6.3)
- [RFC 7230 Section 6.6 - Tear-down](https://tools.ietf.org/html/rfc7230#section-6.6)
- [MDN: Connection Header](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Connection)
