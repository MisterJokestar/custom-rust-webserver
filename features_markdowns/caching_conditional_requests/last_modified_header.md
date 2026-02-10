# Implementation Plan: Send `Last-Modified` Header

**Feature**: Add `Last-Modified` header to static file HTTP responses
**Category**: Caching & Conditional Requests
**Complexity**: 3/10
**Necessity**: 7/10
**Status**: Implementation Plan

---

## Overview

The server will extract the file modification timestamp from the filesystem and include it in the `Last-Modified` HTTP response header (RFC 7231 format: `Wed, 21 Oct 2015 07:28:00 GMT`). This header enables HTTP caching mechanisms and allows clients to perform conditional requests using `If-Modified-Since`.

### Benefits

- Enables browser caching behavior based on file timestamps
- Supports `If-Modified-Since` conditional requests (foundation for 304 Not Modified responses)
- Improves perceived performance for repeated client requests
- Follows HTTP/1.1 specifications (RFC 7231)
- Minimal performance overhead (filesystem metadata is already queried)

### HTTP Spec Compliance

- **RFC 7231 Section 7.2**: Defines `Last-Modified` as an optional response header
- **Format**: HTTP-date (e.g., `Wed, 21 Oct 2015 07:28:00 GMT`)
- **Scope**: Should be included on successful (2xx) responses for static files
- **Omit for**: 404 responses, error pages (no real file timestamp)

---

## Files to Modify

### 1. `src/main.rs`
**Changes**: Extract file metadata and pass modification time to response builder.

**Current flow**:
```
handle_connection()
  ├─ Parse HTTP request
  ├─ Look up route in HashMap<String, PathBuf>
  ├─ Read file contents with fs::read_to_string()
  └─ Send response
```

**New flow**:
```
handle_connection()
  ├─ Parse HTTP request
  ├─ Look up route in HashMap<String, PathBuf>
  ├─ Get file metadata (fs::metadata())
  ├─ Extract modified timestamp and format as HTTP-date
  ├─ Read file contents
  ├─ Add Last-Modified header to response
  └─ Send response
```

### 2. `src/models/http_response.rs`
**Changes**: Add method to set `Last-Modified` header with validation/formatting.

**Rationale**: Centralizes HTTP-date formatting logic and ensures consistency.

### 3. `src/models/http_request.rs` (Optional)
**No changes needed** for initial implementation, but future `If-Modified-Since` support would parse this header here.

---

## Step-by-Step Implementation

### Step 1: Add Helper Function for HTTP-Date Formatting

**File**: `src/main.rs`

Add a utility function to convert a `SystemTime` to RFC 7231 HTTP-date format:

```rust
use std::time::{SystemTime, UNIX_EPOCH};

/// Converts SystemTime to RFC 7231 HTTP-date format
/// Example: "Wed, 21 Oct 2015 07:28:00 GMT"
fn format_http_date(system_time: SystemTime) -> String {
    match system_time.duration_since(UNIX_EPOCH) {
        Ok(duration) => {
            // Convert to seconds since epoch
            let secs = duration.as_secs();

            // Simplified: Calculate days since epoch (Jan 1, 1970)
            // This requires a date calculation algorithm
            let days_since_epoch = secs / 86400; // 86400 = seconds per day

            // Use a helper or lookup to get year/month/day
            let (year, month, day, hour, min, sec) =
                seconds_to_date_time(secs);

            // Month abbreviation lookup
            let months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun",
                         "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
            let weekdays = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];

            // Calculate day of week (0 = Thu, Jan 1, 1970)
            let day_of_week = (days_since_epoch + 4) % 7;

            format!("{}, {:02} {} {} {:02}:{:02}:{:02} GMT",
                weekdays[day_of_week as usize],
                day, months[month - 1], year,
                hour, min, sec)
        }
        Err(_) => String::from("Thu, 01 Jan 1970 00:00:00 GMT"), // Fallback
    }
}

/// Helper function to convert Unix timestamp to calendar date/time
/// Returns (year, month, day, hour, minute, second)
fn seconds_to_date_time(mut secs: u64) -> (u32, u32, u32, u32, u32, u32) {
    const DAYS_PER_YEAR: u64 = 365;
    const DAYS_PER_4_YEARS: u64 = 1461; // 365*4 + 1 (leap day)

    let sec_in_day = secs % 86400;
    let hour = sec_in_day / 3600;
    let min = (sec_in_day % 3600) / 60;
    let sec = sec_in_day % 60;

    let mut days = secs / 86400;

    // Calculate year (simplified, assumes 400-year cycle repeats)
    let mut year = 1970;
    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if days >= days_in_year {
            days -= days_in_year;
            year += 1;
        } else {
            break;
        }
    }

    // Calculate month and day
    let days_in_months = if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month = 1;
    let mut day = days as u32 + 1;
    for &days_in_month in &days_in_months {
        if day > days_in_month {
            day -= days_in_month;
            month += 1;
        } else {
            break;
        }
    }

    (year as u32, month, day, hour as u32, min as u32, sec as u32)
}

fn is_leap_year(year: u64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}
```

