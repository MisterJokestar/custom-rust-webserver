# Feature: Return `416 Range Not Satisfiable` for Out-of-Bounds Ranges

## Overview

The HTTP `Range` request header allows clients to request a specific portion (byte range) of a resource, enabling resume capability for downloads and efficient streaming. This feature implements support for detecting and rejecting **invalid range requests** with a `416 Range Not Satisfiable` response.

**Status:** Not Implemented
**Complexity:** 2/10
**Necessity:** 3/10 (Low priority; useful for robustness and HTTP spec compliance)
**RFC Reference:** [RFC 7233, Section 4.4](https://tools.ietf.org/html/rfc7233#section-4.4)

---

## Current State

The rcomm server currently:
- Does not parse the `Range` request header
- Does not implement partial content delivery (206 responses)
- Returns the full file content (200 OK) for all GET requests
- Has `Content-Length` headers automatically set by `HttpResponse.add_body()`

The server has the 416 status code defined in `http_status_codes.rs` but no handler for range validation.

**Related features (not yet implemented):**
- Basic range request support (206 Partial Content) — more complex
- Multi-part range responses — much more complex

---

## Goal

Implement **defensive range validation**: when a client sends a `Range` header with an out-of-bounds byte range (e.g., `Range: bytes=5000-9999` for a 1000-byte file), respond with:

```
HTTP/1.1 416 Range Not Satisfiable
Content-Range: bytes */1000
Content-Length: 0

```

This prepares the server for future full range request implementation without requiring clients to handle 200 OK responses for unsatisfiable ranges.

**In-scope for this feature:**
1. Parse the `Range` header value
2. Extract file size from the response body
3. Detect out-of-bounds ranges
4. Return 416 with `Content-Range: bytes */<size>` header
5. Return empty body

**Out-of-scope (related features, separate implementations):**
1. Actually serving partial content with 206 responses
2. Multi-part byte-range responses
3. Range parsing for complex formats (e.g., `bytes=0-99,200-299`)

---

## Architecture & Design Decisions

### Range Header Format

The `Range` header format for simple (single) byte ranges per RFC 7233:

```
Range: bytes=<start>-<end>
Range: bytes=<start>-
Range: bytes=-<suffix-length>
```

**Examples:**
- `bytes=0-99` — first 100 bytes
- `bytes=100-` — from byte 100 to end
- `bytes=-100` — last 100 bytes

**This feature's scope:** Validate only **closed ranges** (`bytes=start-end`).
**Why:** The most common format for resume downloads; suffix ranges and open-ended ranges can be skipped for now.

### When to Return 416

Per RFC 7233, return 416 when:
1. The range request cannot be satisfied
2. The range lies entirely beyond the resource size

**Example validations:**
- File is 1000 bytes
- `bytes=0-99` → Valid (satisfiable)
- `bytes=500-999` → Valid (satisfiable)
- `bytes=1000-1999` → Invalid (start >= size)
- `bytes=900-1099` → Invalid (end >= size)
- `bytes=1000-` → Invalid (start >= size)
- `bytes=-100` → Valid (last 100 bytes)
- `bytes=0-` → Valid (entire file)

### Decision: When to Implement 416 Only

Since full range support (206 responses) is not yet implemented, this feature will:
1. Parse and validate the `Range` header
2. Return **416 immediately if out-of-bounds**
3. Return **200 with full content if in-bounds** (no 206 yet)

This allows incremental progress without requiring full 206 implementation upfront. Clients expecting 206 will get 200 (acceptable fallback), but truly bad ranges get explicit 416 rejection.

### Response Structure

Per RFC 7233, a 416 response:
- Status code: `416 Range Not Satisfiable`
- Headers: `Content-Range: bytes */<file-size>` (mandatory)
- Body: Empty (no Content-Length header needed, or `Content-Length: 0`)
- No payload

Example:
```
HTTP/1.1 416 Range Not Satisfiable
Content-Range: bytes */1000
Content-Length: 0

```

### Parsing Strategy

**Approach:** Create a helper function to parse and validate ranges.

```rust
fn parse_range_header(range_str: &str, file_size: u64) -> Result<RangeInfo, RangeError> {
    // Parse "bytes=<start>-<end>"
    // Validate against file_size
    // Return (start, end) or RangeError::OutOfBounds
}
```

**Why separate function:**
- Testable in isolation
- Reusable for future 206 implementation
- Keeps `handle_connection()` clean
- Clear error types for different parse failures

### Integration Point

Range validation should occur **after file is read but before response is built**:

1. Read file (existing)
2. Determine file size from file contents
3. If `Range` header present, parse and validate
4. If out-of-bounds, build 416 response and return early
5. Otherwise, build 200 response with full content (current behavior)

**Location:** `handle_connection()` in `/home/jwall/personal/rusty/rcomm/src/main.rs`

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Current behavior:**
- Always reads full file and serves with 200 OK
- No range header parsing

**Changes required:**
- After reading file, check for `Range` header
- Parse range value
- If present and valid, check bounds against file size
- If out-of-bounds, build 416 response with `Content-Range` header
- If in-bounds (or no Range header), serve as normal (200 OK)

**Key section:**
```rust
fn handle_connection(mut stream: TcpStream, routes: HashMap<String, PathBuf>) {
    // ... parse request, clean route, find file ...

    let contents = fs::read_to_string(filename).unwrap();

    // NEW: Check Range header and validate bounds
    let file_size = contents.len() as u64;
    if let Some(range_header) = http_request.try_get_header("range".to_string()) {
        if let Some(reason) = validate_range_header(&range_header, file_size) {
            let mut response = HttpResponse::build(String::from("HTTP/1.1"), 416);
            response.add_header(
                String::from("Content-Range"),
                format!("bytes */{}", file_size)
            );
            println!("Response: {response}");
            stream.write_all(&response.as_bytes()).unwrap();
            return;
        }
    }

    // Existing response building logic
    let mut response = HttpResponse::build(String::from("HTTP/1.1"), 200);
    // ... set headers and body ...
}
```

### 2. `/home/jwall/personal/rusty/rcomm/src/models/http_status_codes.rs`

**Current behavior:**
- Already has 416 status code defined (line 42): `416 => String::from("Range Not Satisfiable")`

**Changes required:**
- No changes needed (status code already exists!)

---

## Step-by-Step Implementation

### Step 1: Create Range Validation Function

Add to `/home/jwall/personal/rusty/rcomm/src/main.rs` (or as a separate module if preferred):

```rust
/// Validates a Range header value against a file size.
/// Returns None if the range is satisfiable, or Some(reason) if 416 should be sent.
///
/// Per RFC 7233, handles:
/// - bytes=<start>-<end>: Valid if 0 <= start <= end < file_size
/// - bytes=<start>-: Valid if 0 <= start < file_size
/// - bytes=-<suffix>: Always valid (last N bytes)
///
/// For now, only validates closed ranges (start-end format).
fn validate_range_header(range_str: &str, file_size: u64) -> Option<&'static str> {
    // Remove "bytes=" prefix
    let range_part = range_str.strip_prefix("bytes=")
        .unwrap_or_else(|| return Some("Invalid Range header format"))?;

    // Handle suffix range (bytes=-100)
    if range_part.starts_with('-') {
        // Suffix range is always valid if file_size > 0
        return if file_size > 0 {
            None  // Valid
        } else {
            Some("Suffix range invalid for empty resource")
        };
    }

    // Parse start-end range
    let parts: Vec<&str> = range_part.split('-').collect();
    if parts.len() != 2 {
        return Some("Invalid Range format");
    }

    let start_str = parts[0];
    let end_str = parts[1];

    // Parse start
    let start = match start_str.parse::<u64>() {
        Ok(n) => n,
        Err(_) => return Some("Invalid start byte position"),
    };

    // Check if start is within bounds
    if start >= file_size {
        return Some("Range start beyond file size");  // Out of bounds
    }

    // If end is provided, validate it
    if !end_str.is_empty() {
        let end = match end_str.parse::<u64>() {
            Ok(n) => n,
            Err(_) => return Some("Invalid end byte position"),
        };

        // Check if end is within bounds (must be < file_size, and >= start)
        if end >= file_size || end < start {
            return Some("Range end beyond file size or before start");  // Out of bounds
        }
    }
    // If end is empty (bytes=100-), it means to end of file, which is valid

    None  // Valid range
}
```

**Rationale:**
- Takes range string and file size
- Returns `None` for valid ranges (proceed with 200 OK)
- Returns error message for invalid ranges (send 416)
- Handles basic formats without full RFC 7233 complexity

---

### Step 2: Integrate Range Validation in handle_connection

In `/home/jwall/personal/rusty/rcomm/src/main.rs`, modify `handle_connection()`:

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

    // NEW: Validate Range header before serving content
    let file_size = contents.len() as u64;
    if let Some(range_header) = http_request.try_get_header("range".to_string()) {
        if let Some(_error) = validate_range_header(&range_header, file_size) {
            // Range is out of bounds — return 416
            let mut range_response = HttpResponse::build(String::from("HTTP/1.1"), 416);
            range_response.add_header(
                String::from("Content-Range"),
                format!("bytes */{}", file_size)
            );
            println!("Response: {range_response}");
            let _ = stream.write_all(&range_response.as_bytes());
            return;
        }
    }

    // Existing logic: add body and send response
    response.add_body(contents.into());

    println!("Response: {response}");
    stream.write_all(&response.as_bytes()).unwrap();
}
```

**Placement:** After `fs::read_to_string()` and before `response.add_body()`.

**Logic flow:**
1. Parse request (existing)
2. Clean route and find file (existing)
3. Read file contents (existing)
4. **NEW:** Calculate file size and check `Range` header
5. **NEW:** If Range is present and invalid, build 416 and return early
6. Add body and send response (existing)

**Key points:**
- Only validates if `Range` header is present
- Returns early for 416 (no file content sent)
- Falls back to 200 OK for valid or missing Range headers

---

### Step 3: Handle Edge Cases in Parsing

The `validate_range_header()` function already handles:

1. **Missing `bytes=` prefix** — returns error
2. **Suffix ranges** (`bytes=-100`) — always valid if file > 0 bytes
3. **Open-ended ranges** (`bytes=100-`) — valid if start < file_size
4. **Start beyond file** — returns error (out of bounds)
5. **End beyond file** — returns error (out of bounds)
6. **Start > End** — returns error
7. **Invalid numbers** — parse errors caught and reported

---

### Step 4: Build 416 Response Correctly

The 416 response structure:

```rust
let mut response = HttpResponse::build(String::from("HTTP/1.1"), 416);
response.add_header(
    String::from("Content-Range"),
    format!("bytes */{}", file_size)
);
```

**Why `Content-Range: bytes */<size>`?**
- Per RFC 7233, Section 4.2
- `*` means "entire resource size is unknown"
- `<size>` tells client the actual resource size
- Helps client understand why request failed

**Note:** No body needed. `HttpResponse.as_bytes()` will serialize without body if none is added.

---

## Code Snippets

### Complete Range Validation Function

```rust
/// Validates a Range header value against a file size.
/// Returns None if the range is satisfiable, or Some(error_msg) if 416 should be sent.
///
/// Handles:
/// - bytes=<start>-<end>: Valid if 0 <= start <= end < file_size
/// - bytes=<start>-: Valid if 0 <= start < file_size
/// - bytes=-<suffix>: Valid if file_size > 0
///
/// RFC 7233 Section 2.1
fn validate_range_header(range_str: &str, file_size: u64) -> Option<&'static str> {
    // Extract the range specification after "bytes="
    let range_spec = match range_str.strip_prefix("bytes=") {
        Some(spec) => spec,
        None => return Some("Range header must start with 'bytes='"),
    };

    // Handle suffix range: bytes=-N (last N bytes)
    if range_spec.starts_with('-') {
        // Suffix ranges are always satisfiable if resource is non-empty
        // (We don't validate the suffix value itself for simplicity)
        return if file_size > 0 {
            None  // Valid
        } else {
            Some("Suffix range not satisfiable for empty resource")
        };
    }

    // Parse "start-end" or "start-" format
    let parts: Vec<&str> = range_spec.split('-').collect();
    if parts.len() != 2 {
        return Some("Invalid range specification format");
    }

    let start_str = parts[0];
    let end_str = parts[1];

    // Parse start position
    let start: u64 = match start_str.parse() {
        Ok(n) => n,
        Err(_) => return Some("Start byte position is not a valid number"),
    };

    // Start must be less than file size
    if start >= file_size {
        return Some("Range start position is beyond the resource size");
    }

    // If end position is specified, validate it
    if !end_str.is_empty() {
        let end: u64 = match end_str.parse() {
            Ok(n) => n,
            Err(_) => return Some("End byte position is not a valid number"),
        };

        // End must be within bounds and >= start
        if end >= file_size {
            return Some("Range end position is beyond the resource size");
        }
        if end < start {
            return Some("Range end position is before range start position");
        }
    }

    // Range is satisfiable
    None
}
```

### Updated handle_connection Function

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

    // NEW: Range header validation
    let file_size = contents.len() as u64;
    if let Some(range_header) = http_request.try_get_header("range".to_string()) {
        if validate_range_header(&range_header, file_size).is_some() {
            // Range is invalid — return 416
            let mut range_response = HttpResponse::build(String::from("HTTP/1.1"), 416);
            range_response.add_header(
                String::from("Content-Range"),
                format!("bytes */{}", file_size)
            );
            println!("Response: {range_response}");
            let _ = stream.write_all(&range_response.as_bytes());
            return;
        }
    }

    response.add_body(contents.into());

    println!("Response: {response}");
    stream.write_all(&response.as_bytes()).unwrap();
}
```

