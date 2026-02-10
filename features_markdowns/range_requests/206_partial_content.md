# Implementation Plan: Return 206 Partial Content with Content-Range Header for Valid Range Requests

## Overview

HTTP range requests allow clients to request specific byte ranges of a resource instead of downloading the entire file. This feature implements support for the `Range` request header by returning HTTP 206 Partial Content responses with a `Content-Range` header that specifies which bytes are being sent.

**Current State**: The server's `handle_connection()` function in `src/main.rs` reads entire files into memory and returns them with HTTP 200 OK responses. It does not parse or handle the `Range` header from incoming requests.

**Desired State**: When a client sends a valid `Range` header (e.g., `Range: bytes=0-99`), the server should:
1. Parse the Range header to extract the requested byte range
2. Validate that the range is satisfiable (within file bounds)
3. Return HTTP 206 Partial Content with the requested bytes
4. Include a `Content-Range` header indicating which bytes are being sent (e.g., `Content-Range: bytes 0-99/1000`)
5. Handle invalid/unsatisfiable ranges gracefully with HTTP 416 Range Not Satisfiable

**RFC Reference**: [RFC 7233 - HTTP/1.1 Range Requests](https://tools.ietf.org/html/rfc7233)

---

## Files to Modify

1. **`src/models/http_status_codes.rs`** — Add status phrase for code 416
   - Already has 206, verify it's present

2. **`src/models/` (new file)** — Add range parsing logic (optional but recommended)
   - Create `src/models/range_request.rs` to handle Range header parsing
   - Exports `struct RangeSpec` and parsing functions
   - Alternative: Inline logic in `src/main.rs` (simpler, less modular)
   - **Recommendation**: Create separate module for testability and reusability

3. **`src/models.rs`** — Update barrel export if creating new module
   - Add `pub mod range_request;` if new file is created

4. **`src/main.rs`** — Primary changes
   - Import range request parsing logic
   - Modify `handle_connection()` to:
     - Check for Range header in incoming request
     - Parse and validate range
     - Calculate actual bytes to send
     - Build response with status 206 or 416 as appropriate
     - Include Content-Range header

5. **`src/models/http_response.rs`** — Secondary changes (helper method)
   - Add method `add_partial_body()` to set body and auto-calculate Content-Length only for partial content
   - Alternative: Modify `add_body()` to accept optional Content-Range info
   - **Recommendation**: New method `add_partial_body(body, content_range_header)` for clarity

---

## Step-by-Step Implementation

### Step 1: Create Range Request Parsing Module

**File**: `src/models/range_request.rs` (NEW)

Create a new module to encapsulate range parsing logic:

```rust
use std::fmt;

/// Represents a parsed Range header value
/// Format: bytes=0-99, bytes=100-199, bytes=-500, bytes=9500-
#[derive(Debug, Clone, PartialEq)]
pub enum RangeSpec {
    /// Byte range: start-end (inclusive on both ends)
    /// Example: bytes=0-99 represents bytes 0, 1, ..., 99 (100 bytes total)
    ByteRange { start: u64, end: u64 },
    /// Suffix range: last N bytes
    /// Example: bytes=-500 represents the last 500 bytes
    SuffixRange { suffix_length: u64 },
    /// Open-ended range: from start to end of file
    /// Example: bytes=9500- represents from byte 9500 to EOF
    OpenRange { start: u64 },
}

impl fmt::Display for RangeSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RangeSpec::ByteRange { start, end } => write!(f, "bytes={}-{}", start, end),
            RangeSpec::SuffixRange { suffix_length } => write!(f, "bytes=-{}", suffix_length),
            RangeSpec::OpenRange { start } => write!(f, "bytes={}-", start),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RangeParseError {
    /// "bytes" prefix is missing
    MissingBytesPrefix,
    /// Range format is invalid (e.g., "0-100-200")
    MalformedRange,
    /// Numeric parts cannot be parsed as u64
    InvalidNumber,
    /// Range start > end (e.g., bytes=100-50)
    InvalidRange,
    /// Unsupported format (e.g., "bytes=0-99, 200-299" — multiple ranges not supported)
    MultipleRanges,
    /// Empty range spec (e.g., "bytes=")
    EmptyRange,
}

impl fmt::Display for RangeParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RangeParseError::MissingBytesPrefix => write!(f, "Range header missing 'bytes=' prefix"),
            RangeParseError::MalformedRange => write!(f, "Malformed Range header format"),
            RangeParseError::InvalidNumber => write!(f, "Range contains invalid byte offset"),
            RangeParseError::InvalidRange => write!(f, "Range start is greater than end"),
            RangeParseError::MultipleRanges => write!(f, "Multiple byte ranges not supported (RFC 7233 allows but rcomm implements single range)"),
            RangeParseError::EmptyRange => write!(f, "Range specification is empty"),
        }
    }
}

/// Parse a Range header value
/// Supports:
///   - bytes=0-99       (specific range)
///   - bytes=-500       (last 500 bytes)
///   - bytes=9500-      (from 9500 to EOF)
///
/// Does NOT support multiple ranges (bytes=0-99,200-299) per RFC 7233 simplification
pub fn parse_range_header(header_value: &str) -> Result<RangeSpec, RangeParseError> {
    let trimmed = header_value.trim();

    // Check for "bytes=" prefix
    if !trimmed.starts_with("bytes=") {
        return Err(RangeParseError::MissingBytesPrefix);
    }

    let range_part = &trimmed[6..]; // Skip "bytes="

    // Check for multiple ranges (not supported)
    if range_part.contains(',') {
        return Err(RangeParseError::MultipleRanges);
    }

    if range_part.is_empty() {
        return Err(RangeParseError::EmptyRange);
    }

    // Handle suffix range: bytes=-500
    if range_part.starts_with('-') {
        let suffix_str = &range_part[1..];
        if suffix_str.is_empty() {
            return Err(RangeParseError::MalformedRange);
        }
        let suffix_length = suffix_str
            .parse::<u64>()
            .map_err(|_| RangeParseError::InvalidNumber)?;
        if suffix_length == 0 {
            return Err(RangeParseError::MalformedRange);
        }
        return Ok(RangeSpec::SuffixRange { suffix_length });
    }

    // Handle closed or open range: bytes=0-99 or bytes=9500-
    let parts: Vec<&str> = range_part.split('-').collect();
    if parts.len() != 2 {
        return Err(RangeParseError::MalformedRange);
    }

    let start_str = parts[0];
    let end_str = parts[1];

    if start_str.is_empty() {
        return Err(RangeParseError::MalformedRange);
    }

    let start = start_str
        .parse::<u64>()
        .map_err(|_| RangeParseError::InvalidNumber)?;

    // Open-ended range: bytes=9500-
    if end_str.is_empty() {
        return Ok(RangeSpec::OpenRange { start });
    }

    // Closed range: bytes=0-99
    let end = end_str
        .parse::<u64>()
        .map_err(|_| RangeParseError::InvalidNumber)?;

    if start > end {
        return Err(RangeParseError::InvalidRange);
    }

    Ok(RangeSpec::ByteRange { start, end })
}

/// Calculate the actual byte range for a given file size
/// Returns (start_byte, end_byte, total_size) where end_byte is INCLUSIVE
/// Returns Err if the range is not satisfiable
pub fn resolve_range(spec: RangeSpec, file_size: u64) -> Result<(u64, u64, u64), RangeResolveError> {
    match spec {
        RangeSpec::ByteRange { start, end } => {
            // Validate: start must be within bounds
            if start >= file_size {
                return Err(RangeResolveError::RangeNotSatisfiable);
            }
            // Clamp end to file boundary
            let actual_end = std::cmp::min(end, file_size - 1);
            Ok((start, actual_end, file_size))
        }
        RangeSpec::SuffixRange { suffix_length } => {
            // Last N bytes: from (file_size - suffix_length) to (file_size - 1)
            if suffix_length >= file_size {
                // Suffix is larger than file, return entire file
                Ok((0, file_size - 1, file_size))
            } else {
                let start = file_size - suffix_length;
                Ok((start, file_size - 1, file_size))
            }
        }
        RangeSpec::OpenRange { start } => {
            // From start to EOF
            if start >= file_size {
                return Err(RangeResolveError::RangeNotSatisfiable);
            }
            Ok((start, file_size - 1, file_size))
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RangeResolveError {
    /// The range cannot be satisfied (start >= file_size)
    RangeNotSatisfiable,
}

impl fmt::Display for RangeResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RangeResolveError::RangeNotSatisfiable => {
                write!(f, "Range not satisfiable for the given file size")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // PARSING TESTS

    #[test]
    fn parse_closed_byte_range() {
        let result = parse_range_header("bytes=0-99");
        assert_eq!(
            result,
            Ok(RangeSpec::ByteRange {
                start: 0,
                end: 99
            })
        );
    }

    #[test]
    fn parse_open_ended_range() {
        let result = parse_range_header("bytes=9500-");
        assert_eq!(result, Ok(RangeSpec::OpenRange { start: 9500 }));
    }

    #[test]
    fn parse_suffix_range() {
        let result = parse_range_header("bytes=-500");
        assert_eq!(
            result,
            Ok(RangeSpec::SuffixRange {
                suffix_length: 500
            })
        );
    }

    #[test]
    fn parse_single_byte() {
        let result = parse_range_header("bytes=0-0");
        assert_eq!(
            result,
            Ok(RangeSpec::ByteRange {
                start: 0,
                end: 0
            })
        );
    }

    #[test]
    fn parse_with_whitespace() {
        let result = parse_range_header("  bytes=0-99  ");
        assert_eq!(
            result,
            Ok(RangeSpec::ByteRange {
                start: 0,
                end: 99
            })
        );
    }

    #[test]
    fn parse_large_numbers() {
        let result = parse_range_header("bytes=1000000-2000000");
        assert_eq!(
            result,
            Ok(RangeSpec::ByteRange {
                start: 1000000,
                end: 2000000
            })
        );
    }

    #[test]
    fn reject_missing_bytes_prefix() {
        let result = parse_range_header("0-99");
        assert_eq!(result, Err(RangeParseError::MissingBytesPrefix));
    }

    #[test]
    fn reject_multiple_ranges() {
        let result = parse_range_header("bytes=0-99, 200-299");
        assert_eq!(result, Err(RangeParseError::MultipleRanges));
    }

    #[test]
    fn reject_invalid_range_start_greater_than_end() {
        let result = parse_range_header("bytes=100-50");
        assert_eq!(result, Err(RangeParseError::InvalidRange));
    }

    #[test]
    fn reject_malformed_range_too_many_dashes() {
        let result = parse_range_header("bytes=0-50-100");
        assert_eq!(result, Err(RangeParseError::MalformedRange));
    }

    #[test]
    fn reject_invalid_numbers() {
        let result = parse_range_header("bytes=abc-def");
        assert_eq!(result, Err(RangeParseError::InvalidNumber));
    }

    #[test]
    fn reject_empty_range() {
        let result = parse_range_header("bytes=");
        assert_eq!(result, Err(RangeParseError::EmptyRange));
    }

    #[test]
    fn reject_empty_suffix() {
        let result = parse_range_header("bytes=-");
        assert_eq!(result, Err(RangeParseError::MalformedRange));
    }

    // RESOLUTION TESTS

    #[test]
    fn resolve_closed_range_within_bounds() {
        let spec = RangeSpec::ByteRange {
            start: 0,
            end: 99,
        };
        let result = resolve_range(spec, 1000);
        assert_eq!(result, Ok((0, 99, 1000)));
    }

    #[test]
    fn resolve_closed_range_partial_file() {
        let spec = RangeSpec::ByteRange {
            start: 100,
            end: 500,
        };
        let result = resolve_range(spec, 300);
        // end_byte should be clamped to file_size - 1 = 299
        assert_eq!(result, Ok((100, 299, 300)));
    }

    #[test]
    fn resolve_closed_range_out_of_bounds() {
        let spec = RangeSpec::ByteRange {
            start: 1000,
            end: 1999,
        };
        let result = resolve_range(spec, 500);
        assert_eq!(result, Err(RangeResolveError::RangeNotSatisfiable));
    }

    #[test]
    fn resolve_open_range() {
        let spec = RangeSpec::OpenRange { start: 9500 };
        let result = resolve_range(spec, 10000);
        assert_eq!(result, Ok((9500, 9999, 10000)));
    }

    #[test]
    fn resolve_open_range_out_of_bounds() {
        let spec = RangeSpec::OpenRange { start: 10000 };
        let result = resolve_range(spec, 10000);
        assert_eq!(result, Err(RangeResolveError::RangeNotSatisfiable));
    }

    #[test]
    fn resolve_suffix_smaller_than_file() {
        let spec = RangeSpec::SuffixRange {
            suffix_length: 500,
        };
        let result = resolve_range(spec, 1000);
        assert_eq!(result, Ok((500, 999, 1000)));
    }

    #[test]
    fn resolve_suffix_larger_than_file() {
        let spec = RangeSpec::SuffixRange {
            suffix_length: 2000,
        };
        let result = resolve_range(spec, 1000);
        // Returns entire file
        assert_eq!(result, Ok((0, 999, 1000)));
    }

    #[test]
    fn resolve_suffix_equal_to_file_size() {
        let spec = RangeSpec::SuffixRange {
            suffix_length: 1000,
        };
        let result = resolve_range(spec, 1000);
        // Returns entire file
        assert_eq!(result, Ok((0, 999, 1000)));
    }

    #[test]
    fn resolve_single_byte() {
        let spec = RangeSpec::ByteRange {
            start: 0,
            end: 0,
        };
        let result = resolve_range(spec, 100);
        assert_eq!(result, Ok((0, 0, 100)));
    }
}
```

### Step 2: Update Barrel Export

**File**: `src/models.rs`

Add the new module to the barrel export (if creating new file):

```rust
pub mod http_methods;
pub mod http_request;
pub mod http_response;
pub mod http_status_codes;
pub mod range_request;  // ADD THIS LINE
```

### Step 3: Verify HTTP Status Code 416

**File**: `src/models/http_status_codes.rs`

Check that code 416 is already present (line 42):

```rust
416 => String::from("Range Not Satisfiable"),
```

It's already there, so no changes needed. Code 206 (line 13) is also already present:

```rust
206 => String::from("Partial Content"),
```

### Step 4: Add Content-Range Header Helper to HttpResponse

**File**: `src/models/http_response.rs`

Add a new helper method to make setting Content-Range headers easier:

```rust
pub fn add_content_range(&mut self, start: u64, end: u64, total: u64) -> &mut HttpResponse {
    let range_header = format!("bytes {}-{}/{}", start, end, total);
    self.add_header("Content-Range".to_string(), range_header);
    self
}
```

Add test for this method (after the existing tests, around line 181):

```rust
#[test]
fn add_content_range_sets_header() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 206);
    resp.add_content_range(0, 99, 1000);
    let val = resp.try_get_header("Content-Range".to_string());
    assert_eq!(val, Some("bytes 0-99/1000".to_string()));
}

#[test]
fn add_content_range_with_partial_file() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 206);
    resp.add_content_range(500, 999, 1000);
    let output = format!("{resp}");
    assert!(output.contains("content-range: bytes 500-999/1000\r\n"));
}
```

### Step 5: Modify handle_connection() to Support Range Requests

**File**: `src/main.rs`

Import the range request module:

```rust
use rcomm::models::range_request::{parse_range_header, resolve_range, RangeParseError};
```

Modify the `handle_connection()` function (lines 46–75):

Replace:
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
    response.add_body(contents.into());

    println!("Response: {response}");
    stream.write_all(&response.as_bytes()).unwrap();
}
```

With:

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

    // Check for Range header (only for successful (200) GET requests)
    let response_bytes = if response.try_get_header("content-length".to_string()).is_none()
        && http_request.method == rcomm::models::http_methods::HttpMethods::GET {
        // This is a 200 response (successful), check for Range header
        if let Some(range_header) = http_request.try_get_header("range".to_string()) {
            match parse_range_header(&range_header) {
                Ok(range_spec) => {
                    match resolve_range(range_spec, file_size) {
                        Ok((start, end, total)) => {
                            // Valid range: return 206 Partial Content
                            response = HttpResponse::build(String::from("HTTP/1.1"), 206);
                            let partial_bytes = contents
                                .as_bytes()
                                [start as usize..=end as usize]
                                .to_vec();
                            response.add_body(partial_bytes);
                            response.add_content_range(start, end, total);
                            println!("Response: {response}");
                            response.as_bytes()
                        }
                        Err(_) => {
                            // Range not satisfiable: return 416
                            response = HttpResponse::build(String::from("HTTP/1.1"), 416);
                            response.add_header(
                                String::from("Content-Range"),
                                format!("bytes */{}", file_size),
                            );
                            println!("Response: {response}");
                            response.as_bytes()
                        }
                    }
                }
                Err(_) => {
                    // Invalid Range header: ignore and return full content (200)
                    response.add_body(contents.into());
                    println!("Response: {response}");
                    response.as_bytes()
                }
            }
        } else {
            // No Range header: return full content (200)
            response.add_body(contents.into());
            println!("Response: {response}");
            response.as_bytes()
        }
    } else {
        // 404 or other error response, or not a GET request: return as-is
        response.add_body(contents.into());
        println!("Response: {response}");
        response.as_bytes()
    };

    stream.write_all(&response_bytes).unwrap();
}
```

