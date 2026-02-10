# Implementation Plan: Configurable Maximum Number of Request Headers

## Overview

Currently, the HTTP request parser in `src/models/http_request.rs` accepts an unlimited number of headers in the header parsing loop (lines 71-88). While individual header lines are limited to `MAX_HEADER_LINE_LEN = 8192` bytes, there is no limit on the *count* of headers, creating a potential denial-of-service (DoS) attack vector. A client could send thousands of small headers to exhaust server memory or processing time.

This feature implements:
1. A configurable maximum header count limit (default: 100 headers)
2. A new error variant `TooManyHeaders` for when this limit is exceeded
3. Environment variable configuration (`RCOMM_MAX_HEADERS`)
4. HTTP 431 response when limit is violated (following RFC 9110 semantics)

**Rationale:**
- **Security:** Mitigates slowloris-style attacks and header bombing
- **Resource protection:** Prevents unbounded memory consumption from header HashMap
- **Configurability:** Allows operators to tune for their environment (e.g., 50 for strict APIs, 500 for proxies)
- **Standards compliance:** 431 is appropriate for header-related resource limits
- **Observability:** Separate error tracking for header count violations

**Scope:** Minimal, localized to header parsing with server-side configuration integration.

---

## Files to Modify

1. **`src/models/http_request.rs`**
   - Add `MAX_HEADERS` const (or make configurable via function parameter)
   - Add `TooManyHeaders` variant to `HttpParseError` enum
   - Update `build_from_stream()` to track header count and return error when exceeded
   - Add unit test for the new limit

2. **`src/main.rs`**
   - Read `RCOMM_MAX_HEADERS` environment variable (default: 100)
   - Pass max headers limit to `HttpRequest::build_from_stream()` or thread-pool context
   - Update `handle_connection()` to detect the new error variant and return 431
   - Log header count violations

3. **`src/lib.rs`** (optional)
   - If exposing the limit as a config constant, re-export from models module

---

## Step-by-Step Implementation

### Step 1: Add Configuration & Error Variant

**File:** `/home/jwall/personal/rusty/rcomm/src/models/http_request.rs`

**Changes:** Add a new constant and error variant to support configurable header count limits.

**Current code (lines 9-28):**
```rust
const MAX_HEADER_LINE_LEN: usize = 8192;

#[derive(Debug)]
pub enum HttpParseError {
    HeaderTooLong,
    MissingHostHeader,
    MalformedRequestLine,
    IoError(std::io::Error),
}

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

**New code:**
```rust
const MAX_HEADER_LINE_LEN: usize = 8192;
pub const DEFAULT_MAX_HEADERS: usize = 100;

#[derive(Debug)]
pub enum HttpParseError {
    HeaderTooLong,
    TooManyHeaders,
    MissingHostHeader,
    MalformedRequestLine,
    IoError(std::io::Error),
}

