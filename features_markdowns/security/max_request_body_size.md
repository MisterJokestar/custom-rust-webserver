# Feature: Add Configurable Maximum Request Body Size

**Category:** Security
**Complexity:** 2/10
**Necessity:** 9/10

---

## Overview

This feature prevents memory exhaustion attacks by enforcing a configurable maximum request body size. Currently, `HttpRequest::build_from_stream()` reads the entire request body specified by the `Content-Length` header without any limits, making the server vulnerable to denial-of-service (DoS) attacks where attackers send requests with extremely large body sizes.

This feature adds:
1. A configurable maximum request body size limit (default: 10 MB)
2. A new `HttpParseError` variant for oversized bodies
3. Environment variable support for configuration (`RCOMM_MAX_BODY_SIZE`)
4. Graceful rejection of oversized requests with HTTP 413 status code
5. Comprehensive unit and integration tests

---

## Security Impact

**Threat Mitigated:** Memory exhaustion DoS attack where attackers send requests with huge `Content-Length` values, causing the server to allocate unbounded memory and crash.

**Current Vulnerability:**
```rust
// Current code in http_request.rs (lines 96-104)
if let Some(content_length) = request.headers.get("content-length") {
    if let Ok(len) = content_length.parse::<usize>() {
        if len > 0 {
            let mut body_buf = vec![0u8; len];  // VULNERABLE: unbounded allocation
            buf_reader.read_exact(&mut body_buf).map_err(HttpParseError::IoError)?;
            request.add_body(body_buf);
        }
    }
}
```

A client could send `Content-Length: 100000000000` (100 GB) and cause the server to attempt allocating 100 GB of memory.

---

## Files to Modify

1. **`src/models/http_request.rs`** — Add body size validation logic and new error variant
2. **`src/main.rs`** — Obtain max body size from environment and pass to request parsing
3. **`src/models.rs`** (optional) — Re-export any new configuration if extracted to separate module

---

## Step-by-Step Implementation

### Step 1: Add New Error Variant to `HttpParseError`

**File:** `src/models/http_request.rs`

Add a new error variant to the `HttpParseError` enum to represent when a request body exceeds the maximum allowed size:

```rust
#[derive(Debug)]
pub enum HttpParseError {
    HeaderTooLong,
    MissingHostHeader,
    MalformedRequestLine,
    BodyTooLarge,  // NEW
    IoError(std::io::Error),
}
```

Update the `Display` impl to include the new error:

```rust
impl fmt::Display for HttpParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HttpParseError::HeaderTooLong => write!(f, "Header line exceeds maximum length"),
            HttpParseError::MissingHostHeader => write!(f, "Missing required Host header"),
            HttpParseError::MalformedRequestLine => write!(f, "Malformed request line"),
            HttpParseError::BodyTooLarge => write!(f, "Request body exceeds maximum allowed size"),  // NEW
            HttpParseError::IoError(e) => write!(f, "IO error: {e}"),
        }
    }
}
```

### Step 2: Add Max Body Size Constant

**File:** `src/models/http_request.rs`

Add a constant for the default maximum body size at the top of the file, near `MAX_HEADER_LINE_LEN`:

```rust
const MAX_HEADER_LINE_LEN: usize = 8192;
const DEFAULT_MAX_BODY_SIZE: usize = 10 * 1024 * 1024;  // 10 MB default
```

### Step 3: Update `build_from_stream` Signature

**File:** `src/models/http_request.rs`

Modify the `build_from_stream()` method to accept a `max_body_size` parameter:

```rust
pub fn build_from_stream(stream: &TcpStream, max_body_size: usize) -> Result<HttpRequest, HttpParseError> {
    // ... existing header parsing code ...

    // Parse body if Content-Length is present (lines 95-104)
    if let Some(content_length) = request.headers.get("content-length") {
        if let Ok(len) = content_length.parse::<usize>() {
            if len > 0 {
                // NEW: Check if body size exceeds limit
                if len > max_body_size {
                    return Err(HttpParseError::BodyTooLarge);
                }

                let mut body_buf = vec![0u8; len];
                buf_reader.read_exact(&mut body_buf).map_err(HttpParseError::IoError)?;
                request.add_body(body_buf);
            }
        }
    }

    Ok(request)
}
```

