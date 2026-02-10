# Implementation Plan: Support Multi-Part Range Requests with `multipart/byteranges` Responses

## Overview

HTTP range requests allow clients to request specific byte ranges from a resource using the `Range` header (e.g., `Range: bytes=0-99, 200-299`). When a server supports multiple ranges in a single request, it responds with status code `206 Partial Content` and a `multipart/byteranges` response body, where each range is sent as a separate part of a MIME multipart message.

**Current State**: The rcomm server has no support for range requests. All GET requests return the full file (200 OK) regardless of whether a `Range` header is present. The server lacks:
- Range header parsing
- Range validation against resource size
- Single-range response support (206 with Content-Range header)
- Multi-range response support (206 with multipart/byteranges)
- Support for range-not-satisfiable errors (416 status code)

**Desired State**: The server should parse `Range` headers, handle both single-range and multi-range requests, and return appropriate 206 responses with correct boundary delimiters, Content-Range headers, and MIME type declarations for each part.

**Complexity Justification (7/10)**:
- Range parsing and validation logic is moderate
- Multipart MIME encoding requires careful boundary and header formatting
- Multiple edge cases (overlapping ranges, unsupported units, invalid syntax)
- Integration with existing response serialization is complex due to body structure

**Necessity Justification (2/10)**:
- Not commonly used by web clients (most use full GET)
- More valuable for large file serving (videos, archives)
- Resume functionality and parallel downloads benefit from this
- Low impact on typical web server usage

## Files to Modify

1. **`src/models/`** — New module for range handling
   - `src/models/range_request.rs` — Parse and validate Range headers
   - `src/models.rs` (barrel file) — Export RangeRequest

2. **`src/models/http_response.rs`** — Add range response support
   - Add fields for multipart body tracking
   - Add method to serialize multipart responses

3. **`src/main.rs`** — Integrate range request handling
   - Parse Range header from request
   - Validate ranges against file size
   - Build appropriate response (206 vs 200)
   - Serialize single-range or multipart responses

4. **`src/models/http_status_codes.rs`** — Add missing status codes
   - Add 206 (Partial Content) if not present
   - Add 416 (Range Not Satisfiable) if not present

5. **`src/bin/integration_test.rs`** — Add range request tests
   - Test single-range requests
   - Test multi-range requests
   - Test range validation and error cases

## Step-by-Step Implementation

### Step 1: Add Missing HTTP Status Codes

**File**: `src/models/http_status_codes.rs`

Verify that status codes 206 and 416 are present. If missing, add:

```rust
206 => String::from("Partial Content"),
// ... existing codes ...
416 => String::from("Range Not Satisfiable"),
```

Check line 13 and 42 in `http_status_codes.rs`. If these codes are not in the match statement, add them in numerical order.

**Verification**:
```bash
cargo test http_status_codes
```

### Step 2: Create Range Request Model

**File**: `src/models/range_request.rs` (new file)

Create a comprehensive range parsing module:

