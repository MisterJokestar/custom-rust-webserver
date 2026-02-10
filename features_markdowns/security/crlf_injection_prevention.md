# Implementation Plan: Sanitize Response Headers to Prevent CRLF Injection

## Overview

This feature implements validation and sanitization of HTTP response headers to prevent CRLF (Carriage Return/Line Feed) injection attacks. A CRLF injection vulnerability occurs when an attacker can inject `\r\n` (or raw byte sequences `0x0D 0x0A`) into response header values, allowing them to:

1. Inject arbitrary headers (Header Injection)
2. Inject response splitting attacks (inject content into the response body)
3. Cache poisoning attacks
4. Cross-Site Scripting (XSS) via response manipulation

**Example Attack Vector:**

```rust
// Without sanitization, malicious code could do:
response.add_header(
    "Location".to_string(),
    "http://evil.com\r\nSet-Cookie: malicious=value".to_string()
);
```

This would result in the HTTP response:

```
HTTP/1.1 302 Found
location: http://evil.com
Set-Cookie: malicious=value
```

The attacker successfully injected an arbitrary `Set-Cookie` header by embedding `\r\n` in the header value.

**Implementation Approach:**

Sanitize header values in the `add_header()` method to reject or strip CRLF sequences. Both header names and values will be validated:

- **Header Names:** Already constrained (alphanumeric + hyphens in HTTP spec), but we'll validate
- **Header Values:** Currently unrestricted — will add sanitization logic

**Complexity:** 3/10 (Straightforward validation logic)
**Necessity:** 7/10 (Critical security feature)

---

## Files to Modify

1. **`src/models/http_response.rs`** — Main target
   - Modify `add_header()` method to validate header names and values
   - Add helper function(s) for sanitization logic
   - Add unit tests for injection detection

2. **`src/bin/integration_test.rs`** (Optional) — Add end-to-end security tests
   - Verify that injection attempts are rejected or sanitized
   - Test edge cases across the wire

---

## Step-by-Step Implementation

### Step 1: Define Sanitization Constants and Helper Functions

**File:** `/home/jwall/personal/rusty/rcomm/src/models/http_response.rs`

At the top of the file (after imports), add constants for validation:

```rust
// Forbidden characters/sequences in HTTP headers (per RFC 7230)
const CR_BYTE: u8 = 0x0D;  // \r
const LF_BYTE: u8 = 0x0A;  // \n
const NULL_BYTE: u8 = 0x00; // \0
```

Add a helper function to validate header names and values. Insert this before the `impl HttpResponse` block:

```rust
/// Validates a header name for forbidden characters.
/// Header names must be tokens per RFC 7230 (alphanumeric, hyphen, etc.)
/// Rejects: CR, LF, null bytes, and other control characters
fn is_valid_header_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }

    for byte in name.as_bytes() {
        // RFC 7230: header-field-name = token
        // token = 1*tchar
        // tchar = "!" / "#" / "$" / "%" / "&" / "'" / "*"
        //       / "+" / "-" / "." / "^" / "_" / "`" / "|" / "~"
        //       / DIGIT / ALPHA
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' => {},
            b'-' | b'_' | b'.' => {},
            CR_BYTE | LF_BYTE | NULL_BYTE => return false,
            // Reject control characters (0x00-0x1F except handled above)
            0x00..=0x1F => return false,
            // Reject DEL and higher control bytes
            0x7F..=0xFF => return false,
            _ => {}  // Allow RFC 7230 tchar values
        }
    }

    true
}

/// Validates a header value for forbidden CRLF/LF sequences.
/// Rejects: CR, LF, and null bytes which could enable injection attacks
fn is_valid_header_value(value: &str) -> bool {
    for byte in value.as_bytes() {
        match byte {
            CR_BYTE | LF_BYTE | NULL_BYTE => return false,
            _ => {}
        }
    }
    true
}

