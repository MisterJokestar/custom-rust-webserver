# Range Request Header Support - Implementation Plan

## Overview

This plan describes implementing HTTP `Range` request header support for partial content delivery in rcomm. When a client includes a `Range: bytes=start-end` header in a GET request, the server will:

1. Parse and validate the Range header
2. Check file size to ensure ranges are satisfiable
3. Return HTTP 206 Partial Content with the requested byte range
4. Include required headers: `Content-Range`, `Content-Length`, `Accept-Ranges`
5. Handle edge cases and malformed ranges gracefully

This enables clients (browsers, video players, download managers) to:
- Resume interrupted downloads
- Stream video/audio with seek functionality
- Parallelize downloads by fetching multiple ranges
- Reduce bandwidth for large files

### HTTP Specification References

- **RFC 7233** - HTTP Range Requests
  - 206 Partial Content status code
  - `Range` request header format: `bytes=0-499`, `bytes=500-999`, `bytes=-500` (last N bytes)
  - `Content-Range` response header format: `bytes start-end/total`
  - `Accept-Ranges` header to advertise support

- **Status Code 416** - Range Not Satisfiable (if range exceeds file size)

---

## Files to Modify

1. **`src/models/http_request.rs`**
   - Add `RangeRequest` struct to represent parsed Range header
   - Add parsing logic for `Range` header
   - Add validation helper functions

2. **`src/models/http_response.rs`**
   - Add methods to set `Content-Range` header
   - Modify `add_body()` to accept partial content
   - Add helper method to set `Accept-Ranges` header

3. **`src/main.rs`**
   - Update `handle_connection()` to detect and process Range requests
   - Implement logic to read and serve partial file content
   - Add `Accept-Ranges: bytes` to all 200 OK responses
   - Return 416 Range Not Satisfiable when appropriate

4. **`src/models.rs`** (barrel file)
   - Export new `RangeRequest` struct if created as separate module

5. **Unit tests** (within modified files)
   - Test Range header parsing
   - Test boundary conditions
   - Test error cases

6. **Integration tests** (src/bin/integration_test.rs)
   - Test actual Range request/response flow
   - Test multiple range requests (future; single range for now)

---

## Step-by-Step Implementation

### Phase 1: Parsing (src/models/http_request.rs)

#### 1.1 Define RangeRequest struct

```rust
/// Represents a parsed HTTP Range header
/// Supports single range requests: bytes=0-99, bytes=100-199, bytes=-50
#[derive(Debug, Clone, PartialEq)]
pub struct RangeRequest {
    pub start: u64,
    pub end: u64,  // inclusive
}

#[derive(Debug)]
pub enum RangeParseError {
    InvalidFormat,
    InvalidStart,
    InvalidEnd,
    StartGreaterThanEnd,
    SuffixRangeZero,  // bytes=-0 is invalid
}
```

#### 1.2 Add parsing method to HttpRequest

Add to `impl HttpRequest`:

```rust
/// Parse the Range header value and return a RangeRequest if valid.
/// Returns None if no Range header is present.
/// Returns Err if Range header is malformed.
///
/// Supports:
/// - bytes=0-99         (start to end, both inclusive)
/// - bytes=100-         (start to EOF)
/// - bytes=-50          (last N bytes)
pub fn try_parse_range(&self, file_size: u64) -> Result<Option<RangeRequest>, RangeParseError> {
    match self.try_get_header("range".to_string()) {
        None => Ok(None),
        Some(range_header) => {
            RangeRequest::parse(&range_header, file_size).map(Some)
        }
    }
}
```

#### 1.3 Implement RangeRequest::parse

