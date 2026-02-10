# Implementation Plan: Handle `If-None-Match` Request Header

**Feature**: Handle `If-None-Match` request header — return `304 Not Modified` when ETag matches
**Category**: Caching & Conditional Requests
**Complexity**: 3/10
**Necessity**: 6/10
**Estimated Implementation Time**: 2-3 hours

---

## Overview

The `If-None-Match` header is part of HTTP conditional request semantics. When a client sends an `If-None-Match` header with one or more ETags, the server should return `304 Not Modified` if the resource's ETag matches any of the provided values. This allows clients to avoid re-downloading unchanged content, reducing bandwidth and improving page load performance.

### Key HTTP Concepts

- **ETag**: An opaque entity tag that uniquely identifies a version of a resource
- **If-None-Match**: Request header containing one or more ETags; used with GET/HEAD to conditionally fetch only if the resource has changed
- **304 Not Modified**: Response status indicating the client's cached version is current
- **Weak vs Strong ETags**: Strong ETags use exact byte-for-byte matching; weak ETags (prefixed with `W/`) allow for equivalent representations

### Scope for This Implementation

This plan focuses on:
1. Generating strong ETags based on file content (MD5 hash)
2. Supporting single ETag matching in `If-None-Match` header
3. Returning `304 Not Modified` with minimal headers when ETag matches
4. Maintaining backward compatibility (no response changes for requests without `If-None-Match`)

Future enhancements:
- Support for multiple ETags in `If-None-Match` (comma-separated)
- Weak ETag support
- `If-Match` header support (inverse condition)
- `ETag` header generation for dynamic content

---

## Files to Modify

### 1. `src/models/http_request.rs`
- Add method to extract and parse `If-None-Match` header
- Implement header parsing utility to handle comma-separated ETags

### 2. `src/models/http_response.rs`
- Add method to set `ETag` response header
- Ensure `add_body()` does not set `Content-Length` when body is not actually sent (for 304 responses)
- Add optional method to omit `Content-Length` for 304 responses

### 3. `src/main.rs`
- Import hash function for ETag generation
- Modify `handle_connection()` to:
  - Extract `If-None-Match` header from request
  - Calculate ETag for file content
  - Compare ETags
  - Return 304 response if match, or 200 with ETag header if different
- Add helper function `generate_etag()` for consistent ETag calculation

### 4. `src/lib.rs` (if needed)
- May need to expose the hash utility if creating a separate utility module

---

## Step-by-Step Implementation

### Step 1: Add ETag Extraction to HttpRequest

**File**: `src/models/http_request.rs`

Add a new public method to extract the `If-None-Match` header:

```rust
impl HttpRequest {
    /// Extract the If-None-Match header value(s)
    /// Returns the header value as-is (may be quoted, comma-separated, etc.)
    pub fn try_get_if_none_match(&self) -> Option<String> {
        self.try_get_header("if-none-match".to_string())
    }
}
```

**Testing**: Add unit tests for the new method:
- Test successful extraction of valid `If-None-Match` header
- Test when header is missing (returns None)

### Step 2: Add ETag Support to HttpResponse

**File**: `src/models/http_response.rs`

Add a method to set the `ETag` response header:

```rust
impl HttpResponse {
    /// Add an ETag header to the response
    /// The ETag value should be properly quoted (e.g., "\"abc123\"")
    pub fn add_etag(&mut self, etag: String) -> &mut HttpResponse {
        self.add_header("etag".to_string(), etag)
    }

    /// Check if this is a 304 Not Modified response
    pub fn is_not_modified(&self) -> bool {
        self.status_code == 304
    }
}
```

**Important**: For 304 responses, we should NOT call `add_body()`. The 304 response should have no body per RFC 7232.

**Testing**: Add unit tests:
- Test `add_etag()` correctly adds the header
- Verify that 304 responses with no body don't include `Content-Length`

### Step 3: Implement ETag Generation Function

**File**: `src/main.rs`

Add a function to generate strong ETags based on file content using MD5 hashing (available in Rust std library via `std::collections::hash_map`):

```rust
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

fn generate_etag(content: &[u8]) -> String {
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    let hash = hasher.finish();
    // Format as quoted hex string (strong ETag format)
    format!("\"{}\"", format!("{:x}", hash))
}
```

