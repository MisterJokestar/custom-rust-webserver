# Implementation Plan: Request ID Generation and Logging

## 1. Overview of the Feature

Request IDs assign a unique identifier to each incoming HTTP request, enabling end-to-end tracing and correlation across log entries. When a single request generates multiple log lines (access log, error log, debug output), the request ID ties them all together.

**Current State**: The server has no concept of request identity. Log entries for the same request are correlated only by their timing and thread interleaving, which is unreliable under concurrent load.

**Desired State**: Each request is assigned a unique ID at the start of `handle_connection()`. This ID is:
1. Included in all log lines for that request
2. Returned to the client in a `X-Request-Id` response header
3. Generated as a simple monotonic counter (lightweight, no UUID dependency)

Example output:
```
[req-00042] 127.0.0.1:54321 "GET /howdy HTTP/1.1" 200 1234 2ms
[req-00042] File read: pages/howdy/page.html (1234 bytes)
```

Example response header:
```
X-Request-Id: req-00042
```

**Impact**:
- Enables log correlation across multiple entries for the same request
- Allows clients to report their request ID when filing support requests
- Foundation for distributed tracing (if reverse proxied)
- Minimal overhead (atomic counter increment)

---

## 2. Files to be Modified or Created

### New Files

1. **`/home/jwall/personal/rusty/rcomm/src/request_id.rs`**
   - Contains a global atomic counter
   - Provides `next_request_id() -> String` function
   - Thread-safe via `AtomicU64`

### Modified Files

2. **`/home/jwall/personal/rusty/rcomm/src/lib.rs`**
   - Add `pub mod request_id;`

3. **`/home/jwall/personal/rusty/rcomm/src/main.rs`**
   - Generate request ID at start of `handle_connection()`
   - Include ID in all log lines
   - Add `X-Request-Id` header to the response

---

## 3. Step-by-Step Implementation Details

### Step 1: Create the Request ID Module

**File**: `/home/jwall/personal/rusty/rcomm/src/request_id.rs`

```rust
use std::sync::atomic::{AtomicU64, Ordering};

/// Global request counter, starting at 1.
static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Generate the next unique request ID.
///
/// Returns a string like "req-00001", "req-00042", etc.
/// Thread-safe and lock-free (uses atomic increment).
///
/// The counter wraps around at u64::MAX (approximately 1.8 × 10^19),
/// which is effectively unlimited for practical purposes.
pub fn next_request_id() -> String {
    let id = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("req-{id:05}")
}

/// Reset the counter (for testing purposes only).
#[cfg(test)]
fn reset_counter() {
    REQUEST_COUNTER.store(1, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_request_id_increments() {
        reset_counter();
        let id1 = next_request_id();
        let id2 = next_request_id();
        let id3 = next_request_id();
        assert_eq!(id1, "req-00001");
        assert_eq!(id2, "req-00002");
        assert_eq!(id3, "req-00003");
    }

    #[test]
    fn next_request_id_is_unique_across_threads() {
        use std::collections::HashSet;
        use std::sync::{Arc, Mutex};
        use std::thread;

        reset_counter();

        let ids: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
        let mut handles = Vec::new();

        for _ in 0..10 {
            let ids = Arc::clone(&ids);
            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    let id = next_request_id();
                    ids.lock().unwrap().insert(id);
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        // 10 threads × 100 IDs = 1000 unique IDs
        assert_eq!(ids.lock().unwrap().len(), 1000);
    }

    #[test]
    fn next_request_id_format() {
        reset_counter();
        let id = next_request_id();
        assert!(id.starts_with("req-"));
        // Should be zero-padded to at least 5 digits
        assert_eq!(id.len(), "req-00001".len());
    }
}
```

### Step 2: Export the Module

**File**: `/home/jwall/personal/rusty/rcomm/src/lib.rs`

```rust
pub mod request_id;
pub mod models;
```

### Step 3: Integrate into handle_connection()