```rust
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct ByteRange {
    pub start: u64,
    pub end: u64,  // inclusive
}

#[derive(Debug, Clone)]
pub struct RangeRequest {
    pub unit: String,  // Usually "bytes"
    pub ranges: Vec<ByteRange>,
}

#[derive(Debug)]
pub enum RangeParseError {
    InvalidUnit,
    MalformedRange,
    NoRanges,
    InvalidSyntax,
}

impl fmt::Display for RangeParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RangeParseError::InvalidUnit => write!(f, "Only 'bytes' unit is supported"),
            RangeParseError::MalformedRange => write!(f, "Range format invalid (expected 'start-end')"),
            RangeParseError::NoRanges => write!(f, "No ranges specified"),
            RangeParseError::InvalidSyntax => write!(f, "Invalid Range header syntax"),
        }
    }
}

impl RangeRequest {
    /// Parse a Range header value (e.g., "bytes=0-99,200-299")
    /// Returns RangeRequest or RangeParseError
    pub fn parse(header_value: &str) -> Result<RangeRequest, RangeParseError> {
        let parts: Vec<&str> = header_value.split('=').collect();
        if parts.len() != 2 {
            return Err(RangeParseError::InvalidSyntax);
        }

        let unit = parts[0].trim().to_string();
        if unit != "bytes" {
            return Err(RangeParseError::InvalidUnit);
        }

        let ranges_str = parts[1];
        let mut ranges = Vec::new();

        for range_part in ranges_str.split(',') {
            let range_part = range_part.trim();

            // Handle suffix ranges (e.g., "-500" for last 500 bytes)
            if range_part.starts_with('-') {
                // Suffix range: not supported in this iteration
                // Could be implemented later by checking file size
                return Err(RangeParseError::MalformedRange);
            }

            let dash_parts: Vec<&str> = range_part.split('-').collect();
            if dash_parts.len() != 2 {
                return Err(RangeParseError::MalformedRange);
            }

            let start_str = dash_parts[0].trim();
            let end_str = dash_parts[1].trim();

            if start_str.is_empty() && end_str.is_empty() {
                return Err(RangeParseError::MalformedRange);
            }

            let start = start_str.parse::<u64>()
                .map_err(|_| RangeParseError::MalformedRange)?;

            // End can be empty (meaning to end of file)
            let end = if end_str.is_empty() {
                u64::MAX  // Placeholder; will be validated later
            } else {
                end_str.parse::<u64>()
                    .map_err(|_| RangeParseError::MalformedRange)?
            };

            ranges.push(ByteRange { start, end });
        }

        if ranges.is_empty() {
            return Err(RangeParseError::NoRanges);
        }

        Ok(RangeRequest { unit, ranges })
    }

    /// Validate ranges against a resource size and normalize them
    /// Returns validated ranges or error if any range is invalid
    pub fn validate(&mut self, resource_size: u64) -> Result<(), RangeParseError> {
        if self.ranges.is_empty() {
            return Err(RangeParseError::NoRanges);
        }

        // Normalize and validate each range
        for range in &mut self.ranges {
            // Replace u64::MAX with actual end of file
            if range.end == u64::MAX {
                range.end = resource_size - 1;
            }

            // Validate range bounds
            if range.start >= resource_size || range.end >= resource_size {
                return Err(RangeParseError::MalformedRange);
            }

            if range.start > range.end {
                return Err(RangeParseError::MalformedRange);
            }
        }

        Ok(())
    }

    /// Merge overlapping ranges (HTTP allows but doesn't require merging)
    /// This simplifies response generation
    pub fn merge_overlapping(&mut self) {
        if self.ranges.len() <= 1 {
            return;
        }

        // Sort by start position
        self.ranges.sort_by_key(|r| r.start);

        let mut merged = Vec::new();
        let mut current = self.ranges[0].clone();

        for range in self.ranges.iter().skip(1) {
            if range.start <= current.end + 1 {
                // Overlapping or adjacent: merge
                current.end = current.end.max(range.end);
            } else {
                // Non-overlapping: save current and start new
                merged.push(current.clone());
                current = range.clone();
            }
        }
        merged.push(current);

        self.ranges = merged;
    }

    /// Get the total size of all ranges (for Content-Length of multipart response)
    /// This is approximate and includes MIME boundary overhead
    pub fn total_range_bytes(&self) -> u64 {
        self.ranges.iter().map(|r| r.end - r.start + 1).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_range() {
        let result = RangeRequest::parse("bytes=0-99").unwrap();
        assert_eq!(result.unit, "bytes");
        assert_eq!(result.ranges.len(), 1);
        assert_eq!(result.ranges[0].start, 0);
        assert_eq!(result.ranges[0].end, 99);
    }

    #[test]
    fn parse_multiple_ranges() {
        let result = RangeRequest::parse("bytes=0-99,200-299,500-599").unwrap();
        assert_eq!(result.unit, "bytes");
        assert_eq!(result.ranges.len(), 3);
        assert_eq!(result.ranges[0].start, 0);
        assert_eq!(result.ranges[0].end, 99);
        assert_eq!(result.ranges[1].start, 200);
        assert_eq!(result.ranges[2].start, 500);
    }

    #[test]
    fn parse_range_to_eof() {
        let result = RangeRequest::parse("bytes=100-").unwrap();
        assert_eq!(result.ranges[0].start, 100);
        assert_eq!(result.ranges[0].end, u64::MAX);
    }

    #[test]
    fn parse_rejects_non_bytes_unit() {
        let result = RangeRequest::parse("words=0-99");
        assert!(result.is_err());
    }

    #[test]
    fn parse_rejects_invalid_syntax() {
        assert!(RangeRequest::parse("bytes=").is_err());
        assert!(RangeRequest::parse("0-99").is_err());
        assert!(RangeRequest::parse("bytes=0-99-100").is_err());
    }

    #[test]
    fn validate_single_range() {
        let mut req = RangeRequest::parse("bytes=0-99").unwrap();
        assert!(req.validate(1000).is_ok());
        assert_eq!(req.ranges[0].end, 99);
    }

    #[test]
    fn validate_range_to_eof() {
        let mut req = RangeRequest::parse("bytes=100-").unwrap();
        assert!(req.validate(1000).is_ok());
        assert_eq!(req.ranges[0].end, 999);
    }

    #[test]
    fn validate_rejects_out_of_bounds() {
        let mut req = RangeRequest::parse("bytes=0-999").unwrap();
        assert!(req.validate(500).is_err());
    }

    #[test]
    fn validate_rejects_start_beyond_size() {
        let mut req = RangeRequest::parse("bytes=500-999").unwrap();
        assert!(req.validate(500).is_err());
    }

    #[test]
    fn validate_rejects_inverted_range() {
        let mut req = RangeRequest::parse("bytes=100-50").unwrap();
        assert!(req.validate(1000).is_err());
    }

    #[test]
    fn merge_overlapping_ranges() {
        let mut req = RangeRequest::parse("bytes=0-99,50-199,300-399").unwrap();
        req.validate(500).unwrap();
        req.merge_overlapping();

        assert_eq!(req.ranges.len(), 2);
        assert_eq!(req.ranges[0].start, 0);
        assert_eq!(req.ranges[0].end, 199);
        assert_eq!(req.ranges[1].start, 300);
        assert_eq!(req.ranges[1].end, 399);
    }

    #[test]
    fn total_range_bytes() {
        let mut req = RangeRequest::parse("bytes=0-99,200-299").unwrap();
        req.validate(500).unwrap();
        assert_eq!(req.total_range_bytes(), 100 + 100);
    }
}
```

