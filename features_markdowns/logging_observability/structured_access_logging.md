# Implementation Plan: Structured Access Logging in Common Log Format (CLF)

## 1. Overview of the Feature

Structured access logging provides a standardized, machine-parseable record of every HTTP request handled by the server. The Common Log Format (CLF) is a widely adopted standard used by Apache, Nginx, and other web servers, making logs compatible with existing log analysis tools (e.g., GoAccess, AWStats, Splunk).

**Current State**: The server uses ad-hoc `println!` calls in `handle_connection()` (lines 60, 73 of `src/main.rs`) to print raw `Request: {http_request}` and `Response: {response}` output, which dumps the full HTTP message headers. This output is verbose, unstructured, and not suitable for log analysis.

**Desired State**: Each completed request produces a single CLF-formatted log line:

```
host ident authuser [date] "request" status bytes
```

Example:
```
127.0.0.1 - - [12/Feb/2026:14:30:00 +0000] "GET /howdy HTTP/1.1" 200 1234
```

Where:
- `host` — Client IP address (from `TcpStream::peer_addr()`)
- `ident` — Always `-` (RFC 1413 ident not supported)
- `authuser` — Always `-` (no authentication)
- `[date]` — Timestamp in RFC 7231 / CLF format: `[dd/Mon/yyyy:HH:MM:SS +0000]`
- `"request"` — The request line: `"METHOD /path HTTP/version"`
- `status` — HTTP response status code (e.g., 200, 404)
- `bytes` — Size of the response body in bytes (or `-` if no body)

**Impact**:
- Enables log analysis and monitoring with standard tools
- Replaces verbose debug output with concise, structured lines
- Foundation for configurable log levels and log destinations (future features)

---

## 2. Files to be Modified or Created

### Modified Files

1. **`/home/jwall/personal/rusty/rcomm/src/main.rs`**
   - Replace `println!("Request: {http_request}")` and `println!("Response: {response}")` with a single CLF log line after the response is sent
   - Extract client IP from `TcpStream::peer_addr()`
   - Pass `TcpStream` peer address info into the log formatter
   - Expose response status code from `HttpResponse` for logging

### New Files

2. **`/home/jwall/personal/rusty/rcomm/src/models/access_log.rs`**
   - Contains `format_clf_entry()` function that produces a CLF line
   - Contains `format_clf_timestamp()` helper for date formatting
   - Unit tests for formatting logic

### Modified Files (minor)

3. **`/home/jwall/personal/rusty/rcomm/src/models.rs`**
   - Add `pub mod access_log;` to the barrel export

4. **`/home/jwall/personal/rusty/rcomm/src/models/http_response.rs`**
   - Add a public getter `status_code(&self) -> u16` so the log formatter can read the response status
   - Add a public getter `body_len(&self) -> usize` to get body size without cloning

---

## 3. Step-by-Step Implementation Details

### Step 1: Add Getters to HttpResponse

**File**: `/home/jwall/personal/rusty/rcomm/src/models/http_response.rs`

Add public getters for status code and body length after the existing methods:

```rust
pub fn status_code(&self) -> u16 {
    self.status_code
}

pub fn body_len(&self) -> usize {
    self.body.as_ref().map(|b| b.len()).unwrap_or(0)
}
```

### Step 2: Create the Access Log Module

**File**: `/home/jwall/personal/rusty/rcomm/src/models/access_log.rs`

