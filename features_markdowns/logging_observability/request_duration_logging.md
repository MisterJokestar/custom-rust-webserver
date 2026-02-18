# Implementation Plan: Log Request Duration

## 1. Overview of the Feature

Request duration logging measures the time elapsed from when a connection is accepted to when the response is fully sent. This metric is critical for identifying slow endpoints, detecting performance regressions, and understanding server behavior under load.

**Current State**: The server has no timing instrumentation. The `handle_connection()` function in `src/main.rs` processes requests synchronously but does not measure or report how long each request takes.

**Desired State**: Each request logs the time taken in milliseconds, appended to the access log line or emitted as a separate timing entry. The duration is measured using `std::time::Instant` (monotonic clock, immune to wall-clock adjustments).

Example output (appended to CLF-style log):
```
127.0.0.1 - - [12/Feb/2026:14:30:00 +0000] "GET / HTTP/1.1" 200 1234 2ms
```

Or as a standalone timing line:
```
[INFO] GET / -> 200 (2ms)
```

**Impact**:
- Enables performance monitoring and optimization
- Helps identify slow file reads or large responses
- Foundation for request-level metrics and SLA tracking
- Minimal overhead (monotonic clock reads are ~25ns)

---

## 2. Files to be Modified or Created

### Modified Files

1. **`/home/jwall/personal/rusty/rcomm/src/main.rs`**
   - Add `std::time::Instant` import
   - Record start time at the beginning of `handle_connection()`
   - Compute elapsed duration after `stream.write_all()` completes
   - Include duration in the log output

### No New Files Required

This feature is a small, focused change to `handle_connection()`. If the structured access logging feature (CLF) is implemented first, the duration can be appended to `format_clf_entry()` as an extra field. Otherwise, it can be added to the existing `println!` calls.

### Optional: Modify Access Log Module

2. **`/home/jwall/personal/rusty/rcomm/src/models/access_log.rs`** (if CLF feature is implemented first)
   - Add a `duration_ms` parameter to `format_clf_entry()`
   - Append duration as a non-standard extension to the CLF line

---

## 3. Step-by-Step Implementation Details

### Step 1: Add Timing to handle_connection()

**File**: `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Current code** (lines 46–75):
```rust
fn handle_connection(mut stream: TcpStream, routes: HashMap<String, PathBuf>) {
    let http_request = match HttpRequest::build_from_stream(&stream) {
        // ...
    };
    // ... route matching, file reading, response building ...
    stream.write_all(&response.as_bytes()).unwrap();
}
```

**Updated code**:
```rust
use std::time::Instant;

