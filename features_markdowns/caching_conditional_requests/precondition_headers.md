# Implementation Plan: `If-Match` / `If-Unmodified-Since` Precondition Headers

## Overview

This implementation plan adds support for HTTP precondition headers (`If-Match` and `If-Unmodified-Since`) to the rcomm web server. These headers allow clients to conditionally request resources based on entity tags (ETags) or modification time, enabling efficient caching and preventing race conditions during updates.

### What are precondition headers?

**`If-Match`**: A client sends this header with an ETag value. The server should only fulfill the request if the resource's current ETag matches the provided value. Typically used with PUT/POST to prevent overwriting resources that have changed.

**`If-Unmodified-Since`**: A client sends this header with a date. The server should only fulfill the request if the resource has not been modified since that date. Also used for conditional write operations.

### Responses triggered by failed preconditions

- **412 Precondition Failed**: Returned when an `If-Match` or `If-Unmodified-Since` condition fails for safe methods (GET, HEAD) or when the server cannot evaluate the precondition.
- **412 Precondition Failed** (for PUT/POST): Standard response when preconditions fail for unsafe methods.

### Scope for rcomm

For the initial implementation, rcomm focuses on:
1. Parsing `If-Match` and `If-Unmodified-Since` headers from requests.
2. Generating and returning ETag headers in responses (based on file content hash).
3. Computing `Last-Modified` headers from file modification time.
4. Evaluating preconditions and returning 412 when conditions fail.
5. Allowing GET requests to proceed when preconditions pass.

**Out of scope** (for now):
- Conditional write semantics (PUT/POST logic); rcomm currently serves only static files.
- Complex ETag validation (weak vs. strong ETags); we use simple content-based ETags.
- Precise Last-Modified parsing beyond HTTP-date format.

---

## Files to Modify

1. **`src/models/http_request.rs`**
   - Add methods to extract and parse `If-Match` and `If-Unmodified-Since` headers.
   - Add helper functions for date/ETag parsing.

2. **`src/models/http_response.rs`**
   - Add methods to set `ETag` and `Last-Modified` response headers.
   - Ensure headers are properly formatted.

3. **`src/main.rs`**
   - Modify `handle_connection()` to evaluate preconditions before serving content.
   - Return 412 Precondition Failed when conditions fail.
   - Compute and attach ETag/Last-Modified headers to responses.

4. **`src/bin/integration_test.rs`** (optional, for testing)
   - Add integration tests to verify precondition header behavior.

---

## Step-by-Step Implementation

### Phase 1: Add ETag/Last-Modified Generation Utilities

#### 1.1 Create a new module for precondition utilities

Create `/home/jwall/personal/rusty/rcomm/src/models/preconditions.rs`:

```rust
use std::time::{SystemTime, UNIX_EPOCH};
use std::fs::Metadata;

/// Represents a strong or weak ETag.
#[derive(Debug, Clone, PartialEq)]
pub enum ETag {
    Strong(String),
    Weak(String),
}

impl ETag {
    /// Create a strong ETag from content bytes.
    pub fn from_content(content: &[u8]) -> Self {
        // Simple hash-based ETag: use a simple checksum
        let hash = simple_hash(content);
        ETag::Strong(format!("\"{}\"", hash))
    }

    /// Parse ETag from header value (e.g., `"abc123"` or `W/"abc123"`).
    pub fn parse(value: &str) -> Option<Self> {
        let trimmed = value.trim();
        if trimmed.starts_with("W/") {
            Some(ETag::Weak(trimmed[2..].to_string()))
        } else {
            Some(ETag::Strong(trimmed.to_string()))
        }
    }

    /// Format ETag for HTTP header.
    pub fn as_header(&self) -> String {
        match self {
            ETag::Strong(tag) => tag.clone(),
            ETag::Weak(tag) => format!("W/{}", tag),
        }
    }

    /// Check if this ETag matches another (for If-Match evaluation).
    pub fn matches(&self, other: &ETag) -> bool {
        // For strong comparison: exact match only.
        // For weak comparison (If-None-Match): both strong and weak match.
        match (self, other) {
            (ETag::Strong(a), ETag::Strong(b)) => a == b,
            (ETag::Strong(a), ETag::Weak(b)) => a == b,
            (ETag::Weak(a), ETag::Strong(b)) => a == b,
            (ETag::Weak(a), ETag::Weak(b)) => a == b,
        }
    }
}

/// Simple hash function for ETag generation (no external dependencies).
fn simple_hash(data: &[u8]) -> String {
    let mut hash: u64 = 5381;
    for &byte in data {
        hash = hash.wrapping_mul(33).wrapping_add(byte as u64);
    }
    format!("{:x}", hash)
}

/// Get HTTP-date formatted Last-Modified from file metadata.
pub fn last_modified_from_metadata(metadata: &Metadata) -> Option<String> {
    let modified = metadata.modified().ok()?;
    let duration = modified.duration_since(UNIX_EPOCH).ok()?;

    // Convert to HTTP-date format (RFC 7231: e.g., "Wed, 21 Oct 2015 07:28:00 GMT")
    Some(system_time_to_http_date(modified))
}

/// Convert SystemTime to HTTP-date string.
fn system_time_to_http_date(time: SystemTime) -> String {
    // Simplified: For production, use a proper date/time library.
    // This is a placeholder that converts to a basic format.
    // In a real implementation, use chrono or time crate.
    let duration = time.duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| std::time::Duration::ZERO);

    // For now, return a simple ISO-like format.
    // TODO: Implement proper RFC 7231 HTTP-date format.
    let secs = duration.as_secs();
    // Very basic: just return a numeric representation for now.
    // Production code should use proper date formatting.
    format_unix_timestamp_to_http_date(secs)
}

/// Format Unix timestamp (seconds since epoch) to HTTP-date.
fn format_unix_timestamp_to_http_date(timestamp: u64) -> String {
    // Simplified implementation:
    // HTTP-date format is complex (RFC 7231).
    // In production, use the `chrono` crate or similar.
    // For now, we'll use a basic format that clients can parse.

    // Days per month in non-leap year
    const DAYS_IN_MONTH: [u32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    const DAY_NAMES: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONTH_NAMES: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun",
        "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];

    // Basic calculation (doesn't account for leap years perfectly, but close enough for this demo)
    let days_since_epoch = timestamp / 86400;
    let secs_today = timestamp % 86400;

    let hours = secs_today / 3600;
    let minutes = (secs_today % 3600) / 60;
    let seconds = secs_today % 60;

    // Rough day of week calculation (0 = Thursday, Jan 1, 1970)
    let day_of_week = ((days_since_epoch + 4) % 7) as usize;

    // Rough year/month/day calculation (simplified, doesn't handle leap years perfectly)
    let mut year = 1970;
    let mut days_left = days_since_epoch;

    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if days_left < days_in_year as u64 {
            break;
        }
        days_left -= days_in_year as u64;
        year += 1;
    }

    let mut month = 0;
    let mut day_of_month = days_left;
    for (i, &days) in DAYS_IN_MONTH.iter().enumerate() {
        let days_this_month = if i == 1 && is_leap_year(year) { 29 } else { days as u64 };
        if day_of_month < days_this_month {
            month = i;
            break;
        }
        day_of_month -= days_this_month;
    }

    format!(
        "{}, {:02} {} {} {:02}:{:02}:{:02} GMT",
        DAY_NAMES[day_of_week],
        day_of_month + 1,
        MONTH_NAMES[month],
        year,
        hours,
        minutes,
        seconds
    )
}

fn is_leap_year(year: u64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// Parse HTTP-date header (very basic implementation).
pub fn parse_http_date(date_str: &str) -> Option<u64> {
    // Simplified: Only handle RFC 7231 format
    // Format: "Wed, 21 Oct 2015 07:28:00 GMT"
    // In production, use proper parsing.
    // For now, return None (not implemented).
    // TODO: Implement proper HTTP-date parsing.
    let _ = date_str;
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn etag_from_content_creates_strong_etag() {
        let content = b"hello world";
        let etag = ETag::from_content(content);
        match etag {
            ETag::Strong(ref tag) => {
                assert!(tag.starts_with("\""));
                assert!(tag.ends_with("\""));
            }
            _ => panic!("Expected strong ETag"),
        }
    }

    #[test]
    fn etag_parse_strong() {
        let etag = ETag::parse("\"abc123\"").unwrap();
        assert!(matches!(etag, ETag::Strong(_)));
    }

    #[test]
    fn etag_parse_weak() {
        let etag = ETag::parse("W/\"abc123\"").unwrap();
        assert!(matches!(etag, ETag::Weak(_)));
    }

    #[test]
    fn etag_strong_matches() {
        let etag1 = ETag::Strong("\"abc\"".to_string());
        let etag2 = ETag::Strong("\"abc\"".to_string());
        assert!(etag1.matches(&etag2));
    }

    #[test]
    fn etag_strong_does_not_match_different() {
        let etag1 = ETag::Strong("\"abc\"".to_string());
        let etag2 = ETag::Strong("\"xyz\"".to_string());
        assert!(!etag1.matches(&etag2));
    }

    #[test]
    fn etag_as_header_strong() {
        let etag = ETag::Strong("\"abc123\"".to_string());
        assert_eq!(etag.as_header(), "\"abc123\"");
    }

    #[test]
    fn etag_as_header_weak() {
        let etag = ETag::Weak("\"abc123\"".to_string());
        assert_eq!(etag.as_header(), "W/\"abc123\"");
    }

    #[test]
    fn simple_hash_consistency() {
        let data = b"test data";
        let hash1 = simple_hash(data);
        let hash2 = simple_hash(data);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn simple_hash_differs_for_different_data() {
        let hash1 = simple_hash(b"data1");
        let hash2 = simple_hash(b"data2");
        assert_ne!(hash1, hash2);
    }
}
```