---

## Testing Strategy

### Unit Tests

Add unit tests to `/home/jwall/personal/rusty/rcomm/src/main.rs` (or in a `#[cfg(test)]` module) to verify `validate_range_header()`:

```rust
#[cfg(test)]
mod range_tests {
    use super::*;

    #[test]
    fn valid_range_start_end() {
        // bytes=0-99 for 1000-byte file should be valid
        assert_eq!(validate_range_header("bytes=0-99", 1000), None);
    }

    #[test]
    fn valid_range_start_only() {
        // bytes=500- for 1000-byte file should be valid
        assert_eq!(validate_range_header("bytes=500-", 1000), None);
    }

    #[test]
    fn valid_range_suffix() {
        // bytes=-100 for 1000-byte file should be valid
        assert_eq!(validate_range_header("bytes=-100", 1000), None);
    }

    #[test]
    fn invalid_range_start_beyond_size() {
        // bytes=1000-1099 for 1000-byte file should fail
        assert!(validate_range_header("bytes=1000-1099", 1000).is_some());
    }

    #[test]
    fn invalid_range_end_beyond_size() {
        // bytes=500-1500 for 1000-byte file should fail
        assert!(validate_range_header("bytes=500-1500", 1000).is_some());
    }

    #[test]
    fn invalid_range_start_greater_than_end() {
        // bytes=100-50 (invalid order) should fail
        assert!(validate_range_header("bytes=100-50", 1000).is_some());
    }

    #[test]
    fn invalid_range_no_bytes_prefix() {
        // Missing "bytes=" prefix should fail
        assert!(validate_range_header("0-99", 1000).is_some());
    }

    #[test]
    fn invalid_range_non_numeric_start() {
        // bytes=abc-99 should fail
        assert!(validate_range_header("bytes=abc-99", 1000).is_some());
    }

    #[test]
    fn invalid_range_suffix_empty_file() {
        // bytes=-100 for 0-byte file should fail
        assert!(validate_range_header("bytes=-100", 0).is_some());
    }

    #[test]
    fn edge_case_first_byte() {
        // bytes=0-0 (just first byte) should be valid
        assert_eq!(validate_range_header("bytes=0-0", 1000), None);
    }

    #[test]
    fn edge_case_last_byte() {
        // bytes=999-999 (just last byte) for 1000-byte file should be valid
        assert_eq!(validate_range_header("bytes=999-999", 1000), None);
    }

    #[test]
    fn edge_case_entire_file() {
        // bytes=0-999 (entire 1000-byte file) should be valid
        assert_eq!(validate_range_header("bytes=0-999", 1000), None);
    }

    #[test]
    fn edge_case_single_byte_file() {
        // bytes=0-0 for 1-byte file should be valid
        assert_eq!(validate_range_header("bytes=0-0", 1), None);
    }

    #[test]
    fn edge_case_beyond_single_byte_file() {
        // bytes=1-1 for 1-byte file should fail
        assert!(validate_range_header("bytes=1-1", 1).is_some());
    }
}
```