### Step 3: Export Range Request Module

**File**: `src/models.rs`

Add the new module to the barrel export file:

```rust
pub mod range_request;
```

Add after the existing module declarations (around line 3-5).

**Verification**:
```bash
cargo build
```

Should compile without errors.

### Step 4: Enhance HttpResponse for Multipart Bodies

**File**: `src/models/http_response.rs`

Add fields and methods to support multipart responses:

```rust
// Add this struct definition before HttpResponse
pub struct MultipartPart {
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

pub struct HttpResponse {
    version: String,
    status_code: u16,
    status_phrase: String,
    headers: HashMap<String, String>,
    body: Option<Vec<u8>>,
    multipart_boundary: Option<String>,  // Add this field
    multipart_parts: Option<Vec<MultipartPart>>,  // Add this field
}
```

Add these methods to `impl HttpResponse`:

```rust
/// Generate a boundary string for multipart responses
pub fn generate_boundary() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    format!("---boundary-{}-rcomm", nanos)
}

/// Add a part to a multipart response
pub fn add_multipart_part(&mut self, headers: HashMap<String, String>, body: Vec<u8>) -> &mut HttpResponse {
    if self.multipart_parts.is_none() {
        self.multipart_parts = Some(Vec::new());
    }
    if self.multipart_boundary.is_none() {
        self.multipart_boundary = Some(Self::generate_boundary());
    }

    self.multipart_parts.as_mut().unwrap().push(MultipartPart { headers, body });
    self
}

/// Serialize multipart body with boundaries
/// Must be called after all parts are added
pub fn finalize_multipart(&mut self) -> &mut HttpResponse {
    if let Some(parts) = &self.multipart_parts {
        if let Some(boundary) = &self.multipart_boundary {
            let mut multipart_body = Vec::new();

            for part in parts {
                // Write boundary
                multipart_body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());

                // Write part headers
                for (key, value) in &part.headers {
                    multipart_body.extend_from_slice(format!("{}: {}\r\n", key, value).as_bytes());
                }
                multipart_body.extend_from_slice(b"\r\n");

                // Write part body
                multipart_body.extend_from_slice(&part.body);
                multipart_body.extend_from_slice(b"\r\n");
            }

            // Write closing boundary
            multipart_body.extend_from_slice(format!("--{}--\r\n", boundary).as_bytes());

            // Set the full multipart body
            self.add_body(multipart_body);
        }
    }
    self
}

/// Return true if this response has multipart content
pub fn is_multipart(&self) -> bool {
    self.multipart_parts.is_some()
}
```

Update the `build()` method to initialize the new fields:

```rust
pub fn build(version: String, code: u16) -> HttpResponse {
    let headers = HashMap::<String, String>::new();
    let phrase = get_status_phrase(code);
    HttpResponse {
        version,
        status_code: code,
        status_phrase: phrase,
        headers,
        body: None,
        multipart_boundary: None,  // Add this
        multipart_parts: None,  // Add this
    }
}
```

Add tests after line 181:

