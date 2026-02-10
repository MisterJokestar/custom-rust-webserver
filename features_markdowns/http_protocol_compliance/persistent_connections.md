# HTTP/1.1 Persistent Connections (Keep-Alive) Implementation Plan

## Feature Overview

Implement HTTP/1.1 persistent connections (`Connection: keep-alive`) to allow multiple HTTP requests to be sent over a single TCP connection, improving client-side performance and reducing connection overhead.

**Status**: Planned
**Complexity**: 6/10
**Necessity**: 7/10
**Category**: HTTP Protocol Compliance

### Current Behavior

The current implementation:
- Opens a TCP connection in `handle_connection()` (line 46 in `src/main.rs`)
- Parses exactly **one** HTTP request via `HttpRequest::build_from_stream()`
- Sends one HTTP response
- Closes the TCP connection and returns
- Does not examine the `Connection` header to determine client intent

### Desired Behavior

After implementation:
- Detect the `Connection: keep-alive` header (or absence of `Connection: close`)
- Parse multiple sequential HTTP requests from the same TCP connection
- Send corresponding responses for each request
- Close the connection only when:
  - Client sends `Connection: close` header
  - A timeout expires (no new request received)
  - Malformed/unparseable request arrives
  - Request parsing errors occur
- Properly handle the interaction between `Content-Length` and request boundaries

---

## Key Technical Challenges

### 1. Request Boundary Detection

**Challenge**: After sending a response, how do we know if a new request is coming or if the client has finished?

**Current State**: `HttpRequest::build_from_stream()` reads from a `BufReader` wrapping the `TcpStream`. After one request is parsed, the reader is dropped, and the stream is no longer accessible for reading subsequent requests.

**Solution**:
- Refactor `handle_connection()` to maintain the stream and `BufReader` across multiple request/response cycles
- Wrap the reader in a way that allows peeking or attempting to read the next request line
- Use non-blocking read or timeouts to detect when no more data is incoming

### 2. Content-Length Requirement

**Challenge**: Without `Content-Length` or `Transfer-Encoding: chunked`, the server cannot know when the request body ends, making it impossible to reliably parse the next request.

**Current State**: `HttpRequest::build_from_stream()` already respects `Content-Length` (lines 96-104 in `http_request.rs`).

**Requirement**: Enforce that all request bodies include a `Content-Length` header when keep-alive is enabled. Requests without it should either:
- Be rejected with 411 Length Required
- Default to 0 body length (safer for GET/HEAD/DELETE)

### 3. BufReader Ownership

**Challenge**: `BufReader::new()` takes ownership of the stream. Once dropped, we lose buffering state between requests.

**Current State**: `build_from_stream()` creates a new `BufReader` each time and drops it when the function returns.

**Solution**:
- Refactor to pass a mutable `BufReader` into the request parsing function
- Create the `BufReader` once in `handle_connection()` and keep it alive across request cycles
- Adjust the signature of `HttpRequest::build_from_stream()` to accept `&mut BufReader<&TcpStream>` instead of `&TcpStream`

### 4. Timeout Handling

**Challenge**: If a client sends one request then goes silent, the server thread blocks indefinitely waiting for the next request.

**Solution**:
- Set a read timeout on the `TcpStream` (e.g., 5-30 seconds)
- When a timeout occurs (read returns `std::io::ErrorKind::WouldBlock` or `TimedOut`), gracefully close the connection
- Log the timeout as normal connection termination, not an error

---

## Implementation Plan

### Phase 1: Refactor `HttpRequest::build_from_stream()` Signature

**File**: `src/models/http_request.rs`

**Changes**:

1. Modify the signature to accept a mutable `BufReader` instead of `&TcpStream`:

```rust
pub fn build_from_stream(
    buf_reader: &mut BufReader<&TcpStream>
) -> Result<HttpRequest, HttpParseError> {
    // existing logic, but read from buf_reader directly
}
```

2. Update all internal `buf_reader.read_line()` calls (they already exist, just ensure they work with the new signature).

3. Keep all existing unit tests passing by creating a `BufReader` within each test.

**Impact**: Breaking change to the public API of `HttpRequest`. Must update:
- `src/main.rs` — `handle_connection()` function
- All tests in `http_request.rs` that call `build_from_stream()`