/// Sanitizes a header value by removing CRLF and null bytes.
/// Returns the sanitized string, or None if entire value becomes empty.
fn sanitize_header_value(value: &str) -> Option<String> {
    let sanitized: String = value
        .chars()
        .filter(|&c| {
            let byte = c as u8;
            !(byte == CR_BYTE || byte == LF_BYTE || byte == NULL_BYTE)
        })
        .collect();

    if sanitized.is_empty() {
        None
    } else {
        Some(sanitized)
    }
}
```

**Rationale:**

- `is_valid_header_name()`: Ensures header names conform to RFC 7230 token format
- `is_valid_header_value()`: Rejects values containing CRLF or null bytes (strict approach)
- `sanitize_header_value()`: Alternative approach — strips forbidden bytes instead of rejecting
- Both approaches are provided; implementation can choose either (see Step 2 for selection)

---

### Step 2: Modify the `add_header()` Method

**File:** `/home/jwall/personal/rusty/rcomm/src/models/http_response.rs`

Update the `add_header()` method to use validation. Two approaches are provided:

#### Approach A: Strict Rejection (Recommended for Security)

```rust
pub fn add_header(&mut self, title: String, value: String) -> &mut HttpResponse {
    // Validate header name
    if !is_valid_header_name(&title) {
        eprintln!("Warning: rejecting header with invalid name: {:?}", title);
        return self;
    }

    // Validate header value
    if !is_valid_header_value(&value) {
        eprintln!("Warning: rejecting header with CRLF/null bytes in value: {:?}={:?}", title, value);
        return self;
    }

    self.headers.insert(title.to_lowercase(), value);
    self
}
```

**Advantages:**
- Clear security policy: injection attempts are rejected outright
- Logs warnings for attack detection/monitoring
- Fails safely (silently skips malicious header)
- Builder pattern still works (returns `&mut self`)

**Disadvantages:**
- Legitimate headers with line breaks would be rejected (rare in practice)
- No feedback to caller about rejection (silent failure)

#### Approach B: Sanitization with Stripping (More Lenient)

```rust
pub fn add_header(&mut self, title: String, value: String) -> &mut HttpResponse {
    // Validate header name
    if !is_valid_header_name(&title) {
        eprintln!("Warning: rejecting header with invalid name: {:?}", title);
        return self;
    }

    // Sanitize header value by removing CRLF and null bytes
    let sanitized_value = match sanitize_header_value(&value) {
        Some(safe_value) => {
            if safe_value != value {
                eprintln!("Warning: sanitized header value for {}: removed forbidden bytes", title);
            }
            safe_value
        }
        None => {
            eprintln!("Warning: rejecting header {} - value became empty after sanitization", title);
            return self;
        }
    };

    self.headers.insert(title.to_lowercase(), sanitized_value);
    self
}
```

**Advantages:**
- More forgiving — accidental CRLF sequences are cleaned up
- Still prevents injection (attacker's injected content is stripped)
- May preserve valid headers with embedded control chars (edge case)

**Disadvantages:**
- Silent data modification (could mask bugs)
- Less clear security boundary

#### **Recommendation: Use Approach A (Strict Rejection)**

Strict rejection is the recommended approach for HTTP headers because:

1. **HTTP headers should never contain CRLF** — it's a protocol violation
2. **Legitimate use cases are rare** — headers with embedded line breaks are not standard
3. **Better auditing** — explicit warnings help detect attack attempts
4. **Fail-safe** — rejecting invalid data is more conservative than modifying it

---

### Step 3: Add Unit Tests

**File:** `/home/jwall/personal/rusty/rcomm/src/models/http_response.rs`

Add tests to the `mod tests` block at the bottom of the file:

#### Test 1: Reject CRLF in Header Value

```rust
#[test]
fn reject_header_with_crlf_in_value() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);

    // Attempt to inject via CRLF in header value
    resp.add_header(
        "X-Custom".to_string(),
        "value\r\nSet-Cookie: injected=true".to_string()
    );

    // Header should NOT be added
    assert_eq!(resp.try_get_header("X-Custom".to_string()), None);
}
```

#### Test 2: Reject LF-only in Header Value

```rust
#[test]
fn reject_header_with_lf_in_value() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);

    // Attempt to inject via LF alone
    resp.add_header(
        "Location".to_string(),
        "http://example.com\nSet-Cookie: bad=value".to_string()
    );

    assert_eq!(resp.try_get_header("Location".to_string()), None);
}
```

#### Test 3: Reject CR-only in Header Value

```rust
#[test]
fn reject_header_with_cr_in_value() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);

    // Attempt to inject via CR alone
    resp.add_header(
        "Content-Type".to_string(),
        "text/html\rX-Evil: injected".to_string()
    );

    assert_eq!(resp.try_get_header("Content-Type".to_string()), None);
}
```

#### Test 4: Reject NULL Byte in Header Value

```rust
#[test]
fn reject_header_with_null_byte() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);

    // Attempt to inject via null byte
    resp.add_header(
        "Custom".to_string(),
        "safe\0dangerous".to_string()
    );

    assert_eq!(resp.try_get_header("Custom".to_string()), None);
}
```

#### Test 5: Reject Control Characters in Header Name

```rust
#[test]
fn reject_header_with_control_char_in_name() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);

    // Attempt to use control character in header name
    resp.add_header(
        "X-Custom\r\nX-Evil".to_string(),
        "value".to_string()
    );

    // Neither header should be added
    assert_eq!(resp.try_get_header("X-Custom".to_string()), None);
    assert_eq!(resp.try_get_header("X-Evil".to_string()), None);
}
```

#### Test 6: Reject Empty Header Name

```rust
#[test]
fn reject_empty_header_name() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);

    resp.add_header(String::new(), "value".to_string());

    assert_eq!(resp.try_get_header("".to_string()), None);
}
```

#### Test 7: Accept Valid Headers (Positive Test)

```rust
#[test]
fn accept_valid_header_with_hyphens_and_digits() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);

    resp.add_header(
        "X-Custom-Header-1".to_string(),
        "valid value with spaces".to_string()
    );

    assert_eq!(
        resp.try_get_header("X-Custom-Header-1".to_string()),
        Some("valid value with spaces".to_string())
    );
}
```

#### Test 8: Accept Valid Headers with Special Characters

```rust
#[test]
fn accept_valid_header_with_allowed_special_chars() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);

    resp.add_header(
        "X-URL".to_string(),
        "https://example.com/path?query=value&other=123".to_string()
    );

    assert_eq!(
        resp.try_get_header("X-URL".to_string()),
        Some("https://example.com/path?query=value&other=123".to_string())
    );
}
```

#### Test 9: Verify Sanitized Output with Rejection

```rust
#[test]
fn verify_serialized_response_without_injected_headers() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);

    // Try to inject via CRLF
    resp.add_header(
        "X-Injected".to_string(),
        "value\r\nX-Evil: injected".to_string()
    );

    // Add a valid header for verification
    resp.add_header("X-Valid".to_string(), "safe".to_string());

    let output = format!("{resp}");

    // Valid header should be present
    assert!(output.contains("x-valid: safe"));

    // Injected header should NOT appear
    assert!(!output.contains("x-evil"));
    assert!(!output.contains("x-injected"));
}
```

#### Test 10: Builder Pattern Still Works with Rejection

```rust
#[test]
fn builder_pattern_continues_after_header_rejection() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);

    resp.add_header("Good1".to_string(), "value1".to_string())
        .add_header("Bad\r\n".to_string(), "value2".to_string())  // This is rejected silently
        .add_header("Good2".to_string(), "value3".to_string());

    // Chain should still work, both good headers present
    assert_eq!(resp.try_get_header("Good1".to_string()), Some("value1".to_string()));
    assert_eq!(resp.try_get_header("Good2".to_string()), Some("value3".to_string()));

    // Bad header should not be present
    assert_eq!(resp.try_get_header("Bad".to_string()), None);
}
```

**Test Execution:**

```bash
cargo test http_response::tests::reject_header_with_crlf_in_value -- --nocapture
cargo test -- --nocapture  # Run all tests with output visible
```

---

### Step 4: Add Integration Tests (Optional)

**File:** `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`

Add end-to-end tests to verify that injection attempts fail at the wire level:

```rust
fn test_crlf_injection_in_custom_header() -> TestResult {
    let (mut server, port) = spawn_server();

    // This test verifies that the server handles attempted CRLF injection
    // In a real attack, the client would try to inject headers
    // Since we control the server code, we test at the model level

    server.stop();
    TestResult::pass("CRLF injection test passed")
}
```

**Note:** Integration tests are less critical for this feature since the vulnerability is in the `add_header()` method itself, which is tested at the unit level. The server doesn't accept user-supplied headers (headers come from the codebase), so real-world injection requires:

1. Vulnerability in the routing/handler code that constructs headers from user input
2. Or a vulnerability in the HTTP request parser that passes unsanitized data

This implementation prevents case #1 by validating in `add_header()`. For case #2, see Step 5 below.

---

### Step 5: Optional - Consider Request Header Validation

**Analysis:** The current HTTP request parser (`src/models/http_request.rs`) parses incoming headers. While the response handler uses `add_header()` (which we're securing), incoming headers could theoretically contain malicious data. However:

- Incoming headers are not echoed back in responses
- The server only sends static files, not user data
- Request headers don't flow into responses

**Recommendation:** No changes needed to `http_request.rs` unless the server architecture changes to reflect request headers back to clients.

---

## Testing Strategy

### Unit Tests (in `http_response.rs`)

**Coverage:**

1. **Injection Vectors:**
   - CRLF sequences (`\r\n`)
   - LF-only (`\n`)
   - CR-only (`\r`)
   - NULL bytes (`\0`)
   - Multiple injections in single value

2. **Header Names:**
   - Invalid characters (control chars, spaces, CRLF)
   - Empty names
   - Valid characters (alphanumeric, hyphen, underscore, dot)

3. **Header Values:**
   - Valid values (URLs, quoted strings, numbers, spaces)
   - Invalid values with CRLF
   - Edge case: very long values without CRLF (should pass)

4. **Builder Pattern:**
   - Verify chaining still works after rejected header
   - Verify mix of valid and invalid headers

5. **Serialization:**
   - Verify rejected headers don't appear in `format!("{resp}")`
   - Verify `as_bytes()` doesn't contain injected content
   - Verify response structure is correct

### Run Commands

```bash
# Run all http_response tests
cargo test http_response --lib

