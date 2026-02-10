# Implementation Plan: Add Request Timeout

## Overview

Request timeouts are a critical security feature for preventing slowloris and other denial-of-service (DoS) attacks where clients intentionally stall during header or body transmission to exhaust server thread pool resources. Without timeouts, a single malicious client can occupy a worker thread indefinitely, eventually consuming all available workers and rendering the server unable to handle legitimate requests.

**Current State**: The server's `TcpStream` reading operations in `src/models/http_request.rs` (`build_from_stream()` method) and the main handler in `src/main.rs` (`handle_connection()`) have no read timeout configured. The `BufReader::read_line()` and `read_exact()` calls will block indefinitely if data is not received, or if a client connects and transmits data very slowly.

**Desired State**: Each incoming `TcpStream` should have a read timeout applied before parsing begins. If no complete HTTP request (headers + body) is received within a configurable timeout window (default 30 seconds), the connection should be closed gracefully. The timeout should be:
1. Configurable via environment variable (`RCOMM_REQUEST_TIMEOUT` in seconds, default 30)
2. Applied at the socket level via `TcpStream::set_read_timeout()`
3. Properly handled with appropriate error responses or silent closure
4. Tested with both normal requests and stalled clients

## Files to Modify

1. **`src/main.rs`** — Primary changes
   - Extract timeout value from environment variable with fallback default
   - Apply timeout to each accepted `TcpStream` before handing off to handler
   - Pass timeout duration to `handle_connection()` for potential logging/debugging
   - Handle timeout errors gracefully in error response path

2. **`src/models/http_request.rs`** — Secondary changes
   - Update `HttpParseError` enum to include a new `ReadTimeout` variant
   - Update `Display` impl for the error enum to handle timeout messages
   - Optionally document that timeout errors can occur from socket-level reads
   - No internal changes to parsing logic needed (timeout is enforced by socket)

3. **`Cargo.toml`** — Minimal changes
   - No new dependencies required (using `std::time::Duration`)

4. **`src/bin/integration_test.rs`** — Testing
   - Add integration tests for timeout behavior
   - Verify slow/stalled clients are disconnected
   - Verify normal requests are unaffected

## Step-by-Step Implementation

### Step 1: Add Timeout Error Variant to HttpParseError

**File**: `src/models/http_request.rs`

Extend the `HttpParseError` enum (lines 11–17) to include timeout:

**Current code**:
```rust
#[derive(Debug)]
pub enum HttpParseError {
    HeaderTooLong,
    MissingHostHeader,
    MalformedRequestLine,
    IoError(std::io::Error),
}
```

**Replace with**:
```rust
#[derive(Debug)]
pub enum HttpParseError {
    HeaderTooLong,
    MissingHostHeader,
    MalformedRequestLine,
    IoError(std::io::Error),
    ReadTimeout,
}
```

Then update the `Display` impl (lines 19–28) to handle the new variant:

**Current code**:
```rust
impl fmt::Display for HttpParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HttpParseError::HeaderTooLong => write!(f, "Header line exceeds maximum length"),
            HttpParseError::MissingHostHeader => write!(f, "Missing required Host header"),
            HttpParseError::MalformedRequestLine => write!(f, "Malformed request line"),
            HttpParseError::IoError(e) => write!(f, "IO error: {e}"),
        }
    }
}
```

**Replace with**:
```rust
impl fmt::Display for HttpParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HttpParseError::HeaderTooLong => write!(f, "Header line exceeds maximum length"),
            HttpParseError::MissingHostHeader => write!(f, "Missing required Host header"),
            HttpParseError::MalformedRequestLine => write!(f, "Malformed request line"),
            HttpParseError::IoError(e) => write!(f, "IO error: {e}"),
            HttpParseError::ReadTimeout => write!(f, "Request timeout: no data received within timeout window"),
        }
    }
}
```

**Rationale**:
- Distinguishes timeout errors from other IO errors for logging/metrics
- Allows handlers to respond differently if needed (e.g., silent close vs. 408 response)
- Clear error message for debugging and monitoring

### Step 2: Add Helper Function to Main for Timeout Configuration

**File**: `src/main.rs`

Add a new function after `get_address()` (around line 20) to fetch and parse the timeout:

```rust
fn get_request_timeout_secs() -> u64 {
    std::env::var("RCOMM_REQUEST_TIMEOUT")
        .unwrap_or_else(|_| String::from("30"))
        .parse::<u64>()
        .unwrap_or(30)
}
```