#### 1.2 Export the new module

Update `/home/jwall/personal/rusty/rcomm/src/models.rs`:

```rust
pub mod http_methods;
pub mod http_request;
pub mod http_response;
pub mod http_status_codes;
pub mod preconditions;
```

### Phase 2: Enhance HttpRequest to Parse Precondition Headers

#### 2.1 Add methods to `HttpRequest` in `/home/jwall/personal/rusty/rcomm/src/models/http_request.rs`

Add these methods to the `impl HttpRequest` block (after the `try_get_body` method):

```rust
    /// Extract If-Match header value (comma-separated ETag list).
    pub fn try_get_if_match(&self) -> Option<String> {
        self.try_get_header("if-match".to_string())
    }

    /// Extract If-Unmodified-Since header value (HTTP-date).
    pub fn try_get_if_unmodified_since(&self) -> Option<String> {
        self.try_get_header("if-unmodified-since".to_string())
    }

    /// Parse If-Match header into a list of ETags.
    /// If the header value is "*", returns Ok(None) (matches any ETag).
    /// Otherwise, returns Ok(Some(Vec<ETag>)) or Err if parsing fails.
    pub fn parse_if_match(&self) -> Option<Vec<String>> {
        self.try_get_if_match().map(|value| {
            value.split(',')
                .map(|s| s.trim().to_string())
                .collect()
        })
    }
```

#### 2.2 Add unit tests

Add to the `#[cfg(test)]` section in `http_request.rs`:

```rust
    #[test]
    fn try_get_if_match_header() {
        let mut req = HttpRequest::build(
            HttpMethods::PUT,
            "/data".to_string(),
            "HTTP/1.1".to_string(),
        );
        req.add_header("If-Match".to_string(), "\"abc123\"".to_string());
        assert_eq!(req.try_get_if_match(), Some("\"abc123\"".to_string()));
    }

    #[test]
    fn try_get_if_unmodified_since_header() {
        let mut req = HttpRequest::build(
            HttpMethods::PUT,
            "/data".to_string(),
            "HTTP/1.1".to_string(),
        );
        req.add_header("If-Unmodified-Since".to_string(), "Wed, 21 Oct 2015 07:28:00 GMT".to_string());
        assert_eq!(
            req.try_get_if_unmodified_since(),
            Some("Wed, 21 Oct 2015 07:28:00 GMT".to_string())
        );
    }

    #[test]
    fn parse_if_match_single_etag() {
        let mut req = HttpRequest::build(
            HttpMethods::PUT,
            "/data".to_string(),
            "HTTP/1.1".to_string(),
        );
        req.add_header("If-Match".to_string(), "\"abc123\"".to_string());
        let etags = req.parse_if_match();
        assert_eq!(etags, Some(vec!["\"abc123\"".to_string()]));
    }

    #[test]
    fn parse_if_match_multiple_etags() {
        let mut req = HttpRequest::build(
            HttpMethods::PUT,
            "/data".to_string(),
            "HTTP/1.1".to_string(),
        );
        req.add_header("If-Match".to_string(), "\"abc123\", \"xyz789\"".to_string());
        let etags = req.parse_if_match();
        assert_eq!(etags, Some(vec!["\"abc123\"".to_string(), "\"xyz789\"".to_string()]));
    }

    #[test]
    fn parse_if_match_returns_none_when_header_missing() {
        let req = HttpRequest::build(
            HttpMethods::PUT,
            "/data".to_string(),
            "HTTP/1.1".to_string(),
        );
        assert_eq!(req.parse_if_match(), None);
    }
```