**Alternative Approach**: Use `std::hash` with DefaultHasher for simpler implementation (no external dependencies):
- This provides adequate differentiation for file versions
- Hash outputs are deterministic per session
- Trade-off: May produce different hashes across server restarts (acceptable for this feature level)

**Advanced Alternative** (future): Implement file modification time + size based ETags for persistence:

```rust
fn generate_etag_from_metadata(path: &Path) -> Result<String, io::Error> {
    let metadata = fs::metadata(path)?;
    let modified = metadata.modified()?;
    let size = metadata.len();

    let mut hasher = DefaultHasher::new();
    modified.duration_since(UNIX_EPOCH).unwrap().as_nanos().hash(&mut hasher);
    size.hash(&mut hasher);

    let hash = hasher.finish();
    Ok(format!("\"{}\"", format!("{:x}", hash)))
}
```

For this initial implementation, we'll use **content-based hashing** for correctness.

### Step 4: Implement ETag Comparison

**File**: `src/main.rs`

Add a helper function to parse and compare ETags:

```rust
fn etags_match(if_none_match: &str, resource_etag: &str) -> bool {
    // Simple single ETag matching (quoted string comparison)
    // Handles weak ETags with W/ prefix (though we don't generate them)

    // Strip weak indicator if present (W/"xxx" -> "xxx")
    let parse_etag = |s: &str| -> &str {
        let s = s.trim();
        if s.starts_with("W/") {
            &s[2..]
        } else {
            s
        }
    };

    let client_etag = parse_etag(if_none_match);
    let server_etag = parse_etag(resource_etag);

    client_etag == server_etag
}
```

**Note**: This implementation handles single ETag matching. For future support of comma-separated lists:

```rust
fn etags_match(if_none_match: &str, resource_etag: &str) -> bool {
    // Split on commas to support multiple ETags
    for client_etag in if_none_match.split(',') {
        if parse_etag(client_etag.trim()) == parse_etag(resource_etag) {
            return true;
        }
    }
    false
}

fn parse_etag(s: &str) -> &str {
    let s = s.trim();
    if s.starts_with("W/") {
        &s[2..]
    } else {
        s
    }
}
```

### Step 5: Modify `handle_connection()` to Support Conditional Requests

**File**: `src/main.rs`

Update the `handle_connection()` function to check for `If-None-Match` and return 304 when appropriate:

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

    // Read file content
    let contents = fs::read_to_string(filename).unwrap();
    let contents_bytes = contents.as_bytes();

    // Generate ETag for the file
    let etag = generate_etag(contents_bytes);
    response.add_etag(etag.clone());

    // Check If-None-Match header for conditional request
    if let Some(if_none_match) = http_request.try_get_if_none_match() {
        if http_request.method == HttpMethods::GET ||
           http_request.method == HttpMethods::HEAD {
            // Only apply conditional logic for safe methods
            if etags_match(&if_none_match, &etag) {
                // Client has current version - return 304
                let response = HttpResponse::build(String::from("HTTP/1.1"), 304);
                // Note: 304 responses must NOT include a body
                // The ETag header is already set above
                println!("Response: {response}");
                stream.write_all(&response.as_bytes()).unwrap();
                return;
            }
        }
    }

    // Resource is new or has changed - send full response
    response.add_body(contents_bytes.to_vec());

    println!("Response: {response}");
    stream.write_all(&response.as_bytes()).unwrap();
}
```

**Critical Detail**: 304 responses must not include a body and must not include `Content-Length` header. The current `add_body()` method automatically sets `Content-Length`, so we must NOT call `add_body()` for 304 responses.

### Step 6: Update HttpResponse to Handle 304 Responses Correctly

**File**: `src/models/http_response.rs`

Modify the `as_bytes()` method to ensure 304 responses don't include problematic headers:

```rust
pub fn as_bytes(&self) -> Vec<u8> {
    // For 304 responses, don't include body or Content-Length
    if self.status_code == 304 {
        return format!("{self}").as_bytes().to_vec();
    }

    if let Some(body) = &self.body {
        let mut bytes = format!("{self}").as_bytes().to_vec();
        bytes.append(&mut body.clone());
        return bytes
    } else {
        return format!("{self}").as_bytes().to_vec();
    }
}
```

---

## Code Snippets Summary

### Complete ETag generation and comparison functions

```rust
// src/main.rs - Add these near the top with other utility functions

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

