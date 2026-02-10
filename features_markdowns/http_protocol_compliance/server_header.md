# Implementation Plan: Add `Server` Response Header

## Overview

This feature adds automatic inclusion of a `Server` response header to all HTTP responses sent by the rcomm server. The header will identify the server software and version (e.g., `Server: rcomm/0.1.0`), which is a standard HTTP practice for protocol compliance and server identification.

This is a minimal change that improves HTTP protocol compliance by automatically injecting the server header into every response without requiring manual addition at each callsite. The implementation centralizes the server identification in the codebase.

**Complexity:** 1/10 (Straightforward, single responsibility)
**Necessity:** 3/10 (Nice-to-have for protocol compliance, not critical)

---

## Implementation Details

### Option A: Centralized Initialization (Recommended)

Add the `Server` header automatically when `HttpResponse` is built. This ensures the header is always present without requiring changes to every response creation site.

#### Files to Modify

1. **`src/models/http_response.rs`** — Add server header initialization
2. **`src/main.rs`** — Pass server identifier constant (optional, for clarity)
3. **`src/lib.rs`** — Export server version constant (optional)

---

### Step-by-Step Implementation

#### Step 1: Define Server Version Constant

**File:** `/home/jwall/personal/rusty/rcomm/src/lib.rs`

At the top of the file, add a public constant for the server version:

```rust
pub const SERVER_VERSION: &str = "rcomm/0.1.0";
```

This constant:
- Centralizes version management
- Makes it easy to update in one place
- Can be used in documentation, logging, etc.
- Follows semantic versioning format

**Rationale:** By defining the server identifier as a constant in `lib.rs`, it becomes part of the library's public API and can be accessed from both `main.rs` and tests.

#### Step 2: Auto-initialize Server Header in `HttpResponse::build()`

**File:** `/home/jwall/personal/rusty/rcomm/src/models/http_response.rs`

Modify the `build()` method to automatically add the `Server` header:

```rust
impl HttpResponse {
    pub fn build(version: String, code: u16) -> HttpResponse {
        let mut headers = HashMap::<String, String>::new();
        let phrase = get_status_phrase(code);

        // Automatically add Server header to all responses
        headers.insert("server".to_string(), "rcomm/0.1.0".to_string());

        HttpResponse {
            version,
            status_code: code,
            status_phrase: phrase,
            headers,
            body: None,
        }
    }
    // ... rest of implementation
}
```

**Alternative with constant import:**

```rust
use crate::SERVER_VERSION;

impl HttpResponse {
    pub fn build(version: String, code: u16) -> HttpResponse {
        let mut headers = HashMap::<String, String>::new();
        let phrase = get_status_phrase(code);

        headers.insert("server".to_string(), SERVER_VERSION.to_string());

        HttpResponse {
            version,
            status_code: code,
            status_phrase: phrase,
            headers,
            body: None,
        }
    }
    // ...
}
```

**Key Points:**
- Header is stored in lowercase (`"server"`) to match existing header storage convention
- Header is set before any other headers are added
- Consistent with how `Content-Length` is auto-set in `add_body()`
- Works with the builder pattern — users can still call `add_header()` to override if needed

#### Step 3: Update Unit Tests

**File:** `/home/jwall/personal/rusty/rcomm/src/models/http_response.rs`

Add a new test to verify the automatic header injection:

```rust
#[test]
fn build_auto_sets_server_header() {
    let resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
    assert_eq!(
        resp.try_get_header("Server".to_string()),
        Some("rcomm/0.1.0".to_string())
    );
}
```

Update the `display_formats_status_line_and_headers()` test to verify the automatic header:

```rust
#[test]
fn display_formats_status_line_and_headers() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
    resp.add_header("X-Custom".to_string(), "value".to_string());
    let output = format!("{resp}");
    assert!(output.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(output.contains("server: rcomm/0.1.0\r\n"));
    assert!(output.contains("x-custom: value\r\n"));
    assert!(output.ends_with("\r\n"));
}
```

Add a test for overriding the server header (edge case):

```rust
#[test]
fn server_header_can_be_overridden() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
    resp.add_header("Server".to_string(), "custom-server".to_string());
    assert_eq!(
        resp.try_get_header("Server".to_string()),
        Some("custom-server".to_string())
    );
}
```