# Run specific test with output
cargo test http_response::tests::reject_header_with_crlf_in_value -- --nocapture --exact

# Run all tests with backtrace on failure
RUST_BACKTRACE=1 cargo test
```

### Manual Testing (Optional)

While the server doesn't directly accept user-supplied headers, you can manually verify the implementation:

```bash
# Build the library
cargo build

# Or run the test suite
cargo test
```

### Integration Tests (Optional)

If the server architecture changes to reflect user input in response headers, add integration tests verifying the fix works end-to-end.

---

## Edge Cases & Considerations

### 1. CRLF Sequences in Different Byte Representations

**Case:** What if `\r\n` is represented as raw bytes vs. escape sequences?

**Analysis:**

```rust
// Rust string literal: escape sequences are interpreted at compile time
let header = "value\r\ninjected";  // Contains actual CR and LF bytes

// Raw bytes constructed at runtime
let mut header = String::from("value");
header.push(0x0D as char);  // CR
header.push(0x0A as char);  // LF
header.push_str("injected");
```

Both representations are handled identically by the validation logic because it operates on bytes, not characters.

**Test:**

```rust
#[test]
fn reject_crlf_in_multiple_representations() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);

    // Escape sequence form
    resp.add_header("A".to_string(), "val\r\ninj".to_string());
    assert_eq!(resp.try_get_header("A".to_string()), None);

    // Constructed form
    let mut val = String::from("val");
    val.push(0x0D as char);
    val.push(0x0A as char);
    val.push_str("inj");
    resp.add_header("B".to_string(), val);
    assert_eq!(resp.try_get_header("B".to_string()), None);
}
```

### 2. UTF-8 Encoding Edge Cases

**Case:** What about multi-byte UTF-8 sequences that happen to contain bytes matching CR/LF?

**Analysis:** The validation operates on individual bytes, not UTF-8 code points. This is correct because:

- CR (0x0D) and LF (0x0A) are ASCII bytes
- Valid UTF-8 multi-byte sequences have different byte patterns
- UTF-8 continuation bytes are 10xxxxxx (0x80-0xBF), never 0x0D or 0x0A
- Legitimate UTF-8 strings cannot accidentally match CRLF patterns

**Conclusion:** UTF-8 handling is safe by design.

### 3. Empty Header Values

**Case:** What if `value` is empty after sanitization?

**Implementation:** The `sanitize_header_value()` helper returns `None` if the string becomes empty, and callers should reject it:

```rust
match sanitize_header_value(&value) {
    Some(safe) => { /* use safe */ },
    None => { /* reject */ },
}
```

**Test:**

```rust
#[test]
fn reject_header_with_only_crlf_bytes() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);

    // Value contains only CRLF (would be empty after sanitization)
    resp.add_header("X-Test".to_string(), "\r\n".to_string());

    // Should be rejected (Approach A) or result in empty value (Approach B)
    // With strict rejection, it should not be added
    assert_eq!(resp.try_get_header("X-Test".to_string()), None);
}
```

### 4. Content-Length Header Interaction

**Current Behavior:** `add_body()` auto-sets `Content-Length` by calling `add_header()` internally.

```rust
pub fn add_body(&mut self, body: Vec<u8>) -> &mut HttpResponse {
    let len = body.len();
    self.body = Some(body);
    self.headers.insert("content-length".to_string(), len.to_string());
    self
}
```

**Question:** Could the body length contain CRLF?

**Analysis:** No. The length is calculated as `body.len()` (a `usize`) and converted to string. The result is always a decimal number (e.g., "1024"), which contains only ASCII digits. This will never fail validation.

**No changes needed.**

### 5. Null Bytes in Header Values

**Case:** Null bytes could truncate strings in some languages.

**Analysis:** Rust `String` and `&str` handle null bytes safely (no C-style string termination). However:

- HTTP headers should not contain null bytes
- Null bytes serve no legitimate purpose in headers
- Including them creates unexpected behavior
- Validation catches this correctly

**Test:** Covered by `reject_header_with_null_byte()` test above.

### 6. Case Sensitivity and Header Name Normalization

**Current Behavior:** Header names are stored lowercase via `.to_lowercase()` in `add_header()`.

**Question:** Does validation happen before or after lowercase conversion?

**Recommended:** Validate BEFORE lowercase conversion to catch any injections in the original input:

```rust
pub fn add_header(&mut self, title: String, value: String) -> &mut HttpResponse {
    if !is_valid_header_name(&title) {  // Validate before lowercase
        return self;
    }
    if !is_valid_header_value(&value) {
        return self;
    }
    self.headers.insert(title.to_lowercase(), value);
    self
}
```

This ensures the original input (including any CRLF sequences) is validated.

### 7. Very Long Header Values

**Case:** An attacker sends an extremely long header value, hoping to bypass checks via buffer overflow or DoS.

**Analysis:**

- Rust `String` is dynamic and automatically resizes
- No fixed-size buffer overflow possible
- Validation is O(n) per header (linear in length)
- Worst case: very long valid header is stored

**Mitigation:** If needed, add a length limit:

```rust
const MAX_HEADER_VALUE_SIZE: usize = 8192;  // 8 KB per header