**Alternative (Simpler) Implementation**:

If the above feels too nested, extract the range handling into a helper function:

```rust
fn handle_range_request(
    mut response: HttpResponse,
    contents: &str,
    range_header: Option<String>,
    is_success: bool,
) -> (HttpResponse, Vec<u8>) {
    if !is_success || range_header.is_none() {
        // No range or error response: return full content
        response.add_body(contents.as_bytes().to_vec());
        let bytes = response.as_bytes();
        return (response, bytes);
    }

    let range_header = range_header.unwrap();
    let file_size = contents.len() as u64;

    match parse_range_header(&range_header) {
        Ok(range_spec) => match resolve_range(range_spec, file_size) {
            Ok((start, end, total)) => {
                response = HttpResponse::build(String::from("HTTP/1.1"), 206);
                let partial_bytes = contents.as_bytes()[start as usize..=end as usize].to_vec();
                response.add_body(partial_bytes);
                response.add_content_range(start, end, total);
                let bytes = response.as_bytes();
                (response, bytes)
            }
            Err(_) => {
                response = HttpResponse::build(String::from("HTTP/1.1"), 416);
                response.add_header(
                    String::from("Content-Range"),
                    format!("bytes */{}", file_size),
                );
                let bytes = response.as_bytes();
                (response, bytes)
            }
        },
        Err(_) => {
            // Invalid range: ignore and return full content
            response.add_body(contents.as_bytes().to_vec());
            let bytes = response.as_bytes();
            (response, bytes)
        }
    }
}

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

    let is_get = http_request.method == rcomm::models::http_methods::HttpMethods::GET;
    let range_header = if is_get {
        http_request.try_get_header("range".to_string())
    } else {
        None
    };

    let (mut response, filename) = if routes.contains_key(&clean_target) {
        (HttpResponse::build(String::from("HTTP/1.1"), 200),
            routes.get(&clean_target).unwrap().to_str().unwrap())
    } else {
        (HttpResponse::build(String::from("HTTP/1.1"), 404),
            "pages/not_found.html")
    };

    let contents = fs::read_to_string(filename).unwrap();
    let is_success = response.try_get_header("status-code".to_string()).is_none(); // True for 200

    let (response, response_bytes) = handle_range_request(response, &contents, range_header, is_success);

    println!("Response: {response}");
    stream.write_all(&response_bytes).unwrap();
}
```

