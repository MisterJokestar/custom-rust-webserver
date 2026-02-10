# Implementation Plan: Add `X-Content-Type-Options: nosniff` Response Header

## Overview

This feature adds automatic inclusion of the `X-Content-Type-Options: nosniff` response header to all HTTP responses sent by the rcomm server. This header is a security mechanism that prevents browsers from performing MIME-type sniffing (also known as content-type sniffing), which can be exploited to serve potentially dangerous content.

The `nosniff` directive instructs browsers to:
- Strictly follow the `Content-Type` header when deciding how to render a response
- Block a response if the content does not match the declared type
- Prevent attackers from disguising malicious scripts as harmless file types (e.g., serving JavaScript with a CSS MIME type)

This is a minimal change that improves security posture by automatically injecting the header into every response without requiring manual addition at each callsite. The implementation centralizes the security header in the codebase.

**Complexity:** 1/10 (Straightforward, single responsibility)
**Necessity:** 7/10 (Important for web security, recommended best practice)

---

## Implementation Details

### Option A: Centralized Initialization (Recommended)

Add the `X-Content-Type-Options` header automatically when `HttpResponse` is built. This ensures the header is always present without requiring changes to every response creation site.

#### Files to Modify

1. **`src/models/http_response.rs`** — Add security header initialization
2. **`src/main.rs`** — No changes required (header added automatically)
3. **`src/lib.rs`** — Export constant (optional, for clarity)

---

### Step-by-Step Implementation

#### Step 1: Define Security Header Constant (Optional but Recommended)

**File:** `/home/jwall/personal/rusty/rcomm/src/lib.rs`

At the top of the file, add a public constant for the security header value:

```rust
pub const X_CONTENT_TYPE_OPTIONS_VALUE: &str = "nosniff";
```

This constant:
- Centralizes the header value management
- Makes it easy to update or adjust in one place
- Can be referenced in documentation
- Follows the pattern established by `SERVER_VERSION`

**Rationale:** By defining the header value as a constant in `lib.rs`, it becomes part of the library's public API and can be accessed from both `main.rs` and tests. This is optional but recommended for maintainability.

#### Step 2: Auto-initialize Security Header in `HttpResponse::build()`

**File:** `/home/jwall/personal/rusty/rcomm/src/models/http_response.rs`

Modify the `build()` method to automatically add the `X-Content-Type-Options` header:

```rust
impl HttpResponse {
    pub fn build(version: String, code: u16) -> HttpResponse {
        let mut headers = HashMap::<String, String>::new();
        let phrase = get_status_phrase(code);

        // Automatically add X-Content-Type-Options header to all responses
        headers.insert("x-content-type-options".to_string(), "nosniff".to_string());

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
use crate::X_CONTENT_TYPE_OPTIONS_VALUE;

impl HttpResponse {
    pub fn build(version: String, code: u16) -> HttpResponse {
        let mut headers = HashMap::<String, String>::new();
        let phrase = get_status_phrase(code);

        headers.insert("x-content-type-options".to_string(), X_CONTENT_TYPE_OPTIONS_VALUE.to_string());

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
- Header is stored in lowercase (`"x-content-type-options"`) to match existing header storage convention
- Header value is exactly `"nosniff"` per the specification (case-sensitive per browser requirements)
- Header is set before any other headers are added
- Consistent with how `Content-Length` is auto-set in `add_body()`
- Works with the builder pattern — users can still call `add_header()` to override if needed

#### Step 3: Update Unit Tests

**File:** `/home/jwall/personal/rusty/rcomm/src/models/http_response.rs`

Add a new test to verify the automatic header injection:

```rust
#[test]
fn build_auto_sets_x_content_type_options_header() {
    let resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
    assert_eq!(
        resp.try_get_header("X-Content-Type-Options".to_string()),
        Some("nosniff".to_string())
    );
}
```

Add a test to verify the header is present across different status codes:

```rust
#[test]
fn x_content_type_options_header_on_all_status_codes() {
    let codes = vec![200, 400, 404, 500];
    for code in codes {
        let resp = HttpResponse::build("HTTP/1.1".to_string(), code);
        assert_eq!(
            resp.try_get_header("X-Content-Type-Options".to_string()),
            Some("nosniff".to_string()),
            "header missing on status code {code}"
        );
    }
}
```

Update the `display_formats_status_line_and_headers()` test to verify the automatic header:

```rust
#[test]
fn display_formats_status_line_and_headers() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
    resp.add_header("Server".to_string(), "rcomm".to_string());
    let output = format!("{resp}");
    assert!(output.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(output.contains("x-content-type-options: nosniff\r\n"));
    assert!(output.contains("server: rcomm\r\n"));
    assert!(output.ends_with("\r\n"));
}
```

Add a test for overriding the header (edge case, though not recommended):

```rust
#[test]
fn x_content_type_options_header_can_be_overridden() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
    resp.add_header("X-Content-Type-Options".to_string(), "".to_string());
    assert_eq!(
        resp.try_get_header("X-Content-Type-Options".to_string()),
        Some("".to_string())
    );
}
```

**Rationale:** Tests verify:
1. The header is automatically added on every response
2. The exact value matches `"nosniff"`
3. The header is present across all HTTP status codes
4. Users can override if needed (flexibility for edge cases)

---

### Option B: Manual Addition in main.rs (Not Recommended)

If centralization in `HttpResponse::build()` is undesirable, headers could be added at each creation site:

```rust
// In handle_connection()
let mut response = HttpResponse::build(String::from("HTTP/1.1"), 200);
response.add_header("X-Content-Type-Options".to_string(), "nosniff".to_string());
```

**Problems:**
- Requires code repetition in multiple places (error-prone)
- Violates DRY principle
- Inconsistent if headers forgotten at some call sites
- Security header may be accidentally omitted

**Conclusion:** Option A is strongly preferred.

---

## Testing Strategy

### Unit Tests (in `http_response.rs`)

1. **Automatic Header Injection:**
   - Verify header is present on all newly built responses
   - Test with different status codes (200, 404, 500, 400)
   - Verify header format matches specification

2. **Header Value Correctness:**
   - Value is exactly `"nosniff"` (case-sensitive)
   - Header name is lowercase when stored
   - Header is case-insensitive on retrieval

3. **Override Capability:**
   - Allow users to override the header via `add_header()` (edge case, not typical)
   - Verify override works for all status codes

4. **Display Output:**
   - Verify serialized HTTP response contains the header
   - Verify header appears in output with proper formatting
   - Verify proper CRLF formatting

### Integration Tests (in `src/bin/integration_test.rs`)

Add end-to-end tests to verify the header is present in actual HTTP responses:

```rust
fn test_root_response_security_header(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;

    // Verify security header is present
    if let Some(header_value) = resp.headers.get("x-content-type-options") {
        if header_value == "nosniff" {
            Ok(())
        } else {
            Err(format!("wrong header value: {header_value}"))
        }
    } else {
        Err("X-Content-Type-Options header not found".to_string())
    }
}

fn test_404_response_security_header(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/nonexistent")?;
    assert_eq_or_err(&resp.status_code, &404, "status")?;

    // Verify security header is present on 404 responses
    if let Some(header_value) = resp.headers.get("x-content-type-options") {
        if header_value == "nosniff" {
            Ok(())
        } else {
            Err(format!("wrong header value: {header_value}"))
        }
    } else {
        Err("X-Content-Type-Options header not found in 404 response".to_string())
    }
}