**Note**: The date calculation is complex. A production implementation should consider:
- Using a lightweight date library (though it violates the "no external dependencies" constraint)
- Implementing a complete, tested date calculation algorithm
- For this feature, the above logic is simplified and **must be thoroughly tested**

### Step 2: Modify `handle_connection()` Function

**File**: `src/main.rs`

Update the `handle_connection()` function to extract and use file metadata:

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

    // NEW: Extract file metadata for 200 responses
    if response.get_status_code() == 200 {
        if let Ok(metadata) = fs::metadata(filename) {
            if let Ok(modified) = metadata.modified() {
                let last_modified = format_http_date(modified);
                response.add_last_modified(last_modified);
            }
        }
    }

    let contents = fs::read_to_string(filename).unwrap();
    response.add_body(contents.into());

    println!("Response: {response}");
    stream.write_all(&response.as_bytes()).unwrap();
}
```

**Key points**:
- Only add the header for 200 responses (real files)
- Skip for 404 responses (not_found.html may have a timestamp, but logically it's an error)
- Gracefully handle `metadata()` failures with `.unwrap_or()` pattern in production code

### Step 3: Add Method to `HttpResponse` Struct

**File**: `src/models/http_response.rs`

Add a dedicated method for setting the `Last-Modified` header:

```rust
impl HttpResponse {
    // ... existing methods ...

    pub fn add_last_modified(&mut self, last_modified: String) -> &mut HttpResponse {
        self.add_header("Last-Modified".to_string(), last_modified)
    }

    // NEW: Add public getter to retrieve status code (used in handle_connection)
    pub fn get_status_code(&self) -> u16 {
        self.status_code
    }
}
```

**Rationale**:
- `add_last_modified()` is a semantic method that clarifies intent
- `get_status_code()` allows checking the response code before adding headers
- Both methods follow the existing builder pattern

---

## Code Snippets Summary

### Updated Imports in `src/main.rs`
```rust
use std::{
    collections::HashMap,
    fs,
    io::prelude::*,
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},  // NEW
};
```

### Key Function Calls
```rust
// In handle_connection():
if response.get_status_code() == 200 {
    if let Ok(metadata) = fs::metadata(filename) {
        if let Ok(modified) = metadata.modified() {
            response.add_last_modified(format_http_date(modified));
        }
    }
}

// Response will now include:
// Last-Modified: Wed, 21 Oct 2015 07:28:00 GMT
```

---

## Testing Strategy

### Unit Tests

#### 1. Test HTTP-Date Formatting
**File**: `src/main.rs` (add to existing test module)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, Duration};

    #[test]
    fn format_http_date_handles_epoch() {
        let epoch = UNIX_EPOCH;
        let formatted = format_http_date(epoch);
        assert_eq!(formatted, "Thu, 01 Jan 1970 00:00:00 GMT");
    }

    #[test]
    fn format_http_date_handles_known_timestamp() {
        // Test with Oct 21, 2015, 7:28 AM
        let known_time = UNIX_EPOCH + Duration::from_secs(1445412480);
        let formatted = format_http_date(known_time);
        assert_eq!(formatted, "Wed, 21 Oct 2015 07:28:00 GMT");
    }

    #[test]
    fn format_http_date_handles_leap_years() {
        // Feb 29, 2020 (leap year)
        let leap_year = UNIX_EPOCH + Duration::from_secs(1582934400);
        let formatted = format_http_date(leap_year);
        assert!(formatted.contains("29") && formatted.contains("Feb") && formatted.contains("2020"));
    }

    #[test]
    fn is_leap_year_calculation() {
        assert!(is_leap_year(2020));  // Divisible by 400
        assert!(is_leap_year(2024));  // Divisible by 4, not 100
        assert!(!is_leap_year(2021)); // Not divisible by 4
        assert!(!is_leap_year(2100)); // Divisible by 100 but not 400
    }
}
```

