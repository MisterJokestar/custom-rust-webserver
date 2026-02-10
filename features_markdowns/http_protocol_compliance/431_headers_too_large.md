# Implementation Plan: HTTP 431 Request Header Fields Too Large

## Overview

Currently, when a client sends oversized header fields (exceeding `MAX_HEADER_LINE_LEN = 8192` bytes), the server returns a generic `400 Bad Request` response. This feature implements proper HTTP protocol compliance by returning the specific `431 Request Header Fields Too Large` status code, as defined in RFC 7231.

**Rationale:**
- Clients can distinguish between truly malformed requests (`400`) and size policy violations (`431`)
- Better observability: monitoring/logging can track header size issues separately
- Standards compliance: proper HTTP status codes improve interoperability

**Scope:** Minimal, focused change affecting header size validation in the parsing layer.

---

## Files to Modify

1. **`src/models/http_request.rs`**
   - Add `HeaderFieldsTooLarge` variant to `HttpParseError` enum
   - Replace `HeaderTooLong` returns with `HeaderFieldsTooLarge`
   - Update error display message

2. **`src/main.rs`**
   - Update `handle_connection()` to detect the new error variant
   - Return `431` response status code for oversized headers
   - Update error message body

3. **`src/bin/integration_test.rs` (optional, for validation)**
   - Add test case verifying 431 response on oversized headers

---

## Step-by-Step Implementation

### Step 1: Update `HttpParseError` Enum

**File:** `/home/jwall/personal/rusty/rcomm/src/models/http_request.rs`

**Change:** Replace `HeaderTooLong` with `HeaderFieldsTooLarge` to better reflect RFC 7231 semantics.

**Current code (lines 12-28):**
```rust
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
#[derive(Debug)]
pub enum HttpParseError {
    HeaderFieldsTooLarge,
    MissingHostHeader,
    MalformedRequestLine,
    IoError(std::io::Error),
}

impl fmt::Display for HttpParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HttpParseError::HeaderFieldsTooLarge => write!(f, "Request header fields too large"),
            HttpParseError::MissingHostHeader => write!(f, "Missing required Host header"),
            HttpParseError::MalformedRequestLine => write!(f, "Malformed request line"),
            HttpParseError::IoError(e) => write!(f, "IO error: {e}"),
        }
    }
}
```

**Rationale:**
- Rename for clarity and RFC alignment
- Update display text to match HTTP status phrase
- No functional change to enum variant count/ordering

---

### Step 2: Update Error Returns in `build_from_stream()`

**File:** `/home/jwall/personal/rusty/rcomm/src/models/http_request.rs`

**Changes:**
- Line 60: Replace `Err(HttpParseError::HeaderTooLong)` with `Err(HttpParseError::HeaderFieldsTooLarge)`
- Line 80: Replace `Err(HttpParseError::HeaderTooLong)` with `Err(HttpParseError::HeaderFieldsTooLarge)`

**Current code (lines 59-61):**
```rust
if line.len() > MAX_HEADER_LINE_LEN {
    return Err(HttpParseError::HeaderTooLong);
}
```

**New code:**
```rust
if line.len() > MAX_HEADER_LINE_LEN {
    return Err(HttpParseError::HeaderFieldsTooLarge);
}
```

**Current code (lines 79-81):**
```rust
if header_line.len() > MAX_HEADER_LINE_LEN {
    return Err(HttpParseError::HeaderTooLong);
}
```

**New code:**
```rust
if header_line.len() > MAX_HEADER_LINE_LEN {
    return Err(HttpParseError::HeaderFieldsTooLarge);
}
```

---

### Step 3: Update Unit Tests

**File:** `/home/jwall/personal/rusty/rcomm/src/models/http_request.rs`

**Change:** Line 380 test assertion to use new enum variant.

**Current code (line 380):**
```rust
assert!(matches!(result.unwrap_err(), HttpParseError::HeaderTooLong));
```

**New code:**
```rust
assert!(matches!(result.unwrap_err(), HttpParseError::HeaderFieldsTooLarge));
```

---

### Step 4: Update Server Error Handling

**File:** `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Change:** Update `handle_connection()` to map the new error variant to HTTP 431.

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
    // ...
}
```

**New code:**
```rust
fn handle_connection(mut stream: TcpStream, routes: HashMap<String, PathBuf>) {
    let http_request = match HttpRequest::build_from_stream(&stream) {
        Ok(req) => req,
        Err(e) => {
            use rcomm::models::http_request::HttpParseError;

            let (status_code, error_msg) = match e {
                HttpParseError::HeaderFieldsTooLarge => {
                    eprintln!("Request header fields too large");
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
    // ...
}
```

**Alternative (simpler):**
If pattern matching feels verbose, a simple `if let` alternative:

```rust
fn handle_connection(mut stream: TcpStream, routes: HashMap<String, PathBuf>) {
    let http_request = match HttpRequest::build_from_stream(&stream) {
        Ok(req) => req,
        Err(e) => {
            use rcomm::models::http_request::HttpParseError;

            let status = if matches!(e, HttpParseError::HeaderFieldsTooLarge) { 431 } else { 400 };
            let body = match e {
                HttpParseError::HeaderFieldsTooLarge => {
                    eprintln!("Request header fields too large");
                    String::from("Request Header Fields Too Large")
                },
                _ => {
                    eprintln!("Bad request: {e}");
                    format!("Bad Request: {e}")
                }
            };

            let mut response = HttpResponse::build(String::from("HTTP/1.1"), status);
            response.add_body(body.into());
            let _ = stream.write_all(&response.as_bytes());
            return;
        }
    };
    // ...
}
```

