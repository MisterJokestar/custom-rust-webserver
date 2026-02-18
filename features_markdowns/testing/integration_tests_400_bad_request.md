# Integration Tests for 400 Bad Request Responses

**Category:** Testing
**Complexity:** 2/10
**Necessity:** 7/10
**Status:** Planning

---

## Overview

Add integration tests to `src/bin/integration_test.rs` that verify the server correctly returns `400 Bad Request` for malformed HTTP requests. The server already handles parse failures in `handle_connection()` (src/main.rs, lines 47-56), returning 400 with a descriptive body. However, there are no integration tests validating this behavior end-to-end over a real TCP connection.

**Goal:** Ensure the server reliably rejects malformed requests without crashing, and that the 400 response is well-formed.

---

## Current State

### Server Behavior (src/main.rs, lines 46-56)

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
    // ...
}
```

The parsing logic in `HttpRequest::build_from_stream()` rejects requests with:
- Missing or malformed request line (no method, no path, no version)
- Unrecognized HTTP methods (lowercase methods, garbage strings)
- Missing headers (no `Host` header for HTTP/1.1 if enforced)

### Existing Integration Test Infrastructure

The test framework in `src/bin/integration_test.rs` uses:
- `send_request()` — sends well-formed requests via `TcpStream`
- `read_response()` — parses responses into `TestResponse { status_code, status_phrase, headers, body }`
- `run_test()` — wraps test functions returning `Result<(), String>`

For malformed request tests, we need to send **raw bytes** directly to the TCP stream rather than using `send_request()`, since `send_request()` only builds well-formed requests.

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`

**Add:** Raw request helper function and 400 Bad Request test functions.
**Modify:** `main()` function to register new tests.

---

## Step-by-Step Implementation

### Step 1: Add a Raw Request Helper

Add a helper function that sends arbitrary bytes and reads the response:

```rust
fn send_raw_request(addr: &str, raw: &[u8]) -> Result<TestResponse, String> {
    let mut stream = TcpStream::connect(addr).map_err(|e| format!("connect: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| format!("set timeout: {e}"))?;
    stream
        .write_all(raw)
        .map_err(|e| format!("write: {e}"))?;
    read_response(&mut stream)
}
```

**Rationale:** `send_request()` builds valid HTTP/1.1 requests. To test malformed inputs, we need to control every byte sent.

### Step 2: Add Test Functions

```rust
fn test_malformed_request_line(addr: &str) -> Result<(), String> {
    // Completely invalid request line — no method, path, or version
    let resp = send_raw_request(addr, b"GARBAGE\r\n\r\n")?;
    assert_eq_or_err(&resp.status_code, &400, "status")?;
    Ok(())
}

fn test_missing_http_version(addr: &str) -> Result<(), String> {
    // Request line with method and path but no HTTP version
    let resp = send_raw_request(addr, b"GET /\r\nHost: localhost\r\n\r\n")?;
    assert_eq_or_err(&resp.status_code, &400, "status")?;
    Ok(())
}

fn test_empty_request_line(addr: &str) -> Result<(), String> {
    // Just headers, no request line
    let resp = send_raw_request(addr, b"\r\nHost: localhost\r\n\r\n")?;
    assert_eq_or_err(&resp.status_code, &400, "status")?;
    Ok(())
}

fn test_unknown_http_method(addr: &str) -> Result<(), String> {
    // Valid format but unrecognized method
    let resp = send_raw_request(addr, b"BREW / HTTP/1.1\r\nHost: localhost\r\n\r\n")?;
    assert_eq_or_err(&resp.status_code, &400, "status")?;
    Ok(())
}

fn test_lowercase_method_rejected(addr: &str) -> Result<(), String> {
    // The parser only accepts uppercase methods
    let resp = send_raw_request(addr, b"get / HTTP/1.1\r\nHost: localhost\r\n\r\n")?;
    assert_eq_or_err(&resp.status_code, &400, "status")?;
    Ok(())
}

fn test_400_has_body(addr: &str) -> Result<(), String> {
    // Verify the 400 response includes a descriptive body
    let resp = send_raw_request(addr, b"GARBAGE\r\n\r\n")?;
    assert_eq_or_err(&resp.status_code, &400, "status")?;
    assert_contains_or_err(&resp.body, "Bad Request", "body")?;
    Ok(())
}

fn test_server_survives_bad_request(addr: &str) -> Result<(), String> {
    // Send a malformed request, then verify the server still handles valid ones
    let _ = send_raw_request(addr, b"GARBAGE\r\n\r\n");
    let resp = send_request(addr, "GET", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "status after bad request")?;
    Ok(())
}
```

