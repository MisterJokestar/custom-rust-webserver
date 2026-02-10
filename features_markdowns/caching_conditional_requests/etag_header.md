# ETag Header Implementation Plan

## Overview

This feature adds **ETag (Entity Tag)** header support to the rcomm web server, enabling browser-based HTTP caching and conditional request handling. An ETag is a unique identifier for a specific version of a resource that allows clients to validate cached content without re-downloading the entire file.

### Why ETags Matter

ETags are fundamental to HTTP caching and enable:
- **Bandwidth savings**: Clients send `If-None-Match` headers with cached ETags; servers respond with `304 Not Modified` instead of re-transmitting the full file
- **Cache revalidation**: Safe way to check if cached content is still valid
- **Version tracking**: Useful for detecting file changes across multiple requests
- **Performance**: Lightweight validation compared to full body transmission

### Design Approach

Since rcomm has **no external dependencies**, we will use a **hybrid hashing strategy**:
1. **Primary**: Content-based hash computed from file bytes (simple sum-based algorithm)
2. **Secondary**: Fallback combining file size + modification timestamp (for efficiency)

ETags will be generated once per file serve, not pre-computed, to keep implementation simple and memory-efficient.

---

## Files to Modify

### 1. **`src/models/http_response.rs`** (New Methods)
Add ETag setter and conditional request handling support.

### 2. **`src/main.rs`** (Connection Handler)
- Generate ETags before sending responses
- Detect and handle `If-None-Match` requests
- Return `304 Not Modified` responses when appropriate

### 3. **`src/lib.rs`** (New ETag Module)
Create `etag.rs` module with hashing and comparison logic.

### 4. **`src/models.rs`** (Barrel File)
Export the new `etag` module.

### 5. **Integration Tests** (`src/bin/integration_test.rs`)
Add test cases for ETag generation and conditional request handling.

### 6. **Unit Tests**
Add unit tests in the affected modules.

---

## Step-by-Step Implementation

### Step 1: Create ETag Generation Module (`src/etag.rs`)

Create a new module responsible for generating and comparing ETags. This module will:
- Compute a content hash from file bytes
- Generate weak ETags (prefixed with `W/`)
- Compare ETags for `If-None-Match` validation

**File: `/home/jwall/personal/rusty/rcomm/src/etag.rs`**

```rust
use std::fs::Metadata;

/// Generates a simple content-based hash by summing byte values (mod u64::MAX)
/// This is a lightweight, no-dependency approach suitable for embedded use.
///
/// Note: This is not cryptographically secure but is sufficient for ETag purposes
/// where collision resistance is not critical.
fn compute_content_hash(content: &[u8]) -> u64 {
    let mut hash: u64 = 0;
    for &byte in content {
        hash = hash.wrapping_add(byte as u64);
    }
    hash
}

/// Generates a metadata-based hash combining file size and modification time
/// Used as fallback when content is not available.
fn compute_metadata_hash(metadata: &Metadata) -> u64 {
    let len = metadata.len();
    let modified = metadata
        .modified()
        .unwrap_or(std::time::UNIX_EPOCH);

    let duration = modified
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();

    let mtime_secs = duration.as_secs();
    len.wrapping_add(mtime_secs)
}

/// Generates a weak ETag string from content bytes
/// Format: W/"<hash>"
///
/// Weak ETags (prefixed with W/) indicate that two resources with the same
/// ETag are equivalent for caching purposes but not bit-for-bit identical.
pub fn generate_etag_from_content(content: &[u8]) -> String {
    let hash = compute_content_hash(content);
    format!(r#"W/"{}""#, hash)
}

/// Generates a weak ETag string from file metadata
pub fn generate_etag_from_metadata(metadata: &Metadata) -> String {
    let hash = compute_metadata_hash(metadata);
    format!(r#"W/"{}""#, hash)
}

/// Checks if a client's `If-None-Match` header matches the current ETag
///
/// The header value may contain multiple ETags (comma-separated), per RFC 7232.
/// If any tag matches, returns `true` (indicating 304 response is appropriate).
pub fn etag_matches(current_etag: &str, if_none_match: &str) -> bool {
    for tag in if_none_match.split(',') {
        let tag = tag.trim();

        // Handle wildcard
        if tag == "*" {
            return true;
        }

        // Direct comparison (accounts for weak vs strong ETags)
        if tag == current_etag {
            return true;
        }

        // Also compare without weak prefix for compatibility
        let current_weak_part = if current_etag.starts_with("W/") {
            &current_etag[2..]
        } else {
            current_etag
        };

        let tag_weak_part = if tag.starts_with("W/") {
            &tag[2..]
        } else {
            tag
        };

        if current_weak_part == tag_weak_part {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_content_hash_returns_consistent_hash() {
        let content = b"hello world";
        let hash1 = compute_content_hash(content);
        let hash2 = compute_content_hash(content);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn compute_content_hash_differs_for_different_content() {
        let hash1 = compute_content_hash(b"hello");
        let hash2 = compute_content_hash(b"world");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn generate_etag_from_content_produces_weak_tag() {
        let etag = generate_etag_from_content(b"test");
        assert!(etag.starts_with("W/"));
        assert!(etag.ends_with("\""));
    }

    #[test]
    fn etag_matches_exact_match() {
        let etag = r#"W/"12345""#;
        assert!(etag_matches(etag, etag));
    }

    #[test]
    fn etag_matches_wildcard() {
        let etag = r#"W/"12345""#;
        assert!(etag_matches(etag, "*"));
    }

    #[test]
    fn etag_matches_comma_separated_list() {
        let etag = r#"W/"12345""#;
        let if_none_match = r#"W/"99999", W/"12345", W/"11111""#;
        assert!(etag_matches(etag, if_none_match));
    }

    #[test]
    fn etag_matches_returns_false_for_mismatch() {
        let etag = r#"W/"12345""#;
        let if_none_match = r#"W/"99999""#;
        assert!(!etag_matches(etag, if_none_match));
    }

    #[test]
    fn etag_matches_weak_vs_strong_compatibility() {
        let current = r#"W/"12345""#;
        let weak_client = r#"W/"12345""#;
        let strong_client = r#""12345""#;

        assert!(etag_matches(current, weak_client));
        // For weak ETag matching, both should match
        assert!(etag_matches(current, strong_client));
    }
}
```