**File**: `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Current code** (lines 46–75):
```rust
fn handle_connection(mut stream: TcpStream, routes: HashMap<String, PathBuf>) {
    let http_request = match HttpRequest::build_from_stream(&stream) {
        // ...
    };
    // ...
    println!("Request: {http_request}");
    // ...
    println!("Response: {response}");
    stream.write_all(&response.as_bytes()).unwrap();
}
```

**Updated code**:
```rust
fn handle_connection(mut stream: TcpStream, routes: HashMap<String, PathBuf>) {
    let req_id = rcomm::request_id::next_request_id();

    let http_request = match HttpRequest::build_from_stream(&stream) {
        Ok(req) => req,
        Err(e) => {
            eprintln!("[{req_id}] Bad request: {e}");
            let mut response = HttpResponse::build(String::from("HTTP/1.1"), 400);
            response.add_header("X-Request-Id".to_string(), req_id.clone());
            let body = format!("Bad Request: {e}");
            response.add_body(body.into());
            let _ = stream.write_all(&response.as_bytes());
            return;
        }
    };
    let clean_target = clean_route(&http_request.target);

    println!("[{req_id}] Request: {http_request}");

    let (mut response, filename) = if routes.contains_key(&clean_target) {
        (HttpResponse::build(String::from("HTTP/1.1"), 200),
            routes.get(&clean_target).unwrap().to_str().unwrap())
    } else {
        (HttpResponse::build(String::from("HTTP/1.1"), 404),
            "pages/not_found.html")
    };

    // Add request ID to response headers
    response.add_header("X-Request-Id".to_string(), req_id.clone());

    let contents = fs::read_to_string(filename).unwrap();
    response.add_body(contents.into());

    println!("[{req_id}] Response: {response}");
    stream.write_all(&response.as_bytes()).unwrap();
}
```

**Key changes**:
- Generate `req_id` at the very start (before any reads or processing)
- Prefix all log lines with `[{req_id}]`
- Add `X-Request-Id` header to all responses (including error responses)

---

## 4. Code Snippets and Pseudocode

```
GLOBAL COUNTER: AtomicU64 = 1

FUNCTION next_request_id() -> string
    LET id = COUNTER.fetch_and_increment()
    RETURN "req-{id:05}"
END FUNCTION

FUNCTION handle_connection(stream, routes)
    LET req_id = next_request_id()
    // ... all log lines include [{req_id}] ...
    response.add_header("X-Request-Id", req_id)
END FUNCTION
```

---

## 5. Testing Strategy

### Unit Tests (in `request_id.rs`)

- `next_request_id_increments` — Verifies sequential IDs are produced
- `next_request_id_is_unique_across_threads` — Verifies no duplicates under concurrent access (10 threads × 100 IDs)
- `next_request_id_format` — Verifies format starts with `req-` and has proper padding

**Run unit tests**:
```bash
cargo test request_id
```

### Integration Tests (in `src/bin/integration_test.rs`)

```rust
fn test_request_id_header(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    let req_id = resp.headers.get("x-request-id")
        .ok_or("missing X-Request-Id header")?;
    if !req_id.starts_with("req-") {
        return Err(format!("X-Request-Id should start with 'req-', got: {req_id}"));
    }
    Ok(())
}

fn test_request_id_unique(addr: &str) -> Result<(), String> {
    let resp1 = send_request(addr, "GET", "/")?;
    let resp2 = send_request(addr, "GET", "/")?;
    let id1 = resp1.headers.get("x-request-id")
        .ok_or("missing X-Request-Id on first request")?;
    let id2 = resp2.headers.get("x-request-id")
        .ok_or("missing X-Request-Id on second request")?;
    if id1 == id2 {
        return Err(format!("Request IDs should be unique, both were: {id1}"));
    }
    Ok(())
}
```

### Manual Testing

```bash
cargo run
curl -v http://127.0.0.1:7878/
# Response headers should include:
# X-Request-Id: req-00001

