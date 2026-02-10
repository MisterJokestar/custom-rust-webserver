# 414 URI Too Long Implementation Plan

## 1. Overview of the Feature

The HTTP `414 URI Too Long` status code is a critical HTTP protocol compliance feature that protects the server from abuse and maintains protocol adherence. It indicates that the request URI (the target in the request line) exceeds the server's maximum acceptable length.

Currently, rcomm does not validate the length of the request URI, which means:
1. Clients can send arbitrarily long URIs without receiving a proper error response
2. The server may consume excessive memory parsing extremely long request lines
3. The server is non-compliant with HTTP protocol standards

**Goal**: Implement validation of request URI length during HTTP request parsing. When a URI exceeds the maximum allowed length (8192 bytes, aligned with the existing `MAX_HEADER_LINE_LEN` constant), return a `414 URI Too Long` response.

**Key Standards**:
- RFC 7230 (HTTP/1.1 Message Syntax and Routing) — Defines request line structure
- RFC 7231 (HTTP/1.1 Semantics and Content) — Defines 414 status code
- Common server practice: 8192 bytes is a widely-used limit (Apache, Nginx defaults)

**Impact**:
- HTTP protocol compliance: Server correctly handles excessively long URIs per RFC standards
- Security: Prevents potential denial-of-service attacks using oversized request lines
- Consistency: Aligns with existing header length validation already in place
- Client guidance: Properly informs clients that their request URI is too large

---

## 2. Files to be Modified or Created

### Modified Files

1. **`/home/jwall/personal/rusty/rcomm/src/models/http_request.rs`**
   - Add new `HttpParseError` variant: `UriTooLong`
   - Add `MAX_URI_LEN` constant (8192 bytes)
   - Validate URI length in `build_from_stream()` after parsing the request line
   - Update error display for the new variant

2. **`/home/jwall/personal/rusty/rcomm/src/main.rs`**
   - Handle `HttpParseError::UriTooLong` in `handle_connection()` error path
   - Return `414 URI Too Long` response for this error type

3. **`/home/jwall/personal/rusty/rcomm/src/models/http_status_codes.rs`**
   - Status code 414 is already defined in the `get_status_phrase()` function (no changes needed)

---

## 3. Step-by-Step Implementation Details

### Step 1: Add URI Length Validation to HttpRequest

**File**: `/home/jwall/personal/rusty/rcomm/src/models/http_request.rs`

Add a new error variant and constant, then validate during parsing:

**Current Code** (lines 1-17):
```rust
use std::{
    collections::HashMap,
    fmt,
    io::{BufReader, prelude::*},
    net::TcpStream,
};
use super::http_methods::*;

const MAX_HEADER_LINE_LEN: usize = 8192;

#[derive(Debug)]
pub enum HttpParseError {
    HeaderTooLong,
    MissingHostHeader,
    MalformedRequestLine,
    IoError(std::io::Error),
}
```

**Updated Code**:
```rust
use std::{
    collections::HashMap,
    fmt,
    io::{BufReader, prelude::*},
    net::TcpStream,
};
use super::http_methods::*;

const MAX_HEADER_LINE_LEN: usize = 8192;
const MAX_URI_LEN: usize = 8192;

#[derive(Debug)]
pub enum HttpParseError {
    HeaderTooLong,
    UriTooLong,
    MissingHostHeader,
    MalformedRequestLine,
    IoError(std::io::Error),
}
```

**Key Changes**:
- Define `MAX_URI_LEN` constant (8192 bytes, aligned with `MAX_HEADER_LINE_LEN`)
- Add `UriTooLong` variant to `HttpParseError` enum

---

### Step 2: Update HttpParseError Display Implementation

**File**: `/home/jwall/personal/rusty/rcomm/src/models/http_request.rs`

Update the `fmt::Display` implementation to handle the new error variant:

**Current Code** (lines 19-28):
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