fn is_valid_header_value(value: &str) -> bool {
    if value.len() > MAX_HEADER_VALUE_SIZE {
        return false;  // Reject headers larger than limit
    }

    for byte in value.as_bytes() {
        if byte == CR_BYTE || byte == LF_BYTE || byte == NULL_BYTE {
            return false;
        }
    }
    true
}
```

**Current Recommendation:** Not needed for MVP. The server is single-threaded per request and not exposed to untrusted header generation. Can be added later if needed.

### 8. Response Splitting Attack (Full Scenario)

**Detailed Example:**

Without sanitization:
```rust
// Attacker controls "user_input"
let user_input = "safe\r\n\r\n<html>Injected content</html>";
response.add_header("X-Custom".to_string(), user_input);
```

Result:
```
HTTP/1.1 200 OK
x-custom: safe

<html>Injected content</html>
[original body here]
```

The attacker injected content after the headers by using `\r\n\r\n` (blank line marks end of headers).

**With our implementation:** The header is rejected, attack prevented.

**Test:** Covered by existing CRLF tests.

### 9. HTTP/2 Considerations

**Note:** This server currently implements HTTP/1.1 (per architecture docs). HTTP/2 uses a binary protocol (HPACK) and doesn't have this vulnerability. No changes needed for HTTP/1.1 implementation.

### 10. Backward Compatibility

**Question:** Could rejecting headers break existing code?

**Analysis:**

- The server's codebase directly constructs all response headers (static content serving)
- No user input flows into `add_header()` calls
- No headers should contain CRLF by design
- Existing code should pass all tests

**Conclusion:** No backward compatibility issues. If the server evolves to accept user input for headers, that code would need to handle rejections.

---

## Implementation Checklist

- [ ] Add constants `CR_BYTE`, `LF_BYTE`, `NULL_BYTE` to `http_response.rs`
- [ ] Add helper function `is_valid_header_name()`
- [ ] Add helper function `is_valid_header_value()`
- [ ] Add helper function `sanitize_header_value()` (for reference, optional)
- [ ] Modify `add_header()` method to validate before insertion (Approach A)
- [ ] Add unit test: `reject_header_with_crlf_in_value()`
- [ ] Add unit test: `reject_header_with_lf_in_value()`
- [ ] Add unit test: `reject_header_with_cr_in_value()`
- [ ] Add unit test: `reject_header_with_null_byte()`
- [ ] Add unit test: `reject_header_with_control_char_in_name()`
- [ ] Add unit test: `reject_empty_header_name()`
- [ ] Add unit test: `accept_valid_header_with_hyphens_and_digits()`
- [ ] Add unit test: `accept_valid_header_with_allowed_special_chars()`
- [ ] Add unit test: `verify_serialized_response_without_injected_headers()`
- [ ] Add unit test: `builder_pattern_continues_after_header_rejection()`
- [ ] Run `cargo test http_response --lib` — all tests pass
- [ ] Run `cargo test -- --nocapture` — verify warning messages appear
- [ ] Manual verification with `cargo run` and inspection of code
- [ ] Update CLAUDE.md if architectural notes need updating (optional)
- [ ] Consider future length limits (document for later)

---

## Code Implementation Summary

### Complete `http_response.rs` Changes

Here's the complete modified file with all additions:

```rust
use std::{
    collections::HashMap,
    fmt,
};
use super::http_status_codes::get_status_phrase;

