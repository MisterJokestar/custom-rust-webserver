# Feature: Add Configurable Request Timeouts

**Category:** Configuration
**Complexity:** 4/10
**Necessity:** 6/10

---

## Overview

The server currently has no timeouts on TCP connections. A slow or malicious client can hold a worker thread indefinitely by:
- Sending headers very slowly (Slowloris attack)
- Sending a body byte-by-byte
- Opening a connection and never sending anything

This feature adds configurable timeouts for:
1. **Read timeout** — Maximum time to wait for incoming data on a connection
2. **Write timeout** — Maximum time to wait when sending the response

These timeouts are set at the socket level using `TcpStream::set_read_timeout()` and `TcpStream::set_write_timeout()`, which are stdlib APIs requiring no external dependencies.

---

## Files to Modify

1. **`src/main.rs`** — Set timeouts on accepted connections, add config helpers
2. **`src/models/http_request.rs`** — Handle `WouldBlock` / `TimedOut` IO errors gracefully during parsing

---

## Step-by-Step Implementation

### Step 1: Add Timeout Configuration Helpers

**File:** `src/main.rs`

```rust
use std::time::Duration;

const DEFAULT_READ_TIMEOUT_SECS: u64 = 30;
const DEFAULT_WRITE_TIMEOUT_SECS: u64 = 30;

fn get_read_timeout() -> Duration {
    let secs = std::env::var("RCOMM_READ_TIMEOUT")
        .ok()
        .and_then(|val| val.parse::<u64>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(DEFAULT_READ_TIMEOUT_SECS);
    Duration::from_secs(secs)
}

fn get_write_timeout() -> Duration {
    let secs = std::env::var("RCOMM_WRITE_TIMEOUT")
        .ok()
        .and_then(|val| val.parse::<u64>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(DEFAULT_WRITE_TIMEOUT_SECS);
    Duration::from_secs(secs)
}
```

### Step 2: Set Timeouts on Accepted Connections

**File:** `src/main.rs`, in the `main()` listener loop

**Current:**
```rust
for stream in listener.incoming() {
    let routes_clone = routes.clone();
    let stream = stream.unwrap();

    pool.execute(move || {
        handle_connection(stream, routes_clone);
    });
}
```

**New:**
```rust
let read_timeout = get_read_timeout();
let write_timeout = get_write_timeout();

for stream in listener.incoming() {
    let routes_clone = routes.clone();
    let stream = stream.unwrap();

    // Set timeouts before dispatching to worker
    if let Err(e) = stream.set_read_timeout(Some(read_timeout)) {
        eprintln!("Failed to set read timeout: {e}");
    }
    if let Err(e) = stream.set_write_timeout(Some(write_timeout)) {
        eprintln!("Failed to set write timeout: {e}");
    }

    pool.execute(move || {
        handle_connection(stream, routes_clone);
    });
}
```

Timeouts are set on the main thread before dispatching to workers. This is safe because the stream hasn't been used yet, and `TcpStream` is `Send`.

### Step 3: Handle Timeout Errors in Request Parsing

**File:** `src/models/http_request.rs`

When `set_read_timeout()` is active and a read exceeds the timeout, `BufReader::read_line()` returns `io::Error` with kind `WouldBlock` or `TimedOut`. The existing `HttpParseError::IoError` variant already wraps IO errors, so this is handled automatically.

However, it's useful to add a dedicated error variant for clearer logging:

```rust
#[derive(Debug)]
pub enum HttpParseError {
    HeaderTooLong,
    MissingHostHeader,
    MalformedRequestLine,
    ReadTimeout,  // NEW
    IoError(std::io::Error),
}
```

Update the IO error mapping to detect timeouts:

```rust
fn map_io_error(e: std::io::Error) -> HttpParseError {
    match e.kind() {
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => {
            HttpParseError::ReadTimeout
        }
        _ => HttpParseError::IoError(e),
    }
}
```

Then replace `.map_err(HttpParseError::IoError)` calls with `.map_err(map_io_error)`:

```rust
buf_reader.read_line(&mut line).map_err(map_io_error)?;
```

### Step 4: Return Appropriate HTTP Status for Timeouts

**File:** `src/main.rs`, in `handle_connection()`

Update the error handler to return `408 Request Timeout` for timeout errors:

```rust
Err(e) => {
    let status_code = match &e {
        HttpParseError::ReadTimeout => 408,
        _ => 400,
    };
    eprintln!("Request error: {e}");
    let mut response = HttpResponse::build(String::from("HTTP/1.1"), status_code);
    let body = format!("{e}");
    response.add_body(body.into());
    let _ = stream.write_all(&response.as_bytes());
    return;
}
```

### Step 5: Add 408 to Status Code Mapping

**File:** `src/models/http_status_codes.rs`