**Complete replacement for lines 51-107:**

```rust
pub fn build_from_stream(stream: &TcpStream, max_body_size: usize) -> Result<HttpRequest, HttpParseError> {
    let mut buf_reader = BufReader::new(stream);

    // Parse request line
    let mut line = String::new();
    buf_reader.read_line(&mut line).map_err(HttpParseError::IoError)?;
    let line = line.trim_end_matches(|c| c == '\r' || c == '\n');

    if line.len() > MAX_HEADER_LINE_LEN {
        return Err(HttpParseError::HeaderTooLong);
    }

    let mut iter = line.split_whitespace();
    let method_str = iter.next().ok_or(HttpParseError::MalformedRequestLine)?;
    let method = http_method_from_string(method_str).ok_or(HttpParseError::MalformedRequestLine)?;
    let target = iter.next().ok_or(HttpParseError::MalformedRequestLine)?.to_string();
    let version = iter.next().ok_or(HttpParseError::MalformedRequestLine)?.to_string();
    let mut request = HttpRequest::build(method, target, version);

    // Parse headers
    loop {
        let mut header_line = String::new();
        let len = buf_reader.read_line(&mut header_line).map_err(HttpParseError::IoError)?;
        if len == 0 {
            break;
        }
        let header_line = header_line.trim_end_matches(|c| c == '\r' || c == '\n');

        if header_line.len() > MAX_HEADER_LINE_LEN {
            return Err(HttpParseError::HeaderTooLong);
        }

        if header_line.is_empty() {
            break;
        }
        let Some((title, value)) = header_line.split_once(":") else { break; };
        request.add_header(title.to_string(), value.trim().to_string());
    }

    // Validate Host header for HTTP/1.1
    if request.version == "HTTP/1.1" && !request.headers.contains_key("host") {
        return Err(HttpParseError::MissingHostHeader);
    }

    // Parse body if Content-Length is present
    if let Some(content_length) = request.headers.get("content-length") {
        if let Ok(len) = content_length.parse::<usize>() {
            if len > 0 {
                // Check if body size exceeds maximum allowed
                if len > max_body_size {
                    return Err(HttpParseError::BodyTooLarge);
                }

                let mut body_buf = vec![0u8; len];
                buf_reader.read_exact(&mut body_buf).map_err(HttpParseError::IoError)?;
                request.add_body(body_buf);
            }
        }
    }

    Ok(request)
}
```

### Step 4: Update Unit Tests

**File:** `src/models/http_request.rs`

Update existing tests to pass the `max_body_size` parameter. For example, the `build_from_stream_parses_get_request` test (lines 257-279):

```rust
#[test]
fn build_from_stream_parses_get_request() {
    use std::io::Write;
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = std::thread::spawn(move || {
        let mut client = std::net::TcpStream::connect(addr).unwrap();
        client
            .write_all(b"GET /hello HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
    });

    let (stream, _) = listener.accept().unwrap();
    let req = HttpRequest::build_from_stream(&stream, DEFAULT_MAX_BODY_SIZE).unwrap();

    assert_eq!(req.method.to_string(), "GET");
    assert_eq!(req.target, "/hello");
    assert_eq!(req.version, "HTTP/1.1");
    handle.join().unwrap();
}
```

Similarly, update all other `build_from_stream()` calls in tests to include `DEFAULT_MAX_BODY_SIZE`:
- `build_from_stream_parses_post_with_body()` (line 282)
- `build_from_stream_trims_header_ows()` (line 311)
- `build_from_stream_handles_bare_lf()` (line 335)
- `build_from_stream_rejects_oversized_header()` (line 361)
- `build_from_stream_rejects_http11_without_host()` (line 385)

**Add new test for body size limit:**

```rust
#[test]
fn build_from_stream_rejects_oversized_body() {
    use std::io::Write;
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let max_size = 1000;

    let handle = std::thread::spawn(move || {
        let mut client = std::net::TcpStream::connect(addr).unwrap();
        let msg = format!(
            "POST /upload HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n",
            max_size + 1  // One byte over the limit
        );
        client.write_all(msg.as_bytes()).unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
    });

    let (stream, _) = listener.accept().unwrap();
    let result = HttpRequest::build_from_stream(&stream, max_size);

    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), HttpParseError::BodyTooLarge));
    handle.join().unwrap();
}
```