```rust
#[test]
fn multipart_response_boundary_generation() {
    let boundary1 = HttpResponse::generate_boundary();
    let boundary2 = HttpResponse::generate_boundary();
    // Boundaries should be different (based on time)
    // But both should have correct format
    assert!(boundary1.starts_with("---boundary-"));
    assert!(boundary2.starts_with("---boundary-"));
}

#[test]
fn multipart_response_with_two_parts() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 206);
    resp.add_header("Content-Type".to_string(), "multipart/byteranges".to_string());

    let mut part1_headers = HashMap::new();
    part1_headers.insert("Content-Type".to_string(), "text/plain".to_string());
    part1_headers.insert("Content-Range".to_string(), "bytes 0-99/500".to_string());
    resp.add_multipart_part(part1_headers, b"first hundred bytes".to_vec());

    let mut part2_headers = HashMap::new();
    part2_headers.insert("Content-Type".to_string(), "text/plain".to_string());
    part2_headers.insert("Content-Range".to_string(), "bytes 200-299/500".to_string());
    resp.add_multipart_part(part2_headers, b"second hundred bytes".to_vec());

    resp.finalize_multipart();

    let body = resp.try_get_body().unwrap();
    let body_str = String::from_utf8(body).unwrap();

    // Verify boundaries are present
    assert!(body_str.contains("--"));
    assert!(body_str.contains("Content-Range"));
    assert!(body_str.contains("first hundred bytes"));
    assert!(body_str.contains("second hundred bytes"));
}

#[test]
fn multipart_response_has_correct_content_type() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 206);
    let boundary = resp.generate_boundary();
    let ct = format!("multipart/byteranges; boundary={}", boundary);
    resp.add_header("Content-Type".to_string(), ct.clone());

    assert_eq!(
        resp.try_get_header("Content-Type".to_string()),
        Some(ct)
    );
}
```

### Step 5: Integrate Range Request Handling in main.rs

**File**: `src/main.rs`

Import the new modules at the top:

```rust
use rcomm::models::range_request::RangeRequest;
```

Replace the `handle_connection()` function with enhanced version:

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

    let contents = fs::read_to_string(filename).unwrap();
    let file_size = contents.len() as u64;

    // Check for Range header
    if let Some(range_header) = http_request.try_get_header("range".to_string()) {
        // Only process ranges for successful (200) responses, not 404
        if response.status_code == 200 {
            match RangeRequest::parse(&range_header) {
                Ok(mut range_req) => {
                    match range_req.validate(file_size) {
                        Ok(_) => {
                            // Ranges are valid
                            range_req.merge_overlapping();

                            if range_req.ranges.len() == 1 {
                                // Single range: send 206 with Content-Range header
                                response = HttpResponse::build(String::from("HTTP/1.1"), 206);
                                let range = &range_req.ranges[0];
                                let range_bytes = contents.as_bytes();
                                let range_data = range_bytes[range.start as usize..=(range.end as usize)].to_vec();

                                response.add_header(
                                    "Content-Range".to_string(),
                                    format!("bytes {}-{}/{}", range.start, range.end, file_size)
                                );
                                response.add_body(range_data);
                            } else {
                                // Multiple ranges: send 206 with multipart body
                                response = HttpResponse::build(String::from("HTTP/1.1"), 206);
                                let boundary = HttpResponse::generate_boundary();
                                response.add_header(
                                    "Content-Type".to_string(),
                                    format!("multipart/byteranges; boundary={}", boundary)
                                );

                                let range_bytes = contents.as_bytes();
                                for range in &range_req.ranges {
                                    let mut part_headers = HashMap::new();
                                    part_headers.insert("Content-Type".to_string(), "text/plain".to_string());
                                    part_headers.insert(
                                        "Content-Range".to_string(),
                                        format!("bytes {}-{}/{}", range.start, range.end, file_size)
                                    );

                                    let part_data = range_bytes[range.start as usize..=(range.end as usize)].to_vec();
                                    response.add_multipart_part(part_headers, part_data);
                                }

                                response.finalize_multipart();
                            }
                        }
                        Err(_) => {
                            // Invalid ranges: return 416
                            response = HttpResponse::build(String::from("HTTP/1.1"), 416);
                            response.add_header("Content-Range".to_string(), format!("bytes */{}", file_size));
                            response.add_body(b"Requested range not satisfiable".to_vec());
                        }
                    }
                }
                Err(_) => {
                    // Failed to parse Range header; ignore it and return full content (200)
                    response.add_body(contents.into());
                }
            }
        } else {
            // Non-200 response; ignore Range header and return full body
            response.add_body(contents.into());
        }
    } else {
        // No Range header; return full content
        response.add_body(contents.into());
    }

    println!("Response: {response}");
    stream.write_all(&response.as_bytes()).unwrap();
}
```

### Step 6: Add Integration Tests

**File**: `src/bin/integration_test.rs`

Add helper function for sending requests with Range headers:

```rust
fn send_request_with_range(addr: &str, method: &str, target: &str, range: Option<&str>) -> Result<TestResponse, String> {
    let mut stream = TcpStream::connect(addr)
        .map_err(|e| format!("Connection failed: {}", e))?;

    let mut request = format!("{} {} HTTP/1.1\r\nHost: localhost\r\n", method, target);
    if let Some(range_header) = range {
        request.push_str(&format!("Range: {}\r\n", range_header));
    }
    request.push_str("\r\n");

    stream.write_all(request.as_bytes())
        .map_err(|e| format!("Write failed: {}", e))?;
    stream.shutdown(std::net::Shutdown::Write)
        .map_err(|e| format!("Shutdown failed: {}", e))?;

    let mut response = String::new();
    std::io::Read::read_to_string(&mut stream, &mut response)
        .map_err(|e| format!("Read failed: {}", e))?;

    parse_response(&response)
}
```

Add test functions after existing tests:

```rust
fn test_single_range_request(addr: &str) -> Result<(), String> {
    let resp = send_request_with_range(addr, "GET", "/", Some("bytes=0-99"))?;
    assert_eq_or_err(&resp.status_code, &206, "status should be 206")?;
    assert!(resp.headers.contains_key("content-range"), "missing content-range header")?;
    assert!(resp.body.len() <= 100, "body should be at most 100 bytes")?;
    Ok(())
}