fn generate_etag(content: &[u8]) -> String {
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    let hash = hasher.finish();
    format!("\"{}\"", format!("{:x}", hash))
}

fn etags_match(if_none_match: &str, resource_etag: &str) -> bool {
    // Handle single or multiple ETags separated by commas
    for client_etag in if_none_match.split(',') {
        let client_tag = client_etag.trim();
        // Remove weak indicator if present
        let client_tag = if client_tag.starts_with("W/") {
            &client_tag[2..]
        } else {
            client_tag
        };

        // Compare with server's strong ETag
        if client_tag == resource_etag {
            return true;
        }
    }
    false
}
```

### Updated HttpRequest method

```rust
// src/models/http_request.rs - Add to impl HttpRequest

pub fn try_get_if_none_match(&self) -> Option<String> {
    self.try_get_header("if-none-match".to_string())
}
```

### Updated HttpResponse methods

```rust
// src/models/http_response.rs - Add to impl HttpResponse

pub fn add_etag(&mut self, etag: String) -> &mut HttpResponse {
    self.add_header("etag".to_string(), etag)
}

pub fn is_not_modified(&self) -> bool {
    self.status_code == 304
}
```

---

## Testing Strategy

### Unit Tests

#### 1. HttpRequest.try_get_if_none_match()

**File**: `src/models/http_request.rs`

```rust
#[test]
fn try_get_if_none_match_returns_header_value() {
    let mut req = HttpRequest::build(
        HttpMethods::GET,
        "/".to_string(),
        "HTTP/1.1".to_string(),
    );
    req.add_header("If-None-Match".to_string(), "\"abc123\"".to_string());
    assert_eq!(req.try_get_if_none_match(), Some("\"abc123\"".to_string()));
}

#[test]
fn try_get_if_none_match_returns_none_when_missing() {
    let req = HttpRequest::build(
        HttpMethods::GET,
        "/".to_string(),
        "HTTP/1.1".to_string(),
    );
    assert_eq!(req.try_get_if_none_match(), None);
}

#[test]
fn try_get_if_none_match_handles_multiple_etags() {
    let mut req = HttpRequest::build(
        HttpMethods::GET,
        "/".to_string(),
        "HTTP/1.1".to_string(),
    );
    req.add_header("If-None-Match".to_string(), "\"abc\", \"def\", W/\"ghi\"".to_string());
    assert_eq!(
        req.try_get_if_none_match(),
        Some("\"abc\", \"def\", W/\"ghi\"".to_string())
    );
}
```

#### 2. HttpResponse.add_etag() and is_not_modified()

**File**: `src/models/http_response.rs`

```rust
#[test]
fn add_etag_sets_header() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
    resp.add_etag("\"abc123\"".to_string());
    assert_eq!(
        resp.try_get_header("etag".to_string()),
        Some("\"abc123\"".to_string())
    );
}

#[test]
fn is_not_modified_returns_true_for_304() {
    let resp = HttpResponse::build("HTTP/1.1".to_string(), 304);
    assert!(resp.is_not_modified());
}

#[test]
fn is_not_modified_returns_false_for_200() {
    let resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
    assert!(!resp.is_not_modified());
}

#[test]
fn three_oh_four_response_has_no_content_length() {
    let resp = HttpResponse::build("HTTP/1.1".to_string(), 304);
    let output = format!("{resp}");
    assert!(!output.contains("content-length"));
}
```

#### 3. ETag generation and comparison

**File**: `src/main.rs` (unit tests section)

```rust
#[cfg(test)]
mod etag_tests {
    use super::*;

    #[test]
    fn generate_etag_produces_quoted_string() {
        let etag = generate_etag(b"hello world");
        assert!(etag.starts_with("\""));
        assert!(etag.ends_with("\""));
        assert!(etag.len() > 2);
    }

    #[test]
    fn generate_etag_is_deterministic() {
        let content = b"test content";
        let etag1 = generate_etag(content);
        let etag2 = generate_etag(content);
        assert_eq!(etag1, etag2);
    }

    #[test]
    fn generate_etag_differs_for_different_content() {
        let etag1 = generate_etag(b"content1");
        let etag2 = generate_etag(b"content2");
        assert_ne!(etag1, etag2);
    }

