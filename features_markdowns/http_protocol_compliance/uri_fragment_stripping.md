# URI Fragment Stripping Implementation Plan

## Overview

This plan details the implementation of URI fragment parsing and stripping in rcomm's HTTP request handling. URI fragments (the portion of a URI after `#`) should be parsed and removed from the request target before routing, as per HTTP/1.1 specification. Fragments are client-side only and never transmitted to the server in the request line.

### Context

According to RFC 3986 Section 3.5 and HTTP/1.1 (RFC 7230), fragments are client-side anchors used for in-document navigation and should not be sent to the server. However, if a client mistakenly includes a fragment in the request line, the server should strip it before processing.

### Example Behavior

- Request: `GET /page.html#section HTTP/1.1` → Processed as: `GET /page.html HTTP/1.1`
- Request: `GET /docs/intro#overview HTTP/1.1` → Processed as: `GET /docs/intro HTTP/1.1`
- Request: `GET /?id=123#results HTTP/1.1` → Processed as: `GET /?id=123 HTTP/1.1`

### Rationale

1. **HTTP Compliance** — Ensures proper handling according to HTTP standards
2. **Robustness** — Gracefully handles non-standard clients that may include fragments
3. **Routing Accuracy** — Prevents fragment characters from interfering with route matching
4. **Minimal Complexity** — Simple string operation (split at `#`) with no performance impact

---

## Files to Modify

### Primary

1. **`src/models/http_request.rs`**
   - Add a new method `strip_fragment()` to parse and remove fragments from target strings
   - Call this method in `build_from_stream()` after parsing the target from the request line
   - Add unit tests for fragment stripping logic

### Secondary

2. **`src/main.rs`** (Optional documentation/review)
   - Review `handle_connection()` and `clean_route()` to ensure compatibility
   - Add comment documenting that fragment stripping occurs in HTTP request parsing

---

## Step-by-Step Implementation

### Step 1: Add Fragment Stripping Method to HttpRequest

**Location:** `src/models/http_request.rs`

Add a private helper function to strip fragments from a URI target:

```rust
/// Strips the fragment identifier (everything after '#') from a URI target.
///
/// # Examples
///
/// ```ignore
/// assert_eq!(strip_uri_fragment("/page.html#section"), "/page.html");
/// assert_eq!(strip_uri_fragment("/docs?id=1#top"), "/docs?id=1");
/// assert_eq!(strip_uri_fragment("/path"), "/path");
/// ```
fn strip_uri_fragment(target: &str) -> String {
    if let Some(fragment_pos) = target.find('#') {
        target[..fragment_pos].to_string()
    } else {
        target.to_string()
    }
}
```

**Placement:** Add this function inside the `impl HttpRequest` block or as a module-level function before the `impl` block.

**Rationale:**
- Uses `find('#')` to locate the fragment delimiter
- Returns the substring before the `#` if found
- Returns the original target unmodified if no fragment exists
- Simple, efficient O(n) operation that scans the string once

---

### Step 2: Integrate Fragment Stripping into Request Parsing

**Location:** `src/models/http_request.rs` in `build_from_stream()` method

Modify line 66 to strip fragments after parsing the target:

```rust
// Before (current code):
let target = iter.next().ok_or(HttpParseError::MalformedRequestLine)?.to_string();

// After (with fragment stripping):
let target_raw = iter.next().ok_or(HttpParseError::MalformedRequestLine)?;
let target = strip_uri_fragment(target_raw);
```

**Placement:** Lines 64-68 in `build_from_stream()`

**Implementation Detail:**
```rust
let mut iter = line.split_whitespace();
let method_str = iter.next().ok_or(HttpParseError::MalformedRequestLine)?;
let method = http_method_from_string(method_str).ok_or(HttpParseError::MalformedRequestLine)?;
let target_raw = iter.next().ok_or(HttpParseError::MalformedRequestLine)?;
let target = strip_uri_fragment(target_raw);
let version = iter.next().ok_or(HttpParseError::MalformedRequestLine)?.to_string();
```

**Rationale:**
- Fragment stripping happens immediately after parsing the request line
- The parsed target is automatically normalized before being stored in the `HttpRequest` struct
- All downstream code (`handle_connection()`, `clean_route()`) automatically benefits from the normalization
- No changes required elsewhere in the codebase

---

### Step 3: Add Unit Tests

**Location:** `src/models/http_request.rs` in the `tests` module (after line 407)

Add comprehensive tests for fragment stripping behavior:

```rust
#[test]
fn strip_uri_fragment_removes_fragment_from_simple_path() {
    let target = "/page.html#section";
    let stripped = strip_uri_fragment(target);
    assert_eq!(stripped, "/page.html");
}

#[test]
fn strip_uri_fragment_preserves_query_string() {
    let target = "/search?q=test#results";
    let stripped = strip_uri_fragment(target);
    assert_eq!(stripped, "/search?q=test");
}

#[test]
fn strip_uri_fragment_handles_path_without_fragment() {
    let target = "/path/to/resource";
    let stripped = strip_uri_fragment(target);
    assert_eq!(stripped, "/path/to/resource");
}

#[test]
fn strip_uri_fragment_handles_root_path() {
    let target = "/#section";
    let stripped = strip_uri_fragment(target);
    assert_eq!(stripped, "/");
}

#[test]
fn strip_uri_fragment_handles_empty_fragment() {
    let target = "/page.html#";
    let stripped = strip_uri_fragment(target);
    assert_eq!(stripped, "/page.html");
}

#[test]
fn strip_uri_fragment_handles_complex_fragment() {
    let target = "/docs/api?version=2#method-getData";
    let stripped = strip_uri_fragment(target);
    assert_eq!(stripped, "/docs/api?version=2");
}

#[test]
fn build_from_stream_strips_fragment_from_request_target() {
    use std::io::Write;
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = std::thread::spawn(move || {
        let mut client = std::net::TcpStream::connect(addr).unwrap();
        client
            .write_all(b"GET /hello#world HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
    });

    let (stream, _) = listener.accept().unwrap();
    let req = HttpRequest::build_from_stream(&stream).unwrap();

    assert_eq!(req.target, "/hello");
    handle.join().unwrap();
}

#[test]
fn build_from_stream_preserves_query_and_removes_fragment() {
    use std::io::Write;
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = std::thread::spawn(move || {
        let mut client = std::net::TcpStream::connect(addr).unwrap();
        client
            .write_all(b"GET /search?q=rust#top HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
    });

    let (stream, _) = listener.accept().unwrap();
    let req = HttpRequest::build_from_stream(&stream).unwrap();

    assert_eq!(req.target, "/search?q=rust");
    handle.join().unwrap();
}
```

**Placement:** Add these tests to the existing `tests` module before the final closing brace of the test module.

**Test Coverage:**
- Simple fragment removal
- Fragment preservation of query strings
- Paths without fragments (no-op)
- Edge cases: root path, empty fragment
- Complex fragments with special characters
- Integration test: stream parsing with fragment
- Integration test: stream parsing with query string and fragment

---

## Implementation Checklist

- [ ] Add `strip_uri_fragment()` function to `src/models/http_request.rs`
- [ ] Modify `build_from_stream()` to call `strip_uri_fragment()` on parsed target
- [ ] Add 8 new unit tests to `src/models/http_request.rs`
- [ ] Run `cargo test` to ensure all 42 tests pass (original 34 + 8 new)
- [ ] Run `cargo run --bin integration_test` to validate end-to-end behavior
- [ ] Verify `cargo build` completes without warnings
- [ ] Test manually with curl: `curl "http://127.0.0.1:7878/index.html#section"`
- [ ] Review changes for code style consistency

---

## Testing Strategy

### Unit Tests

All 8 unit tests above cover:

1. **Basic Fragment Stripping** — Removes `#section` from `/page.html#section`
2. **Query String Preservation** — Keeps query params when stripping fragment
3. **No-Op Cases** — Unchanged paths without fragments
4. **Edge Cases** — Root path, empty fragment, complex fragments
5. **Integration with Parsing** — Fragment stripping via `build_from_stream()`

### Integration Tests

Add a test to `src/bin/integration_test.rs` (optional):

```rust
fn test_request_with_fragment() -> TestResult {
    let response = http_get("/page#section");
    assert_response_status(&response, "200")?;
    assert_response_body_contains(&response, "<!DOCTYPE html>")?;
    TestResult::Pass
}
```

**Expected Behavior:** Request to `/page#section` should return the same response as `/page` (200 OK with page content).

### Manual Testing

```bash
# Start the server
cargo run &

# Test with curl
curl "http://127.0.0.1:7878/index.html#top"
curl -v "http://127.0.0.1:7878/index.html#top"  # View headers

# Expected: 200 OK with index.html content (fragment never sent to server)
```

### Regression Testing