**Add test for body at exact limit:**

```rust
#[test]
fn build_from_stream_accepts_body_at_limit() {
    use std::io::Write;
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let body_size = 1000;

    let handle = std::thread::spawn(move || {
        let mut client = std::net::TcpStream::connect(addr).unwrap();
        let body = "x".repeat(body_size);
        let msg = format!(
            "POST /upload HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{}",
            body_size,
            body
        );
        client.write_all(msg.as_bytes()).unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
    });

    let (stream, _) = listener.accept().unwrap();
    let result = HttpRequest::build_from_stream(&stream, body_size);

    assert!(result.is_ok());
    let req = result.unwrap();
    assert_eq!(req.try_get_body().unwrap().len(), body_size);
    handle.join().unwrap();
}
```

### Step 5: Add Environment Variable Configuration

**File:** `src/main.rs`

Add a helper function to get the max body size from the environment variable `RCOMM_MAX_BODY_SIZE` with a fallback to the default:

```rust
fn get_max_body_size() -> usize {
    std::env::var("RCOMM_MAX_BODY_SIZE")
        .ok()
        .and_then(|val| val.parse::<usize>().ok())
        .unwrap_or_else(|| rcomm::models::http_request::DEFAULT_MAX_BODY_SIZE)
}
```

**Note:** This requires exporting `DEFAULT_MAX_BODY_SIZE` from the models module.

Alternatively, define the default in `main.rs` to avoid tight coupling:

```rust
const DEFAULT_MAX_BODY_SIZE: usize = 10 * 1024 * 1024;  // 10 MB

fn get_max_body_size() -> usize {
    std::env::var("RCOMM_MAX_BODY_SIZE")
        .ok()
        .and_then(|val| val.parse::<usize>().ok())
        .unwrap_or(DEFAULT_MAX_BODY_SIZE)
}
```

### Step 6: Update `main()` Function

**File:** `src/main.rs`

Modify the `main()` function to obtain the max body size and pass it along in the closure:

```rust
fn main() {
    let port = get_port();
    let address = get_address();
    let max_body_size = get_max_body_size();
    let full_address = format!("{address}:{port}");
    let listener = TcpListener::bind(&full_address).unwrap();

    let pool = ThreadPool::new(4);

    let path = Path::new("./pages");
    let routes = build_routes(String::from(""), path);

    println!("Routes:\n{routes:#?}\n\n");
    println!("Listening on {full_address}");
    println!("Max request body size: {} bytes", max_body_size);

    for stream in listener.incoming() {
        let routes_clone = routes.clone();
        let stream = stream.unwrap();
        let max_body_size = max_body_size;  // Capture for closure

        pool.execute(move || {
            handle_connection(stream, routes_clone, max_body_size);
        });
    }
}
```

### Step 7: Update `handle_connection()` Function

**File:** `src/main.rs`

Modify the function signature to accept `max_body_size` and pass it to `build_from_stream()`:

```rust
fn handle_connection(mut stream: TcpStream, routes: HashMap<String, PathBuf>, max_body_size: usize) {
    let http_request = match HttpRequest::build_from_stream(&stream, max_body_size) {
        Ok(req) => req,
        Err(e) => {
            eprintln!("Bad request: {e}");
            // Send appropriate HTTP status code based on error type
            let status_code = match &e {
                HttpParseError::BodyTooLarge => 413,  // Payload Too Large
                _ => 400,  // Bad Request
            };
            let mut response = HttpResponse::build(String::from("HTTP/1.1"), status_code);
            let body = format!("Bad Request: {e}");
            response.add_body(body.into());
            let _ = stream.write_all(&response.as_bytes());
            return;
        }
    };
    // ... rest of function unchanged ...
}
```

---

## Testing Strategy

### Unit Tests (in `http_request.rs`)