---

### Integration Tests

Add test cases to `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`:

```rust
fn test_range_out_of_bounds(addr: &str) -> Result<(), String> {
    // Send request with out-of-bounds Range header
    // File size is unknown to test, so use a large file first

    // Assume root file is at least 100 bytes
    let mut req = String::from("GET / HTTP/1.1\r\n");
    req.push_str("Host: localhost\r\n");
    req.push_str("Range: bytes=999999-9999999\r\n");
    req.push_str("\r\n");

    let mut stream = std::net::TcpStream::connect(addr)
        .map_err(|e| format!("Connection failed: {e}"))?;
    stream.write_all(req.as_bytes())
        .map_err(|e| format!("Write failed: {e}"))?;

    let mut response = vec![];
    stream.read_to_end(&mut response)
        .map_err(|e| format!("Read failed: {e}"))?;

    let response_str = String::from_utf8_lossy(&response);

    // Check for 416 status
    if !response_str.contains("416") {
        return Err(format!("Expected 416 status, got: {}", response_str.lines().next().unwrap_or("unknown")));
    }

    // Check for Content-Range header
    if !response_str.contains("content-range:") && !response_str.contains("Content-Range:") {
        return Err("Missing Content-Range header".to_string());
    }

    // Body should be empty
    if let Some(body_start) = response_str.find("\r\n\r\n") {
        let body = &response_str[body_start + 4..];
        if !body.is_empty() {
            return Err(format!("Expected empty body, got {} bytes", body.len()));
        }
    }

    Ok(())
}

fn test_range_valid_in_bounds(addr: &str) -> Result<(), String> {
    // Send valid in-bounds Range header
    // Should return 200 OK (not 206, since full range support isn't implemented)

    let mut req = String::from("GET / HTTP/1.1\r\n");
    req.push_str("Host: localhost\r\n");
    req.push_str("Range: bytes=0-99\r\n");
    req.push_str("\r\n");

    let mut stream = std::net::TcpStream::connect(addr)
        .map_err(|e| format!("Connection failed: {e}"))?;
    stream.write_all(req.as_bytes())
        .map_err(|e| format!("Write failed: {e}"))?;

    let mut response = vec![];
    stream.read_to_end(&mut response)
        .map_err(|e| format!("Read failed: {e}"))?;

    let response_str = String::from_utf8_lossy(&response);

    // Should be 200 (not 416, not 206 yet)
    if !response_str.starts_with("HTTP/1.1 200") {
        return Err(format!("Expected 200 status, got: {}", response_str.lines().next().unwrap_or("unknown")));
    }

    Ok(())
}

fn test_range_no_header(addr: &str) -> Result<(), String> {
    // Send request without Range header — should work as before

    let mut req = String::from("GET / HTTP/1.1\r\n");
    req.push_str("Host: localhost\r\n");
    req.push_str("\r\n");

    let mut stream = std::net::TcpStream::connect(addr)
        .map_err(|e| format!("Connection failed: {e}"))?;
    stream.write_all(req.as_bytes())
        .map_err(|e| format!("Write failed: {e}"))?;

    let mut response = vec![];
    stream.read_to_end(&mut response)
        .map_err(|e| format!("Read failed: {e}"))?;

    let response_str = String::from_utf8_lossy(&response);

    // Should be 200 OK
    if !response_str.starts_with("HTTP/1.1 200") {
        return Err(format!("Expected 200 status, got: {}", response_str.lines().next().unwrap_or("unknown")));
    }

    Ok(())
}
```