#### 2. Test `HttpResponse.add_last_modified()`
**File**: `src/models/http_response.rs` (add to existing test module)

```rust
#[test]
fn add_last_modified_stores_header() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
    resp.add_last_modified("Wed, 21 Oct 2015 07:28:00 GMT".to_string());
    let val = resp.try_get_header("Last-Modified".to_string());
    assert_eq!(val, Some("Wed, 21 Oct 2015 07:28:00 GMT".to_string()));
}

#[test]
fn add_last_modified_returns_self_for_chaining() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
    let result = resp.add_last_modified("Wed, 21 Oct 2015 07:28:00 GMT".to_string());
    assert!(std::ptr::eq(result as *const _, &resp as *const _));
}

#[test]
fn last_modified_header_appears_in_response() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
    resp.add_last_modified("Wed, 21 Oct 2015 07:28:00 GMT".to_string());
    let output = format!("{resp}");
    assert!(output.contains("last-modified: Wed, 21 Oct 2015 07:28:00 GMT\r\n"));
}

#[test]
fn get_status_code_returns_correct_code() {
    let resp = HttpResponse::build("HTTP/1.1".to_string(), 404);
    assert_eq!(resp.get_status_code(), 404);
}
```

### Integration Tests

**File**: `src/bin/integration_test.rs` (or new test function)

```rust
#[test]
fn last_modified_header_sent_for_static_files() {
    // 1. Create a temporary test file with known modification time
    let test_file = "pages/test_file.txt";
    fs::write(test_file, "test content").unwrap();

    // Set modification time to a specific value (if possible)
    // fs::set_file_times() or std::filetime crate (external dependency issue)

    // 2. Start server on random port
    let server = TestServer::spawn(7080);

    // 3. Request the file
    let response = http_get("127.0.0.1:7080", "/test_file.txt");

    // 4. Verify Last-Modified header is present
    assert!(response.headers.contains_key("last-modified"),
            "Last-Modified header missing from response");

    // 5. Verify header format matches RFC 7231
    let last_mod = response.headers.get("last-modified").unwrap();
    assert!(is_valid_http_date(last_mod),
            "Last-Modified header format invalid: {}", last_mod);

    // Cleanup
    fs::remove_file(test_file).unwrap();
}

#[test]
fn last_modified_header_not_sent_for_404() {
    let server = TestServer::spawn(7081);

    let response = http_get("127.0.0.1:7081", "/nonexistent");

    assert_eq!(response.status_code, 404);
    assert!(!response.headers.contains_key("last-modified"),
            "Last-Modified header should not appear in 404 responses");
}

fn is_valid_http_date(date: &str) -> bool {
    // Validate against pattern: "Ddd, DD Mmm YYYY HH:MM:SS GMT"
    let parts: Vec<&str> = date.split_whitespace().collect();
    parts.len() == 5 && date.ends_with("GMT")
}
```

### Manual Testing

1. **Start server**:
   ```bash
   cargo run
   ```

2. **Request a static file and inspect headers**:
   ```bash
   curl -i http://127.0.0.1:7878/
   ```
   Expected output:
   ```
   HTTP/1.1 200 OK
   content-length: 1234
   last-modified: Wed, 21 Oct 2015 07:28:00 GMT

   [file content]
   ```

3. **Verify header format**:
   ```bash
   curl -s -i http://127.0.0.1:7878/ | grep -i "last-modified"
   ```

4. **Test with different file types**:
   ```bash
   curl -i http://127.0.0.1:7878/styles.css
   curl -i http://127.0.0.1:7878/script.js
   curl -i http://127.0.0.1:7878/nonexistent  # Should have no Last-Modified
   ```

