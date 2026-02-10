# Feature: Return `405 Method Not Allowed` for Unsupported HTTP Methods

**Category:** HTTP Protocol Compliance
**Complexity:** 2/10
**Necessity:** 7/10
**Status:** Planning

---

## Overview

Currently, rcomm serves all static routes using a GET-only model. Any request to an existing route returns 200 OK regardless of the HTTP method (POST, PUT, DELETE, PATCH, etc.). This violates HTTP specifications and causes incorrect behavior when clients send non-GET requests to valid routes.

This feature implements proper HTTP method validation by:
1. Accepting only GET and HEAD methods on static routes
2. Returning 405 Method Not Allowed with proper `Allow` header for unsupported methods
3. Ensuring the server respects HTTP protocol requirements

**Note:** rcomm serves only static files, so no methods beyond GET/HEAD should be allowed on any route.

---

## Background: HTTP 405 Response

According to RFC 7231, when a request method is not allowed on a target resource:
- Return HTTP status code `405 Method Not Allowed`
- Include an `Allow` header listing supported methods (e.g., `Allow: GET, HEAD`)
- Typically include a response body explaining the restriction

Example:
```
HTTP/1.1 405 Method Not Allowed
Allow: GET, HEAD
Content-Type: text/plain
Content-Length: 42

The POST method is not allowed for this resource.
```

---

## Architecture & Current State

### Request Handling Flow (Current)

```
handle_connection()
  ├─ Parse HttpRequest from stream
  │  └─ method: HttpMethods enum (GET, POST, PUT, DELETE, etc.)
  │  └─ target: String (request path)
  ├─ Check if route exists in HashMap
  ├─ Respond with 200 or 404 (ignores method)
  └─ Write response to stream
```

The problem: Method is parsed but never validated. Any method on an existing route returns 200.

### Files Involved

1. **`src/main.rs`** — `handle_connection()` function
   - Current logic: `if routes.contains_key(&clean_target) → 200 else → 404`
   - Must add: Method validation before selecting response code

2. **`src/models/http_methods.rs`** — HTTP method enum and parser
   - Already defines GET, HEAD, POST, PUT, DELETE, PATCH, CONNECT, OPTIONS, TRACE
   - No helper functions to distinguish allowed vs. unsupported methods

3. **`src/models/http_status_codes.rs`** — Status code to phrase mapping
   - Already contains code 405 → "Method Not Allowed"
   - No changes needed

4. **`src/models/http_response.rs`** — Response builder
   - Already supports arbitrary headers via `add_header()`
   - Works well for adding `Allow` header

---

## Implementation Plan

### Step 1: Add Helper Function in `http_methods.rs`

Add a function to determine if a method is allowed on static routes (only GET and HEAD).

**Location:** `/home/jwall/personal/rusty/rcomm/src/models/http_methods.rs`

**Purpose:** Centralize the logic for determining allowed methods, making it testable and reusable.

**Code to Add:**

```rust
pub fn is_method_allowed_on_static_route(method: &HttpMethods) -> bool {
    matches!(method, HttpMethods::GET | HttpMethods::HEAD)
}
```

**Why:** This isolates the business logic. If you later want to allow OPTIONS (to query allowed methods), you change one function. Tests can validate this logic independently of the server.

**Tests to Add:**

```rust
#[test]
fn get_is_allowed() {
    assert!(is_method_allowed_on_static_route(&HttpMethods::GET));
}

#[test]
fn head_is_allowed() {
    assert!(is_method_allowed_on_static_route(&HttpMethods::HEAD));
}

#[test]
fn post_is_not_allowed() {
    assert!(!is_method_allowed_on_static_route(&HttpMethods::POST));
}

#[test]
fn put_is_not_allowed() {
    assert!(!is_method_allowed_on_static_route(&HttpMethods::PUT));
}

#[test]
fn delete_is_not_allowed() {
    assert!(!is_method_allowed_on_static_route(&HttpMethods::DELETE));
}

#[test]
fn patch_is_not_allowed() {
    assert!(!is_method_allowed_on_static_route(&HttpMethods::PATCH));
}
```

**Impact:** Zero runtime cost if inlined. Minimal code footprint.

---

### Step 2: Modify `handle_connection()` in `src/main.rs`

Update the main request handler to validate HTTP methods and return 405 when appropriate.

**Location:** `/home/jwall/personal/rusty/rcomm/src/main.rs`, lines 46–75