Add to main test runner:

```rust
fn main() {
    let port = pick_free_port();
    let addr = format!("127.0.0.1:{port}");
    let mut server = start_server(port);

    if let Err(e) = wait_for_server(&addr, Duration::from_secs(5)) {
        eprintln!("{e}");
        let _ = server.kill();
        std::process::exit(1);
    }

    let mut results = vec![
        // ... existing tests ...
        run_test("Range: out of bounds", || test_range_out_of_bounds(&addr)),
        run_test("Range: valid in-bounds", || test_range_valid_in_bounds(&addr)),
        run_test("Range: no Range header", || test_range_no_header(&addr)),
    ];

    // ... rest of test runner
}
```

---

### Manual Testing

Use curl to verify 416 behavior:

```bash
# Start server
cargo run &
SERVER_PID=$!
sleep 1

# Test out-of-bounds range
curl -v -H "Range: bytes=999999-9999999" http://127.0.0.1:7878/

# Expected output:
# < HTTP/1.1 416 Range Not Satisfiable
# < content-range: bytes */<file-size>
# < content-length: 0
# <
# [no body]

# Test valid in-bounds range
curl -v -H "Range: bytes=0-99" http://127.0.0.1:7878/

# Expected output:
# < HTTP/1.1 200 OK
# < [headers...]
# [body content]

# Test without Range header
curl -v http://127.0.0.1:7878/

# Expected output:
# < HTTP/1.1 200 OK
# [body content]

kill $SERVER_PID
```