impl fmt::Display for HttpParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HttpParseError::HeaderTooLong => write!(f, "Header line exceeds maximum length"),
            HttpParseError::TooManyHeaders => write!(f, "Too many headers in request"),
            HttpParseError::MissingHostHeader => write!(f, "Missing required Host header"),
            HttpParseError::MalformedRequestLine => write!(f, "Malformed request line"),
            HttpParseError::IoError(e) => write!(f, "IO error: {e}"),
        }
    }
}
```

**Rationale:**
- `DEFAULT_MAX_HEADERS = 100` is a reasonable default (most requests have 10-20 headers)
- Making it public allows main.rs to read environment variables and pass to builder
- `TooManyHeaders` distinguishes from individual header size violations
- Placed after `HeaderTooLong` to minimize existing test breakage

---

### Step 2: Refactor `build_from_stream()` Signature

**File:** `/home/jwall/personal/rusty/rcomm/src/models/http_request.rs`

**Decision:** Pass `max_headers` as a parameter to `build_from_stream()` for flexibility.

**Current code (line 51):**
```rust
pub fn build_from_stream(stream: &TcpStream) -> Result<HttpRequest, HttpParseError> {
```

**New code:**
```rust
pub fn build_from_stream(stream: &TcpStream, max_headers: usize) -> Result<HttpRequest, HttpParseError> {
```

**Rationale:**
- Allows server to pass configured limit without global state
- Testable with different limits
- Non-breaking for callers (they'll need to pass the param—see Step 5)

---

### Step 3: Implement Header Count Validation in Parsing Loop

**File:** `/home/jwall/personal/rusty/rcomm/src/models/http_request.rs`

**Current code (lines 70-88):**
```rust
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
```

**New code:**
```rust
// Parse headers
let mut header_count = 0;
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

    // Check header count before parsing
    if header_count >= max_headers {
        return Err(HttpParseError::TooManyHeaders);
    }

    let Some((title, value)) = header_line.split_once(":") else { break; };
    request.add_header(title.to_string(), value.trim().to_string());
    header_count += 1;
}
```

**Rationale:**
- Counter incremented only after a valid header is parsed (not for empty lines or malformed)
- Check happens *before* adding header to ensure we stay within limit
- Early return when limit exceeded saves processing time

---

### Step 4: Update Existing Unit Tests

**File:** `/home/jwall/personal/rusty/rcomm/src/models/http_request.rs`

**Change:** All existing calls to `build_from_stream()` must pass `max_headers` parameter.

**Example current test (lines 257-279):**
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
    let req = HttpRequest::build_from_stream(&stream).unwrap();

    assert_eq!(req.method.to_string(), "GET");
    assert_eq!(req.target, "/hello");
    assert_eq!(req.version, "HTTP/1.1");
    handle.join().unwrap();
}
```

**Updated code:**
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
    let req = HttpRequest::build_from_stream(&stream, DEFAULT_MAX_HEADERS).unwrap();

    assert_eq!(req.method.to_string(), "GET");
    assert_eq!(req.target, "/hello");
    assert_eq!(req.version, "HTTP/1.1");
    handle.join().unwrap();
}
```

**Apply same change to all 6 tests using `build_from_stream()`:**
- `build_from_stream_parses_get_request()` (line 273)
- `build_from_stream_parses_post_with_body()` (line 302)
- `build_from_stream_trims_header_ows()` (line 327)
- `build_from_stream_handles_bare_lf()` (line 352)
- `build_from_stream_rejects_oversized_header()` (line 377)
- `build_from_stream_rejects_http11_without_host()` (line 401)

**Pattern:** Replace `build_from_stream(&stream)` → `build_from_stream(&stream, DEFAULT_MAX_HEADERS)`

---

### Step 5: Add Unit Test for Header Count Limit

**File:** `/home/jwall/personal/rusty/rcomm/src/models/http_request.rs`

**Add new test at end of tests module (after line 406):**
```rust
#[test]
fn build_from_stream_rejects_too_many_headers() {
    use std::io::Write;
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let max_headers = 5; // Set a low limit for testing
    let handle = std::thread::spawn(move || {
        let mut client = std::net::TcpStream::connect(addr).unwrap();
        // Send request line + 6 headers (exceeds limit of 5)
        let request = "GET / HTTP/1.1\r\n\
                       Host: localhost\r\n\
                       X-Header-1: value1\r\n\
                       X-Header-2: value2\r\n\
                       X-Header-3: value3\r\n\
                       X-Header-4: value4\r\n\
                       X-Header-5: value5\r\n\
                       X-Header-6: value6\r\n\
                       \r\n";
        client.write_all(request.as_bytes()).unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
    });

    let (stream, _) = listener.accept().unwrap();
    let result = HttpRequest::build_from_stream(&stream, max_headers);

    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), HttpParseError::TooManyHeaders));
    handle.join().unwrap();
}