**Rationale**:
- Consistent with existing `get_port()` and `get_address()` pattern
- Defaults to 30 seconds (reasonable balance for typical HTTP requests)
- Gracefully falls back on parse errors
- Easy for operators to override

**Add after line 20 in main.rs**:
```rust
fn get_request_timeout_secs() -> u64 {
    std::env::var("RCOMM_REQUEST_TIMEOUT")
        .unwrap_or_else(|_| String::from("30"))
        .parse::<u64>()
        .unwrap_or(30)
}
```

### Step 3: Apply Timeout in main() Function

**File**: `src/main.rs`

Modify the `main()` function to capture and use the timeout:

**Current code (lines 22–44)**:
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

**Replace with**:
```rust
fn main() {
    let port = get_port();
    let address = get_address();
    let full_address = format!("{address}:{port}");
    let listener = TcpListener::bind(&full_address).unwrap();

    let pool = ThreadPool::new(4);

    let path = Path::new("./pages");
    let routes = build_routes(String::from(""), path);

    let timeout_secs = get_request_timeout_secs();
    let timeout = std::time::Duration::from_secs(timeout_secs);

    println!("Routes:\n{routes:#?}\n\n");
    println!("Listening on {full_address}");
    println!("Request timeout: {timeout_secs}s");

    for stream in listener.incoming() {
        let routes_clone = routes.clone();
        let mut stream = stream.unwrap();

        // Apply read timeout to prevent slowloris attacks
        if let Err(e) = stream.set_read_timeout(Some(timeout)) {
            eprintln!("Failed to set read timeout: {e}");
            continue;
        }

        pool.execute(move || {
            handle_connection(stream, routes_clone);
        });
    }
}
```

**Key changes**:
- Read timeout configuration from environment
- Print timeout value on startup for visibility
- Apply `set_read_timeout()` to each accepted stream
- Log errors if timeout cannot be set (unlikely but defensive)
- Make stream mutable to allow `set_read_timeout()` call

### Step 4: Handle Timeout Errors in handle_connection()

**File**: `src/main.rs`

Modify the `handle_connection()` function to detect and handle timeout errors:

**Current code (lines 46–57)**:
```rust
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
```

**Replace with**:
```rust
fn handle_connection(mut stream: TcpStream, routes: HashMap<String, PathBuf>) {
    let http_request = match HttpRequest::build_from_stream(&stream) {
        Ok(req) => req,
        Err(e) => {
            // Distinguish timeout from other errors
            match &e {
                HttpParseError::ReadTimeout => {
                    // Log but don't send 408 response (too risky with slow client)
                    eprintln!("Request timeout: connection closed without response");
                    return;
                }
                _ => {
                    // Send 400 Bad Request for other parse errors
                    eprintln!("Bad request: {e}");
                    let mut response = HttpResponse::build(String::from("HTTP/1.1"), 400);
                    let body = format!("Bad Request: {e}");
                    response.add_body(body.into());
                    let _ = stream.write_all(&response.as_bytes());
                    return;
                }
            }
        }
    };
```

**Rationale**:
- Timeout errors are handled specially: no response is sent, connection is simply closed
- This prevents the server from engaging with potentially hostile slow clients
- Other parse errors still return 400, as they indicate client-side issues that can be fixed
- Silent closure of timeout connections prevents slow-write attacks (client ignoring response)

### Step 5: Map IO Errors to Timeout in build_from_stream()

**File**: `src/models/http_request.rs`

Update the `build_from_stream()` method to detect timeout errors from underlying IO:

**Around lines 51–107**, the method uses `buf_reader.read_line()` and `buf_reader.read_exact()`. These return `std::io::Error` which wraps timeout information. We need to map timeout errors properly.

Add this helper method before or after `build_from_stream()` (around line 50):

```rust
fn map_io_error_to_parse_error(err: std::io::Error) -> HttpParseError {
    use std::io::ErrorKind;

    match err.kind() {
        ErrorKind::TimedOut | ErrorKind::WouldBlock => HttpParseError::ReadTimeout,
        _ => HttpParseError::IoError(err),
    }
}
```

Then update the error handling in `build_from_stream()` to use this mapper:

**Current code (line 56)**:
```rust
buf_reader.read_line(&mut line).map_err(HttpParseError::IoError)?;
```

**Replace with**:
```rust
buf_reader.read_line(&mut line).map_err(map_io_error_to_parse_error)?;
```

**Current code (line 73)**:
```rust
let len = buf_reader.read_line(&mut header_line).map_err(HttpParseError::IoError)?;
```

