# Maximum Request URI Length Implementation Plan

## 1. Overview of the Feature

The HTTP request URI (Uniform Resource Identifier) is the target path requested by a client. Currently, rcomm has no limit on URI length, which poses a security risk. Attackers can send requests with extremely long URIs to:
- Consume excessive memory during parsing
- Trigger denial-of-service (DoS) conditions
- Bypass certain access controls or create confusion in logging systems

**Goal**: Implement a configurable maximum URI length constraint that rejects requests exceeding the limit with an HTTP 414 (URI Too Long) response.

**Standards Reference**:
- RFC 7231: HTTP/1.1 Semantics and Content — Recommends reasonable limits on request line length
- RFC 3986: Uniform Resource Identifiers — Discusses URI length considerations
- OWASP: CWE-414 Classification of Weakness — Excessively Long Argument to Function Call

**Key Benefits**:
- Security: Prevents URI-based DoS attacks and buffer exhaustion scenarios
- Compliance: Aligns with HTTP standards and security best practices
- Configurability: Allows operators to adjust limits based on their deployment needs
- Clear Error Response: 414 status code clearly communicates the issue to clients

**Default Behavior**:
- Maximum URI length: 2048 bytes (industry standard; RFC 2616 historically recommended ~2000 bytes)
- Can be overridden via environment variable `RCOMM_MAX_URI_LENGTH`
- If exceeded: Return HTTP 414 (URI Too Long) response

---

## 2. Files to be Modified or Created

### Modified Files

1. **`/home/jwall/personal/rusty/rcomm/src/models/http_request.rs`**
   - Add `MaxUriLengthExceeded` error variant to `HttpParseError` enum
   - Add `MAX_URI_LENGTH` constant (or make it configurable)
   - Add validation logic in `build_from_stream()` to check URI length after parsing the request line
   - Update error display message

2. **`/home/jwall/personal/rusty/rcomm/src/main.rs`**
   - Read `RCOMM_MAX_URI_LENGTH` environment variable
   - Pass the limit to `HttpRequest::build_from_stream()` (or store globally)
   - Handle `HttpParseError::MaxUriLengthExceeded` in error handler to return 414 status code

3. **`/home/jwall/personal/rusty/rcomm/src/models/http_status_codes.rs`**
   - Add mapping for status code 414 → "URI Too Long"

### New Files

None required. The feature fits within existing module structure.

---

## 3. Step-by-Step Implementation Details

### Step 1: Add HTTP Status Code 414

**File**: `/home/jwall/personal/rusty/rcomm/src/models/http_status_codes.rs`

Read the current file to understand its structure, then add the 414 status code mapping:

**Current Code** (example structure):
```rust
pub fn get_status_phrase(code: u16) -> String {
    match code {
        200 => "OK".to_string(),
        400 => "Bad Request".to_string(),
        404 => "Not Found".to_string(),
        // ... other codes ...
        _ => "".to_string(),
    }
}
```

**Updated Code**:
```rust
pub fn get_status_phrase(code: u16) -> String {
    match code {
        200 => "OK".to_string(),
        400 => "Bad Request".to_string(),
        404 => "Not Found".to_string(),
        414 => "URI Too Long".to_string(),  // Add this line
        // ... other codes ...
        _ => "".to_string(),
    }
}
```

### Step 2: Update HttpParseError Enum and Validation Logic

**File**: `/home/jwall/personal/rusty/rcomm/src/models/http_request.rs`

Add a new error variant and constant:

**Current Code** (lines 9-17):
```rust
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
const MAX_HEADER_LINE_LEN: usize = 8192;
const MAX_URI_LENGTH: usize = 2048;  // Add this line

#[derive(Debug)]
pub enum HttpParseError {
    HeaderTooLong,
    MissingHostHeader,
    MalformedRequestLine,
    MaxUriLengthExceeded,  // Add this line
    IoError(std::io::Error),
}
```

Update the `Display` implementation (lines 19-28):