#[test]
fn build_from_stream_accepts_headers_within_limit() {
    use std::io::Write;
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let max_headers = 5;
    let handle = std::thread::spawn(move || {
        let mut client = std::net::TcpStream::connect(addr).unwrap();
        // Send request line + exactly 5 headers (within limit)
        let request = "GET / HTTP/1.1\r\n\
                       Host: localhost\r\n\
                       X-Header-1: value1\r\n\
                       X-Header-2: value2\r\n\
                       X-Header-3: value3\r\n\
                       X-Header-4: value4\r\n\
                       X-Header-5: value5\r\n\
                       \r\n";
        client.write_all(request.as_bytes()).unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
    });

    let (stream, _) = listener.accept().unwrap();
    let req = HttpRequest::build_from_stream(&stream, max_headers).unwrap();

    assert_eq!(req.method.to_string(), "GET");
    assert_eq!(req.target, "/");
    handle.join().unwrap();
}
```

**Rationale:**
- Tests boundary condition (at limit)
- Tests rejection condition (exceeds limit)
- Uses low limit for clarity
- Validates both success and failure paths

---

### Step 6: Update Server Configuration & Error Handling

**File:** `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Step 6a: Read configuration at startup (after line 20, in get_address function area)**

Add a new function to read max headers from environment:
```rust
fn get_max_headers() -> usize {
    std::env::var("RCOMM_MAX_HEADERS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(rcomm::models::http_request::DEFAULT_MAX_HEADERS)
}
```

**Step 6b: Update main function (line 22-43)**

**Current code:**
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

**New code:**
```rust
fn main() {
    let port = get_port();
    let address = get_address();
    let max_headers = get_max_headers();
    let full_address = format!("{address}:{port}");
    let listener = TcpListener::bind(&full_address).unwrap();

    let pool = ThreadPool::new(4);

    let path = Path::new("./pages");
    let routes = build_routes(String::from(""), path);

    println!("Routes:\n{routes:#?}\n\n");
    println!("Listening on {full_address} (max headers: {max_headers})");

    for stream in listener.incoming() {
        let routes_clone = routes.clone();
        let stream = stream.unwrap();

        pool.execute(move || {
            handle_connection(stream, routes_clone, max_headers);
        });
    }
}
```

**Rationale:**
- Reads limit at startup (not per-request for efficiency)
- Printed for operator visibility
- Passed to each connection handler

---

### Step 7: Update Connection Handler Signature & Error Handling

**File:** `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Current code (lines 46-57):**
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
    // ... rest of function
}
```

**New code:**
```rust
fn handle_connection(mut stream: TcpStream, routes: HashMap<String, PathBuf>, max_headers: usize) {
    let http_request = match HttpRequest::build_from_stream(&stream, max_headers) {
        Ok(req) => req,
        Err(e) => {
            use rcomm::models::http_request::HttpParseError;

            let (status_code, error_msg) = match e {
                HttpParseError::TooManyHeaders => {
                    eprintln!("Too many headers in request");
                    (431, String::from("Request Header Fields Too Large"))
                },
                _ => {
                    eprintln!("Bad request: {e}");
                    (400, format!("Bad Request: {e}"))
                }
            };

            let mut response = HttpResponse::build(String::from("HTTP/1.1"), status_code);
            response.add_body(error_msg.into());
            let _ = stream.write_all(&response.as_bytes());
            return;
        }
    };
    // ... rest of function unchanged
}
```

**Rationale:**
- Parameter added to match new `build_from_stream()` signature
- 431 returned for TooManyHeaders (same as oversized single header)
- Other errors continue to return 400
- Distinct logging helps identify header count DoS attempts

---

### Step 8: Integration Test (Optional but Recommended)

**File:** `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`

Add test function (check existing test pattern around line 200+):
```rust
fn test_too_many_headers_returns_431() -> TestResult {
    // Send request with many headers (must craft manually to test)
    // This requires sending raw HTTP over TCP

    let mut headers = String::from("GET / HTTP/1.1\r\nHost: localhost\r\n");
    for i in 0..150 {
        headers.push_str(&format!("X-Header-{}: value{}\r\n", i, i));
    }
    headers.push_str("\r\n");

    let request = headers.as_bytes();
    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", SERVER_PORT))
        .map_err(|e| format!("Connection failed: {e}"))?;

    stream.write_all(request)
        .map_err(|e| format!("Write failed: {e}"))?;

    let response = read_response(&mut stream)?;

    if response.status_code == 431 {
        TestResult::Pass
    } else {
        TestResult::Fail(format!(
            "Expected 431 for too many headers, got: {}",
            response.status_code
        ))
    }
}
```