**Replace with**:
```rust
let len = buf_reader.read_line(&mut header_line).map_err(map_io_error_to_parse_error)?;
```

**Current code (line 100)**:
```rust
buf_reader.read_exact(&mut body_buf).map_err(HttpParseError::IoError)?;
```

**Replace with**:
```rust
buf_reader.read_exact(&mut body_buf).map_err(map_io_error_to_parse_error)?;
```

**Rationale**:
- On Unix-like systems (including WSL2), timeout manifests as `ErrorKind::TimedOut`
- On some platforms, non-blocking reads may return `WouldBlock`
- Centralizes timeout detection logic in one place
- Other IO errors still reported as generic IO errors

### Step 6: Add Unit Tests for Timeout Error Handling

**File**: `src/models/http_request.rs`

Add a new test after the existing tests (after line 407):

```rust
#[test]
fn http_parse_error_display_includes_timeout() {
    let err = HttpParseError::ReadTimeout;
    let msg = format!("{err}");
    assert!(msg.contains("timeout"));
}

#[test]
fn build_from_stream_with_timeout_returns_timeout_error() {
    use std::io::Write;
    use std::net::TcpListener;
    use std::time::Duration;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    // Create a client that connects but never sends data
    let handle = std::thread::spawn(move || {
        let mut client = std::net::TcpStream::connect(addr).unwrap();
        // Set a very short timeout on our end for quick test
        client.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
        // Sleep without sending anything
        std::thread::sleep(Duration::from_secs(1));
    });

    let (stream, _) = listener.accept().unwrap();
    // Apply short timeout
    stream.set_read_timeout(Some(Duration::from_millis(100))).unwrap();

    let result = HttpRequest::build_from_stream(&stream);

    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), HttpParseError::ReadTimeout));

    handle.join().unwrap();
}
```

**Rationale**:
- First test verifies error message is descriptive
- Second test validates that actual timeout conditions are detected and reported correctly
- Uses very short timeout (100ms) for quick test execution
- Simulates real-world slow-client scenario

### Step 7: Add Integration Tests for Timeout Behavior

**File**: `src/bin/integration_test.rs`

Add integration tests for timeout scenarios. Add these functions before the `main()` function (around line 150):

```rust
fn test_request_timeout_slow_headers(addr: &str) -> Result<(), String> {
    use std::net::TcpStream;
    use std::io::Write;
    use std::time::Duration;
    use std::thread;

    let mut stream = TcpStream::connect(addr)
        .map_err(|e| format!("Failed to connect: {e}"))?;

    // Send incomplete request line, then wait longer than timeout
    stream.write_all(b"GET / HTTP")
        .map_err(|e| format!("Failed to write: {e}"))?;
    stream.flush()
        .map_err(|e| format!("Failed to flush: {e}"))?;

    // Wait longer than the timeout (default 30 seconds)
    // For testing, we may have shortened this; adjust as needed
    // For now, wait 35 seconds (assumes timeout is 30s)
    thread::sleep(Duration::from_secs(35));

    // Try to read response; should get nothing (connection closed)
    let mut buf = [0u8; 1024];
    match stream.read(&mut buf) {
        Ok(0) => {
            // Connection was closed by server
            Ok(())
        }
        Ok(n) => {
            // Got a response when we shouldn't have
            Err(format!("Expected connection close, got {} bytes", n))
        }
        Err(e) => {
            // Expected error (connection reset or timeout)
            if e.kind() == std::io::ErrorKind::ConnectionReset
                || e.kind() == std::io::ErrorKind::TimedOut {
                Ok(())
            } else {
                Err(format!("Unexpected error: {e}"))
            }
        }
    }
}

fn test_normal_request_not_affected_by_timeout(addr: &str) -> Result<(), String> {
    // Verify that normal requests complete successfully even with timeout configured
    let resp = send_request(addr, "GET", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    assert!(resp.body.len() > 0, "body should not be empty");
    Ok(())
}

fn test_request_timeout_slow_body(addr: &str) -> Result<(), String> {
    use std::net::TcpStream;
    use std::io::Write;
    use std::time::Duration;
    use std::thread;

    let mut stream = TcpStream::connect(addr)
        .map_err(|e| format!("Failed to connect: {e}"))?;

    let body = b"x".repeat(1000);
    let msg = format!(
        "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );

    // Send headers with Content-Length promising 1000 bytes
    stream.write_all(msg.as_bytes())
        .map_err(|e| format!("Failed to write: {e}"))?;
    stream.flush()
        .map_err(|e| format!("Failed to flush: {e}"))?;

    // Send only partial body, then stall
    stream.write_all(&body[0..100])
        .map_err(|e| format!("Failed to write body: {e}"))?;
    stream.flush()
        .map_err(|e| format!("Failed to flush body: {e}"))?;

    // Wait longer than timeout
    thread::sleep(Duration::from_secs(35));

    // Connection should be closed
    let mut buf = [0u8; 1024];
    match stream.read(&mut buf) {
        Ok(0) => Ok(()), // Expected: connection closed
        Ok(n) => Err(format!("Expected connection close, got {} bytes", n)),
        Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset
            || e.kind() == std::io::ErrorKind::TimedOut => Ok(()),
        Err(e) => Err(format!("Unexpected error: {e}")),
    }
}
```