**Updated Code**:
```rust
impl fmt::Display for HttpParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HttpParseError::HeaderTooLong => write!(f, "Header line exceeds maximum length"),
            HttpParseError::UriTooLong => write!(f, "Request URI exceeds maximum length"),
            HttpParseError::MissingHostHeader => write!(f, "Missing required Host header"),
            HttpParseError::MalformedRequestLine => write!(f, "Malformed request line"),
            HttpParseError::IoError(e) => write!(f, "IO error: {e}"),
        }
    }
}
```

**Key Changes**:
- Add new match arm for `UriTooLong` with appropriate error message

---

### Step 3: Add URI Length Validation in build_from_stream()

**File**: `/home/jwall/personal/rusty/rcomm/src/models/http_request.rs`

Validate the URI length immediately after parsing it from the request line:

**Current Code** (lines 51-68):
```rust
pub fn build_from_stream(stream: &TcpStream) -> Result<HttpRequest, HttpParseError> {
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
```

**Updated Code**:
```rust
pub fn build_from_stream(stream: &TcpStream) -> Result<HttpRequest, HttpParseError> {
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

    // Validate URI length
    if target.len() > MAX_URI_LEN {
        return Err(HttpParseError::UriTooLong);
    }

    let version = iter.next().ok_or(HttpParseError::MalformedRequestLine)?.to_string();
    let mut request = HttpRequest::build(method, target, version);
```

**Key Changes**:
- After parsing the `target` (URI), check its length against `MAX_URI_LEN`
- Return `UriTooLong` error if the URI exceeds the limit
- This check happens before creating the `HttpRequest` struct, preventing invalid requests from being constructed

**Rationale**:
- The validation occurs right after extracting the URI and before further processing
- The check is before the Host header validation, allowing any malformed request to be rejected before proceeding
- Using the same 8192-byte limit as headers maintains consistency

---

### Step 4: Handle 414 Response in Main Server Handler

**File**: `/home/jwall/personal/rusty/rcomm/src/main.rs`

Update the error handling in `handle_connection()` to return the correct HTTP status code:

**Current Code** (lines 46-57):
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

**Updated Code**:
```rust
fn handle_connection(mut stream: TcpStream, routes: HashMap<String, PathBuf>) {
    let http_request = match HttpRequest::build_from_stream(&stream) {
        Ok(req) => req,
        Err(e) => {
            // Determine appropriate status code based on error type
            let status_code = match &e {
                HttpParseError::UriTooLong => 414,
                _ => 400,
            };

            eprintln!("Bad request: {e}");
            let mut response = HttpResponse::build(String::from("HTTP/1.1"), status_code);
            let body = format!("Bad Request: {e}");
            response.add_body(body.into());
            let _ = stream.write_all(&response.as_bytes());
            return;
        }
    };
```

**Import Requirements**:
Add this import at the top of `src/main.rs`:
```rust
use rcomm::models::http_request::HttpParseError;
```

**Key Changes**:
- Pattern match on the error type to determine the appropriate HTTP status code
- Return 414 for `UriTooLong`, 400 for all other parse errors
- Maintain the same error message and body format
- Preserve existing error logging behavior

---

## 4. Code Snippets and Pseudocode

### URI Length Validation Logic

```
CONST MAX_URI_LEN = 8192

FUNCTION validate_uri_length(target: string) -> Result<void, HttpParseError>
    IF target.length() > MAX_URI_LEN THEN
        RETURN Err(HttpParseError::UriTooLong)
    END IF
    RETURN Ok(())
END FUNCTION
```

### Error Handling in Request Handler

```
FUNCTION handle_connection(stream, routes)
    TRY
        request = parse_http_request(stream)
    CATCH UriTooLong error
        response = build_response(HTTP/1.1, 414)
        response.set_body(error.message())
        send_response(stream, response)
        RETURN
    CATCH other_error
        response = build_response(HTTP/1.1, 400)
        response.set_body(error.message())
        send_response(stream, response)
        RETURN
    END TRY

    // Continue with normal request handling
END FUNCTION
```