Add to test suite in `main()`:
```rust
run_test("too_many_headers_returns_431", test_too_many_headers_returns_431);
```

**Note:** The exact integration test syntax depends on existing test framework in `src/bin/integration_test.rs`—adapt as needed.

---

## Testing Strategy

### Unit Tests (Compile-Time)

1. **Existing tests:** All 6 `build_from_stream` tests (lines 257-406)
   - Update all calls to pass `DEFAULT_MAX_HEADERS` parameter
   - No logic changes needed; same behavior with default limit of 100

2. **New test 1:** `build_from_stream_rejects_too_many_headers()`
   - Sends 6 headers with limit of 5
   - Verifies `HttpParseError::TooManyHeaders` is returned
   - No request should be created

3. **New test 2:** `build_from_stream_accepts_headers_within_limit()`
   - Sends exactly 5 headers with limit of 5
   - Verifies request is successfully parsed
   - Validates at-limit boundary

### Integration Tests (Runtime)

4. **New test:** `test_too_many_headers_returns_431()`
   - Starts real server with default limit (100)
   - Sends request with 150 headers
   - Verifies HTTP 431 response
   - Validates error message in response body

### Manual Testing

5. **Test with environment override:**
   ```bash
   RCOMM_MAX_HEADERS=10 cargo run &
   sleep 1

   python3 -c "
   import socket
   s = socket.socket()
   s.connect(('127.0.0.1', 7878))
   request = 'GET / HTTP/1.1\r\nHost: localhost\r\n'
   for i in range(15):
       request += f'X-Header-{i}: value\r\n'
   request += '\r\n'
   s.send(request.encode())
   print(s.recv(4096).decode())
   s.close()
   "
   ```

   Expected output: First line should be `HTTP/1.1 431 Request Header Fields Too Large`

6. **Test boundary at limit:**
   ```bash
   RCOMM_MAX_HEADERS=5 cargo run &

   python3 -c "
   import socket
   s = socket.socket()
   s.connect(('127.0.0.1', 7878))
   request = 'GET / HTTP/1.1\r\nHost: localhost\r\n'
   for i in range(5):
       request += f'X-Header-{i}: value\r\n'
   request += '\r\n'
   s.send(request.encode())
   resp = s.recv(4096).decode()
   print('Status:', resp.split('\r\n')[0])
   s.close()
   "
   ```

   Expected output: `Status: HTTP/1.1 200 OK`

7. **Test just over limit:**
   ```bash
   RCOMM_MAX_HEADERS=5 cargo run &

   python3 -c "
   import socket
   s = socket.socket()
   s.connect(('127.0.0.1', 7878))
   request = 'GET / HTTP/1.1\r\nHost: localhost\r\n'
   for i in range(6):
       request += f'X-Header-{i}: value\r\n'
   request += '\r\n'
   s.send(request.encode())
   print(s.recv(4096).decode())
   s.close()
   "
   ```

   Expected output: First line should be `HTTP/1.1 431`

8. **Verify normal requests still work:**
   ```bash
   RCOMM_MAX_HEADERS=100 cargo run &
   sleep 1
   curl -v http://127.0.0.1:7878/
   ```

   Expected: 200 OK with homepage content

---

## Edge Cases & Considerations

### Edge Case 1: Header Count vs. Total Header Size

**Current implementation:** Validates individual header line size (8192 bytes) AND counts headers