---

## Edge Cases & Considerations

### 1. Empty Files

**Case:** File is 0 bytes, client sends `Range: bytes=-100`

**RFC 7233 behavior:** Suffix ranges are undefined for empty resources.

**Implementation:** `validate_range_header()` returns error for suffix ranges on empty files.

**Test:** Verify `bytes=-100` on empty file returns 416.

---

### 2. Single-Byte Files

**Case:** File is 1 byte (size 1), various range requests

**Examples:**
- `bytes=0-0` → Valid (the only byte)
- `bytes=0-1` → Invalid (end beyond size)
- `bytes=1-1` → Invalid (start at boundary)

**Implementation:** Comparisons use `< file_size` correctly.

**Test:** Add edge case tests for single-byte and small files.

---

### 3. Very Large Files

**Case:** File is multi-gigabyte, `Range: bytes=0-999999999999999`

**Concern:** Integer overflow when parsing large byte positions.

**Implementation:** Uses `u64` (up to ~18 exabytes). Rust's `parse()` will safely reject invalid numbers.

**Test:** Not critical for this feature, but keep in mind for future 206 implementation.

---

### 4. Malformed Range Headers

**Case:** Client sends various malformed Range values

**Examples:**
- `Range: bytes` (missing `=`)
- `Range: bytes=` (no range specification)
- `Range: bytes=100` (missing `-`)
- `Range: bytes=100-200-300` (too many dashes)
- `Range: megabytes=0-99` (wrong unit)