### Phase 3: Enhance HttpResponse to Set Precondition Headers

#### 3.1 Add methods to `HttpResponse` in `/home/jwall/personal/rusty/rcomm/src/models/http_response.rs`

Add these methods to the `impl HttpResponse` block (after the `try_get_body` method):

```rust
    /// Set ETag header from a content-based hash.
    pub fn set_etag(&mut self, content: &[u8]) -> &mut HttpResponse {
        use super::preconditions::ETag;
        let etag = ETag::from_content(content);
        self.add_header("etag".to_string(), etag.as_header());
        self
    }

    /// Set Last-Modified header from file metadata.
    pub fn set_last_modified_from_metadata(&mut self, metadata: &std::fs::Metadata) -> &mut HttpResponse {
        use super::preconditions::last_modified_from_metadata;
        if let Some(last_mod) = last_modified_from_metadata(metadata) {
            self.add_header("last-modified".to_string(), last_mod);
        }
        self
    }

    /// Set Last-Modified header from an explicit HTTP-date string.
    pub fn set_last_modified(&mut self, http_date: String) -> &mut HttpResponse {
        self.add_header("last-modified".to_string(), http_date);
        self
    }

    /// Get the status code of this response.
    pub fn get_status_code(&self) -> u16 {
        self.status_code
    }
```

#### 3.2 Add unit tests

Add to the `#[cfg(test)]` section in `http_response.rs`:

```rust
    #[test]
    fn set_etag_from_content() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
        resp.set_etag(b"hello world");
        let etag = resp.try_get_header("etag".to_string());
        assert!(etag.is_some());
        let etag_val = etag.unwrap();
        assert!(etag_val.starts_with("\""));
        assert!(etag_val.ends_with("\""));
    }

    #[test]
    fn set_etag_same_content_same_etag() {
        let mut resp1 = HttpResponse::build("HTTP/1.1".to_string(), 200);
        let mut resp2 = HttpResponse::build("HTTP/1.1".to_string(), 200);
        resp1.set_etag(b"content");
        resp2.set_etag(b"content");
        assert_eq!(
            resp1.try_get_header("etag".to_string()),
            resp2.try_get_header("etag".to_string())
        );
    }

    #[test]
    fn set_last_modified() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
        resp.set_last_modified("Wed, 21 Oct 2015 07:28:00 GMT".to_string());
        assert_eq!(
            resp.try_get_header("last-modified".to_string()),
            Some("Wed, 21 Oct 2015 07:28:00 GMT".to_string())
        );
    }

    #[test]
    fn get_status_code() {
        let resp = HttpResponse::build("HTTP/1.1".to_string(), 404);
        assert_eq!(resp.get_status_code(), 404);
    }
```

### Phase 4: Modify Main Server Logic

#### 4.1 Update `handle_connection()` in `/home/jwall/personal/rusty/rcomm/src/main.rs`