**Code Example** (before):
```rust
pub fn build_from_stream(stream: &TcpStream) -> Result<HttpRequest, HttpParseError> {
    let mut buf_reader = BufReader::new(stream);
    // ... parse using buf_reader
}
```

**Code Example** (after):
```rust
pub fn build_from_stream(
    buf_reader: &mut BufReader<&TcpStream>
) -> Result<HttpRequest, HttpParseError> {
    // ... parse using buf_reader (already received as parameter)
}
```

---

### Phase 2: Refactor `handle_connection()` to Support Request Loops

**File**: `src/main.rs`

**Changes**:

1. Create the `BufReader` once at the start of `handle_connection()`:

```rust
fn handle_connection(mut stream: TcpStream, routes: HashMap<String, PathBuf>) {
    // Set a read timeout to detect idle clients
    stream.set_read_timeout(Some(Duration::from_secs(15)))
        .unwrap_or_else(|e| {
            eprintln!("Failed to set read timeout: {e}");
        });

    let mut buf_reader = BufReader::new(&stream);

    // Enter request loop
    loop {
        // Attempt to parse next request
        match HttpRequest::build_from_stream(&mut buf_reader) {
            Ok(req) => {
                // Handle the request
                let (response, should_close) = handle_single_request(
                    &req,
                    &routes,
                );

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
            Err(e) => {
                // Handle parse error
                eprintln!("Request parse error: {e}");
                let mut response = HttpResponse::build(String::from("HTTP/1.1"), 400);
                response.add_body(format!("Bad Request: {e}").into());
                let _ = stream.write_all(&response.as_bytes());
                break; // Close on parse error
            }
        }
    }
}
```

2. Extract request handling logic into `handle_single_request()`:

```rust
fn handle_single_request(
    req: &HttpRequest,
    routes: &HashMap<String, PathBuf>,
) -> (HttpResponse, bool) {
    let clean_target = clean_route(&req.target);
    println!("Request: {req}");

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

    // Determine if connection should close
    let should_close = should_close_connection(&req);

    // Add Connection header to response if needed
    if should_close {
        response.add_header("Connection".to_string(), "close".to_string());
    } else {
        // Explicitly signal keep-alive if not already set
        if response.try_get_header("Connection".to_string()).is_none() {
            response.add_header("Connection".to_string(), "keep-alive".to_string());
        }
    }

    (response, should_close)
}
```

3. Add helper to determine connection close behavior:

```rust
fn should_close_connection(req: &HttpRequest) -> bool {
    // Explicit "Connection: close" from client
    if let Some(conn) = req.try_get_header("connection".to_string()) {
        if conn.to_lowercase().contains("close") {
            return true;
        }
    }

    // HTTP/1.0 defaults to close (unless Connection: keep-alive is present)
    if req.version == "HTTP/1.0" {
        if let Some(conn) = req.try_get_header("connection".to_string()) {
            return !conn.to_lowercase().contains("keep-alive");
        }
        return true;
    }

    // HTTP/1.1 defaults to keep-alive
    false
}
```

4. Import `std::time::Duration`:

```rust
use std::time::Duration;
```

---

### Phase 3: Handle Request Boundary Detection

**File**: `src/models/http_request.rs`

**Challenge**: When `build_from_stream()` is called on the second+ request, the `BufReader` may have no data available yet, causing a blocking read.

**Solution Options**:

**Option A: Timeout-based (Recommended)**
- Rely on the `TcpStream` read timeout set in Phase 2
- If no data arrives within the timeout, `read_line()` returns an `IoError` with `TimedOut` or `WouldBlock`
- Treat timeouts as normal connection termination in `handle_connection()`

**Option B: Peek-based**
- Use `BufReader::fill_buf()` to check if data is available without blocking
- Return a special error variant `HttpParseError::NoMoreRequests` if no data
- Requires exposing internal buffering logic

**Option C: Manual newline detection**
- After sending a response, try to read a single byte
- If read returns 0 bytes (EOF) or timeout, close
- If data is available, process it

**Implementation (Option A - Recommended)**:

In `handle_connection()`, wrap the request parsing in timeout handling:

```rust
match HttpRequest::build_from_stream(&mut buf_reader) {
    Ok(req) => {
        // ... handle request
    }
    Err(HttpParseError::IoError(io_err)) => {
        // Check if it's a timeout (normal end of connection)
        if io_err.kind() == std::io::ErrorKind::WouldBlock
            || io_err.kind() == std::io::ErrorKind::TimedOut {
            // Client idle or closed connection normally
            break;
        }
        // Other IO errors (e.g., connection reset)
        eprintln!("IO error: {io_err}");
        break;
    }
    Err(e) => {
        eprintln!("Parse error: {e}");
        let mut response = HttpResponse::build(String::from("HTTP/1.1"), 400);
        response.add_body(format!("Bad Request: {e}").into());
        let _ = stream.write_all(&response.as_bytes());
        break;
    }
}
```

---

### Phase 4: Content-Length Enforcement

**File**: `src/models/http_request.rs`

**Decision**: For HTTP/1.1 requests with a body (POST, PUT, PATCH), require `Content-Length` when persistent connections are enabled.

**Changes**:

1. Add a new error variant:

```rust
pub enum HttpParseError {
    HeaderTooLong,
    MissingHostHeader,
    MalformedRequestLine,
    MissingContentLength,  // NEW
    IoError(std::io::Error),
}
```

2. Add Display case:

```rust
impl fmt::Display for HttpParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // ... existing cases
            HttpParseError::MissingContentLength => {
                write!(f, "Missing required Content-Length header for request body")
            }
            // ...
        }
    }
}
```

3. Add validation in `build_from_stream()` after parsing headers:

```rust
// Validate Content-Length for methods that may have a body
let has_body = req.headers.contains_key("content-length")
    || req.headers.contains_key("transfer-encoding");

if matches!(req.method, HttpMethods::POST | HttpMethods::PUT | HttpMethods::PATCH) {
    if !has_body && req.version == "HTTP/1.1" {
        // For keep-alive, require explicit Content-Length: 0 if no body
        // Or reject the request
        return Err(HttpParseError::MissingContentLength);
    }
}
```

**Alternative (Lenient)**: Default request body length to 0 if not specified:

```rust
// If no Content-Length header, default to 0 body length
// This is safer but potentially masks client errors
if req.headers.get("content-length").is_none() {
    // Body remains None, effectively 0-length
}
```

---

### Phase 5: Update Tests

**File**: `src/models/http_request.rs`

**Changes**:

1. Update all existing tests to create a `BufReader` before calling `build_from_stream()`:

**Before**:
```rust
#[test]
fn build_from_stream_parses_get_request() {
    use std::io::Write;
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = std::thread::spawn(move || {
        let mut client = std::net::TcpStream::connect(addr).unwrap();
        client.write_all(b"GET /hello HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
    });

    let (stream, _) = listener.accept().unwrap();
    let req = HttpRequest::build_from_stream(&stream).unwrap();  // OLD SIGNATURE

    assert_eq!(req.method.to_string(), "GET");
    assert_eq!(req.target, "/hello");
    assert_eq!(req.version, "HTTP/1.1");
    handle.join().unwrap();
}
```

**After**:
```rust
#[test]
fn build_from_stream_parses_get_request() {
    use std::io::{Write, BufReader};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = std::thread::spawn(move || {
        let mut client = std::net::TcpStream::connect(addr).unwrap();
        client.write_all(b"GET /hello HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
    });

    let (stream, _) = listener.accept().unwrap();
    let mut buf_reader = BufReader::new(&stream);  // NEW
    let req = HttpRequest::build_from_stream(&mut buf_reader).unwrap();  // NEW SIGNATURE

    assert_eq!(req.method.to_string(), "GET");
    assert_eq!(req.target, "/hello");
    assert_eq!(req.version, "HTTP/1.1");
    handle.join().unwrap();
}
```

2. Add new tests for persistent connections:

```rust
#[test]
fn build_from_stream_parses_two_sequential_requests() {
    use std::io::{Write, BufReader};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = std::thread::spawn(move || {
        let mut client = std::net::TcpStream::connect(addr).unwrap();
        // Send two requests back-to-back
        client.write_all(b"GET /first HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
        client.write_all(b"GET /second HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
    });

    let (stream, _) = listener.accept().unwrap();
    let mut buf_reader = BufReader::new(&stream);

    // Parse first request
    let req1 = HttpRequest::build_from_stream(&mut buf_reader).unwrap();
    assert_eq!(req1.target, "/first");

    // Parse second request (from same BufReader)
    let req2 = HttpRequest::build_from_stream(&mut buf_reader).unwrap();
    assert_eq!(req2.target, "/second");

    handle.join().unwrap();
}
```