Add imports at the top of the file if needed:
```rust
use std::io::Read;  // If not already present
```

Register these tests in the `main()` function's test runner (around line 300+):

```rust
results.push(run_test("Normal request not affected by timeout", || test_normal_request_not_affected_by_timeout(&addr)));
// Note: The slow client tests will add 30+ seconds to test suite
// Commented out for CI/CD; run manually when testing timeout behavior
// results.push(run_test("Request timeout - slow headers", || test_request_timeout_slow_headers(&addr)));
// results.push(run_test("Request timeout - slow body", || test_request_timeout_slow_body(&addr)));
```

**Important Note**: The slow client tests take 30+ seconds each because they must wait longer than the timeout window. These should be marked as manual/long-running tests and possibly skipped in CI/CD pipelines:

```rust
#[ignore]  // Run manually with: cargo run --bin integration_test -- --ignored
fn test_request_timeout_slow_headers(addr: &str) -> Result<(), String> {
    // ...
}
```

## Testing Strategy

### Unit Tests

1. **Test `HttpParseError::ReadTimeout` variant**:
   - Verify the enum variant exists
   - Verify Display impl produces a sensible error message
   - Command: `cargo test models::http_request::tests::http_parse_error_display_includes_timeout`

2. **Test timeout detection in `build_from_stream()`**:
   - Create a TcpStream with a very short read timeout
   - Connect a client that never sends data
   - Verify `ReadTimeout` error is returned
   - Command: `cargo test models::http_request::tests::build_from_stream_with_timeout_returns_timeout_error`

3. **Run all existing unit tests to ensure no regression**:
   - Command: `cargo test models::http_request`
   - Expected: All existing tests + 2 new tests pass

### Integration Tests

1. **Normal requests are unaffected** (required):
   - Send a valid GET request
   - Verify 200 response and body are received
   - Confirms timeout does not break happy path
   - Relatively quick (< 1 second)

2. **Slow header transmission** (manual/optional):
   - Connect and send partial request line
   - Wait longer than timeout window
   - Verify connection is closed by server
   - Takes 30+ seconds per test

3. **Slow body transmission** (manual/optional):
   - Send valid headers with Content-Length
   - Send only partial body
   - Wait longer than timeout window
   - Verify connection is closed
   - Takes 30+ seconds per test

### Manual Testing

```bash
# Start server with custom timeout (10 seconds for faster testing)
RCOMM_REQUEST_TIMEOUT=10 cargo run &
sleep 2

# Test 1: Normal request (should complete immediately)
curl -v http://127.0.0.1:7878/
# Expected: 200 OK with body

# Test 2: Slow client simulation
(echo -n "GET / HTTP"; sleep 12) | nc localhost 7878
# Expected: connection closes after ~10 seconds, no response

# Test 3: Timeout message in logs
tail -f server logs
# Expected: "Request timeout: connection closed without response" messages

# Cleanup
pkill -f "cargo run"
```

### Automated Integration Test Execution

```bash
# Quick test (only normal requests)
cargo run --bin integration_test

# Manual/slow timeout tests (takes 60+ seconds)
cargo test --test integration_test -- --ignored --nocapture
```

## Edge Cases

### 1. **Timeout During Request Line**
- **Scenario**: Client sends "GET / HTTP" but never completes the line with version number, then stalls
- **Expected Behavior**: After timeout window expires, connection is closed, no response sent
- **Implementation**: `read_line()` call in line 56 hits timeout, `map_io_error_to_parse_error()` converts to `ReadTimeout`
- **Test**: Covered by `test_request_timeout_slow_headers()` integration test
- **Risk**: None—this is the primary attack vector (slowloris)