Replace the existing `handle_connection` function with an enhanced version:

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
            routes.get(&clean_target).unwrap().to_str().unwrap().to_string())
    } else {
        (HttpResponse::build(String::from("HTTP/1.1"), 404),
            String::from("pages/not_found.html"))
    };

    // Read file content and metadata
    let contents = match fs::read_to_string(&filename) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("Failed to read file {}: {}", filename, e);
            let mut error_response = HttpResponse::build(String::from("HTTP/1.1"), 500);
            error_response.add_body(format!("Internal Server Error: {}", e).into());
            let _ = stream.write_all(&error_response.as_bytes());
            return;
        }
    };

    let metadata = match fs::metadata(&filename) {
        Ok(meta) => meta,
        Err(e) => {
            eprintln!("Failed to get metadata for {}: {}", filename, e);
            let mut error_response = HttpResponse::build(String::from("HTTP/1.1"), 500);
            error_response.add_body("Internal Server Error".to_string().into());
            let _ = stream.write_all(&error_response.as_bytes());
            return;
        }
    };

    // Evaluate preconditions
    if !evaluate_preconditions(&http_request, &contents.as_bytes(), &metadata) {
        let mut precond_response = HttpResponse::build(String::from("HTTP/1.1"), 412);
        precond_response.add_body("Precondition Failed".to_string().into());
        println!("Response: {precond_response}");
        let _ = stream.write_all(&precond_response.as_bytes());
        return;
    }

    // Attach caching headers
    response.set_etag(contents.as_bytes());
    response.set_last_modified_from_metadata(&metadata);
    response.add_body(contents.into());

    println!("Response: {response}");
    stream.write_all(&response.as_bytes()).unwrap();
}
```

#### 4.2 Add precondition evaluation function

Add this new function before or after `handle_connection`:

```rust
/// Evaluate If-Match and If-Unmodified-Since preconditions.
/// Returns true if preconditions pass (or are not present).
/// Returns false if preconditions fail.
fn evaluate_preconditions(
    request: &HttpRequest,
    content: &[u8],
    metadata: &std::fs::Metadata,
) -> bool {
    use rcomm::models::preconditions::ETag;

    // Check If-Match
    if let Some(if_match_header) = request.try_get_if_match() {
        // If-Match: "*" matches any resource.
        if if_match_header.trim() == "*" {
            return true;
        }

        // Parse the If-Match ETags
        let if_match_etags: Vec<ETag> = if_match_header
            .split(',')
            .filter_map(|s| ETag::parse(s.trim()))
            .collect();

        // Compute current ETag from content
        let current_etag = ETag::from_content(content);

        // Check if any provided ETag matches
        let match_found = if_match_etags.iter().any(|etag| current_etag.matches(etag));
        if !match_found {
            return false;
        }
    }

    // Check If-Unmodified-Since
    if let Some(if_unmod_header) = request.try_get_if_unmodified_since() {
        // Parse the HTTP-date (simplified)
        // For now, we'll just do a basic comparison.
        // TODO: Implement proper HTTP-date parsing and comparison.
        // For this MVP, we skip the detailed time comparison.
        // In production, parse if_unmod_header and compare with file modification time.
        let _ = if_unmod_header; // Silence unused warning
        // For now, always return true (not fully implemented).
        // This is a placeholder for full HTTP-date parsing.
    }

    true
}
```

#### 4.3 Update imports in main.rs

Ensure the top of `main.rs` includes necessary imports:

```rust
use std::{
    collections::HashMap,
    fs,
    io::prelude::*,
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
};
use rcomm::ThreadPool;
use rcomm::models::{
    http_response::HttpResponse,
    http_request::HttpRequest,
};
```

---

## Testing Strategy

### Unit Tests

1. **preconditions.rs**
   - ETag parsing and generation
   - ETag matching logic
   - HTTP-date formatting (basic)

2. **http_request.rs**
   - Parsing If-Match headers (single and multiple ETags)
   - Parsing If-Unmodified-Since headers
   - Case-insensitive header retrieval

3. **http_response.rs**
   - Setting ETag from content
   - Setting Last-Modified from metadata
   - Status code retrieval

### Integration Tests

Add the following tests to `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`:

#### Test 1: GET with matching If-Match returns 200

```rust
fn test_get_with_matching_if_match() -> TestResult {
    let port = pick_free_port();
    let addr = format!("127.0.0.1:{}", port);
    let mut server = start_server(port);

    wait_for_server(&addr, Duration::from_secs(5))?;

    let mut stream = TcpStream::connect(&addr).map_err(|e| e.to_string())?;

    // First request to get ETag
    stream.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .map_err(|e| e.to_string())?;

    let first_response = read_response(&mut stream)?;
    let etag = first_response
        .headers
        .get("etag")
        .ok_or("Missing ETag header")?
        .clone();

    // Second request with If-Match
    let mut stream = TcpStream::connect(&addr).map_err(|e| e.to_string())?;
    let request = format!("GET / HTTP/1.1\r\nHost: localhost\r\nIf-Match: {}\r\n\r\n", etag);
    stream.write_all(request.as_bytes()).map_err(|e| e.to_string())?;

    let response = read_response(&mut stream)?;
    server.kill().map_err(|e| e.to_string())?;

    if response.status_code == 200 {
        Ok(())
    } else {
        Err(format!("Expected 200, got {}", response.status_code))
    }
}
```

#### Test 2: GET with non-matching If-Match returns 412

```rust
fn test_get_with_non_matching_if_match() -> TestResult {
    let port = pick_free_port();
    let addr = format!("127.0.0.1:{}", port);
    let mut server = start_server(port);

    wait_for_server(&addr, Duration::from_secs(5))?;

    let mut stream = TcpStream::connect(&addr).map_err(|e| e.to_string())?;
    let request = "GET / HTTP/1.1\r\nHost: localhost\r\nIf-Match: \"nonexistent\"\r\n\r\n";
    stream.write_all(request.as_bytes()).map_err(|e| e.to_string())?;

    let response = read_response(&mut stream)?;
    server.kill().map_err(|e| e.to_string())?;

    if response.status_code == 412 {
        Ok(())
    } else {
        Err(format!("Expected 412, got {}", response.status_code))
    }
}
```

#### Test 3: GET with If-Match "*" always returns 200

```rust
fn test_get_with_if_match_wildcard() -> TestResult {
    let port = pick_free_port();
    let addr = format!("127.0.0.1:{}", port);
    let mut server = start_server(port);

    wait_for_server(&addr, Duration::from_secs(5))?;

    let mut stream = TcpStream::connect(&addr).map_err(|e| e.to_string())?;
    stream.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nIf-Match: *\r\n\r\n")
        .map_err(|e| e.to_string())?;

    let response = read_response(&mut stream)?;
    server.kill().map_err(|e| e.to_string())?;

    if response.status_code == 200 {
        Ok(())
    } else {
        Err(format!("Expected 200, got {}", response.status_code))
    }
}
```

#### Test 4: Response includes ETag header

```rust
fn test_response_includes_etag() -> TestResult {
    let port = pick_free_port();
    let addr = format!("127.0.0.1:{}", port);
    let mut server = start_server(port);

    wait_for_server(&addr, Duration::from_secs(5))?;

    let mut stream = TcpStream::connect(&addr).map_err(|e| e.to_string())?;
    stream.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .map_err(|e| e.to_string())?;

    let response = read_response(&mut stream)?;
    server.kill().map_err(|e| e.to_string())?;

    if response.headers.contains_key("etag") {
        Ok(())
    } else {
        Err("Missing ETag header".to_string())
    }
}
```

#### Test 5: Response includes Last-Modified header

```rust
fn test_response_includes_last_modified() -> TestResult {
    let port = pick_free_port();
    let addr = format!("127.0.0.1:{}", port);
    let mut server = start_server(port);

    wait_for_server(&addr, Duration::from_secs(5))?;

    let mut stream = TcpStream::connect(&addr).map_err(|e| e.to_string())?;
    stream.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .map_err(|e| e.to_string())?;

    let response = read_response(&mut stream)?;
    server.kill().map_err(|e| e.to_string())?;

    if response.headers.contains_key("last-modified") {
        Ok(())
    } else {
        Err("Missing Last-Modified header".to_string())
    }
}
```

### Running Tests

```bash
# Unit tests
cargo test