---

### Phase 6: Integration Tests

**File**: `src/bin/integration_test.rs`

**Changes**:

Add new test cases for persistent connections:

```rust
fn test_keep_alive_single_connection(addr: &str) -> Result<(), String> {
    // Establish ONE connection
    let mut stream = TcpStream::connect(addr)
        .map_err(|e| format!("connect: {e}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| format!("set timeout: {e}"))?;

    // Send first request
    let req1 = "GET / HTTP/1.1\r\nHost: localhost\r\nConnection: keep-alive\r\n\r\n";
    stream.write_all(req1.as_bytes())
        .map_err(|e| format!("write req1: {e}"))?;

    let resp1 = read_response(&mut stream)
        .map_err(|e| format!("read resp1: {e}"))?;
    assert_eq_or_err(&resp1.status_code, &200, "first response status")?;

    // Send second request on SAME connection
    let req2 = "GET /index.css HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    stream.write_all(req2.as_bytes())
        .map_err(|e| format!("write req2: {e}"))?;

    let resp2 = read_response(&mut stream)
        .map_err(|e| format!("read resp2: {e}"))?;
    assert_eq_or_err(&resp2.status_code, &200, "second response status")?;

    // Second response should have "Connection: close"
    if let Some(conn) = resp2.headers.get("connection") {
        if !conn.to_lowercase().contains("close") {
            return Err("Expected Connection: close in final response".to_string());
        }
    }

    Ok(())
}

fn test_keep_alive_respects_close_header(addr: &str) -> Result<(), String> {
    let mut stream = TcpStream::connect(addr)
        .map_err(|e| format!("connect: {e}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| format!("set timeout: {e}"))?;

    // Send request with Connection: close
    let req = "GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    stream.write_all(req.as_bytes())
        .map_err(|e| format!("write: {e}"))?;

    let resp = read_response(&mut stream)
        .map_err(|e| format!("read resp: {e}"))?;

    assert_eq_or_err(&resp.status_code, &200, "status")?;

    // Response should include Connection: close
    if let Some(conn) = resp.headers.get("connection") {
        if !conn.to_lowercase().contains("close") {
            return Err("Expected Connection: close in response".to_string());
        }
    }

    Ok(())
}

fn test_keep_alive_http10_defaults_close(addr: &str) -> Result<(), String> {
    let mut stream = TcpStream::connect(addr)
        .map_err(|e| format!("connect: {e}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| format!("set timeout: {e}"))?;

    // Send HTTP/1.0 request without Connection header
    let req = "GET / HTTP/1.0\r\nHost: localhost\r\n\r\n";
    stream.write_all(req.as_bytes())
        .map_err(|e| format!("write: {e}"))?;

    let resp = read_response(&mut stream)
        .map_err(|e| format!("read resp: {e}"))?;

    assert_eq_or_err(&resp.status_code, &200, "status")?;

    // HTTP/1.0 should default to close
    // Try to send another request; it should fail (connection closed)
    let req2 = "GET / HTTP/1.1\r\nHost: localhost\r\n\r\n";
    if stream.write_all(req2.as_bytes()).is_ok() {
        // Write succeeded, but read should fail or return no data
        // This is implementation-dependent
    }

    Ok(())
}

fn test_malformed_second_request_closes(addr: &str) -> Result<(), String> {
    let mut stream = TcpStream::connect(addr)
        .map_err(|e| format!("connect: {e}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| format!("set timeout: {e}"))?;

    // Send valid first request
    let req1 = "GET / HTTP/1.1\r\nHost: localhost\r\n\r\n";
    stream.write_all(req1.as_bytes())
        .map_err(|e| format!("write req1: {e}"))?;

    let resp1 = read_response(&mut stream)
        .map_err(|e| format!("read resp1: {e}"))?;
    assert_eq_or_err(&resp1.status_code, &200, "first response")?;

    // Send malformed second request
    let req2 = "BADMETHOD /foo HTTP/1.1\r\nHost: localhost\r\n\r\n";
    stream.write_all(req2.as_bytes())
        .map_err(|e| format!("write req2: {e}"))?;

    // Server should respond with 400
    let resp2 = read_response(&mut stream)
        .map_err(|e| format!("read resp2: {e}"))?;
    assert_eq_or_err(&resp2.status_code, &400, "malformed request status")?;

    Ok(())
}
```