fn test_single_range_includes_content_range(addr: &str) -> Result<(), String> {
    let resp = send_request_with_range(addr, "GET", "/", Some("bytes=0-99"))?;
    let content_range = resp.headers.get("content-range")
        .ok_or("missing content-range header")?;
    assert!(content_range.contains("bytes 0-99"), "content-range format incorrect")?;
    Ok(())
}

fn test_multiple_range_request(addr: &str) -> Result<(), String> {
    let resp = send_request_with_range(addr, "GET", "/", Some("bytes=0-49,100-149"))?;
    assert_eq_or_err(&resp.status_code, &206, "status should be 206")?;

    // Should have multipart content type
    let ct = resp.headers.get("content-type")
        .ok_or("missing content-type")?;
    assert!(ct.contains("multipart/byteranges"), "content-type should be multipart")?;
    assert!(ct.contains("boundary="), "should have boundary parameter")?;

    // Body should contain boundaries
    assert!(resp.body.contains("--"), "body should contain boundary markers")?;
    Ok(())
}

fn test_invalid_range_returns_416(addr: &str) -> Result<(), String> {
    // Request range beyond file size
    let resp = send_request_with_range(addr, "GET", "/", Some("bytes=100000-200000"))?;
    assert_eq_or_err(&resp.status_code, &416, "status should be 416")?;
    assert!(resp.headers.contains_key("content-range"), "should have content-range header")?;
    Ok(())
}

fn test_malformed_range_ignored(addr: &str) -> Result<(), String> {
    // Server should ignore malformed Range header and return 200 with full body
    let resp = send_request_with_range(addr, "GET", "/", Some("bytes=invalid"))?;
    assert_eq_or_err(&resp.status_code, &200, "status should be 200 when range is malformed")?;
    Ok(())
}

fn test_range_on_404_ignored(addr: &str) -> Result<(), String> {
    // Range header should be ignored for 404 responses
    let resp = send_request_with_range(addr, "GET", "/nonexistent", Some("bytes=0-99"))?;
    assert_eq_or_err(&resp.status_code, &404, "status should be 404")?;
    assert!(!resp.headers.contains_key("content-range"), "404 should not have content-range")?;
    Ok(())
}

fn test_range_to_eof(addr: &str) -> Result<(), String> {
    let resp = send_request_with_range(addr, "GET", "/", Some("bytes=100-"))?;
    assert_eq_or_err(&resp.status_code, &206, "status should be 206")?;
    let content_range = resp.headers.get("content-range")
        .ok_or("missing content-range")?;
    assert!(content_range.contains("100-"), "should handle open-ended range")?;
    Ok(())
}