### Step 3: Register Tests in `main()`

Add to the `results` vector in `main()`:

```rust
run_test("malformed_request_line_400", || test_malformed_request_line(&addr)),
run_test("missing_http_version_400", || test_missing_http_version(&addr)),
run_test("empty_request_line_400", || test_empty_request_line(&addr)),
run_test("unknown_http_method_400", || test_unknown_http_method(&addr)),
run_test("lowercase_method_rejected_400", || test_lowercase_method_rejected(&addr)),
run_test("400_has_body", || test_400_has_body(&addr)),
run_test("server_survives_bad_request", || test_server_survives_bad_request(&addr)),
```

---

## Edge Cases & Considerations

### 1. Connection Close After 400

**Scenario:** The server sends a 400 and closes the connection.

**Expected:** The response is fully readable before the connection drops. The `read_response()` helper should handle this since it reads by `Content-Length`.

### 2. Server Worker Thread Survival

**Scenario:** A malformed request hits one worker; subsequent requests should still be served.

**Test:** `test_server_survives_bad_request` verifies this by sending a garbage request followed by a valid GET.

### 3. Partial Writes / Slow Clients

**Scenario:** A slow client sends only part of the request before the timeout.

**Consideration:** The server currently has no read timeout, so this may hang. This is a separate feature (request timeout), not addressed here.

### 4. Binary Garbage

**Scenario:** Random non-ASCII bytes sent to the server.

**Expected:** The parser fails gracefully and returns 400. Not tested here due to potential encoding issues in `read_response()`, but the `GARBAGE` test covers the general case.

---

## Testing Strategy

### Running the Tests

```bash
cargo build && cargo run --bin integration_test
```

### Expected Results

All new tests should show `[PASS]`. The existing 10 tests should remain unaffected.

### Manual Verification

```bash
cargo run &
# Malformed request
echo -ne "GARBAGE\r\n\r\n" | nc 127.0.0.1 7878
# Expected: HTTP/1.1 400 Bad Request

# Unknown method
echo -ne "BREW / HTTP/1.1\r\nHost: localhost\r\n\r\n" | nc 127.0.0.1 7878
# Expected: HTTP/1.1 400 Bad Request
```

---

## Implementation Checklist

- [ ] Add `send_raw_request()` helper function
- [ ] Add `test_malformed_request_line()` test
- [ ] Add `test_missing_http_version()` test
- [ ] Add `test_empty_request_line()` test
- [ ] Add `test_unknown_http_method()` test
- [ ] Add `test_lowercase_method_rejected()` test
- [ ] Add `test_400_has_body()` test
- [ ] Add `test_server_survives_bad_request()` test
- [ ] Register all tests in `main()`
- [ ] Run `cargo build` — no compiler errors
- [ ] Run `cargo run --bin integration_test` — all tests pass

---

## Dependencies

- **No new external dependencies**
- Requires the existing `read_response()` and `assert_eq_or_err()` / `assert_contains_or_err()` helpers
- Does not depend on any other feature being implemented first

---

## References

- [RFC 7230 Section 3.1.1 - Request Line](https://tools.ietf.org/html/rfc7230#section-3.1.1)
- [MDN: HTTP 400 Bad Request](https://developer.mozilla.org/en-US/docs/Web/HTTP/Status/400)