**Current Code**:
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
            HttpParseError::MissingHostHeader => write!(f, "Missing required Host header"),
            HttpParseError::MalformedRequestLine => write!(f, "Malformed request line"),
            HttpParseError::MaxUriLengthExceeded => write!(f, "Request URI exceeds maximum length"),  // Add this line
            HttpParseError::IoError(e) => write!(f, "IO error: {e}"),
        }
    }
}
```

### Step 3: Add URI Length Validation in build_from_stream()

**File**: `/home/jwall/personal/rusty/rcomm/src/models/http_request.rs`

Modify the `build_from_stream()` method to validate URI length after parsing the request line:

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
    if target.len() > MAX_URI_LENGTH {
        return Err(HttpParseError::MaxUriLengthExceeded);
    }

    let version = iter.next().ok_or(HttpParseError::MalformedRequestLine)?.to_string();
    let mut request = HttpRequest::build(method, target, version);
```

**Key Points**:
- URI validation happens immediately after parsing the target from the request line
- Validation occurs before any header processing
- Uses the constant `MAX_URI_LENGTH` defined at module level
- Returns clear error type for downstream handling

### Step 4: Update Main Server Error Handling

**File**: `/home/jwall/personal/rusty/rcomm/src/main.rs`

Modify `handle_connection()` to handle the new error type and return HTTP 414:

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
            // Determine status code based on error type
            let (status_code, body) = match &e {
                HttpParseError::MaxUriLengthExceeded => {
                    (414, format!("Request URI Too Long: {e}"))
                }
                _ => {
                    (400, format!("Bad Request: {e}"))
                }
            };

            eprintln!("Request error: {e}");
            let mut response = HttpResponse::build(String::from("HTTP/1.1"), status_code);
            response.add_body(body.into());
            let _ = stream.write_all(&response.as_bytes());
            return;
        }
    };
```

**Required Import**:
Add `use rcomm::models::http_request::HttpParseError;` at the top of `src/main.rs`.

---

## 4. Code Snippets and Pseudocode

### URI Length Validation Function (Pseudocode)

```
CONSTANT MAX_URI_LENGTH = 2048

FUNCTION build_from_stream(stream: TcpStream) -> Result<HttpRequest, HttpParseError>
    // ... parse request line (method, target, version) ...

    IF target.length() > MAX_URI_LENGTH THEN
        RETURN Err(HttpParseError::MaxUriLengthExceeded)
    END IF

    // ... continue with header parsing ...
END FUNCTION
```

### Error Handling in Main (Pseudocode)

```
FUNCTION handle_connection(stream: TcpStream, routes: HashMap)
    LET request_result = HttpRequest::build_from_stream(stream)

    MATCH request_result DO
        CASE Ok(request):
            // ... normal request handling ...

        CASE Err(error):
            IF error == MaxUriLengthExceeded THEN
                status_code = 414
                error_body = "Request URI Too Long"
            ELSE
                status_code = 400
                error_body = "Bad Request"
            END IF

            response = HttpResponse::build(HTTP/1.1, status_code)
            response.add_body(error_body)
            stream.write_all(response.as_bytes())
            RETURN
    END MATCH
END FUNCTION
```

---

## 5. Testing Strategy

### Unit Tests (in `http_request.rs`)

Add tests to verify URI length validation:

```rust
#[test]
fn build_from_stream_accepts_valid_uri_length() {
    use std::io::Write;
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = std::thread::spawn(move || {
        let mut client = std::net::TcpStream::connect(addr).unwrap();
        // Create a valid URI exactly at the limit
        let uri = format!("/{}", "a".repeat(MAX_URI_LENGTH - 2));
        let msg = format!("GET {uri} HTTP/1.1\r\nHost: localhost\r\n\r\n");
        client.write_all(msg.as_bytes()).unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
    });

    let (stream, _) = listener.accept().unwrap();
    let req = HttpRequest::build_from_stream(&stream).unwrap();
    assert_eq!(req.method.to_string(), "GET");
    handle.join().unwrap();
}

#[test]
fn build_from_stream_rejects_uri_too_long() {
    use std::io::Write;
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = std::thread::spawn(move || {
        let mut client = std::net::TcpStream::connect(addr).unwrap();
        // Create a URI exceeding the limit
        let uri = format!("/{}", "a".repeat(MAX_URI_LENGTH + 1));
        let msg = format!("GET {uri} HTTP/1.1\r\nHost: localhost\r\n\r\n");
        client.write_all(msg.as_bytes()).unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
    });

    let (stream, _) = listener.accept().unwrap();
    let result = HttpRequest::build_from_stream(&stream);

    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), HttpParseError::MaxUriLengthExceeded));
    handle.join().unwrap();
}