fn test_overlapping_ranges_merged(addr: &str) -> Result<(), String> {
    // Overlapping ranges should be merged
    let resp = send_request_with_range(addr, "GET", "/", Some("bytes=0-99,50-149"))?;
    assert_eq_or_err(&resp.status_code, &206, "status should be 206")?;

    // Should result in at least one Content-Range, possibly merged
    let ct = resp.headers.get("content-type")
        .ok_or("missing content-type")?;
    assert!(ct.contains("multipart/byteranges") || ct.contains("text/plain"),
        "content-type should indicate range response")?;
    Ok(())
}
```

Register these tests in `main()` by adding them to the results vector:

```rust
results.push(run_test("Single range request", || test_single_range_request(&addr)));
results.push(run_test("Single range includes Content-Range", || test_single_range_includes_content_range(&addr)));
results.push(run_test("Multiple range request", || test_multiple_range_request(&addr)));
results.push(run_test("Invalid range returns 416", || test_invalid_range_returns_416(&addr)));
results.push(run_test("Malformed range ignored", || test_malformed_range_ignored(&addr)));
results.push(run_test("Range on 404 ignored", || test_range_on_404_ignored(&addr)));
results.push(run_test("Range to EOF", || test_range_to_eof(&addr)));
results.push(run_test("Overlapping ranges merged", || test_overlapping_ranges_merged(&addr)));
```

## Testing Strategy

### Unit Tests

1. **Range Parsing** (`src/models/range_request.rs`):
   - Single range: `bytes=0-99`
   - Multiple ranges: `bytes=0-99,200-299,500-599`
   - Open-ended range: `bytes=100-`
   - Invalid unit: `words=0-99` (rejected)
   - Invalid syntax: `bytes=`, `0-99`, `bytes=0-99-100` (all rejected)
   - Run with: `cargo test range_request::tests`

2. **Range Validation** (`src/models/range_request.rs`):
   - Valid range within file size
   - Range to EOF validation
   - Out-of-bounds range rejection
   - Start position beyond file size
   - Inverted range (start > end) rejection
   - Run with: `cargo test range_request::tests::validate`

3. **Range Merging** (`src/models/range_request.rs`):
   - Overlapping ranges merge correctly
   - Adjacent ranges merge
   - Non-overlapping ranges preserved
   - Run with: `cargo test range_request::tests::merge`

4. **HttpResponse Multipart** (`src/models/http_response.rs`):
   - Boundary generation produces valid format
   - Multipart serialization includes all parts
   - Multipart includes boundary markers
   - Content-Range headers present in each part
   - Run with: `cargo test http_response::tests::multipart`

### Integration Tests

1. **Single-Range Requests**:
   - GET / with `Range: bytes=0-99` returns 206
   - Response includes `Content-Range: bytes 0-99/*` header
   - Response body is exactly 100 bytes (or less if file smaller)
   - Content-Length matches body size

2. **Multi-Range Requests**:
   - GET / with `Range: bytes=0-99,200-299` returns 206
   - Content-Type includes `multipart/byteranges; boundary=...`
   - Response body contains both ranges separated by boundary
   - Each part has `Content-Range` header

3. **Error Handling**:
   - Invalid range (e.g., `bytes=100000-200000` on small file) returns 416
   - 416 response includes `Content-Range: bytes */<size>`
   - Malformed range (e.g., `bytes=invalid`) returns 200 with full body
   - Unsupported unit (e.g., `words=0-99`) returns 200 with full body

4. **Edge Cases**:
   - Range on 404 response: ignored, returns 404
   - Range on 400 response: ignored, returns 400
   - Open-ended range: `bytes=100-` treated as `bytes=100-<end>`
   - Overlapping ranges: merged into fewer ranges or multipart parts
   - Adjacent ranges: merged (e.g., `0-99,100-199` becomes `0-199`)

5. **Run All Integration Tests**:
   ```bash
   cargo run --bin integration_test
   ```
   Expected: 8 new tests + existing tests, all passing

### Manual Testing

```bash
cargo build
cargo run &
sleep 2

# Single range
curl -i -H "Range: bytes=0-99" http://127.0.0.1:7878/
# Expected: 206 Partial Content, 100 bytes

# Multiple ranges
curl -i -H "Range: bytes=0-99,200-299" http://127.0.0.1:7878/
# Expected: 206 Partial Content, multipart/byteranges

# Invalid range
curl -i -H "Range: bytes=999999-9999999" http://127.0.0.1:7878/
# Expected: 416 Range Not Satisfiable