**Current Logic:**
```rust
let (mut response, filename) = if routes.contains_key(&clean_target) {
    (HttpResponse::build(String::from("HTTP/1.1"), 200),
        routes.get(&clean_target).unwrap().to_str().unwrap())
} else {
    (HttpResponse::build(String::from("HTTP/1.1"), 404),
        "pages/not_found.html")
};
```

**New Logic:**
```rust
let (mut response, filename) = if routes.contains_key(&clean_target) {
    if is_method_allowed_on_static_route(&http_request.method) {
        (HttpResponse::build(String::from("HTTP/1.1"), 200),
            routes.get(&clean_target).unwrap().to_str().unwrap())
    } else {
        let mut response = HttpResponse::build(String::from("HTTP/1.1"), 405);
        response.add_header("Allow".to_string(), "GET, HEAD".to_string());
        response.add_body("Method Not Allowed".as_bytes().to_vec());
        println!("Response: {response}");
        stream.write_all(&response.as_bytes()).unwrap();
        return;
    }
} else {
    (HttpResponse::build(String::from("HTTP/1.1"), 404),
        "pages/not_found.html")
};
```

**Code Explanation:**
- First check: Does the route exist?
  - If yes: Is the HTTP method allowed?
    - If allowed: Process normally (200 OK)
    - If not allowed: Send 405 with Allow header, body, then return early
  - If no: Process 404 as before

**Flow Diagram:**
```
Request arrives
├─ Parse request
├─ Clean target route
├─ Route exists?
│  ├─ YES → Method allowed?
│  │        ├─ YES → Return 200 OK
│  │        └─ NO  → Return 405 Method Not Allowed (exit early)
│  └─ NO  → Return 404 Not Found
└─ Write response
```

**Key Points:**
- The `is_method_allowed_on_static_route()` function is imported from `models::http_methods`
- Early return prevents unnecessary file I/O for 405 responses
- Allows header explicitly lists `GET, HEAD` (per HTTP spec)
- Response body is user-friendly but minimal

**Import Statement (if not already present):**
```rust
use rcomm::models::http_methods::is_method_allowed_on_static_route;
```

---

### Step 3: Comprehensive Testing

#### Unit Tests (in `src/models/http_methods.rs`)

Already covered in Step 1. Run:
```bash
cargo test http_methods::tests --lib
```

#### Integration Tests (in `src/bin/integration_test.rs`)

Add tests at the end of the test cases section before `main()`:

**Test 1: POST to Existing Route Returns 405**
```rust
fn test_post_to_existing_route_returns_405(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "POST", "/")?;
    assert_eq_or_err(&resp.status_code, &405, "status")?;
    Ok(())
}
```

**Test 2: 405 Includes Allow Header**
```rust
fn test_405_includes_allow_header(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "POST", "/")?;
    assert_eq_or_err(&resp.status_code, &405, "status")?;
    let allow_header = resp.headers.get("allow")
        .ok_or("missing Allow header".to_string())?;
    assert_contains_or_err(allow_header, "GET", "Allow header")?;
    assert_contains_or_err(allow_header, "HEAD", "Allow header")?;
    Ok(())
}
```

**Test 3: PUT to Existing Route Returns 405**
```rust
fn test_put_to_existing_route_returns_405(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "PUT", "/")?;
    assert_eq_or_err(&resp.status_code, &405, "status")?;
    Ok(())
}
```

**Test 4: DELETE to Existing Route Returns 405**
```rust
fn test_delete_to_existing_route_returns_405(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "DELETE", "/")?;
    assert_eq_or_err(&resp.status_code, &405, "status")?;
    Ok(())
}
```

**Test 5: PATCH to Existing Route Returns 405**
```rust
fn test_patch_to_existing_route_returns_405(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "PATCH", "/")?;
    assert_eq_or_err(&resp.status_code, &405, "status")?;
    Ok(())
}
```

**Test 6: HEAD to Existing Route Still Returns 200**
```rust
fn test_head_to_existing_route_returns_200(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "HEAD", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    Ok(())
}
```

**Test 7: 405 on Static File (CSS)**
```rust
fn test_post_to_static_file_returns_405(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "POST", "/index.css")?;
    assert_eq_or_err(&resp.status_code, &405, "status")?;
    Ok(())
}
```