**Rationale**:
- Ranges are only supported for successful GET requests (status 200)
- If no Range header is present, full content is returned (status 200)
- If Range header is invalid, it is ignored and full content is returned (per RFC 7233)
- If Range header is valid but unsatisfiable, status 416 is returned
- The Content-Range header is added for both 206 and 416 responses

---

## Testing Strategy

### Unit Tests

#### 1. Test Range Parsing (in src/models/range_request.rs)

The module already includes comprehensive tests. Run with:

```bash
cargo test range_request::tests -v
```

Expected: 20 tests pass (13 parsing + 7 resolution)

#### 2. Test Content-Range Header Helper (in src/models/http_response.rs)

Already added in Step 4. Run with:

```bash
cargo test http_response::tests::add_content_range -v
```

Expected: 2 tests pass

### Integration Tests

**File**: `src/bin/integration_test.rs`

Add integration tests for range request scenarios:

```rust
fn test_range_request_valid_byte_range(addr: &str) -> Result<(), String> {
    // Request bytes 0-99 from a resource
    let mut client = std::net::TcpStream::connect(addr)
        .map_err(|e| format!("Connection failed: {e}"))?;

    let request = "GET / HTTP/1.1\r\nHost: localhost\r\nRange: bytes=0-99\r\n\r\n";
    client.write_all(request.as_bytes())
        .map_err(|e| format!("Write failed: {e}"))?;
    client.shutdown(std::net::Shutdown::Write)
        .map_err(|e| format!("Shutdown failed: {e}"))?;

    let resp = parse_response(&mut client)?;

    // Should get 206 Partial Content
    assert_eq_or_err(&resp.status_code, &206, "Expected 206 Partial Content")?;

    // Should have Content-Range header
    let content_range = resp.headers.get("content-range")
        .ok_or("Missing Content-Range header")?;

    // Format: "bytes 0-99/total"
    if !content_range.starts_with("bytes 0-99/") {
        return Err(format!("Invalid Content-Range: {}", content_range));
    }

    // Body should be exactly 100 bytes
    assert_eq_or_err(&(resp.body.len() as u64), &100u64, "Body size should be 100")?;

    Ok(())
}

fn test_range_request_suffix_range(addr: &str) -> Result<(), String> {
    // Request last 100 bytes
    let mut client = std::net::TcpStream::connect(addr)
        .map_err(|e| format!("Connection failed: {e}"))?;

    let request = "GET / HTTP/1.1\r\nHost: localhost\r\nRange: bytes=-100\r\n\r\n";
    client.write_all(request.as_bytes())
        .map_err(|e| format!("Write failed: {e}"))?;
    client.shutdown(std::net::Shutdown::Write)
        .map_err(|e| format!("Shutdown failed: {e}"))?;

    let resp = parse_response(&mut client)?;

    assert_eq_or_err(&resp.status_code, &206, "Expected 206 Partial Content")?;

    let content_range = resp.headers.get("content-range")
        .ok_or("Missing Content-Range header")?;

    if !content_range.ends_with("/") {
        return Err("Content-Range format invalid".to_string());
    }

    // Body should be exactly 100 bytes
    assert_eq_or_err(&(resp.body.len() as u64), &100u64, "Body size should be 100")?;

    Ok(())
}

fn test_range_request_open_ended(addr: &str) -> Result<(), String> {
    // Request from byte 100 to EOF
    let mut client = std::net::TcpStream::connect(addr)
        .map_err(|e| format!("Connection failed: {e}"))?;

    let request = "GET / HTTP/1.1\r\nHost: localhost\r\nRange: bytes=100-\r\n\r\n";
    client.write_all(request.as_bytes())
        .map_err(|e| format!("Write failed: {e}"))?;
    client.shutdown(std::net::Shutdown::Write)
        .map_err(|e| format!("Shutdown failed: {e}"))?;

    let resp = parse_response(&mut client)?;

    assert_eq_or_err(&resp.status_code, &206, "Expected 206 Partial Content")?;

    let content_range = resp.headers.get("content-range")
        .ok_or("Missing Content-Range header")?;

    if !content_range.starts_with("bytes 100-") {
        return Err(format!("Invalid Content-Range: {}", content_range));
    }

    Ok(())
}

fn test_range_request_unsatisfiable(addr: &str) -> Result<(), String> {
    // Request range beyond file size
    let mut client = std::net::TcpStream::connect(addr)
        .map_err(|e| format!("Connection failed: {e}"))?;

    let request = "GET / HTTP/1.1\r\nHost: localhost\r\nRange: bytes=99999-100000\r\n\r\n";
    client.write_all(request.as_bytes())
        .map_err(|e| format!("Write failed: {e}"))?;
    client.shutdown(std::net::Shutdown::Write)
        .map_err(|e| format!("Shutdown failed: {e}"))?;

    let resp = parse_response(&mut client)?;

    // Should get 416 Range Not Satisfiable
    assert_eq_or_err(&resp.status_code, &416, "Expected 416 Range Not Satisfiable")?;

    // Should have Content-Range header with wildcard
    let content_range = resp.headers.get("content-range")
        .ok_or("Missing Content-Range header")?;

    if !content_range.starts_with("bytes */") {
        return Err(format!("Invalid Content-Range for 416: {}", content_range));
    }

    Ok(())
}

fn test_range_request_invalid_header(addr: &str) -> Result<(), String> {
    // Send invalid Range header; should be ignored
    let mut client = std::net::TcpStream::connect(addr)
        .map_err(|e| format!("Connection failed: {e}"))?;

    let request = "GET / HTTP/1.1\r\nHost: localhost\r\nRange: invalid\r\n\r\n";
    client.write_all(request.as_bytes())
        .map_err(|e| format!("Write failed: {e}"))?;
    client.shutdown(std::net::Shutdown::Write)
        .map_err(|e| format!("Shutdown failed: {e}"))?;

    let resp = parse_response(&mut client)?;

    // Should get 200 (ignore invalid Range header)
    assert_eq_or_err(&resp.status_code, &200, "Expected 200 OK")?;

    // Should NOT have Content-Range header
    if resp.headers.contains_key("content-range") {
        return Err("Should not have Content-Range for invalid Range header".to_string());
    }

    Ok(())
}

fn test_range_request_no_range_header(addr: &str) -> Result<(), String> {
    // Normal GET without Range header should still work
    let mut client = std::net::TcpStream::connect(addr)
        .map_err(|e| format!("Connection failed: {e}"))?;

    let request = "GET / HTTP/1.1\r\nHost: localhost\r\n\r\n";
    client.write_all(request.as_bytes())
        .map_err(|e| format!("Write failed: {e}"))?;
    client.shutdown(std::net::Shutdown::Write)
        .map_err(|e| format!("Shutdown failed: {e}"))?;

    let resp = parse_response(&mut client)?;

    // Should get 200 OK with full content
    assert_eq_or_err(&resp.status_code, &200, "Expected 200 OK")?;

    // Should NOT have Content-Range header
    if resp.headers.contains_key("content-range") {
        return Err("Should not have Content-Range for non-Range request".to_string());
    }

    Ok(())
}

fn test_range_request_on_404(addr: &str) -> Result<(), String> {
    // Range header on non-existent resource should be ignored
    let mut client = std::net::TcpStream::connect(addr)
        .map_err(|e| format!("Connection failed: {e}"))?;

    let request = "GET /nonexistent HTTP/1.1\r\nHost: localhost\r\nRange: bytes=0-99\r\n\r\n";
    client.write_all(request.as_bytes())
        .map_err(|e| format!("Write failed: {e}"))?;
    client.shutdown(std::net::Shutdown::Write)
        .map_err(|e| format!("Shutdown failed: {e}"))?;

    let resp = parse_response(&mut client)?;

    // Should get 404 (Range is only for successful responses)
    assert_eq_or_err(&resp.status_code, &404, "Expected 404 Not Found")?;

    // Should NOT have Content-Range header
    if resp.headers.contains_key("content-range") {
        return Err("Should not have Content-Range for 404 responses".to_string());
    }

    Ok(())
}
```