curl -v http://127.0.0.1:7878/
# X-Request-Id: req-00002

# Server logs should show:
# [req-00001] Request: GET / HTTP/1.1 ...
# [req-00001] Response: HTTP/1.1 200 OK ...
# [req-00002] Request: GET / HTTP/1.1 ...
# [req-00002] Response: HTTP/1.1 200 OK ...
```

---

## 6. Edge Cases to Consider

### Case 1: Counter Overflow
**Scenario**: After `u64::MAX` requests (~1.8 × 10^19), the counter wraps around
**Handling**: `AtomicU64::fetch_add` wraps on overflow by default. At 100,000 requests/second, it would take ~5.8 million years to overflow. Non-issue in practice.

### Case 2: Server Restart Resets Counter
**Scenario**: After restart, IDs start from 1 again
**Handling**: Expected behavior. For cross-restart uniqueness, a prefix like the startup timestamp could be added: `req-1707734400-00001`. This is a future enhancement.

### Case 3: Client Sends X-Request-Id Header
**Scenario**: Client includes an `X-Request-Id` header in the request (e.g., from upstream proxy)
**Handling**: Currently ignored — server always generates its own ID. Future enhancement: respect incoming `X-Request-Id` if present.

### Case 4: Concurrent ID Generation
**Scenario**: Multiple worker threads request IDs simultaneously
**Handling**: `AtomicU64::fetch_add(1, Ordering::Relaxed)` is lock-free and handles concurrent access correctly. Each thread gets a unique value.

### Case 5: ID Length Growth
**Scenario**: After 99999 requests, the ID string becomes longer (e.g., `req-100000`)
**Handling**: The `{id:05}` format pads to at least 5 digits but grows for larger numbers. This is acceptable — log parsers handle variable-width fields.

---

## 7. Implementation Checklist

- [ ] Create `/home/jwall/personal/rusty/rcomm/src/request_id.rs` with:
  - [ ] `AtomicU64` global counter
  - [ ] `next_request_id()` function
  - [ ] Unit tests (3 tests)
- [ ] Update `/home/jwall/personal/rusty/rcomm/src/lib.rs` — add `pub mod request_id;`
- [ ] Update `/home/jwall/personal/rusty/rcomm/src/main.rs`:
  - [ ] Generate `req_id` at start of `handle_connection()`
  - [ ] Prefix all log lines with `[{req_id}]`
  - [ ] Add `X-Request-Id` header to all responses
- [ ] Add integration tests:
  - [ ] `test_request_id_header` — verify header is present
  - [ ] `test_request_id_unique` — verify IDs are unique across requests
- [ ] Run `cargo test request_id` — all unit tests pass
- [ ] Run `cargo test` — no regressions
- [ ] Run `cargo run --bin integration_test` — integration tests pass
- [ ] Manual verification: IDs in server output and response headers

---

## 8. Complexity and Risk Analysis

**Complexity**: 2/10
- Single atomic counter with a format function
- One header addition per response
- Prefix addition to existing log lines

**Risk**: Very Low
- `AtomicU64` is lock-free and has negligible overhead
- Adding a response header is pure additive behavior
- No changes to request parsing or routing logic
- Counter is monotonically increasing (no collisions possible)

**Dependencies**: None
- Uses only `std::sync::atomic::AtomicU64`
- No external crates

---

## 9. Future Enhancements

1. **UUID-style IDs**: Use a random-based ID for cross-restart uniqueness
2. **Respect Upstream X-Request-Id**: If the request includes `X-Request-Id`, use that instead of generating a new one
3. **Request ID Propagation**: Include the request ID in error pages and 500 responses for user-facing correlation
4. **Startup Epoch Prefix**: Include server start timestamp in IDs for cross-restart uniqueness without randomness
5. **Configurable Format**: Allow operators to choose between counter, UUID, or custom ID formats