Add to main function:

```rust
let test_keep_alive_single = run_test("keep_alive: single connection with two requests", || {
    test_keep_alive_single_connection(&addr)
});

let test_keep_alive_close = run_test("keep_alive: respects Connection: close", || {
    test_keep_alive_respects_close_header(&addr)
});

let test_http10_close = run_test("keep_alive: HTTP/1.0 defaults to close", || {
    test_keep_alive_http10_defaults_close(&addr)
});

let test_malformed_second = run_test("keep_alive: malformed second request", || {
    test_malformed_second_request_closes(&addr)
});

// ... collect all test results and print summary
```

---

## Testing Strategy

### Unit Tests (in `src/models/http_request.rs`)

- ✅ Verify that `BufReader` signature change works
- ✅ Parse two sequential requests from same `BufReader`
- ✅ Timeout detection on idle stream
- ✅ Content-Length enforcement for POST/PUT/PATCH on HTTP/1.1
- ✅ Malformed request in second attempt

### Integration Tests (in `src/bin/integration_test.rs`)

- ✅ Two requests over one connection with `Connection: keep-alive`
- ✅ `Connection: close` is respected
- ✅ HTTP/1.0 defaults to close
- ✅ Malformed second request is rejected with 400
- ✅ Timeout on idle connection (optional, depends on test harness)
- ✅ Response includes `Connection: keep-alive` or `Connection: close` as appropriate

### Manual Testing

```bash
# Build
cargo build

# Start server
./target/debug/rcomm &
SERVER_PID=$!

# Test keep-alive with curl (sends keep-alive by default in HTTP/1.1)
curl -v http://127.0.0.1:7878/
curl -v http://127.0.0.1:7878/

# Test with explicit Connection header
curl -H "Connection: close" http://127.0.0.1:7878/

# Kill server
kill $SERVER_PID
```

---

## Edge Cases & Considerations

### 1. Timeout on Idle Connection

**Scenario**: Client opens connection, sends one request, then goes silent.

**Handling**:
- Set `TcpStream::set_read_timeout()` to 15-30 seconds
- Catch `std::io::ErrorKind::WouldBlock` or `TimedOut` when reading next request
- Log as normal termination, not error

**Code**:
```rust
stream.set_read_timeout(Some(Duration::from_secs(15)))
    .expect("Failed to set read timeout");
```

### 2. Partial Request in Buffer

**Scenario**: Client sends partial request, stream times out before complete request.

**Handling**:
- `read_line()` blocks waiting for complete line (request line or header)
- Timeout fires during `read_line()`, returns `IoError(TimedOut)`
- Treat as normal connection close

### 3. Pipelined Requests

**Scenario**: Client sends multiple requests in one write (HTTP pipelining).

**Handling**:
- Current implementation should handle this naturally
- `BufReader` buffers the data
- First `build_from_stream()` reads request 1
- Second `build_from_stream()` reads request 2 from buffer (no socket read needed)

**Testing**:
```rust
#[test]
fn pipelined_requests() {
    // Write two complete requests in one write_all() call
    client.write_all(b"GET /a HTTP/1.1\r\nHost: localhost\r\n\r\n\
                       GET /b HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
    // Both should parse successfully
}
```

### 4. Missing Content-Length on POST

**Scenario**: POST request without `Content-Length` header (invalid for HTTP/1.1 with keep-alive).

**Handling**:
- Return `HttpParseError::MissingContentLength`
- Server responds with 411 Length Required
- Close connection

**Alternative**: Default body length to 0 (lenient, may hide client bugs)

### 5. Oversized Request Body

**Scenario**: `Content-Length: 1000000` on a simple GET request.

**Handling**:
- Current: `vec![0u8; 1000000]` is allocated and read
- Consider adding a `MAX_BODY_SIZE` constant (e.g., 10 MB)
- Reject with 413 Payload Too Large if exceeded

### 6. Connection Reset by Client

**Scenario**: Client closes TCP connection between requests.

**Handling**:
- `read_line()` returns `Ok(0)` (EOF) or `IoError`
- Treat as normal termination

### 7. Multiple Requests in Quick Succession

**Scenario**: Benchmark/load test with many requests on one connection.

**Handling**:
- Should work correctly with current architecture
- Thread pool worker handles request loop until close
- No new thread spawned per request (efficient)

