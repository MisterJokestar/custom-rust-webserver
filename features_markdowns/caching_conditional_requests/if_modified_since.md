# Implementation Plan: Handle `If-Modified-Since` Request Header

## Overview

Implement support for HTTP conditional request caching via the `If-Modified-Since` request header. When a client includes an `If-Modified-Since` header with a timestamp, the server should compare it against the file's last modified time. If the file hasn't changed since that timestamp, return a `304 Not Modified` response with no body, allowing the client to use a cached copy. This reduces bandwidth, improves perceived performance, and is a core HTTP caching mechanism (RFC 7232).

**Business Value**: Bandwidth savings, faster perceived load times for repeat visitors, reduced server I/O.
**Complexity**: 3/10 (straightforward file metadata + timestamp parsing + conditional response)
**Necessity**: 7/10 (fundamental caching feature, widely used by browsers)

---

## Files to Modify

1. **`src/models/http_request.rs`**
   - Add convenience method to extract `If-Modified-Since` header value
   - Parse and validate the header format (RFC 7231 HTTP-date format)

2. **`src/models/http_response.rs`**
   - Add method to set `Last-Modified` header
   - Ensure `304` responses don't include a body (per RFC 7232)

3. **`src/models/http_status_codes.rs`**
   - Already includes status code `304`, no changes needed

4. **`src/main.rs`** (handle_connection function)
   - Extract `If-Modified-Since` header from request
   - Fetch the file's metadata (last modified time)
   - Compare timestamps
   - Return `304 Not Modified` if unchanged, `200 OK` with `Last-Modified` header if changed
   - Conditionally add body based on response code

---

## Step-by-Step Implementation

### Step 1: Add HTTP-Date Parsing Utility

Create a new module `src/utils/http_date.rs` to handle RFC 7231 HTTP-date parsing.

**File**: `src/utils/http_date.rs`