```rust
use std::net::SocketAddr;
use std::time::SystemTime;

/// Months array for CLF date formatting
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun",
    "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Format a timestamp in CLF format: [dd/Mon/yyyy:HH:MM:SS +0000]
///
/// Uses UTC (+0000) since std doesn't provide local timezone offset.
pub fn format_clf_timestamp(time: SystemTime) -> String {
    let duration = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();

    // Manual UTC time decomposition (no external crate)
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Convert days since epoch to y/m/d using a civil calendar algorithm
    let (year, month, day) = days_to_date(days);

    format!(
        "[{:02}/{}/{:04}:{:02}:{:02}:{:02} +0000]",
        day, MONTHS[month as usize - 1], year, hours, minutes, seconds
    )
}

/// Convert days since Unix epoch to (year, month, day).
/// Algorithm from Howard Hinnant's civil_from_days.
fn days_to_date(days: u64) -> (i64, u32, u32) {
    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as u32, d as u32)
}

/// Format a single CLF access log entry.
///
/// Format: `host ident authuser [date] "request" status bytes`
///
/// # Arguments
/// * `peer_addr` - Client socket address (IP:port)
/// * `method` - HTTP method string (e.g. "GET")
/// * `target` - Request target URI (e.g. "/howdy")
/// * `version` - HTTP version string (e.g. "HTTP/1.1")
/// * `status_code` - Response status code (e.g. 200)
/// * `body_len` - Response body size in bytes (0 produces "-")
pub fn format_clf_entry(
    peer_addr: Option<SocketAddr>,
    method: &str,
    target: &str,
    version: &str,
    status_code: u16,
    body_len: usize,
) -> String {
    let host = peer_addr
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|| "-".to_string());

    let timestamp = format_clf_timestamp(SystemTime::now());

    let bytes = if body_len > 0 {
        body_len.to_string()
    } else {
        "-".to_string()
    };

    format!(
        "{host} - - {timestamp} \"{method} {target} {version}\" {status_code} {bytes}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn format_clf_entry_basic() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)), 54321);
        let line = format_clf_entry(
            Some(addr), "GET", "/howdy", "HTTP/1.1", 200, 1234,
        );
        assert!(line.starts_with("192.168.1.1 - - ["));
        assert!(line.contains("\"GET /howdy HTTP/1.1\" 200 1234"));
    }

    #[test]
    fn format_clf_entry_no_body() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 80);
        let line = format_clf_entry(
            Some(addr), "HEAD", "/", "HTTP/1.1", 204, 0,
        );
        assert!(line.ends_with("204 -"));
    }

    #[test]
    fn format_clf_entry_no_peer_addr() {
        let line = format_clf_entry(
            None, "GET", "/", "HTTP/1.1", 200, 100,
        );
        assert!(line.starts_with("- - - ["));
    }

    #[test]
    fn format_clf_entry_404() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12345);
        let line = format_clf_entry(
            Some(addr), "GET", "/missing", "HTTP/1.1", 404, 512,
        );
        assert!(line.contains("\"GET /missing HTTP/1.1\" 404 512"));
    }

    #[test]
    fn format_clf_timestamp_contains_brackets() {
        let ts = format_clf_timestamp(SystemTime::now());
        assert!(ts.starts_with('['));
        assert!(ts.ends_with("+0000]"));
    }

    #[test]
    fn format_clf_timestamp_epoch() {
        let ts = format_clf_timestamp(SystemTime::UNIX_EPOCH);
        assert_eq!(ts, "[01/Jan/1970:00:00:00 +0000]");
    }

    #[test]
    fn days_to_date_epoch() {
        let (y, m, d) = days_to_date(0);
        assert_eq!((y, m, d), (1970, 1, 1));
    }

    #[test]
    fn days_to_date_known_date() {
        // 2026-02-12 is day 20496 since epoch
        let (y, m, d) = days_to_date(20496);
        assert_eq!((y, m, d), (2026, 2, 12));
    }
}
```

### Step 3: Export the Access Log Module

**File**: `/home/jwall/personal/rusty/rcomm/src/models.rs`

```rust
pub mod http_methods;
pub mod http_request;
pub mod http_response;
pub mod http_status_codes;
pub mod access_log;  // Add this line
```

### Step 4: Update handle_connection() to Emit CLF Logs

**File**: `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Current code** (lines 46–75):
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

**Updated code**:
```rust
fn handle_connection(mut stream: TcpStream, routes: HashMap<String, PathBuf>) {
    let peer_addr = stream.peer_addr().ok();

    let http_request = match HttpRequest::build_from_stream(&stream) {
        Ok(req) => req,
        Err(e) => {
            eprintln!("Bad request: {e}");
            let mut response = HttpResponse::build(String::from("HTTP/1.1"), 400);
            let body = format!("Bad Request: {e}");
            response.add_body(body.into());
            let _ = stream.write_all(&response.as_bytes());
            // Log the failed request in CLF format
            let log_line = rcomm::models::access_log::format_clf_entry(
                peer_addr, "-", "-", "-",
                response.status_code(), response.body_len(),
            );
            println!("{log_line}");
            return;
        }
    };
    let clean_target = clean_route(&http_request.target);

    let (mut response, filename) = if routes.contains_key(&clean_target) {
        (HttpResponse::build(String::from("HTTP/1.1"), 200),
            routes.get(&clean_target).unwrap().to_str().unwrap())
    } else {
        (HttpResponse::build(String::from("HTTP/1.1"), 404),
            "pages/not_found.html")
    };

    let contents = fs::read_to_string(filename).unwrap();
    response.add_body(contents.into());

    stream.write_all(&response.as_bytes()).unwrap();

    // Emit CLF access log line
    let log_line = rcomm::models::access_log::format_clf_entry(
        peer_addr,
        &http_request.method.to_string(),
        &http_request.target,
        &http_request.version,
        response.status_code(),
        response.body_len(),
    );
    println!("{log_line}");
}
```

**Key changes**:
- Capture `peer_addr` at the start before any reads
- Replace two verbose `println!` calls with a single CLF line after sending the response
- Log failed parse requests with `-` placeholders for method/target/version
- Use the new `status_code()` and `body_len()` getters

---

## 4. Code Snippets and Pseudocode

### CLF Line Format

```
FUNCTION format_clf_entry(peer_addr, method, target, version, status, body_len) -> string
    LET host = peer_addr.ip OR "-"
    LET timestamp = format_clf_timestamp(now)
    LET bytes = IF body_len > 0 THEN body_len.to_string ELSE "-"
    RETURN "{host} - - {timestamp} \"{method} {target} {version}\" {status} {bytes}"
END FUNCTION
```

### Timestamp Formatting

```
FUNCTION format_clf_timestamp(time) -> string
    LET secs = time since epoch
    LET (year, month, day) = civil_from_days(secs / 86400)
    LET hours = (secs % 86400) / 3600
    LET minutes = (secs % 3600) / 60
    LET seconds = secs % 60
    RETURN "[dd/Mon/yyyy:HH:MM:SS +0000]"