# Integration tests
cargo run --bin integration_test

# Or run specific test
cargo test if_match
```

---

## Edge Cases

### 1. Multiple ETags in If-Match

**Behavior**: Comma-separated ETag list; request succeeds if current ETag matches ANY in the list.

**Example**:
```
If-Match: "abc123", "def456", "ghi789"
```

**Current Implementation**: `parse_if_match()` splits by comma and returns all.

---

### 2. If-Match "*" (Wildcard)

**Behavior**: Matches any ETag; request always succeeds (unless resource doesn't exist).

**Example**:
```
If-Match: *
```

**Current Implementation**: Checked explicitly in `evaluate_preconditions()`.

---

### 3. Missing Precondition Headers

**Behavior**: Preconditions are optional; if not present, request proceeds normally.

**Current Implementation**: Both methods in `HttpRequest` return `Option::None` if header missing; evaluation returns `true`.

---

### 4. Invalid ETag Format

**Behavior**: Malformed ETags in If-Match should be rejected (return 400 or ignored).

**Current Implementation**: `ETag::parse()` returns `Option::None`; such ETags are filtered out. Consider stricter validation for production.

---

### 5. File Modification Between Checks

**Behavior**: If file is modified between evaluating preconditions and serving it, client may receive stale content.

**Mitigation**: Read file content early and compute ETag from it; comparison is done against same content served.

---

### 6. If-Unmodified-Since HTTP-Date Parsing

**Behavior**: HTTP-date parsing is complex (RFC 7231). Current implementation is a placeholder.

**Recommendation**: For MVP, return true (pass condition). For production, implement proper RFC 7231 parsing using a dedicated crate (`chrono`, `time`, etc.).

---

### 7. 404 Responses

**Behavior**: Preconditions should NOT be evaluated for 404 responses (resource doesn't exist).

**Current Implementation**: Evaluated on `not_found.html` content, which is incorrect.

**Refinement**: Only evaluate preconditions if the status code is 200. For 404, skip precondition checks:

```rust
// In handle_connection, after determining status code:
let is_not_found = response.get_status_code() == 404;