---

## 5. Testing Strategy

### Unit Tests (in `http_request.rs`)

Add tests to the existing test module to verify:
1. URIs at the maximum length are accepted
2. URIs exceeding the maximum length are rejected
3. The error message is correct
4. The error type matches the expectation

**New Tests to Add**:

```rust
#[test]
fn build_from_stream_accepts_uri_at_max_length() {
    use std::io::Write;
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = std::thread::spawn(move || {
        let mut client = std::net::TcpStream::connect(addr).unwrap();
        // Create a URI of exactly MAX_URI_LEN bytes
        let long_path = "/".to_string() + &"a".repeat(MAX_URI_LEN - 1);
        let msg = format!("GET {} HTTP/1.1\r\nHost: localhost\r\n\r\n", long_path);
        client.write_all(msg.as_bytes()).unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
    });

    let (stream, _) = listener.accept().unwrap();
    let result = HttpRequest::build_from_stream(&stream);

    assert!(result.is_ok());
    assert_eq!(result.unwrap().target.len(), MAX_URI_LEN);
    handle.join().unwrap();
}

#[test]
fn build_from_stream_rejects_oversized_uri() {
    use std::io::Write;
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = std::thread::spawn(move || {
        let mut client = std::net::TcpStream::connect(addr).unwrap();
        // Create a URI exceeding MAX_URI_LEN by 1 byte
        let long_path = "/".to_string() + &"b".repeat(MAX_URI_LEN);
        let msg = format!("GET {} HTTP/1.1\r\nHost: localhost\r\n\r\n", long_path);
        client.write_all(msg.as_bytes()).unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
    });

    let (stream, _) = listener.accept().unwrap();
    let result = HttpRequest::build_from_stream(&stream);

    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), HttpParseError::UriTooLong));
    handle.join().unwrap();
}

#[test]
fn uri_too_long_error_displays_correctly() {
    let error = HttpParseError::UriTooLong;
    let msg = format!("{}", error);
    assert_eq!(msg, "Request URI exceeds maximum length");
}
```

**Run unit tests**:
```bash
cargo test http_request
cargo test UriTooLong
```

### Integration Tests (in `src/bin/integration_test.rs`)

Add new test cases to verify the server returns 414 for excessively long URIs:

```rust
fn test_uri_too_long_returns_414(addr: &str) -> Result<(), String> {
    let long_uri = "/".to_string() + &"a".repeat(8192 + 1);
    let msg = format!("GET {} HTTP/1.1\r\nHost: localhost\r\n\r\n", long_uri);

    let mut stream = TcpStream::connect(addr)
        .map_err(|e| format!("Failed to connect: {}", e))?;
    stream.write_all(msg.as_bytes())
        .map_err(|e| format!("Failed to write: {}", e))?;

    let resp = read_response(&mut stream)?;
    assert_eq_or_err(&resp.status_code, &414, "status code")?;
    Ok(())
}

fn test_uri_at_max_length_succeeds(addr: &str) -> Result<(), String> {
    // Create a valid file path at maximum length
    let long_uri = "/".to_string() + &"a".repeat(8191);
    let msg = format!("GET {} HTTP/1.1\r\nHost: localhost\r\n\r\n", long_uri);

    let mut stream = TcpStream::connect(addr)
        .map_err(|e| format!("Failed to connect: {}", e))?;
    stream.write_all(msg.as_bytes())
        .map_err(|e| format!("Failed to write: {}", e))?;

    let resp = read_response(&mut stream)?;
    // Should either be 200 (if route exists) or 404 (if it doesn't), but NOT 414
    assert!(
        resp.status_code == 200 || resp.status_code == 404,
        "Expected 200 or 404, got {}",
        resp.status_code
    );
    Ok(())
}
```

Add these tests to the `main()` function's test list:
```rust
let results = vec![
    // ... existing tests ...
    run_test("uri_too_long_returns_414", || test_uri_too_long_returns_414(&addr)),
    run_test("uri_at_max_length_succeeds", || test_uri_at_max_length_succeeds(&addr)),
];
```

