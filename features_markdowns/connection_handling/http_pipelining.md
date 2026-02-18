# HTTP/1.1 Pipelining Implementation Plan

## Overview

HTTP/1.1 pipelining allows a client to send multiple requests on a single TCP connection without waiting for each response. The server processes them in order and sends responses back in the same order. Currently, `handle_connection()` in `src/main.rs` reads exactly one request, sends one response, and the connection is dropped when the function returns.

This feature changes `handle_connection()` into a loop that processes multiple sequential requests on the same connection, implementing the foundation of HTTP/1.1 persistent connections and pipelining.

**Complexity**: 7
**Necessity**: 3

**Key Changes**:
- Wrap the request/response logic in `handle_connection()` in a loop
- Detect connection close conditions (client disconnect, `Connection: close` header, errors)
- Add a configurable maximum requests-per-connection limit to prevent abuse
- Handle edge cases: partial reads, malformed follow-up requests, timeouts between requests

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Current State**:
- Line 46-74: `handle_connection()` processes exactly one request then returns
- No loop, no connection reuse
- No `Connection` header handling
- Stream is implicitly closed when function scope ends

**Changes Required**:
- Wrap the request parse → route → respond logic in a loop
- Check `Connection: close` header to break the loop
- Break on parse errors (client disconnect, malformed request)
- Add optional max-requests-per-connection counter

### 2. `/home/jwall/personal/rusty/rcomm/src/models/http_request.rs`

**Current State**:
- `build_from_stream()` reads from the stream once
- Returns `Result<HttpRequest, HttpParseError>`
- An EOF or broken pipe during parsing returns an error

**Changes Required**:
- Ensure `build_from_stream()` correctly handles the case where a stream that previously delivered a valid request now returns EOF (client closed). This should be distinguishable from a malformed request.
- May need a new error variant like `HttpParseError::ConnectionClosed` or similar

### 3. `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`

**Changes Required**:
- Add tests that send multiple requests on a single TCP connection
- Verify each response is correct and in order
- Test `Connection: close` header terminates the loop
- Test that malformed second request doesn't crash the server

---

## Step-by-Step Implementation

### Step 1: Add Connection-Close Detection Helper

**Location**: `src/main.rs`, before `handle_connection()`

```rust
fn should_close_connection(request: &HttpRequest) -> bool {
    match request.headers.get("connection") {
        Some(value) => value.eq_ignore_ascii_case("close"),
        None => {
            // HTTP/1.1 defaults to keep-alive; HTTP/1.0 defaults to close
            request.version != "HTTP/1.1"
        }
    }
}
```

### Step 2: Distinguish EOF from Parse Error

**Location**: `src/models/http_request.rs`

Add a way to distinguish "connection closed cleanly" from "malformed request". Check the current `HttpParseError` variants and ensure that reading zero bytes (EOF) produces a distinguishable error. If the current parser reads the request line and gets an empty string or zero-length read, this should map to a "connection closed" condition.

If needed, add a variant:
```rust
pub enum HttpParseError {
    // ... existing variants ...
    ConnectionClosed,
}
```

### Step 3: Convert `handle_connection()` to a Loop

**Location**: `src/main.rs`, line 46

**Current**:
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
    // ... route matching, response ...
    stream.write_all(&response.as_bytes()).unwrap();
}
```

**New**:
```rust
fn handle_connection(mut stream: TcpStream, routes: HashMap<String, PathBuf>) {
    let max_requests = 100; // Prevent abuse on a single connection

    for _request_num in 0..max_requests {
        let http_request = match HttpRequest::build_from_stream(&stream) {
            Ok(req) => req,
            Err(HttpParseError::ConnectionClosed) => {
                // Client closed the connection cleanly
                break;
            }
            Err(e) => {
                eprintln!("Bad request: {e}");
                let mut response = HttpResponse::build(String::from("HTTP/1.1"), 400);
                let body = format!("Bad Request: {e}");
                response.add_body(body.into());
                let _ = stream.write_all(&response.as_bytes());
                break; // Close connection on parse error
            }
        };

        let close_after = should_close_connection(&http_request);

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

        if close_after {
            response.add_header("Connection".to_string(), "close".to_string());
        }

        println!("Response: {response}");
        if stream.write_all(&response.as_bytes()).is_err() {
            break; // Client disconnected during write
        }

        if close_after {
            break;
        }
    }
}
```

### Step 4: Add Integration Tests

```rust
fn test_pipelining_multiple_requests(addr: &str) -> Result<(), String> {
    let mut stream = TcpStream::connect(addr)
        .map_err(|e| format!("Connect failed: {e}"))?;

    // Send first request
    stream.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .map_err(|e| format!("Write failed: {e}"))?;

    // Read first response (need to parse Content-Length to know when it ends)
    let resp1 = read_response(&mut stream)?;
    assert_eq_or_err(&resp1.status_code, &200, "first request status")?;

    // Send second request on same connection
    stream.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .map_err(|e| format!("Write failed: {e}"))?;

    let resp2 = read_response(&mut stream)?;
    assert_eq_or_err(&resp2.status_code, &200, "second request status")?;

    Ok(())
}