**Rationale:** Tests verify:
1. The header is automatically added on every response
2. The exact format matches expectations
3. Users can override if needed (flexibility)

---

### Option B: Manual Addition in main.rs (Not Recommended)

If centralization in `HttpResponse::build()` is undesirable, headers could be added at each creation site:

```rust
// In handle_connection()
let mut response = HttpResponse::build(String::from("HTTP/1.1"), 200);
response.add_header("Server".to_string(), "rcomm/0.1.0".to_string());
```

**Problems:**
- Requires code repetition in multiple places (error-prone)
- Violates DRY principle
- Inconsistent if headers forgotten at some call sites

**Conclusion:** Option A is strongly preferred.

---

## Testing Strategy

### Unit Tests (in `http_response.rs`)

1. **Automatic Header Injection:**
   - Verify header is present on all newly built responses
   - Test with different status codes (200, 404, 500)
   - Verify header format matches RFC expectations

2. **Header Value Correctness:**
   - Value is exactly `"rcomm/0.1.0"`
   - Header name is lowercase when stored
   - Header is case-insensitive on retrieval

3. **Override Capability:**
   - Allow users to override the header via `add_header()`
   - Verify override works for all status codes

4. **Display Output:**
   - Verify serialized HTTP response contains the header
   - Verify header appears before body content
   - Verify proper CRLF formatting

### Integration Tests (in `src/bin/integration_test.rs`)

Add end-to-end tests to verify the header is present in actual HTTP responses:

```rust
#[test]
fn response_contains_server_header() {
    let (mut server, port) = spawn_server();

    let response = make_request(&format!("GET / HTTP/1.1\r\nHost: localhost:{port}\r\n\r\n"), port);
    assert!(response.contains("Server: rcomm/0.1.0"));

    server.stop();
}
```

Verify the header is present on:
- 200 OK responses
- 404 Not Found responses
- 400 Bad Request responses (error handling path in `handle_connection()`)

### Manual Testing

```bash
# Build and run the server
cargo run &

# Make a request and inspect headers
curl -i http://127.0.0.1:7878/

# Expected output includes:
# Server: rcomm/0.1.0
```

---

## Edge Cases & Considerations

### 1. Header Override Behavior

**Case:** User calls `add_header("Server", "custom")` after building

**Current Behavior:** HashMap insertion would overwrite the default value

**Expected Behavior:** Allow override (flexibility for testing/customization)

**Implementation:** No special handling needed — `add_header()` already overwrites via `HashMap::insert()`

### 2. Case Insensitivity

**Case:** User retrieves header via `try_get_header("SERVER")` or `try_get_header("Server")`

**Expected Behavior:** Both should return `Some("rcomm/0.1.0")`

**Implementation:** Already handled by `.to_lowercase()` in both `add_header()` and `try_get_header()`

**Test:**
```rust
#[test]
fn server_header_case_insensitive() {
    let resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
    assert_eq!(resp.try_get_header("server".to_string()), Some("rcomm/0.1.0".to_string()));
    assert_eq!(resp.try_get_header("SERVER".to_string()), Some("rcomm/0.1.0".to_string()));
    assert_eq!(resp.try_get_header("Server".to_string()), Some("rcomm/0.1.0".to_string()));
}
```

### 3. Version Updates

**Case:** Server version changes from 0.1.0 to 0.2.0

**Implementation:** Single constant update in `src/lib.rs` or hardcoded string in `http_response.rs`

**Recommendation:** Use a constant in `lib.rs` for maintainability:
```rust
pub const SERVER_VERSION: &str = "rcomm/0.2.0";
```

### 4. Performance Impact

**Analysis:**
- Single string insertion into HashMap at response creation (negligible cost)
- No per-request allocation difference
- Header stored once, serialized multiple times (efficient)

**Conclusion:** Negligible performance impact

### 5. HTTP Specification Compliance

**RFC 7231 Section 7.4.2** states the `Server` header:
- Should not be transmitted if the server software information is secret
- Can contain multiple tokens (e.g., `"rcomm/0.1.0 Rust/1.70"`)
- Is optional but recommended for identification purposes

**Our Implementation:** Follows RFC by including a simple, single-token identifier