1. **`build_from_stream_rejects_oversized_body`** — Verify rejection when body size exceeds limit
2. **`build_from_stream_accepts_body_at_limit`** — Verify acceptance when body exactly equals limit
3. **All existing tests updated** — Add `max_body_size` parameter to existing test calls

### Integration Tests (in `src/bin/integration_test.rs`)

Add integration tests to verify the server responds correctly:

```rust
fn test_request_body_too_large() -> TestResult {
    let result = run_request_test(|server_url| {
        // Send POST request with body larger than default limit (10 MB)
        // Expect HTTP 413 response
        let body = vec![0u8; 11_000_000];  // 11 MB
        let request = format!(
            "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );

        let mut stream = TcpStream::connect(server_url).unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        stream.write_all(&body).unwrap();
        stream.shutdown(Shutdown::Write).unwrap();

        let mut response = String::new();
        let mut buf_reader = BufReader::new(&stream);
        buf_reader.read_line(&mut response).unwrap();

        response.contains("413")
    });

    match result {
        true => TestResult::Pass,
        false => TestResult::Fail("Expected HTTP 413 response for oversized body".to_string()),
    }
}

fn test_request_body_at_custom_limit() -> TestResult {
    // Verify that RCOMM_MAX_BODY_SIZE environment variable is respected
    // Run server with RCOMM_MAX_BODY_SIZE=5000
    // Send body of 5000 bytes (should pass)
    // Send body of 5001 bytes (should fail with 413)

    TestResult::Pass  // Placeholder
}
```

### Manual Testing

```bash
# Test 1: Body within default limit (10 MB)
curl -X POST http://localhost:7878/ \
  -H "Content-Type: text/plain" \
  -d "$(head -c 1000000 /dev/zero | tr '\0' 'x')"  # 1 MB body
# Expected: 404 (since POST to / has no handler, but no 413)

# Test 2: Body exceeding default limit
curl -X POST http://localhost:7878/ \
  -H "Content-Type: text/plain" \
  -d "$(head -c 11000000 /dev/zero | tr '\0' 'x')"  # 11 MB body
# Expected: 413 Payload Too Large

# Test 3: Custom limit via environment variable
RCOMM_MAX_BODY_SIZE=1000 cargo run
curl -X POST http://localhost:7878/ \
  -H "Content-Type: text/plain" \
  -d "$(head -c 2000 /dev/zero | tr '\0' 'x')"  # 2000 bytes
# Expected: 413 Payload Too Large

# Test 4: Body exactly at limit
RCOMM_MAX_BODY_SIZE=5000 cargo run
curl -X POST http://localhost:7878/ \
  -H "Content-Type: text/plain" \
  -d "$(head -c 5000 /dev/zero | tr '\0' 'x')"  # 5000 bytes
# Expected: 404 (no handler for POST /, but no 413)
```

---

## Edge Cases

### 1. Invalid Content-Length

Current behavior (line 97): `if let Ok(len) = content_length.parse::<usize>()` silently ignores non-numeric Content-Length. This is preserved; the body size check only applies when Content-Length is a valid number.

**Test:** Sending `Content-Length: abc` should result in no body being read (existing behavior maintained).

### 2. Zero-Length Body

Current behavior (line 98): `if len > 0` skips body reading for zero-length bodies. This is preserved.

**Test:** POST with `Content-Length: 0` should be accepted regardless of limit.

### 3. Missing Content-Length

No body is read if Content-Length header is absent. This is preserved.

**Test:** POST without Content-Length header should not trigger the size limit check.

### 4. Negative Content-Length

Negative values cannot be parsed into `usize`, so they're silently ignored (line 97). This is preserved.

### 5. Very Large Content-Length Values

Attempting to allocate e.g. `usize::MAX` bytes would fail in `vec![0u8; len]` with an OOM error. The size limit check prevents this:

```rust
if len > max_body_size {
    return Err(HttpParseError::BodyTooLarge);
}
```

**Test:** Send `Content-Length: 18446744073709551615` (usize::MAX), expect 413 error (not OOM crash).

### 6. Streaming Large Bodies

The current implementation allocates the entire body in memory via `vec![0u8; len]`. If the limit is very large (e.g., 1 GB), this could still cause memory issues. **Note:** This is a limitation of the current architecture, not this feature. A proper fix would require streaming body handling, which is out of scope.