    #[test]
    fn etags_match_single_etag() {
        let server_etag = "\"abc123\"";
        assert!(etags_match("\"abc123\"", server_etag));
    }

    #[test]
    fn etags_match_returns_false_for_different_etag() {
        let server_etag = "\"abc123\"";
        assert!(!etags_match("\"def456\"", server_etag));
    }

    #[test]
    fn etags_match_handles_multiple_etags() {
        let server_etag = "\"abc123\"";
        assert!(etags_match("\"xyz\", \"abc123\", \"def\"", server_etag));
    }

    #[test]
    fn etags_match_ignores_weak_indicator() {
        let server_etag = "\"abc123\"";
        // Weak ETag matching should succeed with strong ETag
        // (though we don't generate weak ETags)
        assert!(etags_match("W/\"abc123\"", server_etag));
    }

    #[test]
    fn etags_match_handles_whitespace() {
        let server_etag = "\"abc123\"";
        assert!(etags_match("  \"abc123\"  ", server_etag));
        assert!(etags_match("\"xyz\",   \"abc123\"", server_etag));
    }
}
```

### Integration Tests

**File**: `src/bin/integration_test.rs`

Add comprehensive end-to-end tests:

```rust
#[test]
fn test_etag_header_included_in_200_response() {
    let port = pick_free_port();
    let mut server = start_server(port);
    let addr = format!("127.0.0.1:{port}");
    wait_for_server(&addr, Duration::from_secs(5)).unwrap();

    let mut stream = TcpStream::connect(&addr).unwrap();
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .unwrap();

    let response = read_response(&mut stream).unwrap();
    assert_eq!(response.status_code, 200);
    assert!(response.headers.contains_key("etag"));
    let etag = &response.headers["etag"];
    assert!(etag.starts_with("\""));
    assert!(etag.ends_with("\""));

    server.kill().unwrap();
}

#[test]
fn test_if_none_match_matching_etag_returns_304() {
    let port = pick_free_port();
    let mut server = start_server(port);
    let addr = format!("127.0.0.1:{port}");
    wait_for_server(&addr, Duration::from_secs(5)).unwrap();

    // First request to get the ETag
    let mut stream = TcpStream::connect(&addr).unwrap();
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .unwrap();
    let response = read_response(&mut stream).unwrap();
    let etag = response.headers["etag"].clone();
    drop(stream);

    // Second request with If-None-Match header
    let mut stream = TcpStream::connect(&addr).unwrap();
    let request = format!(
        "GET / HTTP/1.1\r\nHost: localhost\r\nIf-None-Match: {}\r\n\r\n",
        etag
    );
    stream.write_all(request.as_bytes()).unwrap();

    let response = read_response(&mut stream).unwrap();
    assert_eq!(response.status_code, 304);
    assert_eq!(response.body, ""); // 304 must not include body
    assert!(response.headers.contains_key("etag"));
    assert!(!response.headers.contains_key("content-length"));

    server.kill().unwrap();
}

#[test]
fn test_if_none_match_different_etag_returns_200() {
    let port = pick_free_port();
    let mut server = start_server(port);
    let addr = format!("127.0.0.1:{port}");
    wait_for_server(&addr, Duration::from_secs(5)).unwrap();

    let mut stream = TcpStream::connect(&addr).unwrap();
    let request = "GET / HTTP/1.1\r\nHost: localhost\r\nIf-None-Match: \"wrong-etag\"\r\n\r\n";
    stream.write_all(request.as_bytes()).unwrap();

    let response = read_response(&mut stream).unwrap();
    assert_eq!(response.status_code, 200);
    assert!(!response.body.is_empty());
    assert!(response.headers.contains_key("etag"));

    server.kill().unwrap();
}

#[test]
fn test_if_none_match_multiple_etags_with_match() {
    let port = pick_free_port();
    let mut server = start_server(port);
    let addr = format!("127.0.0.1:{port}");
    wait_for_server(&addr, Duration::from_secs(5)).unwrap();

    // Get the current ETag
    let mut stream = TcpStream::connect(&addr).unwrap();
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .unwrap();
    let response = read_response(&mut stream).unwrap();
    let etag = response.headers["etag"].clone();
    drop(stream);

    // Send multiple ETags, one of which matches
    let mut stream = TcpStream::connect(&addr).unwrap();
    let request = format!(
        "GET / HTTP/1.1\r\nHost: localhost\r\nIf-None-Match: \"old-etag\", {}, \"future-etag\"\r\n\r\n",
        etag
    );
    stream.write_all(request.as_bytes()).unwrap();

    let response = read_response(&mut stream).unwrap();
    assert_eq!(response.status_code, 304);
    assert_eq!(response.body, "");

    server.kill().unwrap();
}