---

## Edge Cases

### 1. **File Modification Time Extraction Fails**
**Scenario**: `fs::metadata()` returns an error (permission denied, symlink issues, etc.)

**Solution**:
```rust
if let Ok(metadata) = fs::metadata(filename) {
    if let Ok(modified) = metadata.modified() {
        response.add_last_modified(format_http_date(modified));
    }
    // Silently skip if modified() fails
}
// Response is valid without Last-Modified header
```

**Test**:
```rust
#[test]
fn handles_missing_file_metadata_gracefully() {
    // This test requires careful setup to trigger metadata() error
    // Skip if running in restricted environment
}
```

### 2. **SystemTime Precision Loss**
**Scenario**: File timestamp has sub-second precision; HTTP-date only has second precision.

**Solution**: The algorithm truncates to seconds via `duration.as_secs()`. This is correct per RFC 7231, which specifies second-precision for HTTP-date.

**Test**:
```rust
#[test]
fn http_date_truncates_sub_second_precision() {
    let time = UNIX_EPOCH + Duration::from_millis(1445412480123);
    let formatted = format_http_date(time);
    // Should format to nearest second, no milliseconds
    assert_eq!(formatted, "Wed, 21 Oct 2015 07:28:00 GMT");
}
```

### 3. **Dates Before Unix Epoch (Jan 1, 1970)**
**Scenario**: File has modification time before 1970 (very rare, but possible on some systems).

**Solution**: Provide a fallback to Unix epoch:
```rust
match system_time.duration_since(UNIX_EPOCH) {
    Ok(duration) => { /* normal path */ },
    Err(_) => "Thu, 01 Jan 1970 00:00:00 GMT".to_string(), // Fallback
}
```

**Impact**: Minimal; such files are extremely rare in practice.

**Test**:
```rust
#[test]
fn handles_time_before_epoch_with_fallback() {
    let before_epoch = SystemTime::UNIX_EPOCH - Duration::from_secs(3600);
    let formatted = format_http_date(before_epoch);
    assert_eq!(formatted, "Thu, 01 Jan 1970 00:00:00 GMT");
}
```

### 4. **Timezone Handling**
**Scenario**: File timestamp is in local timezone; RFC 7231 requires GMT/UTC.

**Solution**: `SystemTime` is always UTC-based. The conversion is correct.

**Validation**: No special handling needed; this is automatic.

### 5. **404 Responses (not_found.html)**
**Scenario**: The not_found.html file exists on disk and has a modification timestamp, but the response is 404.

**Solution**: The code only adds `Last-Modified` when `status_code == 200`:
```rust
if response.get_status_code() == 200 {
    // Add header
}
```

**Rationale**: 404 responses represent "resource not found," not a real file being served.

**Test**:
```rust
#[test]
fn last_modified_omitted_for_404() {
    // Request nonexistent route
    let response = fetch("http://127.0.0.1:7878/nope");
    assert_eq!(response.status, 404);
    assert!(!response.headers.contains_key("last-modified"));
}
```

### 6. **Symlinks and Hard Links**
**Scenario**: File is a symlink or hard link to another file.

**Solution**: `fs::metadata()` follows symlinks by default and returns the target file's metadata. This is the correct behavior.

**Alternative**: If you wanted the symlink's own timestamp, use `fs::symlink_metadata()`, but this is not necessary for this feature.

### 7. **Concurrent File Modifications**
**Scenario**: File is modified between `metadata()` call and `read_to_string()`.

**Solution**: The timestamps may become stale, but this is acceptable. The header reflects the timestamp at the moment the metadata was read, not when the content was read. This is consistent with most web servers.

**Test**: Not easily testable without race condition setup.

### 8. **Very Large Timestamps (Year > 9999)**
**Scenario**: Hypothetical Unix timestamp far in the future.

**Solution**: The algorithm may overflow. For practical purposes (current era + next 7000 years), this is not a concern.

**Test**:
```rust
#[test]
fn handles_year_2100() {
    // Unix timestamp for Jan 1, 2100
    let year_2100 = UNIX_EPOCH + Duration::from_secs(4102444800);
    let formatted = format_http_date(year_2100);
    assert!(formatted.contains("2100"));
}
```

---