**Run integration tests**:
```bash
cargo run --bin integration_test
```

### Manual Testing

Test the feature with curl or similar HTTP client:

```bash
# Start the server
cargo run &
SERVER_PID=$!

# Test 1: Normal request (should work)
curl -v http://127.0.0.1:7878/

# Test 2: URI at maximum length (should return 404 or 200, not 414)
URI=$(printf 'GET %s HTTP/1.1\r\nHost: localhost\r\n\r\n' "$(printf '/%0.s' {1..8191})" | head -c 8192)
echo -e "GET /$(printf 'a%.0s' {1..8191}) HTTP/1.1\r\nHost: localhost\r\n\r\n" | nc localhost 7878

# Test 3: URI exceeding maximum length (should return 414)
echo -e "GET /$(printf 'b%.0s' {1..8193}) HTTP/1.1\r\nHost: localhost\r\n\r\n" | nc localhost 7878

# Kill the server
kill $SERVER_PID
```

---

## 6. Edge Cases to Consider

### Case 1: URI Exactly at Maximum Length (8192 bytes)
**Scenario**: A request with URI length = 8192 bytes
**Expected Behavior**: Should be accepted (not rejected as too long)
**Implementation Detail**: Check `target.len() > MAX_URI_LEN`, not `>=`
**Test Coverage**: `test_uri_at_max_length_succeeds()`

### Case 2: URI One Byte Over Maximum (8193 bytes)
**Scenario**: A request with URI length = 8193 bytes
**Expected Behavior**: Should return 414 URI Too Long
**Implementation Detail**: The comparison `target.len() > MAX_URI_LEN` correctly catches this
**Test Coverage**: `test_uri_too_long_returns_414()`

### Case 3: Extremely Long URI (e.g., 100MB)
**Scenario**: A malicious request with URI millions of bytes long
**Current Behavior**: May cause the request line to exceed 8192 bytes before URI is extracted
**Mitigation**: The `MAX_HEADER_LINE_LEN` check (line 59) catches this first, returning `HeaderTooLong`
**Expected**: The request line validation at line 59-61 will reject it before we reach URI length check
**Rationale**: Defense in depth — two layers of validation prevent resource exhaustion

### Case 4: URI with Query String
**Scenario**: A request like `GET /path?key1=value1&key2=value2&... HTTP/1.1`
**Current Behavior**: The entire URI including query string is stored in `target`
**Implementation Detail**: Our validation includes the query string in the length check, which is correct
**RFC Compliance**: HTTP specs include query strings as part of the URI
**Test Coverage**: Covered by `test_uri_too_long_returns_414()` which uses a long URI path

### Case 5: URI with URL-Encoded Characters
**Scenario**: A request like `GET /%2F%2F%2F...%2F HTTP/1.1` (many encoded slashes)
**Current Behavior**: URL-encoded sequences count toward the URI length
**Expected Result**: Correct behavior — the actual bytes transmitted are long, so it should count
**Rationale**: The validation occurs at the wire level (bytes received), before any decoding

### Case 6: URI with Special Characters
**Scenario**: Unicode or non-ASCII characters in the URI
**Current Behavior**: `target.len()` counts bytes, which is correct for UTF-8
**Expected Result**: A UTF-8 encoded character might be multiple bytes, correctly counted
**Test Coverage**: Existing tests use ASCII; UTF-8 handling is covered by standard Rust string behavior

### Case 7: POST/PUT Requests with Long URI
**Scenario**: `POST /very/long/uri HTTP/1.1` with a body
**Current Behavior**: URI validation happens before body parsing
**Expected Result**: Should return 414 regardless of request method
**Implementation Detail**: Validation occurs in `build_from_stream()` before body is read
**Test Coverage**: Could add a test with different HTTP methods, but POST/GET/etc. all go through same code path