#[test]
fn test_if_none_match_with_head_returns_304() {
    let port = pick_free_port();
    let mut server = start_server(port);
    let addr = format!("127.0.0.1:{port}");
    wait_for_server(&addr, Duration::from_secs(5)).unwrap();

    // Get the ETag
    let mut stream = TcpStream::connect(&addr).unwrap();
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .unwrap();
    let response = read_response(&mut stream).unwrap();
    let etag = response.headers["etag"].clone();
    drop(stream);

    // HEAD request with If-None-Match
    let mut stream = TcpStream::connect(&addr).unwrap();
    let request = format!(
        "HEAD / HTTP/1.1\r\nHost: localhost\r\nIf-None-Match: {}\r\n\r\n",
        etag
    );
    stream.write_all(request.as_bytes()).unwrap();

    let response = read_response(&mut stream).unwrap();
    assert_eq!(response.status_code, 304);

    server.kill().unwrap();
}

#[test]
fn test_etag_consistency_across_requests() {
    let port = pick_free_port();
    let mut server = start_server(port);
    let addr = format!("127.0.0.1:{port}");
    wait_for_server(&addr, Duration::from_secs(5)).unwrap();

    // Get ETag from first request
    let mut stream = TcpStream::connect(&addr).unwrap();
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .unwrap();
    let response1 = read_response(&mut stream).unwrap();
    let etag1 = response1.headers["etag"].clone();
    drop(stream);

    // Get ETag from second request
    let mut stream = TcpStream::connect(&addr).unwrap();
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .unwrap();
    let response2 = read_response(&mut stream).unwrap();
    let etag2 = response2.headers["etag"].clone();
    drop(stream);

    // ETags should be identical for unchanged content
    assert_eq!(etag1, etag2);

    server.kill().unwrap();
}

#[test]
fn test_if_none_match_ignores_weak_etag_indicator() {
    let port = pick_free_port();
    let mut server = start_server(port);
    let addr = format!("127.0.0.1:{port}");
    wait_for_server(&addr, Duration::from_secs(5)).unwrap();

    // Get the current ETag
    let mut stream = TcpStream::connect(&addr).unwrap();
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .unwrap();
    let response = read_response(&mut stream).unwrap();
    let etag = response.headers["etag"].clone();
    drop(stream);

    // Send ETag with weak indicator (client may do this)
    let mut stream = TcpStream::connect(&addr).unwrap();
    let request = format!(
        "GET / HTTP/1.1\r\nHost: localhost\r\nIf-None-Match: W/{}\r\n\r\n",
        etag
    );
    stream.write_all(request.as_bytes()).unwrap();

    let response = read_response(&mut stream).unwrap();
    assert_eq!(response.status_code, 304);

    server.kill().unwrap();
}
```

---

## Edge Cases & Handling

### 1. 404 Responses

**Behavior**: ETag should be generated and included in 404 responses.

**Rationale**: Clients should be able to cache 404 responses and validate them with If-None-Match.

**Implementation**: Generate ETag for the not_found.html content before determining 200 vs 404.

```rust
// Read file and generate ETag
let contents = fs::read_to_string(filename).unwrap();
let contents_bytes = contents.as_bytes();
let etag = generate_etag(contents_bytes);
response.add_etag(etag.clone());