### 6. Error Path (400 Bad Request)

**Location:** `handle_connection()` at line 51 in `main.rs`

**Current Code:**
```rust
let mut response = HttpResponse::build(String::from("HTTP/1.1"), 400);
```

**Behavior After Change:** The header will be automatically included in the 400 response as well ✓

**Test:** Verify header is present on malformed requests

### 7. Empty Version String Edge Case

**Not Applicable:** The constant is hardcoded and verified at compile time

---

## Implementation Checklist

- [ ] Add `SERVER_VERSION` constant to `src/lib.rs` (optional but recommended)
- [ ] Modify `HttpResponse::build()` in `src/models/http_response.rs` to initialize server header
- [ ] Add unit test: `build_auto_sets_server_header()`
- [ ] Add unit test: `server_header_can_be_overridden()`
- [ ] Add unit test: `server_header_case_insensitive()`
- [ ] Update `display_formats_status_line_and_headers()` test to verify automatic header
- [ ] Run `cargo test` to verify all existing tests still pass
- [ ] Add integration test to verify header in actual HTTP responses
- [ ] Run integration tests: `cargo run --bin integration_test`
- [ ] Manual testing with `curl -i`
- [ ] Verify header appears on 200, 404, and 400 responses
- [ ] Update CLAUDE.md if architectural notes need updating

---

## Code Summary

### Minimal Implementation (No Constant)

**Change in `src/models/http_response.rs`:**

```rust
pub fn build(version: String, code: u16) -> HttpResponse {
    let mut headers = HashMap::<String, String>::new();
    let phrase = get_status_phrase(code);
    headers.insert("server".to_string(), "rcomm/0.1.0".to_string());

    HttpResponse {
        version,
        status_code: code,
        status_phrase: phrase,
        headers,
        body: None,
    }
}
```

### Recommended Implementation (With Constant)

**Change in `src/lib.rs`:**

Add at the top of the file:
```rust
pub const SERVER_VERSION: &str = "rcomm/0.1.0";
```

**Change in `src/models/http_response.rs`:**

Add import:
```rust
use crate::SERVER_VERSION;
```

Update `build()`:
```rust
pub fn build(version: String, code: u16) -> HttpResponse {
    let mut headers = HashMap::<String, String>::new();
    let phrase = get_status_phrase(code);
    headers.insert("server".to_string(), SERVER_VERSION.to_string());

    HttpResponse {
        version,
        status_code: code,
        status_phrase: phrase,
        headers,
        body: None,
    }
}
```

---

## Expected Behavior After Implementation

### Before Implementation

```
GET / HTTP/1.1
Host: localhost:7878

HTTP/1.1 200 OK
content-length: 42

<html>...</html>
```

### After Implementation

```
GET / HTTP/1.1
Host: localhost:7878

HTTP/1.1 200 OK
server: rcomm/0.1.0
content-length: 42

<html>...</html>
```

---

## Rollback Plan

If this feature needs to be removed:
1. Revert the `build()` method to not initialize the server header
2. Remove the constant from `lib.rs` (if added)
3. Remove or revert the unit tests added
4. Run test suite to verify no regressions

The change is completely reversible with no database migrations or configuration needed.

---

## Future Enhancements

1. **Configurable Server String:** Allow environment variable or config file override
   - `RCOMM_SERVER_STRING=rcomm/custom`

2. **Dynamic Version from Cargo.toml:** Read version at compile time using `env!()` macro
   - `pub const SERVER_VERSION: &str = concat!("rcomm/", env!("CARGO_PKG_VERSION"));`

3. **Extended Information:** Include Rust version or OS info
   - `Server: rcomm/0.1.0 (Rust 1.70; Linux)`

4. **Server Banner Obfuscation:** Option to disable or simplify the header for security
   - `Server: rcomm` (version hidden)

---

## Summary

This is a low-complexity, high-value feature that improves HTTP protocol compliance by automatically including a server identification header in all responses. The implementation is centralized in the `HttpResponse::build()` method, ensuring consistency without code duplication. The change is backward-compatible and fully reversible.

**Recommended Approach:** Use the constant-based implementation (Option A with `SERVER_VERSION` in `lib.rs`) for maintainability and future extensibility.