Register these tests in the `main()` function:

```rust
results.push(run_test("Range request: valid byte range", || test_range_request_valid_byte_range(&addr)));
results.push(run_test("Range request: suffix range", || test_range_request_suffix_range(&addr)));
results.push(run_test("Range request: open-ended range", || test_range_request_open_ended(&addr)));
results.push(run_test("Range request: unsatisfiable", || test_range_request_unsatisfiable(&addr)));
results.push(run_test("Range request: invalid header", || test_range_request_invalid_header(&addr)));
results.push(run_test("Range request: no range header", || test_range_request_no_range_header(&addr)));
results.push(run_test("Range request: on 404", || test_range_request_on_404(&addr)));
```

### Manual Testing

```bash
# Build and run server
cargo build
cargo run &
SERVER_PID=$!
sleep 2

# Test: Valid byte range
curl -v -H "Range: bytes=0-99" http://127.0.0.1:7878/
# Expected: 206 Partial Content, Content-Range: bytes 0-99/<total>

# Test: Suffix range (last 100 bytes)
curl -v -H "Range: bytes=-100" http://127.0.0.1:7878/
# Expected: 206 Partial Content

# Test: Open-ended range
curl -v -H "Range: bytes=100-" http://127.0.0.1:7878/
# Expected: 206 Partial Content

# Test: Unsatisfiable range
curl -v -H "Range: bytes=99999-100000" http://127.0.0.1:7878/
# Expected: 416 Range Not Satisfiable, Content-Range: bytes */<total>

# Test: Invalid range header (should ignore)
curl -v -H "Range: invalid" http://127.0.0.1:7878/
# Expected: 200 OK (no Content-Range)

# Test: No range header (normal GET)
curl -v http://127.0.0.1:7878/
# Expected: 200 OK (no Content-Range)

kill $SERVER_PID
```