```rust
use std::time::{SystemTime, UNIX_EPOCH};

/// Parse an HTTP-date string (RFC 7231 format: "Sun, 06 Nov 1994 08:49:37 GMT")
/// Returns a SystemTime if parsing succeeds, None otherwise.
pub fn parse_http_date(date_str: &str) -> Option<SystemTime> {
    // Attempt to parse RFC 1123 format: "Sun, 06 Nov 1994 08:49:37 GMT"
    // Example: "Mon, 10 Feb 2025 14:30:00 GMT"

    // Split by comma to check format
    let parts: Vec<&str> = date_str.split(",").collect();
    if parts.len() != 2 {
        return None;
    }

    let date_time_part = parts[1].trim();
    let segments: Vec<&str> = date_time_part.split_whitespace().collect();

    // Expected format: "06 Nov 1994 08:49:37 GMT"
    if segments.len() != 5 {
        return None;
    }

    let day: u32 = segments[0].parse().ok()?;
    let month_str = segments[1];
    let year: u32 = segments[2].parse().ok()?;
    let time_str = segments[3];
    let tz = segments[4];

    // Only accept GMT/UTC
    if tz != "GMT" {
        return None;
    }

    let month = match month_str {
        "Jan" => 1,
        "Feb" => 2,
        "Mar" => 3,
        "Apr" => 4,
        "May" => 5,
        "Jun" => 6,
        "Jul" => 7,
        "Aug" => 8,
        "Sep" => 9,
        "Oct" => 10,
        "Nov" => 11,
        "Dec" => 12,
        _ => return None,
    };

    let time_parts: Vec<&str> = time_str.split(":").collect();
    if time_parts.len() != 3 {
        return None;
    }

    let hour: u32 = time_parts[0].parse().ok()?;
    let minute: u32 = time_parts[1].parse().ok()?;
    let second: u32 = time_parts[2].parse().ok()?;

    // Basic validation
    if day < 1 || day > 31 || month < 1 || month > 12 || hour > 23 || minute > 59 || second > 59 {
        return None;
    }

    // Convert to Unix timestamp (simplified, doesn't account for leap years precisely)
    // For production, consider using chrono crate; for this exercise, use a simplified approach
    let days_since_epoch = days_between_1970_and_date(year, month, day)?;
    let seconds_in_day = hour as u64 * 3600 + minute as u64 * 60 + second as u64;
    let total_seconds = days_since_epoch as u64 * 86400 + seconds_in_day;

    Some(UNIX_EPOCH + std::time::Duration::from_secs(total_seconds))
}

/// Convert a file's SystemTime last-modified to an HTTP-date string.
pub fn system_time_to_http_date(st: SystemTime) -> String {
    // Simplified: extract seconds since epoch and format
    // For production, use chrono or time crate
    if let Ok(duration) = st.duration_since(UNIX_EPOCH) {
        let total_secs = duration.as_secs();
        let days = total_secs / 86400;
        let secs_in_day = total_secs % 86400;

        let hour = secs_in_day / 3600;
        let minute = (secs_in_day % 3600) / 60;
        let second = secs_in_day % 60;

        let (year, month, day) = date_from_days_since_1970(days);
        let day_of_week = day_name_from_days(days);
        let month_name = month_name(month);

        format!(
            "{}, {:02} {} {} {:02}:{:02}:{:02} GMT",
            day_of_week, day, month_name, year, hour, minute, second
        )
    } else {
        // Fallback to Unix epoch
        "Thu, 01 Jan 1970 00:00:00 GMT".to_string()
    }
}

// Helper: Calculate days since 1970-01-01 to a given date
fn days_between_1970_and_date(year: u32, month: u32, day: u32) -> Option<u32> {
    if year < 1970 {
        return None;
    }

    let mut total_days = 0u32;

    // Add days for complete years
    for y in 1970..year {
        total_days += if is_leap_year(y) { 366 } else { 365 };
    }

    // Add days for complete months in the current year
    let days_in_month = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let feb_days = if is_leap_year(year) { 29 } else { 28 };

    for m in 1..month {
        total_days += match m {
            2 => feb_days,
            _ => days_in_month[(m - 1) as usize],
        };
    }

    total_days += day - 1; // day is 1-indexed

    Some(total_days)
}

// Helper: Convert days since epoch back to (year, month, day)
fn date_from_days_since_1970(mut days: u64) -> (u32, u32, u32) {
    let mut year = 1970u32;

    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if days < days_in_year as u64 {
            break;
        }
        days -= days_in_year as u64;
        year += 1;
    }

    let days_in_month = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let feb_days = if is_leap_year(year) { 29 } else { 28 };

    let mut month = 1u32;
    for m in 1..=12 {
        let days_this_month = match m {
            2 => feb_days,
            _ => days_in_month[(m - 1) as usize],
        };
        if days < days_this_month as u64 {
            month = m;
            break;
        }
        days -= days_this_month as u64;
    }

    let day = (days + 1) as u32;

    (year, month, day)
}

// Helper: Get day of week name (0 = Thursday, 1970-01-01)
fn day_name_from_days(days: u64) -> &'static str {
    // 1970-01-01 was a Thursday
    let day_of_week = (days + 4) % 7;
    match day_of_week {
        0 => "Sun",
        1 => "Mon",
        2 => "Tue",
        3 => "Wed",
        4 => "Thu",
        5 => "Fri",
        6 => "Sat",
        _ => "Thu",
    }
}

// Helper: Get month name
fn month_name(month: u32) -> &'static str {
    match month {
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "May",
        6 => "Jun",
        7 => "Jul",
        8 => "Aug",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        12 => "Dec",
        _ => "Jan",
    }
}

// Helper: Determine if a year is a leap year
fn is_leap_year(year: u32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_http_date_valid_format() {
        let date_str = "Mon, 10 Feb 2025 14:30:00 GMT";
        let result = parse_http_date(date_str);
        assert!(result.is_some());
    }

    #[test]
    fn parse_http_date_invalid_format() {
        let result = parse_http_date("2025-02-10T14:30:00Z");
        assert!(result.is_none());
    }

    #[test]
    fn parse_http_date_invalid_timezone() {
        let result = parse_http_date("Mon, 10 Feb 2025 14:30:00 UTC");
        assert!(result.is_none());
    }

    #[test]
    fn system_time_to_http_date_produces_valid_format() {
        let st = UNIX_EPOCH + std::time::Duration::from_secs(0);
        let formatted = system_time_to_http_date(st);
        assert!(formatted.contains("GMT"));
        assert!(formatted.contains("1970"));
    }
}
```

**Alternative (Simpler) Approach**: If date parsing seems too complex, you can rely on timestamp comparison at the filesystem level without parsing. See Alternative Approach section.