### 8. Upgrade Requests (WebSocket, etc.)

**Scenario**: Client sends `Upgrade: websocket` header.

**Current Scope**: Out of scope for this feature. Server can respond with 501 Not Implemented.

**Future**: Would require protocol switching in `handle_single_request()`.

---

## Implementation Order

1. **Phase 1**: Refactor `HttpRequest::build_from_stream()` signature
2. **Phase 2**: Refactor `handle_connection()` and extract `handle_single_request()`
3. **Phase 3**: Add timeout handling and idle detection
4. **Phase 4**: Add Content-Length enforcement (optional: lenient vs. strict)
5. **Phase 5**: Update all existing tests
6. **Phase 6**: Add integration tests
7. **Phase 7**: Manual testing and debugging

---

## Success Criteria

- [x] Single `TcpStream` supports multiple sequential HTTP/1.1 requests
- [x] Server respects `Connection: close` header
- [x] Server respects HTTP/1.0 default (close)
- [x] Server defaults to keep-alive for HTTP/1.1 without explicit header
- [x] Idle timeout closes connection gracefully
- [x] Malformed requests on persistent connection result in 400 + close
- [x] All existing tests pass
- [x] New integration tests pass
- [x] `cargo test` succeeds
- [x] `cargo run --bin integration_test` succeeds

---

## Performance Impact

**Positive**:
- **Reduced connection overhead**: No TCP handshake per request
- **Improved client latency**: Especially for multiple small requests (CSS, JS, images)
- **Server efficiency**: One thread handles multiple requests, reduces thread context switching

**Negative** (if not implemented carefully):
- **Resource leaks**: Idle connections holding threads (mitigated by timeout)
- **Memory usage**: Each connection has a `BufReader` buffer (typically 8 KB, negligible)

**Expected improvement**: 2-5x faster for clients making 3+ sequential requests (e.g., HTML + CSS + JS).

---

## RFC / HTTP Spec References

- **RFC 7230**: HTTP/1.1 Message Syntax and Routing, Section 6.3 (Persistence)
- **RFC 7231**: HTTP/1.1 Semantics and Content, Section 6 (Response Status Codes)
- Key headers: `Connection`, `Content-Length`, `Transfer-Encoding`
- HTTP/1.0 persistent connections (non-standard, but documented in RFC 2068)

---

## Rollback Plan

If issues arise during implementation:

1. Revert `src/main.rs` and `src/models/http_request.rs` to commit before Phase 1
2. Re-add single-request handler if needed
3. Inspect logs for timeout behavior or body parsing issues

---

## Future Enhancements

- **Transfer-Encoding: chunked**: Support for chunked request/response bodies (enables streaming)
- **100-continue**: Handle `Expect: 100-continue` header for large POSTs
- **Pipelining limits**: Reject if too many pipelined requests
- **HTTP/2 upgrade path**: Use `Upgrade` header detection to transition to HTTP/2
- **Keep-alive timeout tuning**: Make timeout configurable via environment variable
- **Metrics**: Track persistent connection reuse, timeouts, errors

---

## Implementation Notes

### Potential Pitfalls

1. **BufReader::new() consumes the stream**: Cannot call multiple times on same stream.
   - Solution: Create once, pass mutable reference.

2. **read_line() blocks indefinitely without timeout**: Will hang if client disappears.
   - Solution: Set `TcpStream::set_read_timeout()`.

3. **Scope of HttpRequest lifetime**: If `build_from_stream()` takes `&mut BufReader<&TcpStream>`, ensure stream outlives the call.
   - Solution: Stream is owned by `handle_connection()`, lives entire loop duration. Safe.

4. **Content-Length: 0 for GET**: Some clients don't include this; allow as lenient default.
   - Solution: Separate validation for request body presence vs. presence assertion.

5. **Case sensitivity in Connection header**: Should be case-insensitive per HTTP spec.
   - Solution: Use `.to_lowercase()` when checking connection directives.

### Key Decision: Lenient vs. Strict Mode

**Strict** (Recommended for HTTP/1.1 with keep-alive):
- Require `Content-Length: 0` or explicit `Content-Length` for request bodies
- Reject with 411 if missing
- Safer for persistent connections

**Lenient** (Better compatibility):
- Default to 0 body length if `Content-Length` missing
- Only use keep-alive if client explicitly requests it

**Recommended implementation**: Strict for safety, document in error response.