```rust
impl RangeRequest {
    /// Parse a Range header value (e.g., "bytes=0-99")
    /// file_size is required to validate and compute ranges
    pub fn parse(range_str: &str, file_size: u64) -> Result<RangeRequest, RangeParseError> {
        // Must start with "bytes="
        let range_str = range_str.trim();
        if !range_str.starts_with("bytes=") {
            return Err(RangeParseError::InvalidFormat);
        }

        let range_part = &range_str[6..]; // Skip "bytes="

        // Handle three cases:
        // 1. start-end (both inclusive): "0-99"
        // 2. start- (to EOF): "100-"
        // 3. -suffix (last N bytes): "-50"

        if let Some(idx) = range_part.find('-') {
            let before = &range_part[..idx];
            let after = &range_part[idx + 1..];

            if before.is_empty() {
                // Suffix range: "-50"
                let suffix: u64 = after
                    .parse()
                    .map_err(|_| RangeParseError::InvalidEnd)?;
                if suffix == 0 {
                    return Err(RangeParseError::SuffixRangeZero);
                }
                let start = file_size.saturating_sub(suffix);
                let end = file_size - 1;
                return Ok(RangeRequest { start, end });
            }

            // Either "start-end" or "start-"
            let start: u64 = before
                .parse()
                .map_err(|_| RangeParseError::InvalidStart)?;

            if after.is_empty() {
                // "start-" (to EOF)
                let end = file_size - 1;
                return Ok(RangeRequest { start, end });
            }

            // "start-end"
            let end: u64 = after
                .parse()
                .map_err(|_| RangeParseError::InvalidEnd)?;

            if start > end {
                return Err(RangeParseError::StartGreaterThanEnd);
            }

            return Ok(RangeRequest { start, end });
        }

        Err(RangeParseError::InvalidFormat)
    }

    /// Check if this range is satisfiable for a file of given size
    pub fn is_satisfiable(&self, file_size: u64) -> bool {
        self.start < file_size && self.end < file_size && self.start <= self.end
    }

    /// Get the length of the range in bytes
    pub fn len(&self) -> u64 {
        self.end - self.start + 1
    }
}
```

#### 1.4 Add unit tests for Range parsing

```rust
#[cfg(test)]
mod range_tests {
    use super::*;

    #[test]
    fn parse_range_start_end() {
        let range = RangeRequest::parse("bytes=0-99", 1000).unwrap();
        assert_eq!(range.start, 0);
        assert_eq!(range.end, 99);
        assert_eq!(range.len(), 100);
    }

    #[test]
    fn parse_range_start_to_eof() {
        let range = RangeRequest::parse("bytes=500-", 1000).unwrap();
        assert_eq!(range.start, 500);
        assert_eq!(range.end, 999);
        assert_eq!(range.len(), 500);
    }

    #[test]
    fn parse_range_suffix() {
        let range = RangeRequest::parse("bytes=-100", 1000).unwrap();
        assert_eq!(range.start, 900);
        assert_eq!(range.end, 999);
        assert_eq!(range.len(), 100);
    }

    #[test]
    fn parse_range_suffix_larger_than_file() {
        // Suffix range larger than file size should return entire file
        let range = RangeRequest::parse("bytes=-2000", 1000).unwrap();
        assert_eq!(range.start, 0);
        assert_eq!(range.end, 999);
    }

    #[test]
    fn parse_range_invalid_suffix_zero() {
        let err = RangeRequest::parse("bytes=-0", 1000).unwrap_err();
        assert!(matches!(err, RangeParseError::SuffixRangeZero));
    }

    #[test]
    fn parse_range_start_greater_than_end() {
        let err = RangeRequest::parse("bytes=500-100", 1000).unwrap_err();
        assert!(matches!(err, RangeParseError::StartGreaterThanEnd));
    }

    #[test]
    fn parse_range_invalid_format_no_bytes_prefix() {
        let err = RangeRequest::parse("0-99", 1000).unwrap_err();
        assert!(matches!(err, RangeParseError::InvalidFormat));
    }

    #[test]
    fn parse_range_invalid_format_no_hyphen() {
        let err = RangeRequest::parse("bytes=0", 1000).unwrap_err();
        assert!(matches!(err, RangeParseError::InvalidFormat));
    }

    #[test]
    fn parse_range_invalid_start() {
        let err = RangeRequest::parse("bytes=abc-100", 1000).unwrap_err();
        assert!(matches!(err, RangeParseError::InvalidStart));
    }

    #[test]
    fn parse_range_is_satisfiable() {
        let range = RangeRequest::parse("bytes=0-99", 1000).unwrap();
        assert!(range.is_satisfiable(1000));
        assert!(!range.is_satisfiable(50)); // File too small
    }

    #[test]
    fn http_request_try_parse_range_no_header() {
        let req = HttpRequest::build(HttpMethods::GET, "/".to_string(), "HTTP/1.1".to_string());
        let result = req.try_parse_range(1000).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn http_request_try_parse_range_with_header() {
        let mut req = HttpRequest::build(HttpMethods::GET, "/file.bin".to_string(), "HTTP/1.1".to_string());
        req.add_header("Range".to_string(), "bytes=0-99".to_string());
        let result = req.try_parse_range(1000).unwrap();
        assert!(result.is_some());
        let range = result.unwrap();
        assert_eq!(range.start, 0);
        assert_eq!(range.end, 99);
    }
}
```