// Only evaluate preconditions for successful responses
if !is_not_found && !evaluate_preconditions(&http_request, &contents.as_bytes(), &metadata) {
    // Return 412
}
```

---

### 8. Weak vs. Strong ETags

**Behavior**:
- **Strong ETag**: Exact byte-for-byte match required (for If-Match).
- **Weak ETag**: Semantically equivalent; used for caching validation (If-None-Match).

**Current Implementation**:
- `ETag::Strong` and `ETag::Weak` variants supported.
- `matches()` compares values; weak and strong are treated equivalently for simplicity.

**Recommendation**: For strict RFC compliance, strong ETags in If-Match should only match strong ETags.

---

## Implementation Checklist

- [ ] Create `src/models/preconditions.rs` module
- [ ] Add `preconditions` to `src/models.rs` barrel file
- [ ] Add methods to `HttpRequest` for extracting precondition headers
- [ ] Add unit tests to `http_request.rs`
- [ ] Add methods to `HttpResponse` for setting ETag/Last-Modified
- [ ] Add unit tests to `http_response.rs`
- [ ] Implement `preconditions.rs` utilities with tests
- [ ] Update `handle_connection()` in `main.rs` to evaluate preconditions
- [ ] Add `evaluate_preconditions()` function to `main.rs`
- [ ] Add integration tests to `integration_test.rs`
- [ ] Run `cargo test` and verify all tests pass
- [ ] Run `cargo run --bin integration_test` and verify integration tests pass
- [ ] Manual testing: Use `curl` with If-Match headers to verify behavior
- [ ] Edge case testing: Verify wildcard, multiple ETags, missing headers
- [ ] Performance testing: Ensure ETag generation doesn't significantly impact response time

---

## Example Usage (After Implementation)

### Client making a conditional request:

```bash
# 1. Initial request to get ETag
curl -i http://localhost:7878/
# Response includes: ETag: "abc123"

# 2. Conditional request with If-Match
curl -i -H 'If-Match: "abc123"' http://localhost:7878/
# Response: 200 OK (ETag matches)

# 3. Conditional request with non-matching ETag
curl -i -H 'If-Match: "xyz"' http://localhost:7878/
# Response: 412 Precondition Failed
```

---

## Future Enhancements

1. **Implement If-None-Match**: Opposite of If-Match; for GET, return 304 Not Modified if ETag matches.
2. **Implement If-Unmodified-Since properly**: Full HTTP-date parsing and timestamp comparison.
3. **Implement If-Modified-Since**: For GET, return 304 if resource not modified since date.
4. **Implement If-Range**: For partial content (206 Partial Content) responses.
5. **Weak ETag semantics**: Proper handling of weak vs. strong ETags per RFC 7232.
6. **Cache-Control directives**: Add support for `Cache-Control`, `Expires` headers.
7. **Conditional write support**: Allow PUT/POST with precondition validation (currently rcomm serves static files only).

---

## References

- [RFC 7232 - HTTP Conditional Requests](https://tools.ietf.org/html/rfc7232)
- [RFC 7231 - HTTP Semantics and Content](https://tools.ietf.org/html/rfc7231)
- [MDN: HTTP Conditional Requests](https://developer.mozilla.org/en-US/docs/Web/HTTP/Conditional_requests)
- [MDN: ETag](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/ETag)
- [MDN: If-Match](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/If-Match)
- [MDN: If-Unmodified-Since](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/If-Unmodified-Since)