### Step 2: Update `src/lib.rs` to Export Utils

Add a `pub mod utils;` declaration to expose the utility module.

**File**: `src/lib.rs` (add after `pub mod models;`)

```rust
pub mod utils;
```

### Step 3: Create `src/utils/mod.rs`

**File**: `src/utils/mod.rs`

```rust
pub mod http_date;
```

### Step 4: Add Header Extraction Methods to `HttpRequest`

Add convenience methods to extract and validate the `If-Modified-Since` header.

**File**: `src/models/http_request.rs` (add these methods to the `impl HttpRequest` block)

```rust
    /// Try to get the If-Modified-Since header value as a string
    pub fn try_get_if_modified_since(&self) -> Option<String> {
        self.try_get_header("if-modified-since".to_string())
    }
```

### Step 5: Add Header Setting Methods to `HttpResponse`

Add method to set the `Last-Modified` header. This is crucial for informing clients of the file's modification time.

**File**: `src/models/http_response.rs` (add these methods to the `impl HttpResponse` block)

```rust
    /// Set the Last-Modified header (use HTTP-date format)
    pub fn set_last_modified(&mut self, http_date: String) -> &mut HttpResponse {
        self.add_header("last-modified".to_string(), http_date);
        self
    }
```

### Step 6: Update `handle_connection` in `src/main.rs`

Modify the main request handling logic to check for `If-Modified-Since` and conditionally return `304`.

**File**: `src/main.rs` (replace the `handle_connection` function)

```rust
use std::fs::metadata;
use rcomm::utils::http_date::{parse_http_date, system_time_to_http_date};

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

    // Get file metadata to fetch last modified time
    let file_metadata = match metadata(filename) {
        Ok(meta) => Some(meta),
        Err(_) => None,
    };

    // If file exists, set Last-Modified header and check If-Modified-Since
    if let Some(meta) = &file_metadata {
        if let Ok(modified_time) = meta.modified() {
            let last_modified_http_date = system_time_to_http_date(modified_time);
            response.set_last_modified(last_modified_http_date.clone());

            // Check If-Modified-Since header
            if let Some(if_modified_since_str) = http_request.try_get_if_modified_since() {
                if let Some(if_modified_since_time) = parse_http_date(&if_modified_since_str) {
                    // If file hasn't changed, return 304
                    if modified_time <= if_modified_since_time {
                        response = HttpResponse::build(String::from("HTTP/1.1"), 304);
                        response.set_last_modified(last_modified_http_date);
                        // No body for 304 responses
                        println!("Response: {response}");
                        stream.write_all(&response.as_bytes()).unwrap();
                        return;
                    }
                }
            }
        }
    }

    // File was modified or no If-Modified-Since header; send full response
    let contents = fs::read_to_string(filename).unwrap();
    response.add_body(contents.into());

    println!("Response: {response}");
    stream.write_all(&response.as_bytes()).unwrap();
}
```

---

## Alternative Approach: Simplified Timestamp Comparison

If HTTP-date parsing is too complex, you can simplify by comparing raw `SystemTime` values. This avoids string parsing entirely:

**Simplified Step 2 Alternative** (skip http_date parsing module):

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

    // Get file metadata
    if let Ok(meta) = metadata(filename) {
        if let Ok(modified_time) = meta.modified() {
            // Format last-modified for HTTP header
            let http_date_str = format_system_time(modified_time);
            response.set_last_modified(http_date_str.clone());

            // Check If-Modified-Since (as raw seconds comparison)
            if let Some(if_modified_since_str) = http_request.try_get_if_modified_since() {
                // For simplicity, convert both times to Unix timestamps and compare
                if let (Ok(mod_dur), Some(client_dur)) = (
                    modified_time.duration_since(std::time::UNIX_EPOCH),
                    parse_http_date_to_duration(&if_modified_since_str),
                ) {
                    if mod_dur <= client_dur {
                        // File hasn't changed
                        let mut not_modified = HttpResponse::build(String::from("HTTP/1.1"), 304);
                        not_modified.set_last_modified(http_date_str);
                        println!("Response: {not_modified}");
                        stream.write_all(&not_modified.as_bytes()).unwrap();
                        return;
                    }
                }
            }
        }
    }

    // Send full response with body
    let contents = fs::read_to_string(filename).unwrap();
    response.add_body(contents.into());

    println!("Response: {response}");
    stream.write_all(&response.as_bytes()).unwrap();
}