END FUNCTION
```

---

## 5. Testing Strategy

### Unit Tests (in `access_log.rs`)

- `format_clf_entry_basic` — Verifies correct format with all fields present
- `format_clf_entry_no_body` — Verifies `-` is used when body length is 0
- `format_clf_entry_no_peer_addr` — Verifies `-` is used when peer addr is unavailable
- `format_clf_entry_404` — Verifies 404 status and body length are correctly formatted
- `format_clf_timestamp_contains_brackets` — Verifies bracket wrapping
- `format_clf_timestamp_epoch` — Verifies known epoch date formatting
- `days_to_date_epoch` — Verifies 1970-01-01 conversion
- `days_to_date_known_date` — Verifies a known recent date

**Run unit tests**:
```bash
cargo test access_log
```

### Integration Tests (in `src/bin/integration_test.rs`)

CLF output goes to stdout, which is suppressed in integration tests (`Stdio::null()`). To verify CLF logging, the integration test could be modified to capture stdout and validate format. However, this is a lower-priority test since the CLF formatting logic is fully unit-tested.

Optional integration test approach:
- Change `start_server()` to pipe stdout to `Stdio::piped()`
- After sending a request, read the server's stdout and validate CLF format
- This adds complexity and may be deferred to a future enhancement

### Manual Testing

```bash
cargo run
# In another terminal:
curl http://127.0.0.1:7878/
# Server output should show:
# 127.0.0.1 - - [12/Feb/2026:14:30:00 +0000] "GET / HTTP/1.1" 200 1234
```

---

## 6. Edge Cases to Consider

### Case 1: Peer Address Unavailable
**Scenario**: `stream.peer_addr()` returns an error (unlikely but possible with dropped connections)
**Handling**: Use `None` which formats as `-` in the CLF line

### Case 2: Malformed Request (Parse Error)
**Scenario**: Client sends garbage data that fails to parse
**Handling**: Log with `-` for method, target, and version; still log status code (400) and body length

### Case 3: IPv6 Client Address
**Scenario**: Client connects via IPv6 (e.g., `::1`)
**Handling**: `IpAddr::to_string()` handles both IPv4 and IPv6 transparently

### Case 4: Very Large Body Sizes
**Scenario**: Response body is very large (e.g., multi-MB file)
**Handling**: `body_len()` returns `usize`, which formats correctly for any size

### Case 5: Concurrent Log Writes
**Scenario**: Multiple worker threads emit log lines simultaneously
**Handling**: `println!` uses stdout locking internally, so individual lines are atomic. Lines may interleave across requests, but each line will be complete.

### Case 6: UTC Timezone Only
**Scenario**: Server is in a non-UTC timezone
**Handling**: `SystemTime` gives UTC; CLF timestamp shows `+0000`. Local timezone would require platform-specific code or an external crate. UTC is acceptable and common for server logs.

---

## 7. Implementation Checklist

- [ ] Add `status_code()` and `body_len()` getters to `HttpResponse`
- [ ] Create `/home/jwall/personal/rusty/rcomm/src/models/access_log.rs` with:
  - [ ] `format_clf_timestamp()` function
  - [ ] `days_to_date()` helper function
  - [ ] `format_clf_entry()` function
  - [ ] Unit tests (8 tests)
- [ ] Update `/home/jwall/personal/rusty/rcomm/src/models.rs` to export `access_log`
- [ ] Update `/home/jwall/personal/rusty/rcomm/src/main.rs`:
  - [ ] Add import for `access_log`
  - [ ] Capture `peer_addr` in `handle_connection()`
  - [ ] Replace `println!("Request: ...")` and `println!("Response: ...")` with CLF log line
  - [ ] Add CLF logging for error responses (400 Bad Request)
- [ ] Run `cargo test access_log` — all unit tests pass
- [ ] Run `cargo test` — no regressions
- [ ] Run `cargo run --bin integration_test` — integration tests pass
- [ ] Manual verification: CLF lines appear on stdout

---

## 8. Complexity and Risk Analysis

**Complexity**: 4/10
- Date formatting without external crates requires a civil calendar algorithm (moderate)
- CLF format itself is simple string interpolation
- Requires adding getters to `HttpResponse` (trivial)
- No changes to core request/response flow

**Risk**: Low
- Pure additive change to logging output
- Does not affect request handling or response behavior
- Replaces existing verbose logging with structured output (slight behavioral change to stdout)
- Date algorithm is well-known and testable

**Dependencies**: None
- Uses only `std::time::SystemTime` and `std::net::SocketAddr`
- No external crates required

---

## 9. Future Enhancements

1. **Combined Log Format**: Extend CLF to include `Referer` and `User-Agent` fields
2. **Local Timezone**: Add platform-specific timezone offset detection
3. **Log Rotation**: Integrate with configurable log output destination feature
4. **JSON Logging**: Offer structured JSON log format as an alternative to CLF
5. **Request Duration**: Append response time to the log line (separate feature)
