# Integration Tests for Non-GET Methods (POST, PUT, DELETE Returning 405)

**Category:** Testing
**Complexity:** 2/10
**Necessity:** 6/10
**Status:** Planning

---

## Overview

Add integration tests to `src/bin/integration_test.rs` that verify the server's behavior when receiving HTTP methods other than GET on existing routes. Currently, the server does not check the HTTP method — it serves files for any method (GET, POST, PUT, DELETE, etc.) on valid routes.

**Goal:** Document the current behavior and provide a test foundation for when the `405 Method Not Allowed` feature is implemented.

**Note:** These tests should initially validate the **current** behavior (which may return 200 for any method on valid routes). Once the 405 feature is implemented, the expected status codes should be updated to 405. The test functions themselves act as a specification for correct behavior.

---

## Current State

### Server Behavior (src/main.rs, lines 46-74)

The `handle_connection()` function:
1. Parses the HTTP request (including the method via `HttpRequest::build_from_stream()`)
2. Cleans the route target
3. Looks up the route in the hashmap — **without checking the method**
4. Returns 200 if found, 404 if not

The HTTP method is parsed into `HttpMethods` enum but never validated against allowed methods for static routes.

### Request Parser (src/models/http_methods.rs)

The parser recognizes: GET, HEAD, POST, PUT, DELETE, CONNECT, OPTIONS, TRACE, PATCH. Unrecognized methods cause a parse error → 400 Bad Request.

### What These Tests Validate

1. POST/PUT/DELETE/PATCH to an existing route — should eventually return 405
2. POST/PUT/DELETE to a non-existent route — should return 404
3. HEAD to an existing route — should return 200 (HEAD is always allowed)
4. The `Allow` header presence on 405 responses (once implemented)

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`

**Add:** Non-GET method test functions.
**Modify:** `main()` function to register new tests.

---

## Step-by-Step Implementation

### Step 1: Add Test Functions

These tests are written for the **target behavior** (405 for non-GET/HEAD methods). If the 405 feature has not been implemented yet, temporarily adjust the expected status to 200 and add a `TODO` comment.

```rust
fn test_post_to_existing_route(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "POST", "/")?;
    // TODO: Change to 405 once Method Not Allowed feature is implemented
    // For now, the server returns 200 for any method on valid routes
    assert_eq_or_err(&resp.status_code, &200, "POST / status")?;
    Ok(())
}

fn test_put_to_existing_route(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "PUT", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "PUT / status")?;
    Ok(())
}

fn test_delete_to_existing_route(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "DELETE", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "DELETE / status")?;
    Ok(())
}

fn test_patch_to_existing_route(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "PATCH", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "PATCH / status")?;
    Ok(())
}

fn test_head_to_existing_route(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "HEAD", "/")?;
    // HEAD should always return 200 on existing routes
    assert_eq_or_err(&resp.status_code, &200, "HEAD / status")?;
    Ok(())
}

fn test_post_to_nonexistent_route(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "POST", "/does-not-exist")?;
    // Non-existent routes should return 404 regardless of method
    assert_eq_or_err(&resp.status_code, &404, "POST /does-not-exist status")?;
    Ok(())
}

fn test_delete_to_nested_route(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "DELETE", "/howdy")?;
    assert_eq_or_err(&resp.status_code, &200, "DELETE /howdy status")?;
    Ok(())
}

fn test_post_to_css_file(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "POST", "/index.css")?;
    assert_eq_or_err(&resp.status_code, &200, "POST /index.css status")?;
    Ok(())
}
```

### Step 2: Register Tests in `main()`

Add to the `results` vector in `main()`:

```rust
run_test("post_to_existing_route", || test_post_to_existing_route(&addr)),
run_test("put_to_existing_route", || test_put_to_existing_route(&addr)),
run_test("delete_to_existing_route", || test_delete_to_existing_route(&addr)),
run_test("patch_to_existing_route", || test_patch_to_existing_route(&addr)),
run_test("head_to_existing_route", || test_head_to_existing_route(&addr)),
run_test("post_to_nonexistent_route", || test_post_to_nonexistent_route(&addr)),
run_test("delete_to_nested_route", || test_delete_to_nested_route(&addr)),
run_test("post_to_css_file", || test_post_to_css_file(&addr)),
```

---

## Upgrading Tests After 405 Implementation

Once the `405 Method Not Allowed` feature is implemented, update the expected status codes:

```rust
fn test_post_to_existing_route(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "POST", "/")?;
    assert_eq_or_err(&resp.status_code, &405, "POST / status")?;
    // Verify Allow header is present
    let allow = resp.headers.get("allow")
        .ok_or("missing Allow header on 405".to_string())?;
    assert_contains_or_err(allow, "GET", "Allow header")?;
    assert_contains_or_err(allow, "HEAD", "Allow header")?;
    Ok(())
}
```

---

## Edge Cases & Considerations

### 1. HEAD Request Body

**Scenario:** HEAD to `/` should return headers but no body.

**Current behavior:** The server likely returns the full body for HEAD requests since there's no method-specific handling. This is a separate feature (HEAD request support).

**Test approach:** Only check status code for now; body behavior is covered by the HEAD request feature tests.

### 2. Request Body on POST/PUT

**Scenario:** POST and PUT requests typically include a body.

**Current behavior:** The server ignores request bodies (no body parsing implemented). The `send_request()` helper doesn't send a body.

**Test approach:** Send bodyless POST/PUT. The method itself, not the body, determines the 405.

### 3. OPTIONS Method

**Scenario:** OPTIONS is a valid discovery method per HTTP spec.

**Current behavior:** Parsed like any other method, treated as a normal request.

**Note:** OPTIONS handling is a separate feature. These tests don't cover it.

### 4. Method on Non-Existent Routes

**Scenario:** POST to `/does-not-exist`.

**Decision:** Return 404, not 405. The resource doesn't exist, so the method is irrelevant. Route existence check happens before method validation.

---

## Testing Strategy

### Running the Tests

```bash
cargo build && cargo run --bin integration_test
```

### Expected Results (Before 405 Feature)

All tests should pass with 200 status for valid routes (since the server doesn't validate methods yet). The `test_post_to_nonexistent_route` should return 404.

### Expected Results (After 405 Feature)

POST/PUT/DELETE/PATCH on valid routes return 405. HEAD on valid routes returns 200. POST on non-existent routes returns 404.

---

## Implementation Checklist

- [ ] Add 8 non-GET method test functions
- [ ] Register all tests in `main()`
- [ ] Run `cargo build` — no compiler errors
- [ ] Run `cargo run --bin integration_test` — all tests pass
- [ ] Document TODO comments for 405 upgrade

---

## Related Features

- **HTTP Protocol > Return 405 Method Not Allowed**: The feature these tests support. Once implemented, update expected status codes from 200 to 405.
- **HTTP Protocol > Handle HEAD Requests**: HEAD should return headers only (no body). The `test_head_to_existing_route` test validates the status code but not body behavior.
- **HTTP Protocol > OPTIONS Requests**: OPTIONS should return allowed methods. Not covered here.

---

## References

- [RFC 7231 Section 6.5.5 - 405 Method Not Allowed](https://tools.ietf.org/html/rfc7231#section-6.5.5)
- [MDN: HTTP Request Methods](https://developer.mozilla.org/en-US/docs/Web/HTTP/Methods)