# No Range header (baseline)
curl -i http://127.0.0.1:7878/
# Expected: 200 OK with full body
```

## Edge Cases

### 1. **Overlapping Ranges**
- **Scenario**: Client sends `bytes=0-99,50-149` (overlapping)
- **Expected Behavior**: Server can either merge into `0-149` or send both (implementation merges)
- **Implementation**: `RangeRequest::merge_overlapping()` merges overlapping ranges before response generation
- **Test**: `test_overlapping_ranges_merged()`

### 2. **Adjacent Ranges**
- **Scenario**: Client sends `bytes=0-99,100-199` (adjacent)
- **Expected Behavior**: Server can merge into `0-199` or send separately (implementation merges)
- **Implementation**: Merge logic uses `range.start <= current.end + 1` to catch adjacency
- **Test**: Covered by `test_overlapping_ranges_merged()`

### 3. **Out-of-Order Ranges**
- **Scenario**: Client sends `bytes=200-299,0-99` (not in ascending order)
- **Expected Behavior**: Server should process correctly (implementation sorts)
- **Implementation**: `merge_overlapping()` sorts ranges before merging
- **Test**: Add `test_out_of_order_ranges()` to verify

### 4. **Open-Ended Ranges**
- **Scenario**: Client sends `bytes=100-` (no end specified)
- **Expected Behavior**: Server should treat as `bytes=100-<file_size-1>`
- **Implementation**: During validation, `end == u64::MAX` is replaced with `resource_size - 1`
- **Test**: `test_range_to_eof()`

### 5. **Range on Non-200 Response**
- **Scenario**: Client sends `Range: bytes=0-99` for nonexistent resource (404)
- **Expected Behavior**: Server ignores Range header, returns 404 with full body
- **Implementation**: Range parsing only occurs if `response.status_code == 200`
- **Test**: `test_range_on_404_ignored()`

### 6. **Range Beyond File Size**
- **Scenario**: Client sends `Range: bytes=0-999` for 500-byte file
- **Expected Behavior**: Server returns 416 Range Not Satisfiable
- **Implementation**: `validate()` checks if range bounds exceed file size
- **Test**: `test_invalid_range_returns_416()`

### 7. **Invalid Syntax**
- **Scenario**: Client sends `Range: bytes=abc-xyz` or `Range: 0-99` (missing unit)
- **Expected Behavior**: Server ignores malformed header, returns 200 with full body
- **Implementation**: `RangeRequest::parse()` returns error, caught in handler, full body sent
- **Test**: `test_malformed_range_ignored()`

### 8. **Unsupported Units**
- **Scenario**: Client sends `Range: words=0-99` (non-bytes unit)
- **Expected Behavior**: Server ignores unsupported unit, returns 200 with full body
- **Implementation**: `parse()` validates unit == "bytes" and returns error otherwise
- **Test**: Could add `test_unsupported_unit_ignored()`

### 9. **Very Large Files**
- **Scenario**: File size is gigabytes; client requests small range
- **Expected Behavior**: Server only sends requested range (memory efficient)
- **Implementation**: File is read into memory (current design), then sliced; no streaming
- **Limitation**: rcomm reads full file into memory, so very large files may cause OOM
- **Note**: This is a pre-existing limitation, not introduced by range requests

### 10. **Many Ranges**
- **Scenario**: Client sends `Range: bytes=0-99,100-199,200-299,...,9900-9999` (100 ranges)
- **Expected Behavior**: Server handles efficiently, serializes multipart response
- **Implementation**: No limit on number of ranges; multipart serialization iterates through all
- **Test**: Could add `test_many_ranges()` with reasonable limit (e.g., 20 ranges)

### 11. **Empty Ranges**
- **Scenario**: Client sends `Range: bytes=` (no range specified)
- **Expected Behavior**: Server ignores as malformed, returns 200
- **Implementation**: `parse()` returns `NoRanges` error
- **Test**: Covered by `test_malformed_range_ignored()`

### 12. **Suffix Ranges**
- **Scenario**: Client sends `Range: bytes=-500` (last 500 bytes)
- **Expected Behavior**: Server returns last 500 bytes as single range
- **Implementation**: Current implementation rejects suffix ranges with `MalformedRange` error
- **Future**: Could be implemented later by special handling in `parse()` and requiring `validate()` to resolve
- **Note**: Marked as "not supported in this iteration"

## Checklist

- [ ] Add 206 and 416 status codes to `http_status_codes.rs`
- [ ] Create `src/models/range_request.rs` with RangeRequest and ByteRange structs
- [ ] Add parsing logic: `RangeRequest::parse()`
- [ ] Add validation logic: `RangeRequest::validate()`
- [ ] Add merging logic: `RangeRequest::merge_overlapping()`
- [ ] Add 10 unit tests to `range_request.rs`
- [ ] Export module via `src/models.rs`
- [ ] Add `multipart_boundary` and `multipart_parts` fields to `HttpResponse`
- [ ] Add `generate_boundary()` method to `HttpResponse`
- [ ] Add `add_multipart_part()` method to `HttpResponse`
- [ ] Add `finalize_multipart()` method to `HttpResponse`
- [ ] Add `is_multipart()` helper method
- [ ] Update `HttpResponse::build()` to initialize new fields
- [ ] Add 3 unit tests for multipart serialization
- [ ] Modify `handle_connection()` to parse Range header
- [ ] Implement single-range response (206 with Content-Range)
- [ ] Implement multi-range response (206 with multipart/byteranges)
- [ ] Implement range validation error handling (416)
- [ ] Ignore malformed ranges (return 200)
- [ ] Ignore ranges on non-200 responses
- [ ] Add helper `send_request_with_range()` to integration tests
- [ ] Add 8 integration tests for range requests
- [ ] Run `cargo test` (all unit tests pass)
- [ ] Run `cargo run --bin integration_test` (all integration tests pass)
- [ ] Manual testing with curl
- [ ] Verify multipart boundaries are correct
- [ ] Verify Content-Range headers are accurate
- [ ] Test all edge cases

## Success Criteria

1. **All Tests Pass**:
   - `cargo test` shows all unit tests passing (34 existing + 13 new = 47)
   - `cargo run --bin integration_test` shows all tests passing (12 existing + 8 new = 20)

2. **Single-Range Support**:
   - GET with `Range: bytes=0-99` returns 206 Partial Content
   - Response includes `Content-Range: bytes 0-99/*` header
   - Response body is exactly the requested range (100 bytes or less)
   - Content-Length matches actual body size

3. **Multi-Range Support**:
   - GET with `Range: bytes=0-99,200-299` returns 206 Partial Content
   - Content-Type is `multipart/byteranges; boundary=<boundary>`
   - Body contains boundary markers (`--<boundary>`)
   - Each part has `Content-Range` and `Content-Type` headers
   - Parts are properly separated by boundaries

4. **Range Validation**:
   - Out-of-bounds ranges return 416 Range Not Satisfiable
   - Response includes `Content-Range: bytes */<size>`
   - Malformed ranges are ignored (return 200)
   - Unsupported units are ignored (return 200)

5. **Range Merging**:
   - Overlapping ranges are merged before response generation
   - Result is fewer parts or ranges in final response
   - Multi-range response never exceeds actual range count

6. **Protocol Compliance**:
   - Status code 206 used only for successful ranges
   - Status code 416 used for unsatisfiable ranges
   - Content-Range format matches RFC 7233: `bytes <start>-<end>/<size>`
   - Multipart boundary format follows RFC 2046 conventions

7. **Client Behavior**:
   - `curl -i -H "Range: bytes=0-99" http://localhost:7878/` succeeds with 206
   - Browser range requests work correctly (pause/resume downloads)
   - Range requests on error responses (404) are ignored

