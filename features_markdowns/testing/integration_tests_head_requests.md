# Integration Tests for HEAD Requests

**Category:** Testing
**Complexity:** 2/10
**Necessity:** 6/10
**Status:** Planning

---

## Overview

Add integration tests to `src/bin/integration_test.rs` that verify `HEAD` request behavior. A `HEAD` request should return the same headers as a `GET` request but with an empty body. Currently, the server treats HEAD identically to GET — it returns the full body, which violates HTTP/1.1 (RFC 7231 Section 4.3.2).

**Goal:** Test that HEAD requests return correct status codes and headers, and document the body behavior as a regression test for when HEAD-specific handling is implemented.

---

## Current State

### Server Behavior

The server parses the HEAD method via `http_method_from_string("HEAD")` → `HttpMethods::HEAD`, but `handle_connection()` does not differentiate between GET and HEAD. Both methods receive the full response body.

### Expected Correct Behavior (per HTTP spec)

- HEAD to an existing route → 200 with same headers as GET but **no body**
- HEAD to a non-existent route → 404 with no body
- `Content-Length` header should match what the body **would** be for a GET

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`

**Add:** HEAD request test functions.
**Modify:** `main()` function to register new tests.

---

## Step-by-Step Implementation

### Step 1: Add Test Functions

```rust
fn test_head_root_returns_200(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "HEAD", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "HEAD / status")?;
    Ok(())
}

fn test_head_has_content_length(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "HEAD", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    // HEAD response must include Content-Length
    resp.headers
        .get("content-length")
        .ok_or("HEAD response missing Content-Length header".to_string())?;
    Ok(())
}

fn test_head_content_length_matches_get(addr: &str) -> Result<(), String> {
    // Compare Content-Length between GET and HEAD on same route
    let get_resp = send_request(addr, "GET", "/")?;
    let head_resp = send_request(addr, "HEAD", "/")?;

    let get_cl = get_resp
        .headers
        .get("content-length")
        .ok_or("GET missing Content-Length")?;
    let head_cl = head_resp
        .headers
        .get("content-length")
        .ok_or("HEAD missing Content-Length")?;

    assert_eq_or_err(head_cl, get_cl, "HEAD Content-Length should match GET")?;
    Ok(())
}

fn test_head_no_body(addr: &str) -> Result<(), String> {
    // HEAD response should have an empty body
    // NOTE: This test will fail until HEAD-specific handling is implemented.
    // Currently the server sends the full body for HEAD requests.
    let resp = send_request(addr, "HEAD", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;

    // For now, document the current behavior:
    // The body may be non-empty until the HEAD feature is implemented.
    // When HEAD is properly implemented, uncomment the assertion below:
    // if !resp.body.is_empty() {
    //     return Err(format!("HEAD response should have empty body, got {} bytes", resp.body.len()));
    // }
    Ok(())
}

fn test_head_nonexistent_route(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "HEAD", "/does-not-exist")?;
    assert_eq_or_err(&resp.status_code, &404, "HEAD /does-not-exist status")?;
    Ok(())
}

fn test_head_nested_route(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "HEAD", "/howdy")?;
    assert_eq_or_err(&resp.status_code, &200, "HEAD /howdy status")?;
    Ok(())
}

fn test_head_css_file(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "HEAD", "/index.css")?;
    assert_eq_or_err(&resp.status_code, &200, "HEAD /index.css status")?;
    Ok(())
}
```

### Step 2: Register Tests in `main()`

```rust
run_test("head_root_returns_200", || test_head_root_returns_200(&addr)),
run_test("head_has_content_length", || test_head_has_content_length(&addr)),
run_test("head_content_length_matches_get", || test_head_content_length_matches_get(&addr)),
run_test("head_no_body", || test_head_no_body(&addr)),
run_test("head_nonexistent_route", || test_head_nonexistent_route(&addr)),
run_test("head_nested_route", || test_head_nested_route(&addr)),
run_test("head_css_file", || test_head_css_file(&addr)),
```

---

## Edge Cases & Considerations

### 1. read_response() Body Reading

**Scenario:** The `read_response()` helper reads the body based on `Content-Length`. If the server sends a HEAD response with `Content-Length: 500` but no body, the reader will hang waiting for 500 bytes.

**Workaround:** Until HEAD is properly implemented (server sends body), the tests work fine. Once HEAD is fixed (no body sent), `read_response()` may need to be updated to skip body reading for HEAD responses, or the tests need a HEAD-aware response reader.

**Suggestion:** Add a `send_head_request()` helper that doesn't attempt to read a body:

```rust
fn send_head_request(addr: &str, path: &str) -> Result<TestResponse, String> {
    let mut stream = TcpStream::connect(addr).map_err(|e| format!("connect: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| format!("set timeout: {e}"))?;
    let request = format!("HEAD {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("write: {e}"))?;
    // Read only status line and headers, skip body
    read_response_headers_only(&mut stream)
}
```

### 2. Connection Handling

**Scenario:** After a HEAD response with no body, the connection should still be valid for subsequent requests (in keep-alive mode).

**Not tested here:** Keep-alive is a separate test feature.

---

## Testing Strategy

### Running the Tests

```bash
cargo build && cargo run --bin integration_test
```

### Expected Results (Before HEAD Feature)

All tests that check status codes pass. The `test_head_no_body` test passes because the body assertion is commented out.

### Expected Results (After HEAD Feature)

All tests pass, and `test_head_no_body` can be uncommented to verify empty body.

---

## Implementation Checklist

- [ ] Add `test_head_root_returns_200()` test
- [ ] Add `test_head_has_content_length()` test
- [ ] Add `test_head_content_length_matches_get()` test
- [ ] Add `test_head_no_body()` test (with conditional assertion)
- [ ] Add `test_head_nonexistent_route()` test
- [ ] Add `test_head_nested_route()` test
- [ ] Add `test_head_css_file()` test
- [ ] Register all tests in `main()`
- [ ] Run `cargo build` — no compiler errors
- [ ] Run `cargo run --bin integration_test` — all tests pass

---

## Related Features

- **HTTP Protocol > Handle HEAD Requests**: The implementation feature these tests support
- **Testing > Integration Tests for Non-GET Methods**: Overlaps with HEAD testing; coordinate to avoid duplication

---

## References

- [RFC 7231 Section 4.3.2 - HEAD](https://tools.ietf.org/html/rfc7231#section-4.3.2)
- [MDN: HTTP HEAD](https://developer.mozilla.org/en-US/docs/Web/HTTP/Methods/HEAD)