### 2. **Timeout During Header Parsing**
- **Scenario**: Client sends complete request line, some headers, then stalls with incomplete last header
- **Expected Behavior**: Connection closed after timeout
- **Implementation**: Loop in lines 71–88 times out on the `read_line()` call that would read the incomplete header
- **Test**: Part of `test_request_timeout_slow_headers()`
- **Risk**: Low—normal clients send all headers quickly; only intentional slow clients trigger this

### 3. **Timeout During Body Reception**
- **Scenario**: POST request with Content-Length specified, client sends headers but only partial body, then stalls
- **Expected Behavior**: Connection closed during `read_exact()` call at line 100
- **Implementation**: `read_exact()` blocks waiting for full body; timeout fires, error is mapped to `ReadTimeout`
- **Test**: `test_request_timeout_slow_body()` integration test
- **Risk**: Medium—legitimate large file uploads could trigger false positives with very short timeouts. Default 30s should be safe.

### 4. **System Clock Adjustments**
- **Scenario**: System clock is adjusted backward during a slow request
- **Expected Behavior**: Timeout duration is relative, not absolute; clock adjustments should not cause spurious timeouts
- **Implementation**: `std::time::Duration` is relative; OS timer is based on monotonic clock (CLOCK_MONOTONIC on Linux)
- **Test**: Difficult to test without specialized test harness; documented as OS-specific behavior
- **Risk**: Very low in practice; most systems don't adjust clock backward

### 5. **Connection Established but Read Never Starts**
- **Scenario**: TcpStream is accepted but client never sends any bytes (e.g., hangs in ESTABLISHED state)
- **Expected Behavior**: First `read_line()` call will timeout
- **Implementation**: Timeout is applied to socket before any reading occurs (step 3 in main loop), so first read hits timeout immediately if no data arrives
- **Test**: `test_request_timeout_slow_headers()` starts with connect but no data
- **Risk**: None—this is expected behavior, prevents resource exhaustion

### 6. **Legitimate Slow Networks**
- **Scenario**: Client on high-latency network (100+ ms RTT) or slow uplink sends all data but takes > timeout to complete
- **Expected Behavior**: If timeout is too short (< 1-2 seconds), legitimate requests may be dropped
- **Implementation**: Default 30 seconds is conservative; should handle most legitimate slow networks
- **Test**: Manual testing with intentional latency (e.g., `tc` command on Linux)
- **Risk**: Low with default 30s; mitigation is to increase `RCOMM_REQUEST_TIMEOUT` for high-latency deployments

### 7. **Timeout Firing During `write_all()` in Error Handler**
- **Scenario**: Parser times out; error handler tries to send 400 response, but `write_all()` to a slow client also times out
- **Expected Behavior**: Write error is logged, connection is closed anyway
- **Implementation**: Error handler catches timeout errors and doesn't send 400 response, so this scenario doesn't occur. For other errors, `let _ = stream.write_all()` silently ignores write failures
- **Test**: Implicit in `test_request_timeout_slow_headers()` (no 400 response sent)
- **Risk**: Low—timeout errors don't trigger write attempts

### 8. **Multiple Connections Under Timeout**
- **Scenario**: Many slow clients connect simultaneously
- **Expected Behavior**: Each one times out independently; thread pool continues to process legitimate requests
- **Implementation**: Each TcpStream gets its own timeout via `set_read_timeout(Some(timeout))`. Timeout is enforced at socket level (OS kernel), so multiple timeouts don't interfere
- **Test**: Manual stress test with many slow clients (e.g., using `ab -n 100 -c 50 --timeout 60`)
- **Risk**: Low—timeouts are independent per connection

### 9. **Timeout Less Than 1 Second**
- **Scenario**: Operator sets `RCOMM_REQUEST_TIMEOUT=0` or very small value
- **Expected Behavior**: Even normal requests may timeout if they take slightly longer
- **Implementation**: No validation of timeout value; OS enforces the duration
- **Test**: Manual test with `RCOMM_REQUEST_TIMEOUT=1` and normal request
- **Risk**: Medium—bad configuration can break service. Mitigation: document default and why 30s is chosen
- **Mitigation**: Could add validation to warn if timeout < 5 seconds