**Alternative not implemented:** Total cumulative headers size limit (more complex, RFC doesn't mandate)

**Note:** Most HTTP/1.1 servers (nginx, Apache, Tomcat) use both per-line and per-count limits. This implementation follows that pattern.

---

### Edge Case 2: Empty Header Lines & Malformed Headers

**Current behavior in loop:**
- `if header_line.is_empty()` → break (exits loop, no counter increment)
- `if split_once(":")` fails → break (exits loop, no counter increment)

**Expected:** Only *valid* header name-value pairs count toward limit

**Validation:** Unit test sends 5 valid headers + some invalid → only valid ones count

---

### Edge Case 3: Zero or Negative Max Headers

**Current environment parsing:**
```rust
fn get_max_headers() -> usize {
    std::env::var("RCOMM_MAX_HEADERS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())  // usize is unsigned, can't be negative
        .unwrap_or(DEFAULT_MAX_HEADERS)
}
```

**Expected behavior:**
- `RCOMM_MAX_HEADERS=0` → Rejects all requests with at least 1 header (invalid, will 431 on any real request)
- `RCOMM_MAX_HEADERS=abc` → Falls back to DEFAULT (100)
- `RCOMM_MAX_HEADERS=-5` → Falls back to DEFAULT (unsigned, parse fails)

**Mitigation:** Optional validation step to enforce `max_headers >= 1` in `main()`:
```rust
if max_headers == 0 {
    eprintln!("Warning: RCOMM_MAX_HEADERS=0 will reject all requests. Using default.");
    max_headers = DEFAULT_MAX_HEADERS;
}
```

---

### Edge Case 4: Header Continuation Lines (RFC 7230)

**Current implementation:** Does not support continuation/folding of header values across multiple lines

**Example malformed continuation:**
```
X-Long-Header: value part 1
 value part 2
```

**Expected:** RFC 7230 deprecated continuation (obsolete-fold), so no support needed. Second line would trigger `if header_line.is_empty()` check due to leading space, causing loop exit.

**No change needed:** Feature works correctly with deprecated syntax.

---

### Edge Case 5: Host Header Validation Timing

**Current flow (lines 90-93 in http_request.rs):**
```rust
// Validate Host header for HTTP/1.1 (after all headers parsed)
if request.version == "HTTP/1.1" && !request.headers.contains_key("host") {
    return Err(HttpParseError::MissingHostHeader);
}
```

**With this feature:** If Host is the 101st header, loop exits at TooManyHeaders *before* Host validation.

**Expected behavior:** TooManyHeaders should take precedence (correct—size limits checked before semantic requirements)

**No change:** Ordering is sound.

---

### Edge Case 6: POST Body Parsing After Header Limit Hit

**Current flow:** Header parsing happens before body parsing (lines 71-104)

**With this feature:** If header limit exceeded, early return prevents body parsing

**Expected:** Correct—malformed/oversized header section means we can't trust Content-Length anyway

**No change:** Behavior is secure.

---

### Edge Case 7: Extreme Default Limit

**Current default:** 100 headers

**Real-world headers:**
- Minimal request: ~5 headers (Method, Target, Version, Host, User-Agent)
- Browser request: ~15-30 headers
- Complex request with cookies/auth: ~40-50 headers
- Malicious request: 1000+ headers

**Justification for 100:**
- Covers all legitimate use cases
- Still allows modern client complexity
- Prevents naive header bombing
- Can be lowered for strict APIs

**If operators need higher:** `RCOMM_MAX_HEADERS=1000 cargo run` is straightforward

---

## Implementation Checklist

- [ ] **Step 1:** Add `DEFAULT_MAX_HEADERS` const and `TooManyHeaders` enum variant to `http_request.rs`
- [ ] **Step 1b:** Update `Display` impl for `HttpParseError` with new variant message
- [ ] **Step 2:** Change `build_from_stream()` signature to accept `max_headers: usize` parameter
- [ ] **Step 3:** Implement header count validation in parsing loop with early return
- [ ] **Step 4:** Update all 6 existing unit tests to pass `DEFAULT_MAX_HEADERS` to `build_from_stream()`
- [ ] **Step 5a:** Add `test_build_from_stream_rejects_too_many_headers()` unit test
- [ ] **Step 5b:** Add `test_build_from_stream_accepts_headers_within_limit()` unit test
- [ ] **Step 6a:** Add `get_max_headers()` function to `main.rs`
- [ ] **Step 6b:** Update `main()` to read max_headers and pass to pool/threads
- [ ] **Step 7:** Update `handle_connection()` signature to accept `max_headers` parameter
- [ ] **Step 7b:** Update error matching to handle `TooManyHeaders` → 431 response
- [ ] **Step 8:** (Optional) Add integration test `test_too_many_headers_returns_431()`
- [ ] **Run unit tests:** `cargo test` — expect 36+ tests passing (8 new tests added)
- [ ] **Run integration tests:** `cargo run --bin integration_test` — expect 13+ tests passing
- [ ] **Manual test 1:** Verify boundary at limit with low `RCOMM_MAX_HEADERS`
- [ ] **Manual test 2:** Verify 431 response just over limit
- [ ] **Manual test 3:** Verify default limit (100) allows normal requests
- [ ] **Manual test 4:** Verify environment variable override works

---

## Code Review Focus Areas

1. **Parameter threading:** Is `max_headers` passed correctly through main → pool → thread → handle_connection?
2. **Header counting logic:** Does counter increment only on valid headers? Does it check limit *before* adding?
3. **Error variant placement:** Is `TooManyHeaders` in the right position in enum (minimal test breakage)?
4. **HTTP status code:** Is 431 the correct response for header count violations? (Yes, per RFC 9110 Section 15.5.3)
5. **Environment parsing:** Does `get_max_headers()` handle invalid input gracefully?
6. **Test coverage:** Are boundary conditions tested (at limit, over limit, under limit)?
7. **Backward compatibility:** Do existing tests pass with updated signature?
8. **Performance:** Is header counting O(n) only once per request? (Yes, in loop)

---

## Performance Impact

**Expected:** Negligible

- **Per-request overhead:** One integer comparison per header (usize >= usize)
- **Memory:** One usize variable on stack (8 bytes)
- **Parsing path:** Same as before; no additional allocations
- **Configuration:** Read once at startup, passed by value to threads

**Benchmarking note:** If performance matters, a request with exactly 100 headers should be ~1% slower than before due to added comparison per header. Immeasurable in practice.

---

## Documentation/Communication

### CLAUDE.md Updates

Consider adding to the project CLAUDE.md:

```markdown
## Security Configuration

### RCOMM_MAX_HEADERS
- **Type:** Environment variable (unsigned integer)
- **Default:** 100
- **Example:** `RCOMM_MAX_HEADERS=50 cargo run`
- **Purpose:** Limit the number of HTTP headers per request to prevent header-based DoS attacks
- **Typical range:** 50 (strict API) to 500 (proxy/gateway)
- **Violation response:** HTTP 431 Request Header Fields Too Large
```

### Log Output

Server startup should print:
```
Listening on 127.0.0.1:7878 (max headers: 100)
```

Violation logs should indicate:
```
Too many headers in request
```

---

## References

- RFC 9110 Section 15.5.3: [431 Request Header Fields Too Large](https://www.rfc-editor.org/rfc/rfc9110#section-15.5.3)
- RFC 9110 Section 5.1: [HTTP Request Header Fields Size Limits](https://www.rfc-editor.org/rfc/rfc9110#section-5.1)
- OWASP: [Slowloris Attack](https://owasp.org/www-community/attacks/Slowloris)
- nginx documentation: [client_max_body_size](http://nginx.org/en/docs/http/ngx_http_core_module.html#client_max_body_size) (analogous parameter)
- Apache httpd: [LimitRequestFields](https://httpd.apache.org/docs/current/mod/core.html#limitrequestfields)

---

## Success Criteria

1. **Functional:** Server rejects requests with >N headers, returning HTTP 431
2. **Configurable:** Environment variable `RCOMM_MAX_HEADERS` controls the limit
3. **Tested:** Unit tests cover boundary conditions; integration test validates HTTP response
4. **Backward-compatible:** All existing tests pass with default limit of 100
5. **Observable:** Startup message and error logs indicate configuration and violations
6. **Secure:** Prevents header-based DoS while allowing legitimate high-header-count requests via configuration