---

### Phase 2: Response Handling (src/models/http_response.rs)

#### 2.1 Add Content-Range setter

Add to `impl HttpResponse`:

```rust
/// Set the Content-Range header
/// Format: bytes start-end/total
pub fn set_content_range(&mut self, start: u64, end: u64, total: u64) -> &mut HttpResponse {
    let value = format!("bytes {}-{}/{}", start, end, total);
    self.headers.insert("content-range".to_string(), value);
    self
}

/// Set Accept-Ranges header to advertise Range support
pub fn set_accept_ranges(&mut self) -> &mut HttpResponse {
    self.headers.insert("accept-ranges".to_string(), "bytes".to_string());
    self
}
```

#### 2.2 Modify add_body for partial content awareness

The existing `add_body()` automatically sets `Content-Length`, which is correct for both full and partial responses. However, we should document that it works for partial content too.

```rust
/// Add body and set Content-Length header.
/// For partial content (206), the body should already be the partial slice.
/// Content-Length will reflect the partial content size, not the full file size.
pub fn add_body(&mut self, body: Vec<u8>) -> &mut HttpResponse {
    let len = body.len();
    self.body = Some(body);
    self.headers.insert("content-length".to_string(), len.to_string());
    self
}
```

#### 2.3 Add unit tests for Range response

```rust
#[cfg(test)]
mod range_response_tests {
    use super::*;

    #[test]
    fn set_content_range() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 206);
        resp.set_content_range(0, 99, 1000);
        let output = format!("{resp}");
        assert!(output.contains("content-range: bytes 0-99/1000\r\n"));
    }

    #[test]
    fn set_accept_ranges() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
        resp.set_accept_ranges();
        let output = format!("{resp}");
        assert!(output.contains("accept-ranges: bytes\r\n"));
    }

    #[test]
    fn partial_content_206_status() {
        let resp = HttpResponse::build("HTTP/1.1".to_string(), 206);
        let output = format!("{resp}");
        assert!(output.starts_with("HTTP/1.1 206 Partial Content\r\n"));
    }
}
```

---

### Phase 3: Connection Handler (src/main.rs)

#### 3.1 Import necessary types

```rust
use rcomm::models::http_request::RangeRequest;
use std::fs::File;
use std::io::Read;
```

#### 3.2 Create Range request handler function

Add before `handle_connection()`:

```rust
/// Handle a range request by reading only the requested bytes from the file
fn read_range_from_file(filename: &str, range: &RangeRequest) -> Result<Vec<u8>, std::io::Error> {
    let mut file = File::open(filename)?;
    let mut buffer = vec![0u8; range.len() as usize];

    file.seek(std::io::Seek::Start(range.start))?;
    file.read_exact(&mut buffer)?;

    Ok(buffer)
}
```

Need to add import:
```rust
use std::io::Seek;
```

#### 3.3 Update handle_connection