---

## Edge Cases

### 1. **Single Byte Requests**

**Scenario**: Client requests `Range: bytes=0-0` (just the first byte)

**Expected Behavior**: Return 206 with the single byte

**Implementation**: Already handled—`resolve_range()` correctly handles start == end

**Test**: `test_range_request_single_byte()` (add if needed)

---

### 2. **Ranges Larger Than File**

**Scenario**: Client requests `Range: bytes=0-999999` but file is only 100 bytes

**Expected Behavior**: Clamp to file size, return 206 with all available bytes

**Implementation**: `resolve_range()` clamps end to `file_size - 1`

**Test**: Verify in `test_range_request_valid_byte_range()` by comparing Content-Length

---

### 3. **Start of File at Non-Zero Offset**

**Scenario**: Client requests `Range: bytes=100-999` from 1000-byte file

**Expected Behavior**: Return bytes 100–999 (900 bytes), with Content-Range: bytes 100-999/1000

**Implementation**: Straightforward byte slicing

**Test**: `test_range_request_open_ended()` covers this

---

### 4. **Last N Bytes (Suffix Range)**

**Scenario**: Client requests `Range: bytes=-500` (last 500 bytes)

**Expected Behavior**: If file is 1000 bytes, return bytes 500–999

**Implementation**: Calculate `start = file_size - suffix_length`