### 10. **Timeout on Platform Without Timeout Support**
- **Scenario**: Embedded OS or unusual platform doesn't support socket timeouts
- **Expected Behavior**: `set_read_timeout()` returns error; server logs and skips that connection
- **Implementation**: Already handled in main loop: `if let Err(e) = stream.set_read_timeout()` logs and continues
- **Test**: Not testable without such a platform
- **Risk**: Very low—all modern OS support socket timeouts (Linux, macOS, Windows, etc.)

## Checklist

- [ ] Add `ReadTimeout` variant to `HttpParseError` enum
- [ ] Update `Display` impl for `HttpParseError` to handle timeout
- [ ] Add `get_request_timeout_secs()` function to main.rs
- [ ] Modify `main()` function to read and apply timeout to each TcpStream
- [ ] Add timeout detection to error handling in `handle_connection()`
- [ ] Add `map_io_error_to_parse_error()` helper function to http_request.rs
- [ ] Update `build_from_stream()` to use new error mapper (3 locations: request line, headers, body)
- [ ] Add 2 unit tests for timeout error handling in http_request.rs
- [ ] Add 1-3 integration tests in integration_test.rs (with 1 required quick test)
- [ ] Run `cargo test` (verify no regressions in existing unit tests)
- [ ] Run `cargo test models::http_request::tests` (verify new timeout tests pass)
- [ ] Run `cargo run --bin integration_test` (verify quick integration tests pass)
- [ ] Manual testing with slow clients (using sleep + echo/nc)
- [ ] Manual testing with `RCOMM_REQUEST_TIMEOUT` override
- [ ] Verify timeout messages appear in server output
- [ ] Test with various timeout values (10s, 60s, 3600s)
- [ ] Verify normal requests complete even with timeout configured

## Success Criteria

1. **All Tests Pass**:
   - `cargo test` runs all unit tests with no failures
   - New timeout tests specifically pass (2 new unit tests + at least 1 quick integration test)
   - Existing tests continue to pass (no regression)

2. **Timeout Detection Works**:
   - `HttpParseError::ReadTimeout` variant is correctly returned when socket times out
   - Error message is clear and mentions "timeout"
   - Timeout errors are distinguishable from other IO errors in logs

3. **Connection Handling**:
   - Slow clients (stalling during headers or body) are disconnected after timeout window expires
   - No response is sent to timed-out connections (prevents slow-write attacks)
   - Legitimate normal requests complete successfully within timeout window

4. **Configuration**:
   - Server reads `RCOMM_REQUEST_TIMEOUT` environment variable
   - Falls back to 30 seconds if not set or invalid
   - Timeout value is printed on startup for visibility
   - Timeout can be customized per deployment

5. **Logging**:
   - Timeout events are logged with clear messages
   - Non-timeout parse errors still return 400 responses
   - No spurious errors in logs for normal requests

6. **Manual Testing**:
   - `curl http://localhost:7878/` completes successfully
   - `(echo -n "GET / HTTP"; sleep 35) | nc localhost 7878` results in connection close (no response)
   - Server logs show "Request timeout: connection closed without response" for slow clients
   - Changing `RCOMM_REQUEST_TIMEOUT` takes effect without recompilation

## Implementation Difficulty: 4/10

**Rationale**:
- Reading timeout configuration is straightforward (similar to existing port/address)
- Applying socket timeout is one method call: `set_read_timeout()`
- Error mapping is simple pattern matching
- One new error variant and Display case
- Timeout tests require careful timing but are straightforward
- No new dependencies required
- Moderate complexity in integration tests (thread coordination, timing)

## Risk Assessment: Low-Medium

**Backward Compatibility**:
- No breaking changes to public API
- Default timeout (30s) is conservative and should not affect normal requests
- Timeout is transparent to normal clients

**Correctness**:
- Timeout errors are correctly detected via OS kernel
- Error handling is defensive (silent close for timeouts, 400 for other errors)
- All read operations in parse flow are covered

**Performance**:
- Negligible overhead (one environment variable read, one socket option set per connection)
- Improves server resilience by freeing worker threads from slow clients

**Security**:
- Directly mitigates slowloris and similar DoS attacks
- Default 30s timeout is suitable for normal usage
- Operators can tune for their network conditions

**Testing**:
- Normal requests unaffected (required test)
- Timeout behavior can be tested with very short timeouts for speed
- Integration tests may take time (slow client simulation) but can be marked manual/ignored

**Known Risks**:
- Very short timeouts (< 5s) may cause false positives on slow networks
- Mitigation: Default 30s, documentation, operators can override
- Tests with real slow clients require 30+ seconds per test
- Mitigation: Mark slow tests as ignored, run manually or in dedicated CI step