### 7. HTTP/1.0 Requests

HTTP/1.0 requests that include a body should be handled the same way. The size limit applies to all HTTP versions.

**Test:** Send HTTP/1.0 POST with oversized body, expect 413 response.

### 8. Multiple Connections

The thread pool should handle multiple concurrent requests with large bodies. Each thread gets its own `max_body_size` value passed in the closure, so there are no concurrency issues.

---

## Configuration

### Environment Variable

**Name:** `RCOMM_MAX_BODY_SIZE`
**Type:** Unsigned integer (bytes)
**Default:** 10,485,760 bytes (10 MB)
**Example:**

```bash
# Set limit to 5 MB
export RCOMM_MAX_BODY_SIZE=5242880
cargo run

# Set limit to 1 GB
export RCOMM_MAX_BODY_SIZE=1073741824
cargo run
```

### Server Output

When the server starts, it should print the configured limit:

```
Routes:
{...}

Listening on 127.0.0.1:7878
Max request body size: 10485760 bytes
```

---

## HTTP Status Codes

The implementation uses the standard HTTP status code for this scenario:

- **413 Payload Too Large** — Request body exceeds the server's configured maximum size

This is the standard code defined in RFC 7231, replacing the deprecated "Request Entity Too Large" (413).

---

## Backward Compatibility

1. **API Change:** `HttpRequest::build_from_stream()` signature changes to require a `max_body_size` parameter. All callers must be updated.
   - This affects `src/main.rs` (1 call)
   - This affects `src/models/http_request.rs` tests (6 calls)
   - Any external code using rcomm as a library would need updates

2. **Default Behavior:** Requests are rejected with HTTP 413 if they exceed 10 MB (previously accepted). This is a security improvement and unlikely to break legitimate clients.

3. **Environment Variable:** New optional configuration. Existing deployments without `RCOMM_MAX_BODY_SIZE` set will use the sensible default (10 MB).

---

## Code Review Checklist

- [ ] `HttpParseError::BodyTooLarge` variant added
- [ ] Error variant included in `Display` impl
- [ ] `DEFAULT_MAX_BODY_SIZE` constant defined (10 MB)
- [ ] `build_from_stream()` accepts `max_body_size` parameter
- [ ] Body size check performed before allocation: `if len > max_body_size`
- [ ] All 6 existing unit tests updated with max_body_size parameter
- [ ] 2 new unit tests added (oversized body, body at limit)
- [ ] `get_max_body_size()` helper function reads environment variable
- [ ] `main()` calls `get_max_body_size()` and prints configured limit
- [ ] `handle_connection()` accepts max_body_size and passes to build_from_stream()
- [ ] Error handling updated to return 413 for BodyTooLarge errors
- [ ] Integration tests added to verify 413 responses
- [ ] Manual testing performed with curl
- [ ] Documentation updated in CLAUDE.md if needed
- [ ] Commits follow existing style and message conventions

---

## Implementation Notes

1. **No External Dependencies:** This feature uses only stdlib APIs, maintaining rcomm's zero-dependency philosophy.

2. **Thread Safety:** The `max_body_size` value is captured by value in the closure passed to the thread pool, so there are no synchronization concerns.

3. **Performance:** The size check is a single comparison operation (`len > max_body_size`), adding negligible overhead.

4. **Error Messages:** The new error message "Request body exceeds maximum allowed size" is clear and user-friendly, appearing in both error logs and HTTP response bodies.

5. **Testing:** The implementation can be fully tested without external tools using Rust's `TcpListener` test infrastructure (as shown in existing tests).

---

## Future Enhancements (Out of Scope)

1. **Streaming Body Handling:** Implement chunked encoding support and stream body reading to avoid allocating entire bodies in memory.

2. **Configurable Limits per Route:** Allow different size limits for different endpoints (e.g., 1 MB for form submissions, 100 MB for file uploads).

3. **Request Rate Limiting:** Reject clients sending many large requests in a short time.

4. **Metrics/Observability:** Track rejected requests, body sizes, and limit violations for monitoring.

5. **Per-Request Timeout:** Add timeout for slow clients sending partial bodies.