**Implementation:** All return error, triggering 416 response.

**Test:** Unit tests cover these cases.

---

### 5. Multiple Range Requests

**Case:** Client sends `Range: bytes=0-99,200-299` (multi-part range)

**RFC 7233 behavior:** Valid but requires multi-part responses (out of scope).

**Current implementation:** Parses only first segment or rejects with 416.

**Decision:** Reject multi-part ranges for now (return 416). This is acceptable — clients can make multiple requests instead. Full implementation is a future feature.

**Implementation:** `split('-')` will produce wrong number of parts; error is returned.

**Test:** Verify multi-part ranges trigger 416.

---

### 6. Case Sensitivity of Header Name

**Case:** Client sends `range:` (lowercase) vs `Range:` (mixed case)

**Implementation:** `HttpRequest::try_get_header()` normalizes to lowercase, so both work.

**Test:** Already handled by existing header logic.

---

### 7. HTTP Methods with Range

**Case:** Client sends `POST /file HTTP/1.1` with `Range` header

**Current behavior:** Server returns 404 or 405 (not yet implemented).

**This feature:** Only validates Range if GET is being processed (implicitly, since only GETs reach the file-reading logic).

**Test:** POST requests don't enter range validation path (expected).

---

### 8. Content-Range Header in 416 Response