// Helper: Simple HTTP-date formatter (reverse of parser)
fn format_system_time(st: std::time::SystemTime) -> String {
    // Stub implementation; for production use chrono crate
    // This is a placeholder that would need proper date arithmetic
    "Mon, 10 Feb 2025 14:30:00 GMT".to_string()
}

// Helper: Parse HTTP-date to Duration since epoch
fn parse_http_date_to_duration(date_str: &str) -> Option<std::time::Duration> {
    // Stub: would need proper parsing logic
    None
}
```

---

## Code Snippets Summary

### Key Changes in `http_request.rs`:

```rust
pub fn try_get_if_modified_since(&self) -> Option<String> {
    self.try_get_header("if-modified-since".to_string())
}
```

### Key Changes in `http_response.rs`:

```rust
pub fn set_last_modified(&mut self, http_date: String) -> &mut HttpResponse {
    self.add_header("last-modified".to_string(), http_date);
    self
}
```

### Key Changes in `main.rs`:

```rust
use rcomm::utils::http_date::{parse_http_date, system_time_to_http_date};
use std::fs::metadata;

// Inside handle_connection:
if let Some(meta) = &file_metadata {
    if let Ok(modified_time) = meta.modified() {
        let http_date = system_time_to_http_date(modified_time);
        response.set_last_modified(http_date.clone());

        if let Some(if_modified_since) = http_request.try_get_if_modified_since() {
            if let Some(client_time) = parse_http_date(&if_modified_since) {
                if modified_time <= client_time {
                    response = HttpResponse::build(String::from("HTTP/1.1"), 304);
                    response.set_last_modified(http_date);
                    stream.write_all(&response.as_bytes()).unwrap();
                    return;
                }
            }
        }
    }
}
```

---

## Testing Strategy

### Unit Tests

#### 1. HTTP-Date Parsing Tests (`src/utils/http_date.rs`)

```rust
#[test]
fn test_parse_valid_http_date() {
    let result = parse_http_date("Sun, 06 Nov 1994 08:49:37 GMT");
    assert!(result.is_some());
}

#[test]
fn test_parse_invalid_format_iso8601() {
    let result = parse_http_date("1994-11-06T08:49:37Z");
    assert!(result.is_none());
}

#[test]
fn test_parse_invalid_timezone() {
    let result = parse_http_date("Sun, 06 Nov 1994 08:49:37 UTC");
    assert!(result.is_none());
}

#[test]
fn test_parse_missing_day_of_week() {
    let result = parse_http_date("06 Nov 1994 08:49:37 GMT");
    assert!(result.is_none());
}

#[test]
fn test_system_time_to_http_date_epoch() {
    let st = UNIX_EPOCH;
    let formatted = system_time_to_http_date(st);
    assert_eq!(formatted, "Thu, 01 Jan 1970 00:00:00 GMT");
}

#[test]
fn test_system_time_to_http_date_recent() {
    // Test with a known timestamp (e.g., 2025-02-10 14:30:00)
    let st = UNIX_EPOCH + std::time::Duration::from_secs(1739190600);
    let formatted = system_time_to_http_date(st);
    assert!(formatted.contains("2025"));
    assert!(formatted.contains("Feb"));
}

#[test]
fn test_roundtrip_parse_and_format() {
    let original = "Mon, 10 Feb 2025 14:30:00 GMT";
    if let Some(parsed) = parse_http_date(original) {
        let formatted = system_time_to_http_date(parsed);
        // Should be able to parse the formatted version
        assert!(parse_http_date(&formatted).is_some());
    }
}
```

#### 2. HttpRequest Header Extraction Tests (add to `src/models/http_request.rs`)

```rust
#[test]
fn try_get_if_modified_since_returns_value_when_present() {
    let mut req = HttpRequest::build(
        HttpMethods::GET,
        "/".to_string(),
        "HTTP/1.1".to_string(),
    );
    req.add_header("If-Modified-Since".to_string(), "Mon, 10 Feb 2025 14:30:00 GMT".to_string());
    let result = req.try_get_if_modified_since();
    assert_eq!(result, Some("Mon, 10 Feb 2025 14:30:00 GMT".to_string()));
}