### Case 8: HTTP/1.0 vs HTTP/1.1
**Scenario**: Different HTTP versions with long URIs
**Expected Result**: Should reject both identically
**Implementation Detail**: Validation is version-agnostic
**Test Coverage**: Covered by unit tests which don't specify version uniqueness

### Case 9: Requests Without Host Header (HTTP/1.1)
**Scenario**: Long URI in a request missing required Host header
**Expected Behavior**: Should return 414 (URI too long) before checking for Host header
**Implementation Detail**: URI validation at line 70 occurs before Host header validation at line 91-93
**Test Coverage**: This is the natural order of operations and is tested

### Case 10: Fragmented TCP Packets
**Scenario**: A very long URI arrives in multiple TCP packets
**Current Behavior**: `read_line()` from `BufReader` handles reassembly automatically
**Expected Result**: Validation still works correctly regardless of packet boundaries
**Rationale**: `BufReader::read_line()` abstracts away packet details

---

## 7. Implementation Checklist

- [ ] Add `MAX_URI_LEN` constant to `/home/jwall/personal/rusty/rcomm/src/models/http_request.rs`
- [ ] Add `UriTooLong` variant to `HttpParseError` enum
- [ ] Update `HttpParseError::fmt()` to handle `UriTooLong` variant
- [ ] Add URI length validation in `HttpRequest::build_from_stream()` after parsing the target
- [ ] Add import of `HttpParseError` to `/home/jwall/personal/rusty/rcomm/src/main.rs`
- [ ] Update error handling in `handle_connection()` to return 414 for `UriTooLong`
- [ ] Add unit tests to `http_request.rs`:
  - [ ] `test_build_from_stream_accepts_uri_at_max_length()`
  - [ ] `test_build_from_stream_rejects_oversized_uri()`
  - [ ] `test_uri_too_long_error_displays_correctly()`
- [ ] Add integration tests to `src/bin/integration_test.rs`:
  - [ ] `test_uri_too_long_returns_414()`
  - [ ] `test_uri_at_max_length_succeeds()`
- [ ] Run unit tests: `cargo test http_request`
- [ ] Run all unit tests: `cargo test`
- [ ] Run integration tests: `cargo run --bin integration_test`
- [ ] Verify no regressions in existing tests
- [ ] Manual testing with curl/nc to verify 414 response

---

## 8. Complexity and Risk Analysis

**Complexity**: 2/10
- Simple length comparison operation
- One new error variant in an existing enum
- Error handling mirrors existing pattern for `HeaderTooLong`
- No changes to core HTTP model structure
- No algorithmic complexity

**Risk**: Very Low
- Pure additive feature (backward compatible)
- Validation is deterministic and straightforward
- Standard HTTP error with well-defined semantics (RFC 7231)
- Similar to existing `HeaderTooLong` validation, proven pattern
- No performance impact (single length check per request)
- Aligned with existing codebase error handling patterns

**Dependencies**: None
- Uses only standard library (no external crates)
- Complies with project's no-external-dependencies constraint
- Works with existing `std::io::BufReader` infrastructure

**Consistency**: High
- Uses the same maximum length constant (8192 bytes) as header validation
- Follows existing error type pattern (`HttpParseError` enum)
- Returns correct HTTP status code via `get_status_phrase()` (already supports 414)
- Error handling approach matches existing code style

---

## 9. Future Enhancements

1. **Configurable URI Limit**: Allow URI length limit to be configured via environment variable or config file
2. **Separate Limits**: Distinguish between path and query string length limits if needed
3. **Detailed Error Response**: Include the actual URI length in the error response body for debugging
4. **Metrics**: Add counters for 414 responses to track potential attacks
5. **Logging**: Enhanced logging of oversized URI attempts with optional rate limiting info
6. **Dynamic Limits**: Allow different limits per virtual host or location (if multi-host feature added)
7. **Request Rate Limiting**: Combined with rate limiting feature to detect scanning/abuse patterns