8. **Code Quality**:
   - No new `.unwrap()` in critical paths
   - Range parsing is robust to malformed input
   - Error handling uses `Result` types appropriately
   - Multipart serialization is correct and tested

9. **Backward Compatibility**:
   - Requests without Range header work identically to before
   - Full file retrieval (200 OK) is unaffected
   - Error responses (404, 400) unchanged
   - Existing integration tests all pass

## Implementation Difficulty: 7/10

**Rationale**:
- Range parsing is moderate complexity (splitting, parsing integers, validation)
- Multipart MIME encoding has specific format requirements (boundaries, headers)
- Integration with existing response serialization is non-trivial
- Edge cases (overlapping ranges, validation) require careful logic
- 5 new structs/methods + modifications to 3 existing files
- 13+ unit tests and 8+ integration tests to implement and verify

## Risk Assessment: Medium

**Risks**:
- **Boundary Collisions**: Generated boundary could theoretically match file content (low probability but possible)
  - Mitigation: Use UUID or timestamp-based boundary, add suffix
- **Multipart Parsing**: Clients must correctly parse boundaries and parts
  - Mitigation: Thorough testing with multiple clients (curl, browsers)
- **Large File Memory**: Entire file read into memory before slicing
  - Mitigation: Document limitation; this is pre-existing rcomm design
- **String Slicing**: Unsafe if byte offsets don't align with UTF-8 boundaries
  - Mitigation: Work with byte slices (Vec<u8>), not string indices
- **Performance**: Many ranges means many iterations and boundary writes
  - Mitigation: Merge overlapping ranges; reasonable limit on parts (could add)

**Backward Compatibility**: LOW RISK
- Requests without Range header treated identically
- Existing error handling paths unchanged
- Only new functionality added

**Correctness**: MEDIUM RISK
- Range validation logic must be airtight
- Multipart format must match RFC 7233 exactly
- Content-Range headers must be accurate
- Mitigation: Comprehensive test suite covering all cases

