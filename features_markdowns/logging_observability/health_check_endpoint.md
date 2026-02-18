# Implementation Plan: Health Check Endpoint

## 1. Overview of the Feature

A health check endpoint provides a lightweight, standardized URL that monitoring systems, load balancers, and container orchestrators (Kubernetes, Docker) can poll to determine if the server is alive and accepting requests.

**Current State**: The server has no dedicated health check mechanism. Monitoring tools must request an actual page (e.g., `/`) and hope the response indicates server health, which conflates content serving issues with server availability. If `pages/index.html` is deleted, a `GET /` returns 404, but the server itself is healthy.

**Desired State**: The server responds to `GET /health` (and optionally `GET /healthz`) with a minimal `200 OK` response containing server status information. This endpoint is:
1. Not file-backed — responds directly from the handler without reading from `pages/`
2. Always available regardless of the `pages/` directory contents
3. Returns a simple JSON body with server status
4. Lightweight — minimal overhead for frequent polling

Example response:
```
HTTP/1.1 200 OK
Content-Type: application/json
Content-Length: 27

{"status":"ok","uptime":42}
```

**Impact**:
- Enables standard health monitoring for production deployments
- Required for Kubernetes liveness/readiness probes
- Provides uptime and basic status metrics
- Foundation for more detailed status pages

---

## 2. Files to be Modified or Created

### Modified Files

1. **`/home/jwall/personal/rusty/rcomm/src/main.rs`**
   - Add a check for `/health` and `/healthz` routes in `handle_connection()` before the route lookup
   - Build the health response directly (no file read)
   - Track server start time for uptime calculation

### No New Files Required

The health check is a small addition to the request handler. If it grows in complexity (e.g., adding dependency checks), it could be extracted into its own module later.

---

## 3. Step-by-Step Implementation Details

### Step 1: Track Server Start Time

**File**: `/home/jwall/personal/rusty/rcomm/src/main.rs`

Add a static for the server start time at the top of the file:

```rust
use std::time::Instant;
use std::sync::OnceLock;

static SERVER_START: OnceLock<Instant> = OnceLock::new();
```

Initialize in `main()`:
```rust
fn main() {
    SERVER_START.set(Instant::now()).unwrap();
    // ... rest of main ...
}
```

### Step 2: Add Health Check Handler

**File**: `/home/jwall/personal/rusty/rcomm/src/main.rs`

Add a function that builds the health check response:

```rust
fn build_health_response() -> HttpResponse {
    let uptime_secs = SERVER_START
        .get()
        .map(|start| start.elapsed().as_secs())
        .unwrap_or(0);

    let body = format!("{{\"status\":\"ok\",\"uptime\":{uptime_secs}}}");

    let mut response = HttpResponse::build(String::from("HTTP/1.1"), 200);
    response.add_header("Content-Type".to_string(), "application/json".to_string());
    response.add_body(body.into());
    response
}
```

### Step 3: Add Health Route Check in handle_connection()

**File**: `/home/jwall/personal/rusty/rcomm/src/main.rs`

Modify `handle_connection()` to intercept `/health` and `/healthz` before the file-based route lookup:

**Current code** (lines 58–74):
```rust
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
```

**Updated code**:
```rust
    let clean_target = clean_route(&http_request.target);

    println!("Request: {http_request}");

    // Health check endpoint — respond directly without file lookup
    if clean_target == "/health" || clean_target == "/healthz" {
        let response = build_health_response();
        println!("Response: {response}");
        let _ = stream.write_all(&response.as_bytes());
        return;
    }

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
```

**Key changes**:
- Health check is handled before the route table lookup
- No file I/O — response is generated entirely in memory
- Returns immediately after sending the response
- Both `/health` and `/healthz` are supported (Kubernetes convention)

---

## 4. Code Snippets and Pseudocode

```
GLOBAL SERVER_START: Instant

FUNCTION main()
    SERVER_START = Instant::now()
    // ...
END FUNCTION

FUNCTION build_health_response() -> HttpResponse
    LET uptime = SERVER_START.elapsed().as_secs()
    LET body = {"status": "ok", "uptime": uptime}
    LET response = HttpResponse(200, "application/json", body)
    RETURN response
END FUNCTION

FUNCTION handle_connection(stream, routes)
    // ... parse request ...

    IF target == "/health" OR target == "/healthz" THEN
        LET response = build_health_response()
        SEND response
        RETURN
    END IF

    // ... normal route handling ...
END FUNCTION
```

---

## 5. Testing Strategy

### Unit Tests

No dedicated unit tests needed for the health response builder — it's a simple function that assembles a response. The correctness is verified through integration tests.

### Integration Tests (in `src/bin/integration_test.rs`)