fn handle_connection(mut stream: TcpStream, routes: HashMap<String, PathBuf>) {
    let start = Instant::now();

    let http_request = match HttpRequest::build_from_stream(&stream) {
        Ok(req) => req,
        Err(e) => {
            eprintln!("Bad request: {e}");
            let mut response = HttpResponse::build(String::from("HTTP/1.1"), 400);
            let body = format!("Bad Request: {e}");
            response.add_body(body.into());
            let _ = stream.write_all(&response.as_bytes());
            let elapsed = start.elapsed();
            eprintln!("  -> 400 ({}ms)", elapsed.as_millis());
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

    let elapsed = start.elapsed();
    println!(
        "{} {} -> {} ({}ms)",
        http_request.method, http_request.target,
        if routes.contains_key(&clean_target) { 200 } else { 404 },
        elapsed.as_millis()
    );
}
```

### Step 2 (If CLF Feature Exists): Extend format_clf_entry

If the structured access logging feature is already implemented, modify `format_clf_entry()` to accept an optional duration:

```rust
pub fn format_clf_entry(
    peer_addr: Option<SocketAddr>,
    method: &str,
    target: &str,
    version: &str,
    status_code: u16,
    body_len: usize,
    duration_ms: Option<u128>,
) -> String {
    // ... existing CLF formatting ...

    let duration_str = duration_ms
        .map(|ms| format!(" {ms}ms"))
        .unwrap_or_default();

    format!(
        "{host} - - {timestamp} \"{method} {target} {version}\" {status_code} {bytes}{duration_str}"
    )
}
```

And in `handle_connection()`:
```rust
let elapsed = start.elapsed();
let log_line = rcomm::models::access_log::format_clf_entry(
    peer_addr,
    &http_request.method.to_string(),
    &http_request.target,
    &http_request.version,
    response.status_code(),
    response.body_len(),
    Some(elapsed.as_millis()),
);
println!("{log_line}");
```

---

## 4. Code Snippets and Pseudocode

```
FUNCTION handle_connection(stream, routes)
    LET start = Instant::now()

    LET request = parse_request(stream)
    LET response = build_response(request, routes)
    send_response(stream, response)

    LET elapsed = start.elapsed()
    LOG "{method} {target} -> {status} ({elapsed}ms)"
END FUNCTION
```

---

## 5. Testing Strategy

### Unit Tests

No new unit tests needed for the timing instrumentation itself — `Instant::now()` and `Duration::as_millis()` are standard library functions with well-defined behavior.

If the CLF feature is extended with duration, add unit tests for the new parameter:

```rust
#[test]
fn format_clf_entry_with_duration() {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 80);
    let line = format_clf_entry(
        Some(addr), "GET", "/", "HTTP/1.1", 200, 100, Some(42),
    );
    assert!(line.ends_with("200 100 42ms"));
}

#[test]
fn format_clf_entry_without_duration() {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 80);
    let line = format_clf_entry(
        Some(addr), "GET", "/", "HTTP/1.1", 200, 100, None,
    );
    assert!(line.ends_with("200 100"));
}
```

### Integration Tests

Duration logging is visible in stdout, which is suppressed during integration tests. No new integration tests needed.

### Manual Testing

```bash
cargo run
# In another terminal:
curl http://127.0.0.1:7878/
# Server output should include duration, e.g.:
# GET / -> 200 (1ms)
```

---

## 6. Edge Cases to Consider

### Case 1: Very Fast Requests
**Scenario**: Request completes in less than 1ms
**Handling**: `as_millis()` returns 0. Consider using `as_micros()` for sub-millisecond precision, or format as `<1ms`.
**Alternative**: Use `elapsed.as_secs_f64() * 1000.0` for fractional milliseconds: `0.42ms`

### Case 2: Very Slow Requests (Timeout)
**Scenario**: Request parsing times out after 30 seconds
**Handling**: Duration correctly reflects the timeout wait time. The log line will show the full elapsed time (e.g., `30001ms`).

### Case 3: Connection Error Before Parsing
**Scenario**: `TcpStream` fails during `build_from_stream()` with an IO error
**Handling**: Duration is still measured from `start` to the error handler, so the elapsed time is logged.

### Case 4: Clock Monotonicity
**Scenario**: System clock is adjusted during request
**Handling**: `Instant` uses a monotonic clock (CLOCK_MONOTONIC), so it is immune to system clock changes. Duration is always non-negative.

### Case 5: Duration Overflow
**Scenario**: A request somehow takes longer than u128::MAX milliseconds (~10^25 years)
**Handling**: Not a realistic concern. `Duration::as_millis()` returns `u128`.

---

## 7. Implementation Checklist

- [ ] Add `use std::time::Instant;` to `src/main.rs` imports
- [ ] Add `let start = Instant::now();` at the start of `handle_connection()`
- [ ] Compute `start.elapsed()` after `stream.write_all()`
- [ ] Include duration in log output
- [ ] If CLF feature exists: extend `format_clf_entry()` with `duration_ms` parameter
- [ ] Run `cargo test` — no regressions
- [ ] Run `cargo run --bin integration_test` — integration tests pass
- [ ] Manual verification: duration values appear in server output

---

## 8. Complexity and Risk Analysis

**Complexity**: 2/10
- Two lines of code: `Instant::now()` and `start.elapsed()`
- One additional format argument in the log line
- No architectural changes

**Risk**: Very Low
- `Instant::now()` has negligible overhead (~25ns)
- Monotonic clock, no system clock dependency
- Pure additive change to logging output
- No effect on request handling behavior

**Dependencies**: None
- Uses only `std::time::Instant` and `std::time::Duration`
- Available in all Rust editions

---

## 9. Future Enhancements

1. **Microsecond Precision**: Use `as_micros()` for sub-millisecond timing
2. **P99 Latency Tracking**: Maintain a rolling window of durations for percentile stats
3. **Slow Request Threshold**: Log at `WARN` level when duration exceeds a configurable threshold
4. **Per-Phase Timing**: Break down duration into parse time, file read time, and write time
5. **Histogram Endpoint**: Expose latency histograms via the health check endpoint