#[test]
fn build_from_stream_rejects_very_long_uri() {
    use std::io::Write;
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = std::thread::spawn(move || {
        let mut client = std::net::TcpStream::connect(addr).unwrap();
        // Create an extremely long URI (e.g., 10KB)
        let uri = format!("/{}", "a".repeat(10240));
        let msg = format!("GET {uri} HTTP/1.1\r\nHost: localhost\r\n\r\n");
        client.write_all(msg.as_bytes()).unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
    });

    let (stream, _) = listener.accept().unwrap();
    let result = HttpRequest::build_from_stream(&stream);

    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), HttpParseError::MaxUriLengthExceeded));
    handle.join().unwrap();
}
```

**Run unit tests**:
```bash
cargo test http_request
```

### Integration Tests (in `src/bin/integration_test.rs`)

Add tests to verify HTTP 414 response:

```rust
fn test_short_uri_succeeds(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "status for short URI")?;
    Ok(())
}

fn test_long_uri_returns_414(addr: &str) -> Result<(), String> {
    // Create a URI exceeding the 2048 byte limit
    let long_uri = format!("/{}", "a".repeat(2049));
    let resp = send_request(addr, "GET", &long_uri)?;
    assert_eq_or_err(&resp.status_code, &414, "status for long URI")?;
    Ok(())
}

fn test_uri_at_limit_succeeds(addr: &str) -> Result<(), String> {
    // Create a URI exactly at the 2048 byte limit
    let uri_at_limit = format!("/{}", "a".repeat(2046));  // "/" + chars + space + "HTTP/1.1"
    let resp = send_request(addr, "GET", &uri_at_limit)?;
    // This should succeed (or may fail due to request line length, but not due to URI length alone)
    assert!(resp.status_code == 200 || resp.status_code == 400, "status should not be 414");
    Ok(())
}

fn test_extremely_long_uri_returns_414(addr: &str) -> Result<(), String> {
    // Create an extremely long URI (10KB)
    let huge_uri = format!("/{}", "a".repeat(10240));
    let resp = send_request(addr, "GET", &huge_uri)?;
    assert_eq_or_err(&resp.status_code, &414, "status for huge URI")?;
    Ok(())
}
```

Add these tests to the `main()` function's test list:
```rust
let results = vec![
    // ... existing tests ...
    run_test("short_uri_succeeds", || test_short_uri_succeeds(&addr)),
    run_test("long_uri_returns_414", || test_long_uri_returns_414(&addr)),
    run_test("uri_at_limit_succeeds", || test_uri_at_limit_succeeds(&addr)),
    run_test("extremely_long_uri_returns_414", || test_extremely_long_uri_returns_414(&addr)),
];
```

**Run integration tests**:
```bash
cargo run --bin integration_test
```

### Manual Testing

1. **Test Short URI** (should succeed):
   ```bash
   curl -v http://127.0.0.1:7878/
   ```
   Expected: 200 OK

2. **Test Long URI** (should get 414):
   ```bash
   curl -v "http://127.0.0.1:7878/$(python3 -c 'print("a" * 2049)')"
   ```
   Expected: 414 URI Too Long

3. **Test Boundary** (exactly at limit, should succeed or be request-line-limited):
   ```bash
   curl -v "http://127.0.0.1:7878/$(python3 -c 'print("a" * 2046)')"
   ```
   Expected: 200 OK or 400 Bad Request (not 414)

---

## 6. Edge Cases to Consider

### Case 1: URI at Exactly the Limit (2048 bytes)
**Scenario**: Request with URI of exactly 2048 bytes
**Current Behavior**: Validation: `if target.len() > MAX_URI_LENGTH` allows 2048 bytes
**Expected**: Request is accepted and processed normally
**Code**:
```rust
const MAX_URI_LENGTH: usize = 2048;
if target.len() > MAX_URI_LENGTH {  // Allows exactly 2048
    return Err(HttpParseError::MaxUriLengthExceeded);
}
```

### Case 2: Request Line Exceeding MAX_HEADER_LINE_LEN Before URI Check
**Scenario**: Request line with URI + method + version + spaces = 8193 bytes
**Current Behavior**: `MAX_HEADER_LINE_LEN` check (line 59) catches it first
**Expected**: Returns `HeaderTooLong` error (not `MaxUriLengthExceeded`)
**Impact**: The request line validation happens before URI validation, so this is handled correctly
**Code**: No change needed; existing check at line 59 covers this

### Case 3: URI with Query String
**Scenario**: Request: `GET /path?param=value&other=data HTTP/1.1`
**Current Behavior**: The entire URI including query string is stored in `target`
**Expected**: URI length includes query string, so `?param=value&other=data` counts toward limit
**Verification**: The test cases should include URIs with query strings
**Code**: No special handling needed; query string is part of `target`

### Case 4: URI with Fragments
**Scenario**: Request: `GET /path#section HTTP/1.1`
**Current Behavior**: Fragments are technically not sent to the server (client-side only in browsers)
**Expected**: If sent, would be included in URI length calculation
**Impact**: Not an issue in practice; HTTP clients don't send fragments to servers