**Test 8: 405 on Nested Route**
```rust
fn test_post_to_nested_route_returns_405(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "POST", "/howdy")?;
    assert_eq_or_err(&resp.status_code, &405, "status")?;
    Ok(())
}
```

**Test 9: 404 Unaffected (Method Doesn't Matter for Non-Existent Routes)**
```rust
fn test_post_to_nonexistent_route_returns_404(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "POST", "/does-not-exist")?;
    assert_eq_or_err(&resp.status_code, &404, "status")?;
    Ok(())
}
```

**Integration Test Registration:**

Add these test calls to the `main()` function in the integration test framework:

```rust
// In main(), after existing tests:
tests.push(run_test("POST to existing route returns 405", || {
    test_post_to_existing_route_returns_405(&addr)
}));
tests.push(run_test("405 includes Allow header", || {
    test_405_includes_allow_header(&addr)
}));
tests.push(run_test("PUT to existing route returns 405", || {
    test_put_to_existing_route_returns_405(&addr)
}));
tests.push(run_test("DELETE to existing route returns 405", || {
    test_delete_to_existing_route_returns_405(&addr)
}));
tests.push(run_test("PATCH to existing route returns 405", || {
    test_patch_to_existing_route_returns_405(&addr)
}));
tests.push(run_test("HEAD to existing route returns 200", || {
    test_head_to_existing_route_returns_200(&addr)
}));
tests.push(run_test("POST to static file returns 405", || {
    test_post_to_static_file_returns_405(&addr)
}));
tests.push(run_test("POST to nested route returns 405", || {
    test_post_to_nested_route_returns_405(&addr)
}));
tests.push(run_test("POST to nonexistent route returns 404", || {
    test_post_to_nonexistent_route_returns_404(&addr)
}));
```

#### Manual Testing

After implementation, verify using `curl`:

```bash
# Build
cargo build

# Start server in background
cargo run &
SERVER_PID=$!

# Test GET (should work)
curl -i http://127.0.0.1:7878/

# Test POST (should return 405)
curl -i -X POST http://127.0.0.1:7878/

# Test PUT (should return 405)
curl -i -X PUT http://127.0.0.1:7878/

# Test DELETE (should return 405)
curl -i -X DELETE http://127.0.0.1:7878/

# Verify Allow header
curl -i -X POST http://127.0.0.1:7878/ | grep -i allow

# Cleanup
kill $SERVER_PID
```

---

## Edge Cases & Considerations

### Edge Case 1: Requests to Non-Existent Routes

**Scenario:** POST request to `/does-not-exist`

**Current behavior:** Returns 404 Not Found

**After implementation:** Should still return 404 (not 405)

**Reasoning:** The resource doesn't exist, so we can't say the method is disallowed. 404 is the correct response.

**Implementation consideration:** The code checks route existence BEFORE method validation, so this works correctly.

### Edge Case 2: HEAD Requests

**Scenario:** HEAD request to existing static route

**Current behavior:** Returns 200 OK (GET works, HEAD is allowed)

**After implementation:** Should still return 200

**Reasoning:** HEAD is semantically equivalent to GET and should be allowed per RFC 7231.

**Code snippet:** The `is_method_allowed_on_static_route()` includes HEAD.

### Edge Case 3: Content-Length on 405 Response

**Scenario:** POST request with a body to existing route

**Current behavior:** (Not applicable yet)

**After implementation:** Return 405 with Content-Length header

**Reasoning:** All responses with bodies must include Content-Length. HttpResponse::add_body() automatically sets this.

**Code:** `response.add_body()` handles it automatically.

### Edge Case 4: Case-Sensitive Method Names

**Scenario:** POST vs. post vs. Post

**Current behavior:** Only uppercase methods parse correctly (per http_request.rs line 65)

**After implementation:** Same behavior (request parsing rejects lowercase, so never reaches handle_connection)

**Code:** Already handled in http_method_from_string(), returns None for non-uppercase.

### Edge Case 5: Unknown Methods

**Scenario:** Request with "UNKNOWN", "BREW", or other invalid HTTP method

**Current behavior:** Request parsing fails, returns 400 Bad Request

**After implementation:** Same behavior (invalid methods never reach handle_connection)

**Code:** Already handled in http_request.rs build_from_stream().

### Edge Case 6: Routes with Trailing Slashes

**Scenario:** POST to `/` vs. `/howdy/` vs. `/howdy`

**Current behavior:** clean_route() strips trailing slashes, so `/howdy/` → `/howdy`

**After implementation:** Method validation applies to cleaned routes, so all three work the same

**Code:** clean_route() is called before method check, so consistent behavior.

---

## Error Handling

### Request Parsing Errors (Already Handled)
- Bad request line → 400 Bad Request (before this feature)
- Missing Host header (HTTP/1.1) → 400 Bad Request
- Oversized headers → 400 Bad Request

### New Paths in This Feature
- Route exists + method not allowed → 405 Method Not Allowed (this feature)
- Route exists + method allowed → 200 OK (existing behavior)
- Route doesn't exist → 404 Not Found (existing behavior)

**No new error conditions introduced by this feature.**

---

## Performance Impact

### Complexity Added
- One extra method validation check per request (one `matches!()` call)
- Early return for 405 saves file I/O

### Memory Impact
- Negligible: No additional allocations beyond the response object
- Allow header is static string "GET, HEAD"

### Benchmark Results (Estimated)
- 405 responses: Faster than 200 (no file read)
- 200/404 responses: Negligible difference (one boolean check)

---

## Rollback / Revert

If this feature needs to be removed:
1. Delete `is_method_allowed_on_static_route()` from `src/models/http_methods.rs` and its tests
2. Remove the method check from `handle_connection()` in `src/main.rs`, revert to the original if-else
3. Remove integration tests added in Step 3
4. Rebuild and retest

---

## Success Criteria

- [x] Unit tests pass: `cargo test http_methods::tests --lib`
- [x] Integration tests pass: `cargo run --bin integration_test`
- [x] GET requests to existing routes still return 200
- [x] HEAD requests to existing routes still return 200
- [x] POST/PUT/DELETE/PATCH/OPTIONS to existing routes return 405
- [x] 405 responses include `Allow: GET, HEAD` header
- [x] Requests to non-existent routes still return 404 (regardless of method)
- [x] Server starts without errors
- [x] Manual curl testing confirms behavior

---

## Dependencies & Conflicts

- **No new external dependencies**
- **No conflicts** with existing code (only adds logic, doesn't modify existing functions beyond handle_connection)
- **Works with all current routes** (static HTML, CSS, JS files)

---

## Future Enhancements

1. **OPTIONS Method Support**
   - Return 200 OK with `Allow` header for OPTIONS requests
   - Allows clients to query allowed methods programmatically

2. **Custom Error Pages for 405**
   - Load from `pages/method_not_allowed.html` (like 404)
   - Provide richer user experience

3. **Configurable Allowed Methods**
   - Environment variable or config file to control allowed methods
   - Allows stricter or more permissive servers

4. **Method Logging**
   - Log rejected methods separately from accepted ones
   - Helps detect client misconfiguration or attacks

---

## References

- [RFC 7231 Section 6.5.5 - Method Not Allowed](https://tools.ietf.org/html/rfc7231#section-6.5.5)
- [MDN: HTTP 405 Method Not Allowed](https://developer.mozilla.org/en-US/docs/Web/HTTP/Status/405)
- [rcomm Architecture (CLAUDE.md)](../../CLAUDE.md)

---

## Appendix: Complete Code Changes Summary

### File 1: `src/models/http_methods.rs`

**Add** after `http_method_from_string()` function:

```rust
pub fn is_method_allowed_on_static_route(method: &HttpMethods) -> bool {
    matches!(method, HttpMethods::GET | HttpMethods::HEAD)
}

#[cfg(test)]
mod test_static_route_methods {
    use super::*;

    #[test]
    fn get_is_allowed() {
        assert!(is_method_allowed_on_static_route(&HttpMethods::GET));
    }

    #[test]
    fn head_is_allowed() {
        assert!(is_method_allowed_on_static_route(&HttpMethods::HEAD));
    }

    #[test]
    fn post_is_not_allowed() {
        assert!(!is_method_allowed_on_static_route(&HttpMethods::POST));
    }

    #[test]
    fn put_is_not_allowed() {
        assert!(!is_method_allowed_on_static_route(&HttpMethods::PUT));
    }

    #[test]
    fn delete_is_not_allowed() {
        assert!(!is_method_allowed_on_static_route(&HttpMethods::DELETE));
    }

    #[test]
    fn patch_is_not_allowed() {
        assert!(!is_method_allowed_on_static_route(&HttpMethods::PATCH));
    }
}
```

### File 2: `src/main.rs`

**Import** at the top:
```rust
use rcomm::models::http_methods::is_method_allowed_on_static_route;
```

**Replace** the route/response selection logic (lines 62–68):

```rust
let (mut response, filename) = if routes.contains_key(&clean_target) {
    if is_method_allowed_on_static_route(&http_request.method) {
        (HttpResponse::build(String::from("HTTP/1.1"), 200),
            routes.get(&clean_target).unwrap().to_str().unwrap())
    } else {
        let mut response = HttpResponse::build(String::from("HTTP/1.1"), 405);
        response.add_header("Allow".to_string(), "GET, HEAD".to_string());
        response.add_body("Method Not Allowed".as_bytes().to_vec());
        println!("Response: {response}");
        stream.write_all(&response.as_bytes()).unwrap();
        return;
    }
} else {
    (HttpResponse::build(String::from("HTTP/1.1"), 404),
        "pages/not_found.html")
};
```

### File 3: `src/bin/integration_test.rs`

**Add** 9 test functions after `test_concurrent_requests()`:

```rust
fn test_post_to_existing_route_returns_405(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "POST", "/")?;
    assert_eq_or_err(&resp.status_code, &405, "status")?;
    Ok(())
}

fn test_405_includes_allow_header(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "POST", "/")?;
    assert_eq_or_err(&resp.status_code, &405, "status")?;
    let allow_header = resp.headers.get("allow")
        .ok_or("missing Allow header".to_string())?;
    assert_contains_or_err(allow_header, "GET", "Allow header")?;
    assert_contains_or_err(allow_header, "HEAD", "Allow header")?;
    Ok(())
}

fn test_put_to_existing_route_returns_405(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "PUT", "/")?;
    assert_eq_or_err(&resp.status_code, &405, "status")?;
    Ok(())
}

fn test_delete_to_existing_route_returns_405(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "DELETE", "/")?;
    assert_eq_or_err(&resp.status_code, &405, "status")?;
    Ok(())
}

fn test_patch_to_existing_route_returns_405(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "PATCH", "/")?;
    assert_eq_or_err(&resp.status_code, &405, "status")?;
    Ok(())
}

fn test_head_to_existing_route_returns_200(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "HEAD", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    Ok(())
}

fn test_post_to_static_file_returns_405(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "POST", "/index.css")?;
    assert_eq_or_err(&resp.status_code, &405, "status")?;
    Ok(())
}

fn test_post_to_nested_route_returns_405(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "POST", "/howdy")?;
    assert_eq_or_err(&resp.status_code, &405, "status")?;
    Ok(())
}

fn test_post_to_nonexistent_route_returns_404(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "POST", "/does-not-exist")?;
    assert_eq_or_err(&resp.status_code, &404, "status")?;
    Ok(())
}
```

**Add** test registrations in `main()` (after existing test registrations):

```rust
tests.push(run_test("POST to existing route returns 405", || {
    test_post_to_existing_route_returns_405(&addr)
}));
tests.push(run_test("405 includes Allow header", || {
    test_405_includes_allow_header(&addr)
}));
tests.push(run_test("PUT to existing route returns 405", || {
    test_put_to_existing_route_returns_405(&addr)
}));
tests.push(run_test("DELETE to existing route returns 405", || {
    test_delete_to_existing_route_returns_405(&addr)
}));
tests.push(run_test("PATCH to existing route returns 405", || {
    test_patch_to_existing_route_returns_405(&addr)
}));
tests.push(run_test("HEAD to existing route returns 200", || {
    test_head_to_existing_route_returns_200(&addr)
}));
tests.push(run_test("POST to static file returns 405", || {
    test_post_to_static_file_returns_405(&addr)
}));
tests.push(run_test("POST to nested route returns 405", || {
    test_post_to_nested_route_returns_405(&addr)
}));
tests.push(run_test("POST to nonexistent route returns 404", || {
    test_post_to_nonexistent_route_returns_404(&addr)
}));
```

---

## Notes for Implementation

1. **Order matters**: Check route existence → Check method → Serve or reject
2. **Early return**: Exit immediately after sending 405 to avoid the file-read flow
3. **Allow header**: Required by spec; lists all allowed methods for the resource
4. **Response body**: Simple text is fine for a static server; no need for HTML
5. **Logging**: Existing println!() statements will show 405 responses
6. **No new dependencies**: Uses existing HttpResponse builder pattern