**Case:** What should `Content-Range: bytes */size` contain if file is returned to 404 handler?

**Current code:** File reading is specific to found routes; 404s don't read files.

**Implementation:** Only valid routes trigger range validation (after file read).

**Edge case:** 404 response doesn't trigger range code. OK.

---

### 9. Interaction with Future 206 Implementation

**Case:** This feature returns 416 for out-of-bounds. Future feature will return 206 for valid ranges.

**Design:** Current code returns 200 for valid ranges (conservative). When 206 is added:

```rust
// Future: if valid range, return 206 with partial content
if let Some(range_header) = http_request.try_get_header("range".to_string()) {
    if validate_range_header(&range_header, file_size).is_none() {
        // Valid range — return 206 in future
        // For now, fall through to 200 OK
    } else {
        // Invalid range — return 416 (this feature)
        return 416_response;
    }
}
```

**Benefit:** No changes needed to range validation logic; just add 206 handling.

---

### 10. Performance Implications

**Case:** Large files, range validation adds overhead

**Current overhead:**
- One header lookup: `try_get_header("range")`
- One validation function call: simple string parsing
- No file seeking or partial reads yet

**Performance impact:** Negligible. Validation is O(n) on header string length, which is typically < 100 bytes.

**Test:** Not needed for this feature (inherently fast).

---

## Implementation Checklist

- [ ] Add `validate_range_header()` function to `/home/jwall/personal/rusty/rcomm/src/main.rs`
- [ ] Integrate range validation in `handle_connection()` after file read
- [ ] Handle 416 response with `Content-Range: bytes */size` header
- [ ] Return early with no body for 416 responses
- [ ] Add unit tests for `validate_range_header()` with various inputs
- [ ] Add integration tests for out-of-bounds, in-bounds, and no-Range cases
- [ ] Test with curl manually to verify behavior
- [ ] Verify integration tests pass: `cargo run --bin integration_test`
- [ ] Run unit tests: `cargo test`
- [ ] Review code for style consistency and error handling

---

## Success Criteria

1. **RFC 7233 Compliance:** Out-of-bounds ranges return 416 with `Content-Range: bytes */<size>`
2. **Header Format:** `Content-Range` header correctly specifies resource size
3. **Empty Body:** 416 response has no body (or empty body)
4. **Valid Ranges:** Valid (in-bounds) Range headers allow 200 OK responses (no 206 yet)
5. **No Range Header:** Requests without Range header work as before (200 OK)
6. **Malformed Ranges:** Badly formatted Range headers trigger 416
7. **No Breaking Changes:** Existing GET requests work unchanged
8. **Tests Pass:** All integration and unit tests pass
9. **Clean Code:** Consistent error handling, no new `.unwrap()` calls
10. **Testable:** `validate_range_header()` is isolated and unit-testable