Replace the current implementation with:

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
    let clean_target = clean_route(&http_request.target);

    println!("Request: {http_request}");

    let (mut response, filename) = if routes.contains_key(&clean_target) {
        (HttpResponse::build(String::from("HTTP/1.1"), 200),
            routes.get(&clean_target).unwrap().to_str().unwrap())
    } else {
        (HttpResponse::build(String::from("HTTP/1.1"), 404),
            "pages/not_found.html")
    };

    // Always advertise Range support for successful responses
    if response.status_code == 200 {
        response.set_accept_ranges();
    }

    // Get file size for Range processing
    let file_size = fs::metadata(filename)
        .map(|m| m.len())
        .unwrap_or(0);

    // Process Range header if present
    if response.status_code == 200 {
        match http_request.try_parse_range(file_size) {
            Ok(Some(range)) => {
                // Range header was provided
                if range.is_satisfiable(file_size) {
                    // Valid range - serve partial content
                    response = HttpResponse::build(String::from("HTTP/1.1"), 206);
                    response.set_accept_ranges();
                    response.set_content_range(range.start, range.end, file_size);

                    match read_range_from_file(filename, &range) {
                        Ok(partial_body) => {
                            response.add_body(partial_body);
                        }
                        Err(e) => {
                            eprintln!("Error reading file range: {e}");
                            response = HttpResponse::build(String::from("HTTP/1.1"), 500);
                            response.add_body("Internal Server Error".as_bytes().to_vec());
                        }
                    }
                } else {
                    // Range not satisfiable (starts at or beyond EOF)
                    response = HttpResponse::build(String::from("HTTP/1.1"), 416);
                    response.add_header("Content-Range".to_string(), format!("bytes */{}", file_size));
                    response.add_body("Range Not Satisfiable".as_bytes().to_vec());
                }
            }
            Ok(None) => {
                // No Range header - serve full file (existing behavior)
                let contents = fs::read_to_string(filename).unwrap();
                response.add_body(contents.into());
            }
            Err(e) => {
                // Malformed Range header - ignore and serve full file
                // Per RFC 7233, invalid Range headers should be ignored
                eprintln!("Invalid Range header: {:?}", e);
                let contents = fs::read_to_string(filename).unwrap();
                response.add_body(contents.into());
            }
        }
    } else {
        // 404 or error response - serve full body
        let contents = fs::read_to_string(filename).unwrap();
        response.add_body(contents.into());
    }

    println!("Response: {response}");
    stream.write_all(&response.as_bytes()).unwrap();
}
```

Note: Need to store `status_code` as a public field in `HttpResponse` or add a getter. Check existing structure and modify accordingly.

#### 3.4 Update HttpResponse struct if needed

If `status_code` is private, add a getter:

In `src/models/http_response.rs`:

```rust
impl HttpResponse {
    pub fn status_code(&self) -> u16 {
        self.status_code
    }
}
```

Or make the field public if simpler for this codebase's style.

---

### Phase 4: Integration Tests (src/bin/integration_test.rs)

#### 4.1 Add Range request test helpers

Add to the test file:

```rust
fn send_range_request(stream: &mut TcpStream, target: &str, start: u64, end: u64) -> Result<TestResponse, String> {
    let request = format!(
        "GET {target} HTTP/1.1\r\nHost: localhost\r\nRange: bytes={start}-{end}\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).map_err(|e| e.to_string())?;
    read_response(stream)
}

fn send_range_request_suffix(stream: &mut TcpStream, target: &str, suffix: u64) -> Result<TestResponse, String> {
    let request = format!(
        "GET {target} HTTP/1.1\r\nHost: localhost\r\nRange: bytes=-{suffix}\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).map_err(|e| e.to_string())?;
    read_response(stream)
}
```

#### 4.2 Add Range request test cases

```rust
fn test_range_request_start_end() -> TestResult {
    let port = pick_free_port();
    let addr = format!("127.0.0.1:{port}");
    let mut server = start_server(port);

    wait_for_server(&addr, Duration::from_secs(5))
        .map_err(|e| TestError::ServerStartup(e))?;

    let mut stream = TcpStream::connect(&addr).map_err(|e| TestError::Connection(e.to_string()))?;

    // Send range request for first 100 bytes
    let response = send_range_request(&mut stream, "/", 0, 99)
        .map_err(|e| TestError::Request(e))?;

    assert_eq!(response.status_code, 206, "Expected 206 Partial Content");
    assert!(response.headers.contains_key("content-range"), "Missing Content-Range header");
    assert!(response.headers.contains_key("accept-ranges"), "Missing Accept-Ranges header");
    assert_eq!(response.body.len(), 100, "Expected 100 bytes in body");

    server.kill().map_err(|e| TestError::ServerShutdown(e.to_string()))?;
    TestResult::Pass
}

fn test_range_request_suffix() -> TestResult {
    let port = pick_free_port();
    let addr = format!("127.0.0.1:{port}");
    let mut server = start_server(port);

    wait_for_server(&addr, Duration::from_secs(5))
        .map_err(|e| TestError::ServerStartup(e))?;

    let mut stream = TcpStream::connect(&addr).map_err(|e| TestError::Connection(e.to_string()))?;

    // Send suffix range request for last 50 bytes
    let response = send_range_request_suffix(&mut stream, "/", 50)
        .map_err(|e| TestError::Request(e))?;

    assert_eq!(response.status_code, 206, "Expected 206 Partial Content");
    assert!(response.headers.contains_key("content-range"), "Missing Content-Range header");

    server.kill().map_err(|e| TestError::ServerShutdown(e.to_string()))?;
    TestResult::Pass
}

fn test_range_not_satisfiable() -> TestResult {
    let port = pick_free_port();
    let addr = format!("127.0.0.1:{port}");
    let mut server = start_server(port);

    wait_for_server(&addr, Duration::from_secs(5))
        .map_err(|e| TestError::ServerStartup(e))?;

    let mut stream = TcpStream::connect(&addr).map_err(|e| TestError::Connection(e.to_string()))?;

    // Send range request that's out of bounds (assuming index.html is smaller than 1 million bytes)
    let response = send_range_request(&mut stream, "/", 1_000_000, 1_000_100)
        .map_err(|e| TestError::Request(e))?;

    assert_eq!(response.status_code, 416, "Expected 416 Range Not Satisfiable");
    assert!(response.headers.contains_key("content-range"), "Missing Content-Range header");

    server.kill().map_err(|e| TestError::ServerShutdown(e.to_string()))?;
    TestResult::Pass
}

fn test_full_request_includes_accept_ranges() -> TestResult {
    let port = pick_free_port();
    let addr = format!("127.0.0.1:{port}");
    let mut server = start_server(port);

    wait_for_server(&addr, Duration::from_secs(5))
        .map_err(|e| TestError::ServerStartup(e))?;

    let mut stream = TcpStream::connect(&addr).map_err(|e| TestError::Connection(e.to_string()))?;

    // Send regular GET request (no Range header)
    let request = format!("GET / HTTP/1.1\r\nHost: localhost\r\n\r\n");
    stream.write_all(request.as_bytes()).map_err(|e| TestError::Request(e.to_string()))?;
    let response = read_response(&mut stream)
        .map_err(|e| TestError::Request(e))?;

    assert_eq!(response.status_code, 200, "Expected 200 OK");
    assert!(response.headers.contains_key("accept-ranges"), "Missing Accept-Ranges header on full request");

    server.kill().map_err(|e| TestError::ServerShutdown(e.to_string()))?;
    TestResult::Pass
}
```

Then add these tests to the main test suite runner function.

---

## Testing Strategy

### Unit Tests (in modified files)

**In `src/models/http_request.rs`:**
- Range header parsing with valid formats (bytes=0-99, bytes=100-, bytes=-50)
- Edge cases (suffix larger than file, zero ranges, negative numbers)
- Invalid formats (missing "bytes=", malformed values)
- Satisfiability checks against various file sizes

**In `src/models/http_response.rs`:**
- Content-Range header formatting
- Accept-Ranges header setting
- Status code 206 generation

### Integration Tests (in `src/bin/integration_test.rs`)

1. **Basic Range Request**: Send `Range: bytes=0-99` and verify 206 response
2. **Suffix Range**: Send `Range: bytes=-50` and verify last 50 bytes are returned
3. **Open-ended Range**: Send `Range: bytes=100-` and verify from byte 100 to EOF
4. **Range Not Satisfiable**: Send `Range: bytes=999999-999999` and verify 416
5. **Full File with Accept-Ranges**: Verify normal GET includes `Accept-Ranges: bytes`
6. **Invalid Range Ignored**: Send malformed Range header, verify full file is returned
7. **Multiple Ranges (Future)**: Only single ranges supported initially

### Edge Cases to Test

1. **Boundary Conditions**
   - Range start = 0, end = file_size - 1 (entire file)
   - Range exactly at file boundaries
   - Very large files (10GB+)

2. **Malformed Ranges**
   - `Range: bytes=100-50` (start > end)
   - `Range: bytes=-0` (zero suffix)
   - `Range: bytes` (missing =)
   - `Range: kilobytes=0-100` (wrong unit)
   - `Range: bytes=abc-def` (non-numeric)

3. **RFC Compliance**
   - Invalid Range headers silently ignored (serve full file)
   - 416 returned when range.start >= file_size
   - Content-Range format: `bytes start-end/total`
   - Accept-Ranges: bytes on 200/206 responses
   - No Accept-Ranges on 4xx/5xx responses (debatable; current plan: omit)

---

## Code Snippets Summary

### Key Struct Definitions

```rust
// In http_request.rs
#[derive(Debug, Clone, PartialEq)]
pub struct RangeRequest {
    pub start: u64,
    pub end: u64,
}

#[derive(Debug)]
pub enum RangeParseError {
    InvalidFormat,
    InvalidStart,
    InvalidEnd,
    StartGreaterThanEnd,
    SuffixRangeZero,
}
```

### Key Methods

**Parsing:**
```rust
impl HttpRequest {
    pub fn try_parse_range(&self, file_size: u64) -> Result<Option<RangeRequest>, RangeParseError>
}

impl RangeRequest {
    pub fn parse(range_str: &str, file_size: u64) -> Result<RangeRequest, RangeParseError>
    pub fn is_satisfiable(&self, file_size: u64) -> bool
    pub fn len(&self) -> u64
}
```

**Response:**
```rust
impl HttpResponse {
    pub fn set_content_range(&mut self, start: u64, end: u64, total: u64) -> &mut HttpResponse
    pub fn set_accept_ranges(&mut self) -> &mut HttpResponse
}
```

**File Reading:**
```rust
fn read_range_from_file(filename: &str, range: &RangeRequest) -> Result<Vec<u8>, std::io::Error>
```

---

## Implementation Order

1. **Parsing Module** (http_request.rs)
   - Define `RangeRequest` struct
   - Implement `RangeRequest::parse()`
   - Add `try_parse_range()` to `HttpRequest`
   - Write comprehensive unit tests
   - Verify all parsing edge cases pass

2. **Response Module** (http_response.rs)
   - Add `set_content_range()` method
   - Add `set_accept_ranges()` method
   - Add 206 status code handling (already in status_codes.rs)
   - Write unit tests for new methods

3. **Main Handler** (main.rs)
   - Add imports and helper function
   - Update `handle_connection()` to detect Range requests
   - Implement logic flow (parse → check satisfiable → serve partial or 416)
   - Test locally with curl/browser

4. **Integration Tests** (src/bin/integration_test.rs)
   - Add test helper functions
   - Implement all test cases
   - Run full integration test suite

5. **Documentation**
   - Update CLAUDE.md with new Range request handling
   - Document known limitations (single range only for now)

---

## Known Limitations & Future Work

### Current Scope (v1)

- **Single Range Only**: Supports `bytes=0-99` format only
  - Multi-range requests (e.g., `bytes=0-99,200-299`) not supported
  - Return 200 full content if multiple ranges detected (or 416)

- **No Conditional Requests**: Range requests without ETag/If-Range always succeed
  - Future: Support `If-Range` header to prevent stale range responses

- **No HEAD Method for Range**: HEAD requests ignore Range header
  - Future: Support HEAD with Range to get Content-Length without body

### Design Decisions

1. **Malformed Ranges Are Ignored**: Per RFC 7233 Section 3.1, invalid Range headers should be treated as absent (serve full file)

2. **No Multiple Ranges**: Single range per request keeps implementation simple; multi-range requires multipart/byteranges MIME type

3. **File Size Checked at Request Time**: No caching of file metadata; each request queries filesystem

4. **Accept-Ranges Only on 200/206**: Not advertised on error responses; could be added if needed

---

## Checklist for Implementation

- [ ] Define RangeRequest struct and RangeParseError enum
- [ ] Implement RangeRequest::parse() with all edge cases
- [ ] Add try_parse_range() method to HttpRequest
- [ ] Write 10+ unit tests for parsing
- [ ] Add set_content_range() and set_accept_ranges() to HttpResponse
- [ ] Write 5+ unit tests for response helpers
- [ ] Update handle_connection() to process Range requests
- [ ] Add read_range_from_file() helper
- [ ] Test locally with curl: `curl -H "Range: bytes=0-99" http://localhost:7878/`
- [ ] Implement integration tests (4+ test cases)
- [ ] Run full test suite: `cargo test`
- [ ] Run integration tests: `cargo run --bin integration_test`
- [ ] Verify with real browser video player or download manager
- [ ] Update CLAUDE.md with Range request feature documentation

---

## References

- **RFC 7233**: Hypertext Transfer Protocol (HTTP/1.1): Range Requests
  - https://tools.ietf.org/html/rfc7233
- **MDN: HTTP Range Requests**
  - https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Range
- **HTTP Status Code 206 Partial Content**
  - https://httpwg.org/specs/rfc7231.html#status.206
- **Content-Range Header**
  - https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Content-Range