**Rationale:** Separates distinct error conditions into appropriate HTTP status codes, improving client/observer visibility.

---

### Step 5: Add Integration Test (Optional)

**File:** `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`

**Add test function:**
```rust
fn test_oversized_header_returns_431() -> TestResult {
    // Generate a header value exceeding MAX_HEADER_LINE_LEN (8192 bytes)
    let long_header_value = "x".repeat(8193);
    let request = format!(
        "GET / HTTP/1.1\r\nHost: localhost\r\nX-Large: {}\r\n\r\n",
        long_header_value
    );

    let response = send_request(&request)?;

    if response.starts_with("HTTP/1.1 431") {
        TestResult::Pass
    } else {
        TestResult::Fail(format!("Expected 431 status, got: {}", response.lines().next().unwrap_or("unknown")))
    }
}
```

**Add to test suite in `main()`:**
```rust
run_test("oversized_header_returns_431", test_oversized_header_returns_431);
```

---

## Testing Strategy

### Unit Tests (Compile-Time)

1. **Existing test:** `build_from_stream_rejects_oversized_header()` (line 361-382)
   - Already sends a header exceeding the limit
   - Update assertion to expect `HeaderFieldsTooLarge` variant
   - No functional change needed, just variant name

### Integration Tests (Runtime)

2. **New test:** `test_oversized_header_returns_431()`
   - Send HTTP request with oversized header (>8192 bytes)
   - Verify response status line is `HTTP/1.1 431`
   - Verify response body contains appropriate message

### Manual Testing

3. **Test with curl/telnet:**
   ```bash
   # Start server
   cargo run &

   # Send request with ~9KB header value
   python3 -c "
   import socket
   s = socket.socket()
   s.connect(('127.0.0.1', 7878))
   header_val = 'x' * 9000
   request = f'GET / HTTP/1.1\r\nHost: localhost\r\nX-Big: {header_val}\r\n\r\n'
   s.send(request.encode())
   print(s.recv(4096).decode())
   s.close()
   "
   ```

   Expected output: First line should be `HTTP/1.1 431 Request Header Fields Too Large`

4. **Verify other errors still return 400:**
   - Malformed request line (missing method/target/version)
   - Missing Host header for HTTP/1.1
   - IO errors

---

## Edge Cases & Considerations

### Edge Case 1: Request Line Exceeds Limit

**Current behavior:** Returns `HeaderFieldsTooLarge` (via first check at line 59)

**Expected:** Correct — RFC 9110 treats request-line as "header field" for size purposes. However, technically the status 431 applies to header fields specifically, not the request line.

**Mitigation options:**
- (A) Keep current behavior: Treat request-line as part of header fields conceptually
- (B) Add new error variant `RequestLineTooLong` returning 414 (URI Too Long) for request-target specifically
- *Recommend (A)* for this implementation: simpler, aligns with most server implementations

### Edge Case 2: Cumulative Header Size vs. Individual Line Size

**Current implementation:** Validates individual header *line* length (8192 bytes per line)

**Alternative:** Total cumulative headers size limit (not implemented here)

**Note:** RFC 9110 doesn't mandate specific limits. Current approach is reasonable and common (nginx, Apache use similar per-line limits).

### Edge Case 3: Client Retries After 431

**Expected behavior:** Server consistently returns 431 for oversized headers. Client must reduce header sizes and retry.

**No implementation needed:** Status code alone handles this correctly.

### Edge Case 4: Chunked Transfer Encoding

**Current implementation:** Only parses `Content-Length` body (line 96-104)

**Impact on this feature:** None. Chunked encoding doesn't affect header field size limits.

---

## Implementation Checklist

- [ ] **Rename enum variant:** `HeaderTooLong` → `HeaderFieldsTooLarge` in `/home/jwall/personal/rusty/rcomm/src/models/http_request.rs`
- [ ] **Update Display impl:** Change error message text to match HTTP 431 semantics
- [ ] **Update error returns:** Line 60 and Line 80 in `build_from_stream()`
- [ ] **Update unit test:** Line 380 assertion in `http_request.rs`
- [ ] **Update server handler:** Modify `handle_connection()` in `/home/jwall/personal/rusty/rcomm/src/main.rs` to return 431 for oversized headers
- [ ] **Run unit tests:** `cargo test` — all 34 tests should pass
- [ ] **Run integration tests:** `cargo run --bin integration_test` — all 12+ tests should pass
- [ ] **(Optional) Add integration test:** New test in `src/bin/integration_test.rs` for 431 response
- [ ] **Manual verification:** Test with curl/Python to confirm 431 response

---

## Code Review Focus Areas

1. **Error handling symmetry:** Both request-line and header-line size checks use same variant?
2. **Status code correctness:** 431 appropriate for field size, not request-line size?
3. **Message clarity:** Error body communicates issue clearly to client?
4. **Backward compatibility:** Does rename break anything? Check imports/matches.
5. **Test coverage:** Unit test updated? New integration test comprehensive?

---

## Performance Impact

**Expected:** Negligible
- Same validation logic (byte comparison)
- Same parsing path
- Only change: enum variant naming and error response status code

---

## Documentation/Communication

- Update any API docs if `HttpParseError` is public (it is, in models module)
- Add comment explaining the 431 vs 400 distinction in `handle_connection()`
- Consider noting in CLAUDE.md under "Known Issues" if clarifying error handling improvements

---

## References

- RFC 9110 Section 15.5.3: [431 Request Header Fields Too Large](https://www.rfc-editor.org/rfc/rfc9110#section-15.5.3)
- RFC 7231 (historical): Defines 431 status code
- HTTP/1.1 Header Field Size Limits: Common practice is 4KB–8KB per line, 16KB–32KB total
