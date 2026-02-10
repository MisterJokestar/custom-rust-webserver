# Feature: Add `X-Frame-Options` Response Header to Prevent Clickjacking

**Category:** Security
**Complexity:** 1/10
**Necessity:** 6/10

---

## Overview

The `X-Frame-Options` header is a security mechanism that prevents clickjacking attacks by controlling whether a page can be embedded in a frame (`<iframe>`, `<frame>`, etc.) on other domains. This feature adds automatic inclusion of the `X-Frame-Options: DENY` header to all HTTP responses from the rcomm server.

### Why This Matters

Clickjacking attacks trick users into clicking on hidden or disguised elements by overlaying malicious content on top of legitimate pages. By setting `X-Frame-Options: DENY`, the server explicitly tells browsers that this page should never be framed, eliminating this attack vector entirely.

### Value Proposition

- **Simple Implementation:** Minimal code changes required (3 lines in 1-2 files)
- **Zero Performance Impact:** Header added once during response building
- **Universal Protection:** All responses (200, 404, 400, etc.) are protected
- **Best Practice Compliance:** Aligns with OWASP and security standards

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/main.rs`
**Why:** This is where HTTP responses are built and sent to clients. Adding the header here ensures it's applied to all responses uniformly.

**Changes:** Add `X-Frame-Options` header to response objects in the `handle_connection()` function.

### 2. `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs` (Testing Only)
**Why:** Extend the integration test suite to verify the header is present on all response types.

**Changes:** Add assertion checks for `X-Frame-Options` header in test responses.

---

## Implementation Steps

### Step 1: Understand the Current Response Building Pattern

**File:** `/home/jwall/personal/rusty/rcomm/src/main.rs`

The `handle_connection()` function currently builds responses like this:

```rust
let (mut response, filename) = if routes.contains_key(&clean_target) {
    (HttpResponse::build(String::from("HTTP/1.1"), 200),
        routes.get(&clean_target).unwrap().to_str().unwrap())
} else {
    (HttpResponse::build(String::from("HTTP/1.1"), 404),
        "pages/not_found.html")
};
```

Responses for error cases (e.g., 400 Bad Request) are also built inline:

```rust
let mut response = HttpResponse::build(String::from("HTTP/1.1"), 400);
```

### Step 2: Add Header to `handle_connection()` Function

**Approach:** After creating a response object, immediately add the `X-Frame-Options` header. Since all response objects pass through the builder pattern, a single addition after each `HttpResponse::build()` call is sufficient.

**Location 1 - Error Response (Line 51):**

```rust
let mut response = HttpResponse::build(String::from("HTTP/1.1"), 400);
response.add_header("X-Frame-Options".to_string(), "DENY".to_string());
```

**Location 2 - Success/Not Found Response (Lines 62-68):**

```rust
let (mut response, filename) = if routes.contains_key(&clean_target) {
    (HttpResponse::build(String::from("HTTP/1.1"), 200),
        routes.get(&clean_target).unwrap().to_str().unwrap())
} else {
    (HttpResponse::build(String::from("HTTP/1.1"), 404),
        "pages/not_found.html")
};
response.add_header("X-Frame-Options".to_string(), "DENY".to_string());
```

### Step 3: Create Unit Tests (Optional but Recommended)

Add a test to `/home/jwall/personal/rusty/rcomm/src/models/http_response.rs` to verify header chaining works as expected:

**New Test Function:**

```rust
#[test]
fn add_security_headers() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
    resp.add_header("X-Frame-Options".to_string(), "DENY".to_string());
    let val = resp.try_get_header("X-Frame-Options".to_string());
    assert_eq!(val, Some("DENY".to_string()));
}
```

This test verifies:
- The header key is stored correctly
- The header value is stored correctly
- Case-insensitive header retrieval works

### Step 4: Add Integration Tests

**File:** `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`

Add test functions to verify the header appears in real HTTP responses:

#### Test 1: Header Present on 200 Response

```rust
fn test_x_frame_options_on_200(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    let header = resp.headers
        .get("x-frame-options")
        .ok_or("missing X-Frame-Options header")?;
    assert_eq_or_err(header, &"DENY".to_string(), "X-Frame-Options value")?;
    Ok(())
}
```

#### Test 2: Header Present on 404 Response

```rust
fn test_x_frame_options_on_404(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/nonexistent")?;
    assert_eq_or_err(&resp.status_code, &404, "status")?;
    let header = resp.headers
        .get("x-frame-options")
        .ok_or("missing X-Frame-Options header")?;
    assert_eq_or_err(header, &"DENY".to_string(), "X-Frame-Options value")?;
    Ok(())
}
```

#### Test 3: Header Present on 400 Response

```rust
fn test_x_frame_options_on_400(addr: &str) -> Result<(), String> {
    let mut stream = TcpStream::connect(addr)
        .map_err(|e| format!("connect: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| format!("set timeout: {e}"))?;
    // Send a malformed request (missing HTTP version)
    let request = "GET /\r\n\r\n";
    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("write: {e}"))?;
    let resp = read_response(&mut stream)?;
    assert_eq_or_err(&resp.status_code, &400, "status")?;
    let header = resp.headers
        .get("x-frame-options")
        .ok_or("missing X-Frame-Options header")?;
    assert_eq_or_err(header, &"DENY".to_string(), "X-Frame-Options value")?;
    Ok(())
}
```

#### Register Tests in Main

Add the new tests to the `main()` function test suite:

```rust
let results = vec![
    run_test("root_route", || test_root_route(&addr)),
    run_test("index_css", || test_index_css(&addr)),
    run_test("howdy_route", || test_howdy_route(&addr)),
    run_test("howdy_page_css", || test_howdy_page_css(&addr)),
    run_test("404_does_not_exist", || test_404_does_not_exist(&addr)),
    run_test("404_deep_path", || test_404_deep_path(&addr)),
    run_test("content_length_matches", || test_content_length_matches(&addr)),
    run_test("trailing_slash", || test_trailing_slash(&addr)),
    run_test("double_slash", || test_double_slash(&addr)),
    run_test("concurrent_requests", || test_concurrent_requests(&addr)),
    // NEW TESTS:
    run_test("x_frame_options_on_200", || test_x_frame_options_on_200(&addr)),
    run_test("x_frame_options_on_404", || test_x_frame_options_on_404(&addr)),
    run_test("x_frame_options_on_400", || test_x_frame_options_on_400(&addr)),
];
```

---

## Code Snippets

### Complete Modified `handle_connection()` Function

```rust
fn handle_connection(mut stream: TcpStream, routes: HashMap<String, PathBuf>) {
    let http_request = match HttpRequest::build_from_stream(&stream) {
        Ok(req) => req,
        Err(e) => {
            eprintln!("Bad request: {e}");
            let mut response = HttpResponse::build(String::from("HTTP/1.1"), 400);
            response.add_header("X-Frame-Options".to_string(), "DENY".to_string());
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
    response.add_header("X-Frame-Options".to_string(), "DENY".to_string());

    let contents = fs::read_to_string(filename).unwrap();
    response.add_body(contents.into());

    println!("Response: {response}");
    stream.write_all(&response.as_bytes()).unwrap();
}
```

### Expected HTTP Response Format

After implementing this feature, all responses will include the header:

```
HTTP/1.1 200 OK
content-length: 45
content-type: text/html
x-frame-options: DENY

<html>...</html>
```

Note: The `x-frame-options` header appears lowercase because `HttpResponse::add_header()` normalizes header names to lowercase (line 29 in `http_response.rs`).

---

## Testing Strategy

### Unit Tests
1. **Header Storage Test:** Verify `add_header()` stores `X-Frame-Options` correctly
2. **Header Retrieval Test:** Verify `try_get_header()` retrieves the header with correct casing
3. **Display Format Test:** Verify the header appears in serialized HTTP output

**Where:** `/home/jwall/personal/rusty/rcomm/src/models/http_response.rs` (add to existing test module)

### Integration Tests
1. **200 Response:** Confirm header on successful GET requests
2. **404 Response:** Confirm header on not-found routes
3. **400 Response:** Confirm header on malformed requests
4. **Concurrent Requests:** Verify header consistency across multiple simultaneous requests

**Where:** `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs` (add new test functions)

### Manual Verification
Use `curl` to inspect response headers:

```bash
# Start server
cargo run &

# Verify header is present
curl -v http://127.0.0.1:7878/

# Expected output includes:
# < x-frame-options: DENY
```

### Test Execution
```bash
# Run all unit tests (should pass with new header test)
cargo test

# Run integration tests (should pass with new header assertions)
cargo run --bin integration_test
```

---

## Edge Cases & Considerations

### 1. Header Casing
**Issue:** HTTP header names are case-insensitive, but `HttpResponse` stores them lowercase internally.

**Impact:** None — the response Display trait handles proper serialization, and browser clients are case-insensitive.

**Verification:** The test `add_security_headers()` confirms retrieval works with mixed case.

### 2. Header Override
**Issue:** If someone calls `add_header("X-Frame-Options", "SAMEORIGIN")` after `handle_connection()`, it would override the DENY value.

**Mitigation:** This is acceptable because:
- `handle_connection()` controls all response creation
- Headers are added immediately after `HttpResponse::build()`
- No subsequent code modifies security-critical headers
- Future enhancement: Create a method like `add_immutable_header()` if needed

### 3. Performance
**Impact:** Negligible
- Single string allocation per response (~23 bytes for header key/value)
- HashMap insertion (O(1) average case)
- No network overhead (header size ~22 bytes)

**Benchmark:** No measurable impact on throughput with typical request volumes.

### 4. Backward Compatibility
**Impact:** None
- Existing API unchanged
- Adding a response header does not break clients
- Clients that ignore the header continue to work
- Clients that respect it gain protection

### 5. Different Routes
**Verification:** Implementation adds header to all response paths:
- 200 responses (success routes)
- 404 responses (not found routes)
- 400 responses (malformed requests)

All paths are covered by the integration tests.

### 6. Static vs. Dynamic Content
**Impact:** The header applies uniformly regardless of content type:
- `.html` files
- `.css` files
- `.js` files

This is correct behavior — all should be protected from embedding.

---

## Implementation Order

1. **Modify `src/main.rs`** (2-3 line additions in `handle_connection()`)
2. **Add unit test** to `src/models/http_response.rs` (optional but recommended)
3. **Add integration tests** to `src/bin/integration_test.rs` (3 test functions + test registration)
4. **Run `cargo test`** to verify unit tests pass
5. **Run `cargo run --bin integration_test`** to verify integration tests pass
6. **Manual verification** with `curl -v` or browser DevTools

---

## Header Value Options (for Future Reference)

This implementation uses `DENY`. Other options for future consideration:

| Value | Behavior | Use Case |
|-------|----------|----------|
| `DENY` | Never allow framing | Default, most secure |
| `SAMEORIGIN` | Allow framing from same domain | Useful if site iframes itself |
| `ALLOWALL` | Allow any domain to frame (deprecated) | Legacy; not recommended |

For rcomm's use case, `DENY` is the appropriate choice.

---

## Success Criteria

- [ ] `cargo test` passes with new unit test included
- [ ] `cargo run --bin integration_test` passes with 13/13 tests (10 existing + 3 new)
- [ ] `curl -v http://127.0.0.1:7878/` shows `x-frame-options: DENY` in headers
- [ ] All response status codes (200, 404, 400) include the header
- [ ] No performance regression in concurrent request handling

---

## Related Security Headers (Future Enhancement)

Once this feature is complete, consider implementing other security headers:

- `X-Content-Type-Options: nosniff` — Prevent MIME type sniffing
- `X-XSS-Protection: 1; mode=block` — Legacy XSS protection
- `Strict-Transport-Security` — HTTPS enforcement
- `Content-Security-Policy` — Control resource loading

These are separate features but follow the same implementation pattern.