// Check If-None-Match early, before determining route validity
if let Some(if_none_match) = http_request.try_get_if_none_match() {
    if http_request.method == HttpMethods::GET ||
       http_request.method == HttpMethods::HEAD {
        if etags_match(&if_none_match, &etag) {
            let response = HttpResponse::build(String::from("HTTP/1.1"), 304);
            return;
        }
    }
}
```

### 2. POST/PUT/DELETE Requests

**Behavior**: If-None-Match should be ignored for non-safe methods; proceed with 200.

**Rationale**: Per RFC 7232, If-None-Match is only meaningful for GET/HEAD/conditional safe methods.

**Implementation**: Check method before applying conditional logic (already in code snippet).

### 3. Malformed ETag Headers

**Examples**:
- Missing quotes: `If-None-Match: abc123` (invalid)
- Empty header: `If-None-Match: ` (invalid)
- Garbled: `If-None-Match: !!!???` (invalid)

**Behavior**: If ETag format is invalid, treat as non-matching and return 200 with content.

**Rationale**: Conservative approach; when in doubt, serve fresh content.

**Implementation**: The comparison function will simply not match invalid formats:

```rust
fn etags_match(if_none_match: &str, resource_etag: &str) -> bool {
    // If the client sends completely mangled ETags, they won't match
    // and we'll serve fresh content (safe fallback)
    for client_etag in if_none_match.split(',') {
        let client_tag = client_etag.trim();
        if client_tag.is_empty() {
            continue; // Skip empty segments
        }
        let client_tag = if client_tag.starts_with("W/") {
            &client_tag[2..]
        } else {
            client_tag
        };

        if client_tag == resource_etag {
            return true;
        }
    }
    false
}
```

### 4. Asterisk (*) in If-None-Match

**Behavior**: Not supported in initial implementation.

**Rationale**: The asterisk wildcard (meaning "any ETag") is rare in practice and primarily used with conditional PUT requests (If-Match).

**Future Enhancement**: If needed, add support:

```rust
fn etags_match(if_none_match: &str, resource_etag: &str) -> bool {
    if if_none_match.trim() == "*" {
        return true; // Matches any representation
    }
    // ... rest of matching logic
}
```

### 5. Case Sensitivity

**Current Behavior**: Header names are normalized to lowercase during parsing.

**Risk**: ETags are case-sensitive strings; `"ABC123"` != `"abc123"`.

**Implementation**: Already correct due to lowercasing only header names, not values.

### 6. Server Restarts and ETag Persistence

**Current Limitation**: ETags are generated per-request using content hash; they won't be consistent across server restarts due to hash seed randomization in std::collections::hash_map.

**Impact**: Clients cannot use ETags between server restarts.

**Mitigation**: For this implementation level, this is acceptable. Add to "Future Enhancements":
- Implement file modification time + size-based ETags for persistence
- Or use a deterministic hashing algorithm (e.g., SHA-256) if external deps allowed

### 7. Large Files

**Current Limitation**: Entire file is read into memory to generate ETag.

**Impact**: May cause memory pressure on large files.

**Mitigation**: For this implementation level, acceptable (file serving already reads into memory). Future enhancement:
- Stream-based hashing for large files

### 8. Conditional Request on Routes Without Files

**Behavior**: Non-existent routes map to 404 page; If-None-Match applies to 404 content.

**Example**: Request to `/this/doesnt/exist` with If-None-Match matching 404 content returns 304 instead of 404.

**Rationale**: This is actually correct per HTTP semantics; the 404 response itself is a representation that can be cached.

**Alternative Behavior**: Only apply conditional logic to 200 responses, not 404. This requires restructuring:

```rust
// Check If-None-Match only for successful routes
if routes.contains_key(&clean_target) {
    if let Some(if_none_match) = http_request.try_get_if_none_match() {
        // ... conditional logic
    }
}
```

**Recommendation**: Implement the alternative behavior to avoid confusing 304 responses for failed routes.

---

## Implementation Checklist

- [ ] Add `try_get_if_none_match()` method to `HttpRequest`
- [ ] Add `add_etag()` and `is_not_modified()` methods to `HttpResponse`
- [ ] Verify 304 responses don't include body or Content-Length
- [ ] Implement `generate_etag()` function in `main.rs`
- [ ] Implement `etags_match()` function in `main.rs`
- [ ] Update `handle_connection()` to check If-None-Match and return 304
- [ ] Add unit tests for `try_get_if_none_match()`
- [ ] Add unit tests for `add_etag()`
- [ ] Add unit tests for `is_not_modified()`
- [ ] Add unit tests for `generate_etag()`
- [ ] Add unit tests for `etags_match()`
- [ ] Add integration tests for 304 responses
- [ ] Add integration tests for multiple ETags
- [ ] Add integration tests for ETag consistency
- [ ] Add integration tests for different paths (ensure separate ETags)
- [ ] Test with curl to verify real-world behavior
- [ ] Verify backward compatibility (requests without If-None-Match unchanged)
- [ ] Verify 404 pages receive ETags
- [ ] Document ETag generation strategy in code comments

---

## Manual Testing Guide

### Test 1: Basic ETag Retrieval

```bash
# Start server
cargo run &
SERVER_PID=$!