fn test_connection_close_header(addr: &str) -> Result<(), String> {
    let mut stream = TcpStream::connect(addr)
        .map_err(|e| format!("Connect failed: {e}"))?;

    // Send request with Connection: close
    stream.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .map_err(|e| format!("Write failed: {e}"))?;

    let resp = read_response(&mut stream)?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;

    // Server should have closed the connection — next read should return 0 bytes
    let mut buf = [0u8; 1];
    match stream.read(&mut buf) {
        Ok(0) | Err(_) => Ok(()),
        Ok(_) => Err("Expected connection to be closed after Connection: close".to_string()),
    }
}
```

---

## Edge Cases & Handling

### 1. Client Sends Partial Request Then Disconnects
- `build_from_stream()` returns an error (EOF mid-parse)
- The loop breaks; connection is cleaned up
- No server crash

### 2. Client Sends Malformed Second Request
- First request succeeds; second request parse fails
- Send 400 Bad Request, then break out of the loop
- First response was already delivered successfully

### 3. Max Requests Per Connection Reached
- After 100 requests (configurable), the loop exits naturally
- Prevents a single client from monopolizing a worker thread indefinitely

### 4. Write Failure Mid-Response
- `stream.write_all()` returns `Err` — client disconnected
- Loop breaks; no panic (currently the code uses `.unwrap()` which would panic)

### 5. HTTP/1.0 Clients
- HTTP/1.0 defaults to `Connection: close` unless `Connection: keep-alive` is explicitly sent
- `should_close_connection()` handles this by checking the HTTP version

### 6. Interaction with Request Timeouts (Future Feature)
- When request timeouts are added later, the idle timeout between requests on a persistent connection should be shorter than the initial request timeout
- The loop structure accommodates this naturally — set a read timeout before each `build_from_stream()` call

---

## Dependencies & Ordering

This feature has a strong dependency on:
- **Arc Route Sharing** — Without `Arc`, the cloned `HashMap` is held for the entire duration of the persistent connection (potentially many requests), increasing memory pressure. Implement Arc sharing first.

This feature is a prerequisite for:
- **HTTP/1.1 persistent connections** (FEATURES.md: HTTP Protocol Compliance section) — Pipelining is a superset of keep-alive behavior
- **Limit requests per persistent connection** (FEATURES.md: Security section)

---

## Implementation Checklist

- [ ] Add `ConnectionClosed` variant to `HttpParseError` (or equivalent EOF detection)
- [ ] Update `build_from_stream()` to return `ConnectionClosed` on clean EOF
- [ ] Add `should_close_connection()` helper function
- [ ] Convert `handle_connection()` body into a loop
- [ ] Add max-requests-per-connection counter
- [ ] Handle write errors without panicking (replace `.unwrap()` with `.is_err()` check)
- [ ] Add `Connection: close` response header when closing
- [ ] Add integration test: multiple requests on one connection
- [ ] Add integration test: `Connection: close` terminates connection
- [ ] Add integration test: malformed second request doesn't crash server
- [ ] Run `cargo test` and `cargo run --bin integration_test`

---

## Backward Compatibility

Existing single-request clients (including all current integration tests) will continue to work. They open a connection, send one request, read one response, and close. The server loop will detect the client close (EOF) and exit cleanly. No existing behavior changes.