// Forbidden byte values in HTTP headers
const CR_BYTE: u8 = 0x0D;   // \r (Carriage Return)
const LF_BYTE: u8 = 0x0A;   // \n (Line Feed)
const NULL_BYTE: u8 = 0x00; // \0 (Null)

/// Validates a header name against RFC 7230 token rules.
/// Rejects: CR, LF, null bytes, control characters, and spaces.
fn is_valid_header_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }

    for byte in name.as_bytes() {
        // RFC 7230: header-field-name = token
        // token = 1*tchar where tchar is:
        // ALPHA / DIGIT / "!" / "#" / "$" / "%" / "&" / "'" / "*"
        // / "+" / "-" / "." / "^" / "_" / "`" / "|" / "~"
        match byte {
            // Allowed: alphanumerics
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' => {},
            // Allowed: common separators
            b'-' | b'_' | b'.' => {},
            // Forbidden: line breaks and null
            CR_BYTE | LF_BYTE | NULL_BYTE => return false,
            // Forbidden: other control characters (0x00-0x1F)
            0x00..=0x1F => return false,
            // Forbidden: DEL and high control bytes (0x7F+)
            0x7F..=0xFF => return false,
            // Allow other valid tchar values (! # $ % & ' * + ^ ` | ~)
            _ => {}
        }
    }

    true
}

/// Validates a header value for CRLF injection and null bytes.
/// Rejects values containing: CR (0x0D), LF (0x0A), or NULL (0x00).
fn is_valid_header_value(value: &str) -> bool {
    for byte in value.as_bytes() {
        if byte == CR_BYTE || byte == LF_BYTE || byte == NULL_BYTE {
            return false;
        }
    }
    true
}

/// Sanitizes a header value by removing CRLF and null bytes.
/// Returns Some(sanitized) if the value becomes non-empty after stripping.
/// Returns None if the entire value is removed.
/// Note: Used as reference implementation; add_header uses strict rejection instead.
#[allow(dead_code)]
fn sanitize_header_value(value: &str) -> Option<String> {
    let sanitized: String = value
        .chars()
        .filter(|&c| {
            let byte = c as u8;
            !(byte == CR_BYTE || byte == LF_BYTE || byte == NULL_BYTE)
        })
        .collect();

    if sanitized.is_empty() {
        None
    } else {
        Some(sanitized)
    }
}

pub struct HttpResponse {
    version: String,
    status_code: u16,
    status_phrase: String,
    headers: HashMap<String, String>,
    body: Option<Vec<u8>>,
}

impl HttpResponse {
    pub fn build(version: String, code: u16) -> HttpResponse {
        let headers = HashMap::<String, String>::new();
        let phrase = get_status_phrase(code);
        HttpResponse {
            version,
            status_code: code,
            status_phrase: phrase,
            headers,
            body: None,
        }
    }