**Test**: `test_range_request_suffix_range()`

---

### 5. **Suffix Larger Than File**

**Scenario**: Client requests `Range: bytes=-5000` but file is only 1000 bytes

**Expected Behavior**: Return entire file (bytes 0–999), not an error

**Implementation**: `resolve_range()` clamps suffix: `if suffix_length >= file_size { return (0, file_size - 1, file_size) }`

**Test**: Add test case `test_range_request_suffix_larger_than_file()`

---

### 6. **Unsatisfiable Range (Start Beyond EOF)**

**Scenario**: Client requests `Range: bytes=99999-100000` from 1000-byte file

**Expected Behavior**: Return 416 Range Not Satisfiable with `Content-Range: bytes */1000`

**Implementation**: `resolve_range()` returns `Err(RangeResolveError::RangeNotSatisfiable)`

**Test**: `test_range_request_unsatisfiable()`

---

### 7. **Invalid Range Header Format**

**Scenario**: Client sends malformed Range headers:
- `Range: bytes` (missing =)
- `Range: bytes=` (empty)
- `Range: bytes=100-50` (start > end)
- `Range: bytes=0-99, 200-299` (multiple ranges — not supported)

**Expected Behavior**: Ignore invalid header and return full content (200 OK)

**Implementation**: `parse_range_header()` returns `Err`, which is caught and ignored in handler