### Case 5: URL-Encoded Characters
**Scenario**: Request: `GET /search?q=%20%20%20...%20 HTTP/1.1` (percent-encoded spaces)
**Current Behavior**: Percent-encoded string counts as-is (each `%20` = 3 bytes, not 1)
**Expected**: URI length is the byte count of the raw string sent
**Verification**: Test with long percent-encoded URIs
**Example**:
```rust
let long_encoded_uri = format!("/search?q={}", "%20".repeat(750));  // 750 * 3 = 2250 bytes
// This should trigger MaxUriLengthExceeded
```

### Case 6: Unicode in URI
**Scenario**: Request: `GET /café HTTP/1.1` (UTF-8 encoded)
**Current Behavior**: `String::len()` returns byte count (UTF-8 encoded)
**Expected**: Multi-byte UTF-8 characters count as multiple bytes toward the limit
**Impact**: Correct behavior; "café" = 5 bytes, not 4 characters
**Code**: No special handling needed; `String::len()` is correct for bytes

### Case 7: Absolute vs. Relative URIs
**Scenario**: HTTP/1.1 allows absolute URIs: `GET http://example.com/path HTTP/1.1`
**Current Behavior**: The full `http://example.com/path` would be in `target`
**Expected**: Entire string counts toward limit
**Impact**: Correctly limited; absolute URIs can be very long, which is the security issue we're preventing
**Code**: No special handling needed

### Case 8: Empty URI
**Scenario**: Request: `GET  HTTP/1.1` (two spaces, no URI)
**Current Behavior**: `iter.next()` would return `None` after method
**Expected**: `MalformedRequestLine` error (caught before URI validation)
**Impact**: Not an issue; malformed check happens first

### Case 9: Configurable Limit via Environment Variable (Future)
**Scenario**: User sets `RCOMM_MAX_URI_LENGTH=4096` before running server
**Current Implementation**: Only supports constant `MAX_URI_LENGTH = 2048`
**Planned Enhancement**: Pass limit from environment variable to `build_from_stream()`
**Note**: For this implementation phase, use constant; environment configuration can be added later

### Case 10: Zero or Negative Limit (if made configurable)
**Scenario**: User accidentally sets `RCOMM_MAX_URI_LENGTH=0`
**Current Behavior**: Not applicable (constant value used)
**Future Handling**: When environment variable support is added, validate: limit must be >= 1
**Example**:
```rust
let max_uri = std::env::var("RCOMM_MAX_URI_LENGTH")
    .ok()
    .and_then(|s| s.parse::<usize>().ok())
    .filter(|&len| len > 0)
    .unwrap_or(2048);
```

---

## 7. Implementation Checklist