Run full test suite to ensure no regressions:

```bash
cargo test                            # All unit tests (expect 42)
cargo run --bin integration_test      # All integration tests
```

---

## Edge Cases & Considerations

### Handled Cases

1. **Multiple `#` Characters**
   - Target: `/page#section#subsection`
   - Behavior: Removes everything after **first** `#`
   - Result: `/page`
   - Rationale: RFC 3986 defines fragment as everything after the first `#`

2. **Fragment with Special Characters**
   - Target: `/page#sec-tion_123.part`
   - Behavior: Strips entire fragment including special chars
   - Result: `/page`

3. **Fragment on Root Path**
   - Target: `/#top`
   - Behavior: Returns `/`
   - Result: `/`

4. **Query String with Fragment**
   - Target: `/search?q=test&id=5#results`
   - Behavior: Preserves query string, removes fragment
   - Result: `/search?q=test&id=5`

5. **Fragment Containing Query String Syntax**
   - Target: `/page#?id=5` (non-standard but possible)
   - Behavior: Removes entire `#?id=5` fragment
   - Result: `/page`

6. **Empty Fragment**
   - Target: `/page#`
   - Behavior: Removes the `#` and empty fragment
   - Result: `/page`

### Non-Cases (Out of Scope)

- **Fragment Decoding** — No URL decoding of fragment content (already removed)
- **Fragment Validation** — No validation of fragment syntax (already removed)
- **Percent-Encoding** — No special handling of `%23` (encoded `#`) — this is intentional as RFC 3986 distinguishes encoded vs. unencoded

---

## Performance Impact

- **Time Complexity:** O(n) where n = length of target string (single `find()` operation)
- **Space Complexity:** O(n) for creating a new String in the `to_string()` call
- **Optimization Opportunity:** Could avoid allocation if target contains no `#` (see below)

### Optional: Zero-Copy Optimization

If performance becomes critical, use `str` slices instead of owned Strings:

```rust
fn strip_uri_fragment(target: &str) -> &str {
    if let Some(fragment_pos) = target.find('#') {
        &target[..fragment_pos]
    } else {
        target
    }
}
```

**Trade-off:** Requires changing the `target` field from `String` to `&str` with lifetime annotations (more invasive, unnecessary for current scope).

---

## Backwards Compatibility

- **No Breaking Changes** — Fragment stripping is transparent to callers
- **Route Matching Unaffected** — `clean_route()` in `src/main.rs` operates on already-normalized targets
- **Existing Tests** — All 34 existing tests continue to pass
- **New Behavior** — Strictly improves HTTP compliance; no valid HTTP/1.1 requests are broken

---

## Code Style & Conventions

- Follow existing pattern of using `find()` + slice operations (consistent with Rust practices)
- Maintain builder pattern consistency with existing methods (`build()`, `add_header()`, etc.)
- Use inline documentation comment style matching existing code in `http_request.rs`
- Keep function signature simple: `fn strip_uri_fragment(target: &str) -> String`

---

## Related Work / Dependencies

- **No External Crates Required** — Uses only `std::string::String` methods
- **No Changes to Other Modules** — Self-contained in `http_request.rs`
- **Compatible with `clean_route()` in `src/main.rs`** — Operates independently on targets before routing

---

## Future Enhancements

1. **URI Normalization** — Extend to handle URL-encoded characters (`%20`, `%3F`, etc.)
2. **Query String Parsing** — If query parameters need explicit access, add a `try_get_query_param()` method
3. **Fragment Preservation** — If use case arises, add a method to extract fragment for logging/debugging
4. **Percent-Encoding Decoding** — Add `decode_uri()` helper if needed for file serving with spaces/special chars

---

## References

- **RFC 3986 Section 3.5** — Uniform Resource Identifier (URI) Generic Syntax — Fragment Identifier
- **RFC 7230 Section 5.3** — HTTP/1.1 Message Syntax and Routing — Request Target
- **RFC 7231 Section 4.3** — HTTP/1.1 Semantics and Content — Request Methods

---

## Summary

This is a low-complexity, high-value change that:

1. Improves HTTP/1.1 compliance
2. Handles edge cases gracefully
3. Requires changes to only one file (`src/models/http_request.rs`)
4. Adds 8 comprehensive unit tests
5. Has zero performance impact on the happy path (no fragment)
6. Requires no external dependencies

The implementation is straightforward: add a single `strip_uri_fragment()` function and integrate it into the request parsing pipeline, with comprehensive test coverage.