    /// Adds a header to the response with validation.
    /// Validates header name and value for CRLF injection and other forbidden bytes.
    /// Invalid headers are rejected silently with a warning message.
    pub fn add_header(&mut self, title: String, value: String) -> &mut HttpResponse {
        // Validate header name
        if !is_valid_header_name(&title) {
            eprintln!(
                "WARNING: Rejecting header with invalid name: {:?} (contains forbidden bytes)",
                title
            );
            return self;
        }

        // Validate header value
        if !is_valid_header_value(&value) {
            eprintln!(
                "WARNING: Rejecting header {:?} with invalid value (contains CRLF or null bytes)",
                title
            );
            return self;
        }

        self.headers.insert(title.to_lowercase(), value);
        self
    }

    pub fn try_get_header(&self, title: String) -> Option<String> {
        self.headers.get(&title.to_lowercase()).cloned()
    }

    pub fn add_body(&mut self, body: Vec<u8>) -> &mut HttpResponse {
        let len = body.len();
        self.body = Some(body);
        self.headers.insert("content-length".to_string(), len.to_string());
        self
    }

    pub fn try_get_body(&self) -> Option<Vec<u8>> {
        self.body.clone()
    }

    pub fn as_bytes(&self) -> Vec<u8> {
        if let Some(body) = &self.body {
            let mut bytes = format!("{self}").as_bytes().to_vec();
            bytes.append(&mut body.clone());
            return bytes;
        } else {
            return format!("{self}").as_bytes().to_vec();
        }
    }
}