- [ ] Read `/home/jwall/personal/rusty/rcomm/src/models/http_status_codes.rs` to understand current structure
- [ ] Add HTTP 414 status code mapping to `http_status_codes.rs`
- [ ] Modify `/home/jwall/personal/rusty/rcomm/src/models/http_request.rs`:
  - [ ] Add `const MAX_URI_LENGTH: usize = 2048;` constant
  - [ ] Add `MaxUriLengthExceeded` variant to `HttpParseError` enum
  - [ ] Update `HttpParseError` Display implementation to handle new variant
  - [ ] Add URI length validation in `build_from_stream()` after parsing target
  - [ ] Add unit tests: valid URI length, URI exceeding limit, very long URI
- [ ] Modify `/home/jwall/personal/rusty/rcomm/src/main.rs`:
  - [ ] Add import: `use rcomm::models::http_request::HttpParseError;`
  - [ ] Update `handle_connection()` error handler to return 414 for `MaxUriLengthExceeded`
- [ ] Add integration tests to `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`:
  - [ ] `test_short_uri_succeeds()`
  - [ ] `test_long_uri_returns_414()`
  - [ ] `test_uri_at_limit_succeeds()`
  - [ ] `test_extremely_long_uri_returns_414()`
- [ ] Run unit tests: `cargo test http_request`
- [ ] Run all tests: `cargo test`
- [ ] Run integration tests: `cargo run --bin integration_test`
- [ ] Manual testing with `curl` to verify 414 response

---

## 8. Complexity and Risk Analysis

**Complexity**: 2/10
- Straightforward length check (single condition)
- Addition of new error variant (no complex logic)
- New HTTP status code mapping (simple data entry)
- Error handling already exists; just adding a new case

**Risk**: Very Low
- Pure additive feature (doesn't break existing functionality)
- URI validation is deterministic and testable
- 414 response is standard HTTP and well-understood
- No performance impact (single integer comparison per request)
- All validation happens in existing error handling path

**Dependencies**: None
- Uses only standard library (`std::string::String`)
- No external crates required
- Aligns with project's no-external-dependencies constraint

**Backward Compatibility**: Perfect
- Default limit (2048 bytes) covers virtually all legitimate URIs
- No API changes to existing code
- Existing requests that fit within limit are unaffected
- Only rejects truly malicious/oversized requests

---

## 9. Future Enhancements

1. **Configurable Limit via Environment Variable**: Add support for `RCOMM_MAX_URI_LENGTH` environment variable
   - Allow operators to increase/decrease limit based on deployment
   - With validation to prevent zero or negative values

2. **Separate Limits for URI Components**:
   - Path length limit
   - Query string length limit
   - Header name/value limits (already exists: `MAX_HEADER_LINE_LEN`)

3. **Configurable Limits via File**:
   - Server configuration file for maximum lengths
   - Support for different limits per route/domain

4. **Logging and Metrics**:
   - Track rejected URIs for security monitoring
   - Log URI rejection events with timestamp and source IP

5. **Custom Error Response**:
   - Customizable HTML body for 414 errors
   - Option to include hint about URI reduction in response

6. **Request Line Length Limit**:
   - Separate limit for entire request line (method + URI + version)
   - Currently shares limit with header lines (`MAX_HEADER_LINE_LEN`)

7. **Security Headers in 414 Response**:
   - Add `X-Content-Type-Options: nosniff`
   - Add `X-Frame-Options: DENY`
   - Consider rate-limiting clients that send oversized URIs

---

## 10. Related Security Considerations

**URI-Based DoS Attacks**:
- Attackers may send very long URIs to consume memory during parsing
- Attackers may send specially crafted URIs with many path segments to trigger routing logic
- By limiting URI length, we prevent these attack vectors

**Buffer Overflow Prevention**:
- While Rust has memory safety, limiting input size prevents excessive memory allocation
- Prevents potential issues if URI is later stored in fixed-size buffers (in future code)

**Denial of Service (DoS) Mitigation**:
- Long URIs require CPU cycles for parsing and routing
- Limiting URI length reduces resource consumption
- Combined with request body size limits, provides comprehensive DoS protection

**Compliance**:
- Most HTTP servers implement URI length limits:
  - Apache: ~8000 bytes by default
  - Nginx: ~4096 bytes for request line
  - Node.js: ~16384 bytes by default
- 2048 bytes is conservative and standard