```rust
fn test_health_endpoint(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/health")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;

    let content_type = resp.headers.get("content-type")
        .ok_or("missing Content-Type header")?;
    assert_eq_or_err(content_type, &"application/json".to_string(), "content-type")?;

    // Verify body contains expected JSON fields
    assert_contains_or_err(&resp.body, "\"status\":\"ok\"", "status field")?;
    assert_contains_or_err(&resp.body, "\"uptime\":", "uptime field")?;

    Ok(())
}

fn test_healthz_endpoint(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/healthz")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    assert_contains_or_err(&resp.body, "\"status\":\"ok\"", "body")?;
    Ok(())
}

fn test_health_does_not_shadow_route(addr: &str) -> Result<(), String> {
    // If someone has a pages/health/page.html, the health endpoint should take precedence.
    // This is intentional — health is a reserved path.
    let resp = send_request(addr, "GET", "/health")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    assert_contains_or_err(&resp.body, "\"status\":\"ok\"", "body")?;
    Ok(())
}
```

Add to the test runner in `main()`:
```rust
run_test("health_endpoint", || test_health_endpoint(&addr)),
run_test("healthz_endpoint", || test_healthz_endpoint(&addr)),
```

### Manual Testing

```bash
cargo run

# Test health endpoint
curl -v http://127.0.0.1:7878/health
# Expected: 200 OK, {"status":"ok","uptime":5}

# Test healthz alias
curl -v http://127.0.0.1:7878/healthz
# Expected: same response

# Test with jq for JSON parsing
curl -s http://127.0.0.1:7878/health | jq .
# Expected: {"status": "ok", "uptime": 42}

# Verify uptime increases
sleep 5
curl -s http://127.0.0.1:7878/health | jq .uptime
# Expected: a number ~5 higher than before
```

---

## 6. Edge Cases to Consider

### Case 1: Health Endpoint vs. File Route Conflict
**Scenario**: A file `pages/health/page.html` exists, creating a route at `/health`
**Handling**: The health check is checked before the route table, so it takes precedence. The `/health` route is effectively reserved. This is intentional — health check availability should not depend on the filesystem.

### Case 2: POST to /health
**Scenario**: A monitoring tool sends `POST /health` instead of `GET`
**Handling**: The current check only matches on path, not method. A POST to `/health` will still return the health response. This is acceptable — health checks should respond to any method.
**Future**: If `405 Method Not Allowed` is implemented, health could be restricted to GET and HEAD.

### Case 3: Uptime Overflow
**Scenario**: Server runs for longer than `u64::MAX` seconds (~584 billion years)
**Handling**: Not a realistic concern. `Duration::as_secs()` returns `u64`.

### Case 4: Health Check Under Load
**Scenario**: Server is overloaded; health check request is queued behind many other requests
**Handling**: The health check is processed like any other request through the thread pool. Under extreme load, it may be slow to respond. This correctly indicates the server is unhealthy (overloaded). A more sophisticated approach would bypass the thread pool, but that's a future enhancement.

### Case 5: Trailing Slash
**Scenario**: Client requests `/health/` with a trailing slash
**Handling**: `clean_route("/health/")` produces `/health`, which matches the health check. Works correctly.

### Case 6: JSON Encoding
**Scenario**: Status or uptime values could theoretically need escaping
**Handling**: `"ok"` is a static string with no special characters. Uptime is a number. No JSON escaping needed. If more fields are added (e.g., server name with special characters), a proper JSON builder should be used.

---

## 7. Implementation Checklist

- [ ] Add `OnceLock<Instant>` static for server start time in `src/main.rs`
- [ ] Initialize `SERVER_START` at the beginning of `main()`
- [ ] Add `build_health_response()` function
- [ ] Add `/health` and `/healthz` check in `handle_connection()` before route lookup
- [ ] Add integration tests:
  - [ ] `test_health_endpoint` — verify 200 + JSON body + status field
  - [ ] `test_healthz_endpoint` — verify /healthz alias works
- [ ] Run `cargo test` — no regressions
- [ ] Run `cargo run --bin integration_test` — integration tests pass
- [ ] Manual verification: JSON response with uptime

---

## 8. Complexity and Risk Analysis

**Complexity**: 2/10
- Simple string comparison for path matching
- Minimal JSON response (no serialization library needed)
- Single `OnceLock` for server start time

**Risk**: Very Low
- Pure additive feature — does not change existing route handling
- Reserved paths (`/health`, `/healthz`) are unlikely to conflict with user content
- No file I/O, no external dependencies
- Response is deterministic and tiny (< 50 bytes)

**Dependencies**: None
- Uses `std::time::Instant` and `std::sync::OnceLock`
- No external crates

---

## 9. Future Enhancements

1. **Readiness vs. Liveness**: Separate `/ready` endpoint that checks if routes are loaded
2. **Dependency Checks**: Include filesystem accessibility in health status
3. **Detailed Metrics**: Add thread pool utilization, request count, error rate to health response
4. **Configurable Path**: Allow customizing the health check path via environment variable
5. **Custom Health Logic**: Allow user-defined health check scripts or conditions
6. **HEAD Support**: Respond to HEAD requests with headers only (no body) for lightweight polling