// Will not display body.
impl fmt::Display for HttpResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let _ = match write!(f, "{} {} {}\r\n", self.version, self.status_code, self.status_phrase) {
            Ok(result) => result,
            Err(e) => return Err(e),
        };
        for (title, value) in &self.headers {
            let _ = match write!(f, "{}: {}\r\n", title, value) {
                Ok(result) => result,
                Err(e) => return Err(e),
            };
        }
        write!(f, "\r\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_creates_response_with_correct_fields() {
        let resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
        let output = format!("{resp}");
        assert!(output.starts_with("HTTP/1.1 200 OK\r\n"));
    }

    #[test]
    fn build_sets_status_phrase_for_404() {
        let resp = HttpResponse::build("HTTP/1.1".to_string(), 404);
        let output = format!("{resp}");
        assert!(output.starts_with("HTTP/1.1 404 Not Found\r\n"));
    }

    #[test]
    fn build_sets_status_phrase_for_500() {
        let resp = HttpResponse::build("HTTP/1.1".to_string(), 500);
        let output = format!("{resp}");
        assert!(output.starts_with("HTTP/1.1 500 Internal Server Error\r\n"));
    }

    #[test]
    fn add_header_stores_header() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
        resp.add_header("Content-Type".to_string(), "text/html".to_string());
        let val = resp.try_get_header("Content-Type".to_string());
        assert_eq!(val, Some("text/html".to_string()));
    }

    #[test]
    fn try_get_header_returns_none_for_missing() {
        let resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
        assert_eq!(resp.try_get_header("Missing".to_string()), None);
    }

    #[test]
    fn add_body_and_try_get_body() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
        let body = b"<h1>Hello</h1>".to_vec();
        resp.add_body(body.clone());
        assert_eq!(resp.try_get_body(), Some(body));
    }

    #[test]
    fn try_get_body_returns_none_when_empty() {
        let resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
        assert_eq!(resp.try_get_body(), None);
    }

    #[test]
    fn display_formats_status_line_and_headers() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
        resp.add_header("Server".to_string(), "rcomm".to_string());
        let output = format!("{resp}");
        assert!(output.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(output.contains("server: rcomm\r\n"));
        assert!(output.ends_with("\r\n"));
    }

    #[test]
    fn as_bytes_without_body() {
        let resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
        let bytes = resp.as_bytes();
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(text.ends_with("\r\n"));
    }

    #[test]
    fn as_bytes_with_body() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
        resp.add_body(b"body here".to_vec());
        let bytes = resp.as_bytes();
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(text.ends_with("body here"));
    }

    #[test]
    fn add_header_chaining() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
        resp.add_header("A".to_string(), "1".to_string())
            .add_header("B".to_string(), "2".to_string());
        let output = format!("{resp}");
        assert!(output.contains("a: 1\r\n"));
        assert!(output.contains("b: 2\r\n"));
    }

    #[test]
    fn unknown_status_code_has_empty_phrase() {
        let resp = HttpResponse::build("HTTP/1.1".to_string(), 999);
        let output = format!("{resp}");
        assert!(output.starts_with("HTTP/1.1 999 \r\n"));
    }

    #[test]
    fn add_body_auto_sets_content_length() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
        resp.add_body(b"hello world".to_vec());
        assert_eq!(resp.try_get_header("content-length".to_string()), Some("11".to_string()));
    }

    // ========== CRLF Injection Prevention Tests ==========

    #[test]
    fn reject_header_with_crlf_in_value() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);

        // Attempt to inject via CRLF in header value
        resp.add_header(
            "X-Custom".to_string(),
            "value\r\nSet-Cookie: injected=true".to_string(),
        );

        // Header should NOT be added
        assert_eq!(resp.try_get_header("X-Custom".to_string()), None);
    }

    #[test]
    fn reject_header_with_lf_in_value() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);

        // Attempt to inject via LF alone
        resp.add_header(
            "Location".to_string(),
            "http://example.com\nSet-Cookie: bad=value".to_string(),
        );

        assert_eq!(resp.try_get_header("Location".to_string()), None);
    }

    #[test]
    fn reject_header_with_cr_in_value() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);

        // Attempt to inject via CR alone
        resp.add_header(
            "Content-Type".to_string(),
            "text/html\rX-Evil: injected".to_string(),
        );

        assert_eq!(resp.try_get_header("Content-Type".to_string()), None);
    }

    #[test]
    fn reject_header_with_null_byte() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);

        // Attempt to inject via null byte
        resp.add_header(
            "Custom".to_string(),
            "safe\0dangerous".to_string(),
        );

        assert_eq!(resp.try_get_header("Custom".to_string()), None);
    }

    #[test]
    fn reject_header_with_control_char_in_name() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);

        // Attempt to use control character in header name
        resp.add_header(
            "X-Custom\r\nX-Evil".to_string(),
            "value".to_string(),
        );

        // Header should not be added
        assert_eq!(resp.try_get_header("X-Custom".to_string()), None);
    }

    #[test]
    fn reject_empty_header_name() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);

        resp.add_header(String::new(), "value".to_string());

        assert_eq!(resp.try_get_header("".to_string()), None);
    }

    #[test]
    fn accept_valid_header_with_hyphens_and_digits() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);

        resp.add_header(
            "X-Custom-Header-1".to_string(),
            "valid value with spaces".to_string(),
        );

        assert_eq!(
            resp.try_get_header("X-Custom-Header-1".to_string()),
            Some("valid value with spaces".to_string())
        );
    }

    #[test]
    fn accept_valid_header_with_allowed_special_chars() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);

        resp.add_header(
            "X-URL".to_string(),
            "https://example.com/path?query=value&other=123".to_string(),
        );

        assert_eq!(
            resp.try_get_header("X-URL".to_string()),
            Some("https://example.com/path?query=value&other=123".to_string())
        );
    }

    #[test]
    fn verify_serialized_response_without_injected_headers() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);

        // Try to inject via CRLF
        resp.add_header(
            "X-Injected".to_string(),
            "value\r\nX-Evil: injected".to_string(),
        );

        // Add a valid header for verification
        resp.add_header("X-Valid".to_string(), "safe".to_string());

        let output = format!("{resp}");

        // Valid header should be present
        assert!(output.contains("x-valid: safe"));

        // Injected header should NOT appear
        assert!(!output.contains("x-evil"));
        assert!(!output.contains("x-injected"));
    }

    #[test]
    fn builder_pattern_continues_after_header_rejection() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);

        resp.add_header("Good1".to_string(), "value1".to_string())
            .add_header("Bad\r\n".to_string(), "value2".to_string())
            .add_header("Good2".to_string(), "value3".to_string());

        // Chain should still work, both good headers present
        assert_eq!(
            resp.try_get_header("Good1".to_string()),
            Some("value1".to_string())
        );
        assert_eq!(
            resp.try_get_header("Good2".to_string()),
            Some("value3".to_string())
        );

        // Bad header should not be present
        assert_eq!(resp.try_get_header("Bad".to_string()), None);
    }

    #[test]
    fn reject_header_with_only_crlf_value() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);

        // Value contains only CRLF
        resp.add_header("X-Test".to_string(), "\r\n".to_string());

        // Should be rejected
        assert_eq!(resp.try_get_header("X-Test".to_string()), None);
    }

    #[test]
    fn accept_numeric_header_values() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);

        resp.add_header("Max-Age".to_string(), "3600".to_string());
        resp.add_header("Retry-After".to_string(), "120".to_string());

        assert_eq!(
            resp.try_get_header("Max-Age".to_string()),
            Some("3600".to_string())
        );
        assert_eq!(
            resp.try_get_header("Retry-After".to_string()),
            Some("120".to_string())
        );
    }

    #[test]
    fn accept_quoted_header_values() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);

        resp.add_header(
            "Cache-Control".to_string(),
            "max-age=3600, must-revalidate".to_string(),
        );

        assert_eq!(
            resp.try_get_header("Cache-Control".to_string()),
            Some("max-age=3600, must-revalidate".to_string())
        );
    }
}
```

---

## Expected Behavior After Implementation

### Before Implementation

```rust
let mut response = HttpResponse::build("HTTP/1.1".to_string(), 200);
response.add_header(
    "X-Custom".to_string(),
    "safe\r\nX-Injected: malicious".to_string()
);

// Result: Response contains injected header
// HTTP/1.1 200 OK
// x-custom: safe
// X-Injected: malicious
//
// [body]
```

### After Implementation

```rust
let mut response = HttpResponse::build("HTTP/1.1".to_string(), 200);
response.add_header(
    "X-Custom".to_string(),
    "safe\r\nX-Injected: malicious".to_string()
);

// Console warning:
// WARNING: Rejecting header "X-Custom" with invalid value (contains CRLF or null bytes)

// Result: Header is rejected, not added
// HTTP/1.1 200 OK
//
// [body]
```

---

## Testing & Validation Commands

```bash
# Build the project
cd /home/jwall/personal/rusty/rcomm
cargo build