---

## Code Diff Summary

**File: `/home/jwall/personal/rusty/rcomm/src/main.rs`**

```diff
+ /// Validates a Range header value against a file size.
+ /// Returns None if the range is satisfiable, or Some(error_msg) if 416 should be sent.
+ fn validate_range_header(range_str: &str, file_size: u64) -> Option<&'static str> {
+     let range_spec = match range_str.strip_prefix("bytes=") {
+         Some(spec) => spec,
+         None => return Some("Range header must start with 'bytes='"),
+     };
+
+     if range_spec.starts_with('-') {
+         return if file_size > 0 {
+             None
+         } else {
+             Some("Suffix range not satisfiable for empty resource")
+         };
+     }
+
+     let parts: Vec<&str> = range_spec.split('-').collect();
+     if parts.len() != 2 {
+         return Some("Invalid range specification format");
+     }
+
+     let start: u64 = match parts[0].parse() {
+         Ok(n) => n,
+         Err(_) => return Some("Start byte position is not a valid number"),
+     };
+
+     if start >= file_size {
+         return Some("Range start position is beyond the resource size");
+     }
+
+     if !parts[1].is_empty() {
+         let end: u64 = match parts[1].parse() {
+             Ok(n) => n,
+             Err(_) => return Some("End byte position is not a valid number"),
+         };
+
+         if end >= file_size || end < start {
+             return Some("Range end position is invalid");
+         }
+     }
+
+     None
+ }

  fn handle_connection(mut stream: TcpStream, routes: HashMap<String, PathBuf>) {
      // ... request parsing ...

      let contents = fs::read_to_string(filename).unwrap();
+
+     // NEW: Range header validation
+     let file_size = contents.len() as u64;
+     if let Some(range_header) = http_request.try_get_header("range".to_string()) {
+         if validate_range_header(&range_header, file_size).is_some() {
+             let mut response = HttpResponse::build(String::from("HTTP/1.1"), 416);
+             response.add_header(
+                 String::from("Content-Range"),
+                 format!("bytes */{}", file_size)
+             );
+             println!("Response: {response}");
+             let _ = stream.write_all(&response.as_bytes());
+             return;
+         }
+     }

      response.add_body(contents.into());
      // ... rest of function ...
  }
```

---

## Future Enhancements

1. **206 Partial Content Implementation:** Use `validate_range_header()` to serve actual byte ranges instead of full content
2. **Multi-Part Byte Ranges:** Support `Range: bytes=0-99,200-299` with `multipart/byteranges` responses
3. **Content-Range in Responses:** Include `Content-Range` header in 200 OK responses when valid Range is requested (optional, per spec)
4. **Range Request Syntax Expansion:** Parse more complex range formats (currently handles only `bytes=start-end`, `bytes=start-`, `bytes=-suffix`)
5. **Range Header Validation Improvements:** More detailed error messages for debugging client issues
6. **Accept-Ranges Header:** Advertise range support with `Accept-Ranges: bytes` in responses

---

## References

- [RFC 7233 - HTTP/1.1 Range Requests](https://tools.ietf.org/html/rfc7233)
- [RFC 7233, Section 2.1 - Range Syntax](https://tools.ietf.org/html/rfc7233#section-2.1)
- [RFC 7233, Section 4.4 - 416 Response](https://tools.ietf.org/html/rfc7233#section-4.4)
- [MDN - HTTP Range Requests](https://developer.mozilla.org/en-US/docs/Web/HTTP/Range_requests)
- [MDN - Content-Range Header](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Content-Range)