**Test**: `test_range_request_invalid_header()`

---

### 8. **Range Header on 404 Responses**

**Scenario**: Client requests `Range: bytes=0-99` for non-existent resource

**Expected Behavior**: Ignore Range, return 404 (range support only for successful responses)

**Implementation**: Range parsing only happens for 200 responses

**Test**: `test_range_request_on_404()`

---

### 9. **Multiple Range Requests (Not Supported)**

**Scenario**: Client sends `Range: bytes=0-99, 200-299` (multiple ranges)

**Expected Behavior**: Reject with error or ignore (rcomm chose to reject)

**Implementation**: `parse_range_header()` detects comma and returns `Err(RangeParseError::MultipleRanges)`

**Alternative**: Could ignore and return full content

**Test**: Already in unit tests for parsing

---

### 10. **Range Requests on HEAD Method**

**Scenario**: Client sends `HEAD` with `Range: bytes=0-99`

**Expected Behavior**: RFC 7233 allows this, but rcomm's simple implementation only supports GET

**Implementation**: Range parsing only happens for GET (method check in handler)

**Test**: Could add `test_range_request_on_head()` to verify it's ignored

---

### 11. **Content-Length Header for Partial Content**

**Scenario**: After setting partial body with `add_body()`, Content-Length should match the range size

**Expected Behavior**: `Content-Length` = end - start + 1

**Implementation**: `add_body()` automatically calculates from body length (already correct)

**Test**: Verify in `test_range_request_valid_byte_range()` by checking headers

---

### 12. **Empty File**

**Scenario**: Requesting range from an empty file

**Expected Behavior**: Return 416 Range Not Satisfiable

**Implementation**: `resolve_range()` checks `if start >= file_size` (for empty file, 0 >= 0 is true)

**Test**: Add `test_range_request_empty_file()`

---

### 13. **Concurrent Range Requests**

**Scenario**: Multiple clients request different ranges simultaneously

**Expected Behavior**: Thread pool handles independently; no interference

**Implementation**: Each `handle_connection()` thread has its own response object

**Test**: Manual testing with `ab -c 10` or similar concurrent load tool

---

### 14. **Large Files**