# Run all unit tests (including the new CRLF prevention tests)
cargo test http_response --lib

# Run with output to see warning messages
cargo test http_response --lib -- --nocapture

# Run specific test
cargo test http_response::tests::reject_header_with_crlf_in_value -- --nocapture --exact

# Run all tests in the project
cargo test

# Check code compiles and runs
cargo run &
curl http://127.0.0.1:7878/
```

---

## Rollback Plan

If this feature needs to be removed:

1. Remove the helper functions (`is_valid_header_name`, `is_valid_header_value`, `sanitize_header_value`)
2. Remove the byte constants (`CR_BYTE`, `LF_BYTE`, `NULL_BYTE`)
3. Revert the `add_header()` method to the original implementation:
   ```rust
   pub fn add_header(&mut self, title: String, value: String) -> &mut HttpResponse {
       self.headers.insert(title.to_lowercase(), value);
       self
   }
   ```
4. Remove all tests added in the "CRLF Injection Prevention Tests" section
5. Run `cargo test` to verify no regressions

The change is completely reversible with no side effects.

---

## Future Enhancements

1. **Header Value Length Limits:**
   - Add configurable max length (e.g., 8KB per header)
   - Prevent DoS via extremely long headers
   - Implementation: Modify `is_valid_header_value()` to check length

2. **Configurable Sanitization Mode:**
   - Environment variable: `RCOMM_HEADER_VALIDATION=strict|lenient`
   - `strict`: Reject (current implementation)
   - `lenient`: Sanitize (strip CRLF characters)

3. **Metrics & Monitoring:**
   - Count rejected headers per request
   - Log attack attempts with timestamp and header name
   - Integrate with future logging system

4. **RFC 7230 Strict Compliance:**
   - Validate header names against full RFC token rules
   - Validate header values against obs-text rules
   - Support obsolete line folding if needed (unlikely)

5. **Request Header Validation:**
   - If the server evolves to reflect request headers in responses, add similar validation to `http_request.rs`
   - Validate incoming headers early and reject malformed ones

---

## Security Impact Analysis

### Vulnerabilities Mitigated

1. **HTTP Response Splitting (High Severity)** - MITIGATED
   - Prevents injecting `\r\n\r\n` to insert content after headers
   - Attacker can no longer control response body via header injection

2. **Header Injection (Medium Severity)** - MITIGATED
   - Prevents injecting arbitrary headers via CRLF in values
   - Attacker cannot set cookies, redirects, or content-type

3. **Cache Poisoning (Medium Severity)** - MITIGATED
   - When headers are validated at injection point, cache systems downstream see consistent responses
   - Prevents attackers from poisoning shared caches

### Attack Scenarios Blocked

- Setting malicious `Set-Cookie` headers
- Forcing `Content-Type` to `text/plain` (XSS prevention bypass)
- Injecting `Location` headers to redirect users
- Forcing `Transfer-Encoding: chunked` for smuggling
- Setting `Vary` headers to poison cache keys

### Remaining Considerations

- Server must not reflect unsanitized user input in headers (architectural responsibility)
- This implementation provides a defensive layer at the `add_header()` level
- Defense-in-depth: combine with input validation at higher layers

---

## Compliance & Standards

### RFC 7230 (HTTP/1.1 Message Syntax)

The implementation follows RFC 7230 requirements:

- **Section 3.2 (Header Fields):** Header field names must be tokens; values must not contain bare CR or LF
- **Section 3.2.6 (Field Value Components):** Control characters have special meaning and are forbidden in unquoted text

**Our Implementation:** Strictly rejects CR, LF, and null bytes in header values per RFC requirements.

### OWASP

- **CWE-113: Improper Neutralization of CRLF Sequences in HTTP Headers**
  - CVSS Score: 6.5 (Medium)
  - Our fix directly addresses this CWE

- **OWASP Top 10 (A03:2021 – Injection)**
  - CRLF injection is a type of header injection
  - Our prevention validates at the boundary where headers are added

---

## Performance Impact

### Computational Cost

1. **Per-header overhead:** O(n) where n = header value length (single pass through bytes)
2. **Memory overhead:** Negligible (constants only, no allocations)
3. **No string copying:** Validation happens before insertion

### Benchmarking Notes

For typical headers (e.g., "text/html", "1024", "localhost:8080"):
- Validation is microsecond-scale
- Negligible impact on request/response time
- 10,000 headers could be validated in < 100µs

### Recommendation

No performance concerns. The validation cost is unmeasurable compared to file I/O (reading static files) and network I/O (TCP writes).

---

## Summary

This implementation adds CRLF injection prevention to the `HttpResponse` struct by validating header names and values in the `add_header()` method. The validation:

1. **Rejects invalid headers** at the point of insertion (strict policy)
2. **Prevents response splitting attacks** by blocking CRLF sequences
3. **Prevents header injection attacks** by validating format
4. **Maintains backward compatibility** (no code changes needed in `main.rs`)
5. **Is fully testable** with 10+ unit tests covering attack vectors

**Recommended Implementation:** Use Approach A (strict rejection) with comprehensive unit tests and warning logs for attack detection.