fn test_bad_request_response_security_header(addr: &str) -> Result<(), String> {
    let mut stream = TcpStream::connect(addr).map_err(|e| format!("connect: {e}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).map_err(|e| format!("timeout: {e}"))?;

    // Send malformed request
    let request = "INVALID REQUEST\r\n\r\n";
    stream.write_all(request.as_bytes()).map_err(|e| format!("write: {e}"))?;

    let resp = read_response(&mut stream)?;
    assert_eq_or_err(&resp.status_code, &400, "status")?;

    // Verify security header is present on 400 responses
    if let Some(header_value) = resp.headers.get("x-content-type-options") {
        if header_value == "nosniff" {
            Ok(())
        } else {
            Err(format!("wrong header value: {header_value}"))
        }
    } else {
        Err("X-Content-Type-Options header not found in 400 response".to_string())
    }
}
```

### Manual Testing

```bash
# Build and run the server
cargo run &

# Make a request and inspect headers
curl -i http://127.0.0.1:7878/

# Expected output includes:
# x-content-type-options: nosniff

# Test against different routes and status codes
curl -i http://127.0.0.1:7878/index.css
curl -i http://127.0.0.1:7878/nonexistent

# All responses should include the header
```

---

## Edge Cases & Considerations

### 1. Header Override Behavior

**Case:** User calls `add_header("X-Content-Type-Options", "")` after building

**Current Behavior:** HashMap insertion would overwrite the default value with an empty string

**Expected Behavior:** Allow override (flexibility for edge cases, though not recommended)

**Implementation:** No special handling needed — `add_header()` already overwrites via `HashMap::insert()`

**Security Note:** Overriding or removing this header is not recommended and should only be done in exceptional circumstances where security has been independently verified.

### 2. Case Insensitivity

**Case:** User retrieves header via `try_get_header("X-CONTENT-TYPE-OPTIONS")` or `try_get_header("x-content-type-options")`

**Expected Behavior:** Both should return `Some("nosniff")`

**Implementation:** Already handled by `.to_lowercase()` in both `add_header()` and `try_get_header()`

**Test:**
```rust
#[test]
fn x_content_type_options_header_case_insensitive() {
    let resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
    assert_eq!(resp.try_get_header("x-content-type-options".to_string()), Some("nosniff".to_string()));
    assert_eq!(resp.try_get_header("X-CONTENT-TYPE-OPTIONS".to_string()), Some("nosniff".to_string()));
    assert_eq!(resp.try_get_header("X-Content-Type-Options".to_string()), Some("nosniff".to_string()));
}
```

### 3. RFC and Browser Specification Compliance

**Standard:** Originally introduced by Internet Explorer, now part of most modern browser security standards

**RFC Reference:** Not formally specified in an RFC, but widely documented in:
- OWASP guidance
- MDN Web Docs
- Browser documentation

**Our Implementation:** Follows best practices by setting the value to `"nosniff"` (the only defined value by current browsers)

**Browser Behavior:**
- Chrome/Chromium: Enforces on MIME type mismatches
- Firefox: Enforces on HTML/XML content type mismatches
- Safari: Enforces based on browser security policy
- Edge: Enforces consistently with Chrome

### 4. Performance Impact

**Analysis:**
- Single string insertion into HashMap at response creation (negligible cost)
- No per-request allocation difference
- Header stored once, serialized multiple times (efficient)

**Conclusion:** Negligible performance impact

### 5. Error Path (400 Bad Request)

**Location:** `handle_connection()` at line 51 in `src/main.rs`

**Current Code:**
```rust
let mut response = HttpResponse::build(String::from("HTTP/1.1"), 400);
```

**Behavior After Change:** The header will be automatically included in the 400 response as well ✓

**Test:** Verify header is present on malformed requests

### 6. Content-Type Header Interaction

**Case:** Server responds with a file but incorrect Content-Type

**Browser Behavior with Header:** Browser will reject the response or refuse to execute scripts if the MIME type doesn't match

**Expected Outcome:** This is the desired security behavior — prevents attackers from serving JavaScript disguised as images

**Example:**
```
Response Headers:
Content-Type: image/png
X-Content-Type-Options: nosniff

Response Body: (JavaScript code)

Browser Result: Blocks execution (security success)
```

### 7. Interaction with Other Security Headers

**Complementary Headers:**
- `X-XSS-Protection: 1; mode=block` — XSS attack protection (separate feature)
- `X-Frame-Options: DENY` — Clickjacking protection (separate feature)
- `Content-Security-Policy` — Broader content security (separate feature)

**Relationship:** This header addresses MIME-type sniffing specifically and works independently of other security headers

### 8. Backward Compatibility

**Impact on Clients:**
- Modern browsers: Recognize and enforce the header ✓
- Legacy browsers (IE6): Ignore the header (no negative impact)
- API clients: Ignore the header (no impact on data consumption)

**Conclusion:** Fully backward compatible

---

## Implementation Checklist

- [ ] Add `X_CONTENT_TYPE_OPTIONS_VALUE` constant to `src/lib.rs` (optional but recommended)
- [ ] Modify `HttpResponse::build()` in `src/models/http_response.rs` to initialize security header
- [ ] Add unit test: `build_auto_sets_x_content_type_options_header()`
- [ ] Add unit test: `x_content_type_options_header_on_all_status_codes()`
- [ ] Add unit test: `x_content_type_options_header_case_insensitive()`
- [ ] Add unit test: `x_content_type_options_header_can_be_overridden()`
- [ ] Update `display_formats_status_line_and_headers()` test to verify automatic header
- [ ] Run `cargo test` to verify all existing tests still pass
- [ ] Add integration test: `test_root_response_security_header()`
- [ ] Add integration test: `test_404_response_security_header()`
- [ ] Add integration test: `test_bad_request_response_security_header()`
- [ ] Run integration tests: `cargo run --bin integration_test`
- [ ] Manual testing with `curl -i` for visual verification
- [ ] Verify header appears on 200, 404, and 400 responses
- [ ] Verify header value is exactly `"nosniff"` (case-sensitive)

---

## Code Summary

### Minimal Implementation (No Constant)

**Change in `src/models/http_response.rs`:**

```rust
pub fn build(version: String, code: u16) -> HttpResponse {
    let mut headers = HashMap::<String, String>::new();
    let phrase = get_status_phrase(code);
    headers.insert("x-content-type-options".to_string(), "nosniff".to_string());

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
pub const X_CONTENT_TYPE_OPTIONS_VALUE: &str = "nosniff";
```

**Change in `src/models/http_response.rs`:**

Add import:
```rust
use crate::X_CONTENT_TYPE_OPTIONS_VALUE;
```

Update `build()`:
```rust
pub fn build(version: String, code: u16) -> HttpResponse {
    let mut headers = HashMap::<String, String>::new();
    let phrase = get_status_phrase(code);
    headers.insert("x-content-type-options".to_string(), X_CONTENT_TYPE_OPTIONS_VALUE.to_string());

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
x-content-type-options: nosniff
content-length: 42

<html>...</html>
```

### Multiple Requests

All responses (200, 404, 400, etc.) will include the header:

```
GET /nonexistent HTTP/1.1
Host: localhost:7878

HTTP/1.1 404 Not Found
x-content-type-options: nosniff
content-length: 21

<html>Not Found</html>
```

---

## Rollback Plan

If this feature needs to be removed:
1. Revert the `build()` method to not initialize the security header
2. Remove the constant from `lib.rs` (if added)
3. Remove the unit tests added
4. Run test suite to verify no regressions

The change is completely reversible with no database migrations or configuration needed.

---

## Security Justification

### Why This Header Matters

**Vulnerability: MIME-Type Sniffing**

Older browsers had a feature where they would "sniff" the content type of responses, potentially overriding the `Content-Type` header. This could be exploited:

```
Attacker uploads "image.png" containing JavaScript
Server responds with Content-Type: image/png
Browser (without header): Sniffs and detects JavaScript, executes it
Browser (with header): Respects Content-Type, blocks execution
```

### Risk Mitigation

This header:
- Eliminates MIME-sniffing attacks against modern browsers
- Ensures strict content-type adherence
- Prevents cross-type exploitation vectors
- Is recommended by OWASP and security experts

### Minimal Risk Introduction

The header:
- Does not restrict legitimate functionality
- Does not affect API usage
- Has no negative side effects on compliant clients

---

## Future Enhancements

1. **Configurable Security Headers:** Allow environment variable override
   - `RCOMM_SECURITY_HEADERS=true/false`

2. **Additional Security Headers:** Add other recommended headers as separate features
   - `X-Frame-Options: DENY` (clickjacking protection)
   - `X-XSS-Protection: 1; mode=block` (XSS protection)
   - `Content-Security-Policy` (comprehensive content policy)

3. **Security Header Validation:** Add a check to ensure headers are present
   - Compile-time assertion that responses include security headers

---

## Summary

This is a low-complexity, high-security-value feature that improves the security posture of the rcomm web server by automatically including the `X-Content-Type-Options: nosniff` header in all responses. The implementation is centralized in the `HttpResponse::build()` method, ensuring consistency without code duplication. The change is backward-compatible, fully reversible, and has negligible performance impact.

**Recommended Approach:** Use the constant-based implementation (Option A with `X_CONTENT_TYPE_OPTIONS_VALUE` in `lib.rs`) for maintainability and consistency with the existing `SERVER_VERSION` pattern.

**Security Impact:** HIGH — Eliminates a known vector for MIME-type sniffing attacks
**Code Impact:** MINIMAL — Single line in one method, optional constant definition
**Testing Impact:** MODERATE — Requires unit tests for each status code and integration tests