**Scenario**: Requesting range from multi-megabyte file

**Expected Behavior**: Efficiently slice bytes without loading unnecessary portions

**Caveat**: Current implementation reads entire file into memory—not optimal for huge files

**Future Enhancement**: Stream from disk instead of buffering

**Test**: Manual test with large file in pages/ directory

---

### 15. **Zero-Length Ranges**

**Scenario**: Client requests `Range: bytes=0-0` (technically 1 byte, not zero)

**Expected Behavior**: Return 206 with 1 byte

**Implementation**: Correctly handled

**Potential Edge Case**: Client sends `Range: bytes=100-99` (start > end)—parsed as error

**Test**: Already covered in parsing tests

---

## Implementation Checklist

- [ ] Create `src/models/range_request.rs` with RangeSpec, parsing, and resolution logic
- [ ] Add `pub mod range_request;` to `src/models.rs`
- [ ] Verify status code 206 and 416 are in `http_status_codes.rs`
- [ ] Add `add_content_range()` method to `HttpResponse`
- [ ] Add unit tests for `add_content_range()`
- [ ] Import range request module in `src/main.rs`
- [ ] Modify `handle_connection()` to parse Range header and handle 206/416 responses
- [ ] Add 7 integration tests for range requests
- [ ] Run `cargo test` (all unit tests pass)
- [ ] Run `cargo run --bin integration_test` (all integration tests pass, 12 + 7 = 19 total)
- [ ] Manual testing with curl
- [ ] Test edge cases (unsatisfiable ranges, invalid headers, 404s)
- [ ] Verify Content-Length and Content-Range headers are correct
- [ ] Review code for style consistency

---

## Success Criteria

1. **HTTP/1.1 Compliance**:
   - Valid byte ranges return 206 Partial Content
   - Unsatisfiable ranges return 416 Range Not Satisfiable
   - Invalid Range headers are ignored (return 200 with full content)
   - Content-Range header is always present in 206 and 416 responses

2. **Range Parsing**:
   - `bytes=0-99` (closed range) ✓
   - `bytes=-500` (suffix range) ✓
   - `bytes=9500-` (open-ended range) ✓
   - Rejects invalid formats ✓
   - Rejects multiple ranges ✓

3. **All Tests Pass**:
   - `cargo test models::range_request::tests` (20 tests)
   - `cargo test http_response::tests::add_content_range` (2 tests)
   - `cargo run --bin integration_test` (7 new tests + 12 existing = 19 total)

4. **Client Behavior**:
   - `curl -H "Range: bytes=0-99" http://localhost:7878/` returns 206 with partial content
   - Content-Range header format: `bytes START-END/TOTAL`
   - Content-Length matches actual body size
   - 416 responses include `Content-Range: bytes */TOTAL`

5. **Code Quality**:
   - No additional `.unwrap()` in critical paths (range parsing returns Result)
   - Module-based design (range logic separated from handler)
   - Comprehensive unit tests (20 tests)
   - Clear error types and messages

6. **No Breaking Changes**:
   - GET without Range header still returns 200 with full content
   - All existing tests still pass
   - HEAD, OPTIONS, and error responses unaffected

---

## Implementation Difficulty: 4/10

**Rationale**:
- Range parsing is straightforward state machine logic (per RFC 7233)
- Range resolution is simple arithmetic (clamping and bounds checking)
- Integration into handler is conditional—no changes to core routing or file serving
- Comprehensive test coverage already planned
- No external dependencies or complex algorithms

---

## Risk Assessment: Low-Medium

**Risks**:
- Memory usage: Reading entire file into memory (existing limitation, not introduced by this feature)
- Off-by-one errors in byte slicing (mitigated by comprehensive tests)
- Invalid Range headers could be ignored instead of rejected (RFC allows; not an issue)

**Mitigations**:
- Thorough unit tests for range parsing and resolution
- Integration tests covering all major scenarios
- Manual testing with curl before deployment

---

## Future Enhancements

1. **Multiple Range Requests**: Support `bytes=0-99, 200-299` (multipart/byteranges response)
2. **Streaming Large Files**: Read file in chunks instead of loading entirely into memory
3. **Conditional Range Requests**: Support `If-Range` header (e.g., only if ETag matches)
4. **Accept-Ranges Header**: Advertise range support in response headers
5. **Range Requests for HEAD**: Support range requests with HEAD method
6. **Compression with Ranges**: Handle Content-Encoding and partial compressed content

---

## References

- [RFC 7233 - HTTP/1.1 Range Requests](https://tools.ietf.org/html/rfc7233)
- [MDN - Range Request Overview](https://developer.mozilla.org/en-US/docs/Web/HTTP/Range_requests)
- [HTTP Content-Range Header](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Content-Range)
- [HTTP 206 Partial Content](https://developer.mozilla.org/en-US/docs/Web/HTTP/Status/206)
- [HTTP 416 Range Not Satisfiable](https://developer.mozilla.org/en-US/docs/Web/HTTP/Status/416)