## Implementation Checklist

- [ ] Add date/time calculation helper functions to `src/main.rs`
- [ ] Add `format_http_date()` function with proper RFC 7231 formatting
- [ ] Add leap year logic and calendar calculations
- [ ] Update `handle_connection()` to extract file metadata
- [ ] Add `get_status_code()` method to `HttpResponse`
- [ ] Add `add_last_modified()` method to `HttpResponse`
- [ ] Update imports in `src/main.rs` to include `SystemTime` and `UNIX_EPOCH`
- [ ] Write unit tests for date formatting functions
- [ ] Write unit tests for `HttpResponse` methods
- [ ] Write integration tests for 200 and 404 responses
- [ ] Run full test suite: `cargo test`
- [ ] Run integration tests: `cargo run --bin integration_test`
- [ ] Manual test with `curl -i http://127.0.0.1:7878/`
- [ ] Verify all edge cases are handled
- [ ] Review code for `.unwrap()` safety and error handling

---

## Potential Future Enhancements

1. **`If-Modified-Since` Support** (Conditional Requests)
   - Parse `If-Modified-Since` header in `HttpRequest`
   - Compare with file's `Last-Modified`
   - Return 304 Not Modified if file hasn't changed
   - Reduces bandwidth for cached resources

2. **`ETag` Support**
   - Calculate file hash or use inode+mtime as ETag
   - Provide strong/weak ETag validation
   - Support `If-None-Match` requests

3. **Cache Control Headers**
   - Add `Cache-Control` header with appropriate directives
   - `max-age`, `public`, `private`, `no-cache`, etc.
   - Coordinate with `Last-Modified` for effective caching

4. **Conditional Range Requests**
   - Combine with `Range` header and 206 Partial Content
   - Use `Last-Modified` for cache validation in range requests

5. **Performance Optimization**
   - Cache file metadata (timestamp, size) to avoid repeated `fs::metadata()` calls
   - Use file mmap for large files
   - Implement async I/O with tokio (requires external dependency)

---

## Risk Assessment

### Low Risk

- **Date Calculation**: Thoroughly testable algorithm; any bugs are caught immediately.
- **Header Addition**: Simple string insertion into existing response builder.
- **Backward Compatibility**: Adding headers is non-breaking; clients can ignore unknown headers.

### Medium Risk

- **Leap Year Logic**: Requires careful implementation; off-by-one errors are possible.
- **Timezone Bugs**: UTC conversion must be correct; any error affects all responses.

### Mitigation

- Write comprehensive unit tests covering:
  - Known historical dates
  - Leap year transitions
  - Epoch boundaries
- Use established date libraries if dependency constraint is relaxed
- Review RFC 7231 carefully for exact format requirements

---

## References

- **RFC 7231**: HTTP/1.1 Semantics and Content
  - [Section 7.2: Last-Modified](https://tools.ietf.org/html/rfc7231#section-7.2)
  - [Section 7.1.1: HTTP-date](https://tools.ietf.org/html/rfc7231#section-7.1.1)

- **Rust Documentation**:
  - [std::fs::metadata()](https://doc.rust-lang.org/std/fs/fn.metadata.html)
  - [std::fs::Metadata::modified()](https://doc.rust-lang.org/std/fs/struct.Metadata.html#method.modified)
  - [std::time::SystemTime](https://doc.rust-lang.org/std/time/struct.SystemTime.html)

- **HTTP Caching**:
  - [MDN: HTTP Caching](https://developer.mozilla.org/en-US/docs/Web/HTTP/Caching)
  - [MDN: Last-Modified](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Last-Modified)

---

## Summary

This feature adds HTTP caching support by including the `Last-Modified` header with each static file response. The implementation is straightforward:

1. Extract file modification time using `fs::metadata()`
2. Convert to RFC 7231 HTTP-date format
3. Add header to 200 responses only
4. Omit from error responses

The main challenge is implementing a correct date formatting algorithm without external dependencies. The provided implementation handles leap years, timezone conversion, and edge cases. Comprehensive testing ensures reliability.

**Estimated Effort**: 4-6 hours (including testing and edge case handling)
**Testing Coverage**: Unit tests (8-10), Integration tests (2-3), Manual tests (3-5)