#[test]
fn try_get_if_modified_since_returns_none_when_missing() {
    let req = HttpRequest::build(
        HttpMethods::GET,
        "/".to_string(),
        "HTTP/1.1".to_string(),
    );
    assert_eq!(req.try_get_if_modified_since(), None);
}
```

#### 3. HttpResponse Last-Modified Tests (add to `src/models/http_response.rs`)

```rust
#[test]
fn set_last_modified_adds_header() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
    resp.set_last_modified("Mon, 10 Feb 2025 14:30:00 GMT".to_string());
    let value = resp.try_get_header("last-modified".to_string());
    assert_eq!(value, Some("Mon, 10 Feb 2025 14:30:00 GMT".to_string()));
}

#[test]
fn set_last_modified_supports_chaining() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 304);
    resp.set_last_modified("Mon, 10 Feb 2025 14:30:00 GMT".to_string())
        .add_header("Cache-Control".to_string(), "max-age=3600".to_string());
    let output = format!("{resp}");
    assert!(output.contains("last-modified: Mon, 10 Feb 2025 14:30:00 GMT"));
    assert!(output.contains("cache-control: max-age=3600"));
}

#[test]
fn three_oh_four_response_has_no_body() {
    let resp = HttpResponse::build("HTTP/1.1".to_string(), 304);
    assert_eq!(resp.try_get_body(), None);
}
```

### Integration Tests

Add tests to `src/bin/integration_test.rs` to verify end-to-end behavior:

```rust
fn test_if_modified_since_returns_304() -> TestResult {
    // Start server
    // Send request without If-Modified-Since, get 200 + Last-Modified header
    // Extract Last-Modified value
    // Send second request with If-Modified-Since = Last-Modified
    // Verify response is 304 with no body
    // Verify Last-Modified header is still present
}

fn test_if_modified_since_returns_200_when_file_newer() -> TestResult {
    // Send request with If-Modified-Since timestamp older than current file
    // Verify response is 200 with full body
    // Verify Last-Modified header is present
}

fn test_304_response_omits_body() -> TestResult {
    // Verify Content-Length is not set (or is 0) for 304 responses
    // Verify response body is empty
}

fn test_last_modified_header_format() -> TestResult {
    // Fetch a file and verify Last-Modified header follows RFC 7231 format
    // "Day, DD Mon YYYY HH:MM:SS GMT"
}

fn test_invalid_if_modified_since_format_ignored() -> TestResult {
    // Send request with malformed If-Modified-Since (e.g., ISO 8601)
    // Server should ignore it and return 200 with full body
}
```

### Manual Testing

```bash
# Test 1: Get file and capture Last-Modified
curl -i http://localhost:7878/

# Response should include:
# Last-Modified: Mon, 10 Feb 2025 14:30:00 GMT

# Test 2: Request with If-Modified-Since matching Last-Modified
curl -i -H "If-Modified-Since: Mon, 10 Feb 2025 14:30:00 GMT" http://localhost:7878/

# Response should be:
# HTTP/1.1 304 Not Modified
# Last-Modified: Mon, 10 Feb 2025 14:30:00 GMT
# (no body)

# Test 3: Request with If-Modified-Since older than Last-Modified
curl -i -H "If-Modified-Since: Sun, 09 Feb 2025 00:00:00 GMT" http://localhost:7878/