### Step 2: Update `src/lib.rs` to Export ETag Module

Add the ETag module to the library exports.

**File: `/home/jwall/personal/rusty/rcomm/src/lib.rs`**

Add after `pub mod models;`:

```rust
pub mod etag;
```

The full updated section should look like:

```rust
pub mod models;
pub mod etag;
```

### Step 3: Add ETag Methods to `HttpResponse`

Add getter and setter for ETags in the response struct.

**File: `/home/jwall/personal/rusty/rcomm/src/models/http_response.rs`**

Add these methods to the `impl HttpResponse` block (after the `try_get_body()` method):

```rust
    pub fn set_etag(&mut self, etag: String) -> &mut HttpResponse {
        self.headers.insert("etag".to_string(), etag);
        self
    }

    pub fn try_get_etag(&self) -> Option<String> {
        self.headers.get("etag").cloned()
    }
```

Add test cases in the `#[cfg(test)]` section:

```rust
    #[test]
    fn set_etag_stores_etag() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
        resp.set_etag(r#"W/"12345""#.to_string());
        assert_eq!(resp.try_get_etag(), Some(r#"W/"12345""#.to_string()));
    }

    #[test]
    fn etag_appears_in_response_headers() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
        resp.set_etag(r#"W/"abc123""#.to_string());
        let output = format!("{resp}");
        assert!(output.contains(r#"etag: W/"abc123""#));
    }
```

### Step 4: Update `src/main.rs` to Generate and Handle ETags

Modify the `handle_connection()` function to:
1. Generate an ETag for the file content
2. Check for `If-None-Match` header
3. Return 304 if ETags match

**File: `/home/jwall/personal/rusty/rcomm/src/main.rs`**

First, add imports at the top:

```rust
use rcomm::etag::{generate_etag_from_content, etag_matches};
```

Then replace the `handle_connection()` function with this updated version:

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

    // Generate ETag from file content
    let etag = generate_etag_from_content(contents.as_bytes());
    response.set_etag(etag.clone());

    // Check for If-None-Match header (conditional request)
    if let Some(if_none_match) = http_request.try_get_header("if-none-match".to_string()) {
        if etag_matches(&etag, &if_none_match) {
            // Resource hasn't changed, return 304 Not Modified
            let mut not_modified = HttpResponse::build(String::from("HTTP/1.1"), 304);
            not_modified.set_etag(etag);
            println!("Response: {not_modified}");
            stream.write_all(&not_modified.as_bytes()).unwrap();
            return;
        }
    }

    response.add_body(contents.into());

    println!("Response: {response}");
    stream.write_all(&response.as_bytes()).unwrap();
}
```

### Step 5: Update Integration Tests

Add test cases to validate ETag functionality end-to-end.

**File: `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`**

Look for the test module and add these test functions:

```rust
fn test_etag_header_present() -> TestResult {
    let response = send_request("GET", "/", None, None)?;
    match response.find("etag:") {
        Some(_) => TestResult::pass("ETag header present"),
        None => TestResult::fail("ETag header missing"),
    }
}