# Request homepage and capture ETag
RESPONSE=$(curl -v http://localhost:7878/)
# Look for: etag: "..." in response headers

kill $SERVER_PID
```

### Test 2: 304 Not Modified with Matching ETag

```bash
cargo run &
SERVER_PID=$!

# Get initial ETag
ETAG=$(curl -s -i http://localhost:7878/ | grep -i etag | cut -d' ' -f2 | tr -d '\r')
echo "Retrieved ETag: $ETAG"

# Make conditional request
curl -i -H "If-None-Match: $ETAG" http://localhost:7878/

# Should see: HTTP/1.1 304 Not Modified
# Should NOT see response body

kill $SERVER_PID
```

### Test 3: 200 OK with Different ETag

```bash
cargo run &
SERVER_PID=$!

curl -i -H "If-None-Match: \"different-etag\"" http://localhost:7878/

# Should see: HTTP/1.1 200 OK
# Should see response body

kill $SERVER_PID
```

### Test 4: Multiple ETags

```bash
cargo run &
SERVER_PID=$!

ETAG=$(curl -s -i http://localhost:7878/ | grep -i etag | cut -d' ' -f2 | tr -d '\r')

curl -i -H "If-None-Match: \"old\", $ETAG, \"future\"" http://localhost:7878/

# Should see: HTTP/1.1 304 Not Modified

kill $SERVER_PID
```

### Test 5: Run Integration Tests

```bash
cargo run --bin integration_test
```

---

## Implementation Difficulty & Risks

### Low-Risk Areas
- HttpRequest/HttpResponse model modifications (straightforward, well-tested)
- ETag generation (no external dependencies)
- Integration tests (follow existing patterns)

### Moderate-Risk Areas
- 304 response handling (must not include Content-Length; currently add_body() always sets it)
- ETag persistence across requests (hash randomization issue; acceptable for scope)

### Mitigation Strategies
1. **Thorough unit testing** of edge cases before integration tests
2. **Manual curl testing** to verify HTTP semantics
3. **Code review** of handle_connection() changes
4. **Gradual integration**: Implement in order, test after each step

---

## Success Criteria

- All unit tests pass (34+ existing tests + new ETag tests)
- All integration tests pass (12+ existing tests + new conditional request tests)
- `cargo test` runs successfully with 100% pass rate
- `cargo test --bin integration_test` runs successfully
- Manual curl tests show correct behavior
- No regressions in existing functionality
- ETags are generated for all responses (200, 404)
- 304 responses have no body
- 304 responses include ETag header
- Requests without If-None-Match unaffected

---

## Future Enhancements

1. **Weak ETag Support**: Generate weak ETags (prefixed with `W/`) for equivalent representations
2. **Multiple ETag Matching**: Already implemented in this plan; allow comma-separated lists
3. **If-Match Header**: Opposite of If-None-Match; return 412 Precondition Failed if ETag doesn't match
4. **If-Modified-Since**: Return 304 if resource unchanged since timestamp
5. **Last-Modified Header**: Include in responses for timestamp-based validation
6. **Persistent ETags**: Use file mtime + size for consistent ETags across restarts
7. **Streaming ETag Calculation**: For large files, hash while streaming
8. **Cache-Control Integration**: Combine with Cache-Control headers for full caching semantics
9. **Compression Support**: Handle ETags for pre-compressed content variants
10. **HEAD Request Optimization**: Avoid reading file body for HEAD requests

---

## References

- **RFC 7232 - HTTP Conditional Requests**: https://tools.ietf.org/html/rfc7232
  - Sections 2 (ETags), 3 (If-None-Match), 4.1 (304 Not Modified)
- **RFC 7234 - HTTP Caching**: https://tools.ietf.org/html/rfc7234
- **MDN - ETag**: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/ETag
- **MDN - If-None-Match**: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/If-None-Match
- **Rust std::hash**: https://doc.rust-lang.org/std/hash/index.html

---

## Notes

- This implementation prioritizes correctness and simplicity over performance
- No external dependencies required (uses std::hash)
- ETag format is compliant with RFC 7232
- Handles common use cases; edge cases documented for future work