# Response should be:
# HTTP/1.1 200 OK
# Last-Modified: Mon, 10 Feb 2025 14:30:00 GMT
# Content-Length: ...
# (full body included)
```

---

## Edge Cases & Considerations

### 1. **Timestamp Precision**
   - Filesystem timestamps may have sub-second precision, but HTTP-date uses second precision.
   - Solution: Use `<=` comparison (not `<`) to treat equal timestamps as "not modified". This is safe because it errs on the side of caching.

### 2. **File Not Found (404 Responses)**
   - `If-Modified-Since` should only apply to successful (2xx) responses, not 404s.
   - Solution: Only set `Last-Modified` header when the file actually exists. For 404 responses, skip the conditional check entirely.

### 3. **Invalid Header Values**
   - Clients may send malformed `If-Modified-Since` headers.
   - Solution: If parsing fails, treat it as a cache miss and return the full response (200). This is graceful degradation.

### 4. **Timezone Handling**
   - HTTP dates must be in GMT/UTC only (RFC 7231).
   - Solution: Enforce GMT conversion; reject other timezones.

### 5. **Leap Seconds**
   - HTTP doesn't handle leap seconds (POSIX time ignores them).
   - Solution: No special handling needed; UNIX timestamps naturally skip leap seconds.

### 6. **304 Must Not Include Body**
   - Clients may close the connection if a 304 contains a body (per HTTP spec).
   - Solution: Never call `add_body()` on a 304 response. Set `Last-Modified` but leave body as `None`.

### 7. **304 May Include Other Headers**
   - `Cache-Control`, `ETag`, `Expires`, etc., can be included in 304 responses.
   - Solution: Current implementation supports arbitrary headers; just don't add `Content-Length` unless a body exists.

### 8. **Conditional Request Methods**
   - Only GET and HEAD requests typically include `If-Modified-Since`.
   - Solution: The implementation works for any method, but practical client usage is limited to GET/HEAD.

### 9. **If-Modified-Since vs. ETag**
   - This feature uses weak comparison (timestamp-based). ETag uses strong comparison (content-based).
   - Solution: Both can coexist; ETag support is a separate feature.

### 10. **Symlinks & Hard Links**
    - File metadata may vary for symlinks.
    - Solution: Use `metadata()` (follows symlinks) not `symlink_metadata()`. No special handling needed.

---

## Implementation Checklist

- [ ] Create `src/utils/mod.rs` with module declaration
- [ ] Implement `src/utils/http_date.rs` with parsing and formatting logic
- [ ] Add `pub mod utils;` to `src/lib.rs`
- [ ] Add `try_get_if_modified_since()` method to `HttpRequest`
- [ ] Add `set_last_modified()` method to `HttpResponse`
- [ ] Update `handle_connection()` in `src/main.rs` to check for conditional requests
- [ ] Add unit tests for HTTP-date parsing in `http_date.rs`
- [ ] Add unit tests for header methods in `http_request.rs` and `http_response.rs`
- [ ] Add integration tests in `src/bin/integration_test.rs`
- [ ] Manual test with `curl` to verify end-to-end behavior
- [ ] Run full test suite: `cargo test`
- [ ] Verify no regressions in existing tests

---

## Performance & Impact

**Performance**: Minimal impact. HTTP-date parsing is only performed when the `If-Modified-Since` header is present. File metadata lookup (`fs::metadata()`) is cheap. Timestamp comparison is O(1). Expected overhead: <1ms per request.

**Bandwidth Savings**: Significant for repeat visitors. A typical HTML file (~50 KB) avoids transmission, saving bandwidth. Over 1 million requests with 30% cache hits, ~15 GB bandwidth saved.

**Browser Support**: All modern browsers (Chrome, Firefox, Safari, Edge) support `If-Modified-Since`. Fallback: Browsers that don't send the header still receive full responses (200 OK), maintaining backward compatibility.

---

## Rollout & Debugging

### Logging
Add debug output to trace conditional request handling:

```rust
if let Some(if_modified_since_str) = http_request.try_get_if_modified_since() {
    println!("If-Modified-Since: {}", if_modified_since_str);
    if let Some(client_time) = parse_http_date(&if_modified_since_str) {
        if let Ok(modified_time) = meta.modified() {
            println!("File modified at: {:?}, client cached at: {:?}", modified_time, client_time);
            if modified_time <= client_time {
                println!("Returning 304 Not Modified");
            }
        }
    }
}
```

### Monitoring
- Count 304 responses to measure cache effectiveness
- Monitor CPU cost of date parsing (negligible but trackable)
- Verify no regression in average response time

---

## Future Enhancements

1. **ETag Support** (weak or strong): Content-based caching for dynamic content.
2. **Expires & Cache-Control Headers**: More flexible caching directives.
3. **Conditional GET with Range Requests**: If-Modified-Since + Range for partial updates.
4. **Async File I/O**: Parallel metadata lookups for multiple files.
5. **Caching Headers Customization**: Per-route configuration for cache behavior.

---

## References

- RFC 7232 (HTTP Conditional Requests): https://tools.ietf.org/html/rfc7232
- RFC 7231 (HTTP Semantics - Date/Time): https://tools.ietf.org/html/rfc7231#section-7.1.1
- MDN - HTTP If-Modified-Since: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/If-Modified-Since
- MDN - HTTP Last-Modified: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Last-Modified