Add `408` to the `get_status_phrase()` function:

```rust
408 => "Request Timeout",
```

### Step 6: Print Timeout Configuration on Startup

**File:** `src/main.rs`

```rust
println!("Listening on {full_address}");
println!("Read timeout: {}s", read_timeout.as_secs());
println!("Write timeout: {}s", write_timeout.as_secs());
```

---

## Environment Variables

| Variable | Type | Default | Description |
|----------|------|---------|-------------|
| `RCOMM_READ_TIMEOUT` | `u64` (seconds) | `30` | Max time to wait for client data |
| `RCOMM_WRITE_TIMEOUT` | `u64` (seconds) | `30` | Max time to wait when sending response |

---

## Edge Cases & Handling

### 1. Timeout During Header Parsing
If the client sends partial headers and stalls, the read timeout fires, `read_line()` returns a `TimedOut` error, and the server responds with `408 Request Timeout`.

### 2. Timeout During Body Read
Same behavior — `read_exact()` times out, returning `408`.

### 3. Write Timeout During Response
If the client is slow to receive the response, `stream.write_all()` returns an IO error. The existing `let _ = stream.write_all(...)` in the error path silently drops this. In the success path (`handle_connection` line 74), the `unwrap()` would panic. This should be changed to `let _ =` or proper error handling (related to the Error Handling features).

### 4. Timeout of 0
Filtered out by `.filter(|&n| n > 0)`, falling back to the default. Setting `Duration::from_secs(0)` would be interpreted as "no timeout" by the OS, which is the opposite of intended behavior.

### 5. Very Large Timeouts
`RCOMM_READ_TIMEOUT=86400` (24 hours) is valid but defeats the purpose. No upper bound is enforced — operators are trusted.

### 6. Persistent Connections (Future)
When keep-alive is implemented, a separate idle timeout between requests will be needed. The read timeout set here applies per-read-call, not per-connection, so it naturally works for keep-alive scenarios where the server waits for the next request.

---

## Testing Strategy

### Unit Tests

```rust
#[test]
fn map_io_error_detects_timeout() {
    let err = std::io::Error::new(std::io::ErrorKind::TimedOut, "timed out");
    let parsed = map_io_error(err);
    assert!(matches!(parsed, HttpParseError::ReadTimeout));
}

#[test]
fn map_io_error_passes_through_other_errors() {
    let err = std::io::Error::new(std::io::ErrorKind::ConnectionReset, "reset");
    let parsed = map_io_error(err);
    assert!(matches!(parsed, HttpParseError::IoError(_)));
}
```

### Integration Tests

```rust
fn test_request_timeout() -> TestResult {
    // 1. Start server with RCOMM_READ_TIMEOUT=2
    // 2. Open TCP connection but send nothing
    // 3. Wait for response (should get 408 after ~2 seconds)
    // 4. Verify response contains "408"
}

fn test_slow_headers_timeout() -> TestResult {
    // 1. Start server with RCOMM_READ_TIMEOUT=2
    // 2. Send partial request line, then stall
    // 3. Verify 408 response after timeout
}
```

### Manual Testing

```bash
# Start with short timeout
RCOMM_READ_TIMEOUT=5 cargo run

# In another terminal, open connection and stall
nc localhost 7878
# Wait 5 seconds, observe "408 Request Timeout" response

# Normal requests still work
curl http://localhost:7878/
# Observe: normal 200 response
```

---

## Implementation Checklist

- [ ] Add `DEFAULT_READ_TIMEOUT_SECS` and `DEFAULT_WRITE_TIMEOUT_SECS` constants
- [ ] Add `get_read_timeout()` and `get_write_timeout()` helpers
- [ ] Set `set_read_timeout()` and `set_write_timeout()` on accepted streams
- [ ] Add `HttpParseError::ReadTimeout` variant
- [ ] Add `map_io_error()` function to detect timeout errors
- [ ] Update `build_from_stream()` to use `map_io_error()`
- [ ] Return HTTP 408 for `ReadTimeout` errors in `handle_connection()`
- [ ] Add `408 => "Request Timeout"` to status code mapping
- [ ] Print timeout config on startup
- [ ] Add unit tests for `map_io_error()`
- [ ] Add integration tests for timeout behavior
- [ ] Run `cargo build` and `cargo test`
- [ ] Run `cargo run --bin integration_test`
- [ ] Manual test with `nc` or slow client

---

## Backward Compatibility

**Behavioral change:** Previously, connections with no timeout could hang worker threads indefinitely. With this feature, connections that stall for longer than the configured timeout (default: 30 seconds) receive a `408 Request Timeout` response and are closed. This is a security improvement that could theoretically affect very slow legitimate clients on poor networks.

Environment variables are new and optional. When unset, the 30-second default is reasonable for nearly all use cases.