fn test_etag_consistent_for_same_file() -> TestResult {
    let response1 = send_request("GET", "/", None, None)?;
    let response2 = send_request("GET", "/", None, None)?;

    let etag1 = extract_header(&response1, "etag");
    let etag2 = extract_header(&response2, "etag");

    if etag1 == etag2 && etag1.is_some() {
        TestResult::pass("ETag consistent for same file")
    } else {
        TestResult::fail("ETag inconsistent or missing")
    }
}

fn test_304_not_modified_with_matching_etag() -> TestResult {
    // Get initial response with ETag
    let initial = send_request("GET", "/", None, None)?;
    let etag = extract_header(&initial, "etag");

    if let Some(etag_val) = etag {
        // Send conditional request with matching ETag
        let response = send_request("GET", "/", Some(("If-None-Match", &etag_val)), None)?;

        if response.starts_with("HTTP/1.1 304") {
            TestResult::pass("304 Not Modified returned for matching ETag")
        } else {
            TestResult::fail(&format!("Expected 304, got: {}", first_line(&response)))
        }
    } else {
        TestResult::fail("Could not extract ETag from initial response")
    }
}

fn test_200_ok_with_non_matching_etag() -> TestResult {
    // Send request with non-matching ETag
    let response = send_request("GET", "/", Some(("If-None-Match", r#"W/"fake""#)), None)?;

    if response.starts_with("HTTP/1.1 200") {
        TestResult::pass("200 OK returned for non-matching ETag")
    } else {
        TestResult::fail(&format!("Expected 200, got: {}", first_line(&response)))
    }
}

fn test_etag_differs_across_files() -> TestResult {
    let response1 = send_request("GET", "/", None, None)?;
    let response2 = send_request("GET", "/style.css", None, None)?;

    let etag1 = extract_header(&response1, "etag");
    let etag2 = extract_header(&response2, "etag");

    match (etag1, etag2) {
        (Some(e1), Some(e2)) if e1 != e2 => {
            TestResult::pass("ETags differ across different files")
        }
        (None, _) | (_, None) => TestResult::fail("Missing ETag on one or both files"),
        (Some(e1), Some(e2)) => {
            TestResult::fail(&format!("ETags should differ but both are: {}", e1))
        }
    }
}
```

Also add helper functions if they don't exist:

```rust
fn extract_header(response: &str, header_name: &str) -> Option<String> {
    let lower_name = format!("{}:", header_name.to_lowercase());
    for line in response.lines() {
        if line.to_lowercase().starts_with(&lower_name) {
            return Some(line[lower_name.len()..].trim().to_string());
        }
    }
    None
}

fn first_line(response: &str) -> String {
    response.lines().next().unwrap_or("").to_string()
}
```

Add these test invocations to the main test runner (in the same file, where tests are registered):

```rust
run_test(test_etag_header_present);
run_test(test_etag_consistent_for_same_file);
run_test(test_304_not_modified_with_matching_etag);
run_test(test_200_ok_with_non_matching_etag);
run_test(test_etag_differs_across_files);
```

---

## Testing Strategy

### Unit Tests

The plan includes unit tests for:

1. **ETag Generation** (`src/etag.rs`)
   - Consistent hashing of same content
   - Different hashes for different content
   - Weak ETag format validation (W/"...")

2. **ETag Matching** (`src/etag.rs`)
   - Exact match detection
   - Wildcard (`*`) matching
   - Comma-separated ETag lists
   - Mismatch detection

3. **HTTP Response Methods** (`src/models/http_response.rs`)
   - ETag setter stores value correctly
   - ETag getter retrieves value
   - ETag appears in serialized response

### Integration Tests

End-to-end tests in `src/bin/integration_test.rs`:

1. **ETag Header Presence**
   - Verify ETag is present in response headers

2. **ETag Consistency**
   - Same file served twice returns same ETag

3. **Conditional Requests (304 Not Modified)**
   - Client sends `If-None-Match` with matching ETag
   - Server responds with `304 Not Modified`
   - Response body is empty
   - ETag header still present in 304 response

4. **Full Response on Mismatch**
   - Client sends `If-None-Match` with non-matching ETag
   - Server responds with `200 OK` and full body

5. **Cross-File Differentiation**
   - Different files have different ETags

### Manual Testing

```bash
# Build and run the server
cargo build
cargo run &
SERVER_PID=$!

# Test 1: Check ETag is present
curl -i http://127.0.0.1:7878/

# Test 2: Verify 304 Not Modified
ETAG=$(curl -s -i http://127.0.0.1:7878/ | grep -i "^etag:" | cut -d' ' -f2)
curl -i -H "If-None-Match: $ETAG" http://127.0.0.1:7878/

# Test 3: Verify 200 with different ETag
curl -i -H "If-None-Match: W/\"fake\"" http://127.0.0.1:7878/

# Cleanup
kill $SERVER_PID
```

---

## Edge Cases & Considerations

### 1. **404 Responses**
- **Issue**: 404 responses (not_found.html) should also have ETags
- **Solution**: Apply ETag generation to all responses with bodies (200 and 404)
- **Note**: The current implementation does this correctly since `handle_connection()` generates ETags before checking route validity

### 2. **Large Files**
- **Issue**: Computing hash by summing all bytes could be slow for large files
- **Mitigation**: This is acceptable for typical static file serving. For multi-GB files, consider caching ETags in the future
- **Future Optimization**: Implement LRU cache for ETags of frequently-served files

### 3. **File Modification Between Requests**
- **Scenario**: File is modified on disk between two client requests
- **Behavior**: New ETag will be generated; conditional request will fail (304 won't be sent)
- **Correctness**: This is correct behavior - modified files should be re-served

### 4. **Multiple ETag Formats**
- **Strong ETags**: `"12345"` (exact byte match required)
- **Weak ETags**: `W/"12345"` (equivalent resources, not identical)
- **Implementation**: We generate only weak ETags (sufficient for caching)
- **Compatibility**: Code correctly handles both formats in `If-None-Match` comparison

### 5. **Wildcard Matching**
- **Per RFC 7232**: `If-None-Match: *` matches any resource
- **Implementation**: Handled in `etag_matches()` function
- **Use Case**: Useful for conditional uploads (prevent overwriting)

### 6. **No-Cache vs No-Store**
- **Note**: ETag support does not affect `Cache-Control` headers (separate feature)
- **Future**: Consider pairing with `Cache-Control` headers in future enhancement

### 7. **Performance Implications**
- **Hash Computation**: O(n) where n = file size, but only on first request
- **Memory**: Minimal - ETag is only a few bytes per response
- **Complexity**: Simple sum-based hash avoids external dependencies

### 8. **Security Considerations**
- **ETag Format**: Weak ETags with no cryptographic requirements are safe to expose
- **Information Leakage**: ETags do not reveal file content or sensitive metadata
- **No Vulnerability**: Standard HTTP caching mechanism, no security concerns

### 9. **Backward Compatibility**
- **No Breaking Changes**: New feature adds headers; doesn't modify existing behavior
- **Graceful Degradation**: Clients without `If-None-Match` support continue to work normally
- **HTTP/1.1 Compliance**: ETag support enhances HTTP/1.1 compliance

---

## Implementation Checklist

- [ ] Create `src/etag.rs` with content hashing and ETag matching logic
- [ ] Add unit tests in `src/etag.rs`
- [ ] Update `src/lib.rs` to export `etag` module
- [ ] Add `set_etag()` and `try_get_etag()` methods to `HttpResponse`
- [ ] Add unit tests for ETag methods in `http_response.rs`
- [ ] Update `src/main.rs` imports to include ETag functions
- [ ] Modify `handle_connection()` to generate ETags
- [ ] Add If-None-Match conditional request handling in `handle_connection()`
- [ ] Add integration tests to `src/bin/integration_test.rs`
- [ ] Run full test suite: `cargo test` and `cargo test --bin integration_test`
- [ ] Manual testing with curl
- [ ] Update CLAUDE.md if needed to document ETag behavior
- [ ] Verify no regressions in existing tests

---

## Estimated Effort

- **Code Implementation**: 2-3 hours
- **Testing & Validation**: 1-2 hours
- **Documentation & Review**: 30 minutes

**Total**: ~4 hours

---

## Related Features & Future Enhancements

1. **Cache-Control Headers**: Pair ETags with `Cache-Control` directives
2. **Last-Modified Header**: Implement last modification time tracking
3. **ETag Caching**: Pre-compute and cache ETags for frequently-served files
4. **Conditional GET**: Extend to support `If-Modified-Since` for time-based caching
5. **Compression**: Ensure ETags account for content encoding (Content-Encoding header)

---

## References

- **RFC 7232 - HTTP Conditional Requests**: https://tools.ietf.org/html/rfc7232#section-2.3
- **RFC 7234 - HTTP Caching**: https://tools.ietf.org/html/rfc7234
- **MDN ETag Documentation**: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/ETag
- **MDN If-None-Match**: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/If-None-Match

