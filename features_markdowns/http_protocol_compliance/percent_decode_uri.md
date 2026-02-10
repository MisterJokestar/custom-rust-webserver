# Percent-Decode Request URI

## Feature Overview

**Category:** HTTP Protocol Compliance
**Complexity:** 3/10
**Necessity:** 7/10

Implement URL percent-decoding (also called URL-decoding or percent-encoding decoding) for HTTP request URIs. This allows the server to properly handle encoded characters such as spaces (`%20`), forward slashes (`%2F`), and other special characters in request paths.

### Current Behavior

Currently, the `HttpRequest` struct stores the raw request target (path) as-is without any percent-decoding. This means:
- A request to `/hello%20world` is matched literally against the routes
- Encoded characters are never translated to their actual values
- Special characters in filenames must already be unencoded in the URI

### Desired Behavior

After implementation:
- A request to `/hello%20world` is decoded to `/hello world` before route matching
- `%2F` is decoded to `/`
- Invalid percent sequences (e.g., `%ZZ`, `%2`) are handled gracefully
- Query strings (if added in future) should NOT be percent-decoded by this function, as they use different rules
- The `target` field in `HttpRequest` contains the decoded path

### Examples of Percent-Encoding

| Encoded | Decoded | ASCII Value |
|---------|---------|-------------|
| `%20`   | ` `     | 32          |
| `%2F`   | `/`     | 47          |
| `%2E`   | `.`     | 46          |
| `%3F`   | `?`     | 63          |
| `%40`   | `@`     | 64          |
| `%7E`   | `~`     | 126         |

---

## Files to Modify

### 1. **`/home/jwall/personal/rusty/rcomm/src/models/http_request.rs`**
   - Add a new public function `percent_decode(s: &str) -> String` (or similar) to decode percent-encoded strings
   - Modify the `build_from_stream()` method to decode the `target` immediately after parsing it from the request line
   - Add comprehensive unit tests for the decoding function

### 2. **`/home/jwall/personal/rusty/rcomm/src/main.rs`**
   - No changes needed initially, but `clean_route()` will now operate on already-decoded targets
   - Verify that path traversal checks still work correctly with decoded paths

---

## Step-by-Step Implementation

### Step 1: Implement the Percent-Decoding Function

Add a new function to `/home/jwall/personal/rusty/rcomm/src/models/http_request.rs` (outside the impl block or as a module-level helper):

```rust
/// Decodes percent-encoded characters in a UTF-8 string.
///
/// Converts sequences like %20 (space), %2F (/), etc. to their actual characters.
/// Invalid sequences are left unchanged (e.g., %ZZ, %G, or incomplete % at end).
///
/// # Arguments
/// * `encoded` - A percent-encoded string
///
/// # Returns
/// A decoded string with percent-sequences replaced
///
/// # Examples
/// ```
/// assert_eq!(percent_decode("%20"), " ");
/// assert_eq!(percent_decode("hello%20world"), "hello world");
/// assert_eq!(percent_decode("path%2Fto%2Ffile"), "path/to/file");
/// assert_eq!(percent_decode("invalid%ZZ"), "invalid%ZZ"); // invalid hex, left unchanged
/// ```
fn percent_decode(encoded: &str) -> String {
    let bytes = encoded.as_bytes();
    let mut decoded = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            // Try to parse the next two characters as hex
            let hex_str = match std::str::from_utf8(&bytes[i + 1..i + 3]) {
                Ok(s) => s,
                Err(_) => {
                    // Not valid UTF-8, keep the % and continue
                    decoded.push(bytes[i]);
                    i += 1;
                    continue;
                }
            };

            match u8::from_str_radix(hex_str, 16) {
                Ok(byte) => {
                    // Successfully decoded a byte
                    decoded.push(byte);
                    i += 3; // skip the %, and two hex digits
                }
                Err(_) => {
                    // Invalid hex sequence, keep the % and move forward
                    decoded.push(bytes[i]);
                    i += 1;
                }
            }
        } else if bytes[i] == b'%' {
            // % at end of string or only one character remaining, keep it
            decoded.push(bytes[i]);
            i += 1;
        } else {
            // Regular character, copy it
            decoded.push(bytes[i]);
            i += 1;
        }
    }

    // Convert bytes back to string; if invalid UTF-8 sequences are created,
    // they will be replaced with replacement character (U+FFFD)
    String::from_utf8_lossy(&decoded).into_owned()
}
```

**Key Design Decisions:**

1. **Invalid hex sequences are preserved:** If `%ZZ` appears, it stays as `%ZZ` rather than being lost
2. **Incomplete sequences at end are preserved:** `hello%` becomes `hello%`, not an error
3. **Uses `from_str_radix(hex_str, 16)`:** Standard Rust approach for hex parsing
4. **UTF-8 safety:** Uses `String::from_utf8_lossy()` to handle potential invalid UTF-8 bytes gracefully
5. **Case insensitive:** `%2f` and `%2F` both decode to `/`

### Step 2: Integrate Decoding into `build_from_stream()`

Modify the `build_from_stream()` method in the `HttpRequest` impl block. After parsing the target from the request line (around line 66), decode it:

**Before (current line 66):**
```rust
let target = iter.next().ok_or(HttpParseError::MalformedRequestLine)?.to_string();
```

**After:**
```rust
let target_encoded = iter.next().ok_or(HttpParseError::MalformedRequestLine)?;
let target = percent_decode(target_encoded);
```

Or more concisely:
```rust
let target = percent_decode(
    iter.next().ok_or(HttpParseError::MalformedRequestLine)?
);
```

This ensures that every `HttpRequest` created via `build_from_stream()` has an already-decoded target.

### Step 3: Add Unit Tests for Percent-Decoding

Add the following tests to the `#[cfg(test)]` module in `http_request.rs`:

```rust
#[test]
fn percent_decode_simple_space() {
    assert_eq!(percent_decode("%20"), " ");
}

#[test]
fn percent_decode_multiple_spaces() {
    assert_eq!(percent_decode("hello%20world"), "hello world");
}

#[test]
fn percent_decode_forward_slash() {
    assert_eq!(percent_decode("path%2Fto%2Ffile"), "path/to/file");
}

#[test]
fn percent_decode_mixed_characters() {
    assert_eq!(percent_decode("/api%2Fv1%2Fusers%20list"), "/api/v1/users list");
}

#[test]
fn percent_decode_no_encoding() {
    assert_eq!(percent_decode("/simple/path"), "/simple/path");
}

#[test]
fn percent_decode_invalid_hex_preserved() {
    assert_eq!(percent_decode("invalid%ZZ"), "invalid%ZZ");
}

#[test]
fn percent_decode_incomplete_percent_at_end() {
    assert_eq!(percent_decode("hello%"), "hello%");
}

#[test]
fn percent_decode_incomplete_percent_one_char() {
    assert_eq!(percent_decode("hello%2"), "hello%2");
}

#[test]
fn percent_decode_lowercase_hex() {
    assert_eq!(percent_decode("%2f"), "/");
    assert_eq!(percent_decode("%20"), " ");
}

#[test]
fn percent_decode_uppercase_hex() {
    assert_eq!(percent_decode("%2F"), "/");
    assert_eq!(percent_decode("%20"), " ");
}

#[test]
fn percent_decode_mixed_case_hex() {
    assert_eq!(percent_decode("%2F"), "/");
    assert_eq!(percent_decode("%2f"), "/");
}

#[test]
fn percent_decode_null_byte() {
    // %00 decodes to null byte (ASCII 0)
    let result = percent_decode("%00");
    assert_eq!(result.as_bytes()[0], 0);
}

#[test]
fn percent_decode_high_ascii() {
    // %FF is 255 in decimal
    let result = percent_decode("%FF");
    assert_eq!(result.as_bytes()[0], 255);
}

#[test]
fn percent_decode_empty_string() {
    assert_eq!(percent_decode(""), "");
}

#[test]
fn percent_decode_only_percent() {
    assert_eq!(percent_decode("%"), "%");
}

#[test]
fn percent_decode_consecutive_encoded() {
    assert_eq!(percent_decode("%20%20%20"), "   ");
}
```

### Step 4: Verify Integration with Request Parsing

Add an integration test to `http_request.rs` that verifies percent-encoded URIs are properly decoded when parsing from a TCP stream:

```rust
#[test]
fn build_from_stream_decodes_percent_encoded_target() {
    use std::io::Write;
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = std::thread::spawn(move || {
        let mut client = std::net::TcpStream::connect(addr).unwrap();
        client
            .write_all(b"GET /hello%20world HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
    });

    let (stream, _) = listener.accept().unwrap();
    let req = HttpRequest::build_from_stream(&stream).unwrap();

    assert_eq!(req.target, "/hello world");
    handle.join().unwrap();
}

#[test]
fn build_from_stream_preserves_invalid_percent_sequences() {
    use std::io::Write;
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = std::thread::spawn(move || {
        let mut client = std::net::TcpStream::connect(addr).unwrap();
        client
            .write_all(b"GET /path%ZZ HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
    });

    let (stream, _) = listener.accept().unwrap();
    let req = HttpRequest::build_from_stream(&stream).unwrap();

    assert_eq!(req.target, "/path%ZZ");
    handle.join().unwrap();
}
```

---

## Testing Strategy

### Unit Tests (in `http_request.rs`)
- **Basic encoding:** Single character decoding (`%20` → space)
- **Multiple sequences:** Multiple encoded chars in one string
- **Invalid hex:** Invalid hex digits are preserved as-is
- **Incomplete sequences:** `%` at end or with only one hex digit
- **Case insensitivity:** Both `%2f` and `%2F` work
- **Empty strings and edge cases**
- **High-value bytes:** `%FF`, `%00`

### Integration Tests (in `http_request.rs`)
- Full HTTP request parsing with percent-encoded URIs
- Verify the decoded target is stored in the request

### End-to-End Tests (in `src/bin/integration_test.rs`)
- Send requests with percent-encoded paths to the running server
- Verify the server routes to the correct file
- Test that `/hello%20world` serves the same content as the file system route (if applicable)
- Verify that malicious paths like `/..%2F../` are handled safely (by existing `clean_route()`)

### Manual Testing
```bash
# Start the server
cargo run

# In another terminal, test with curl
curl http://127.0.0.1:7878/hello%20world
curl http://127.0.0.1:7878/path%2Fto%2Ffile
curl http://127.0.0.1:7878/invalid%ZZ
```

---

## Edge Cases & Security Considerations

### 1. **Path Traversal Attacks**
**Concern:** Could `../` be hidden as `%2E%2E%2F` to bypass security?

**Mitigation:** The existing `clean_route()` function (in `main.rs`) already strips out `..` and `.` after decoding. Since we decode at parse time, `clean_route()` will catch encoded traversal attempts.

**Verification:**
- Test that `/api%2F..%2F/secrets` properly decodes to `/api/..//secrets` then is cleaned to `/api/secrets`
- Ensure the decoded path still prevents access to parent directories

### 2. **Double Encoding**
**Concern:** Could `%252F` (encoded `%2F`) be problematic?

**Current approach:** Single pass decoding, so `%252F` → `%2F` → (stops here, not decoded again). This is correct per HTTP spec.

**Verification:** Test that `%252F` becomes `%2F` and stays that way.

### 3. **Invalid UTF-8 Sequences**
**Concern:** What if `%C0%80` (invalid UTF-8 overlong encoding of null) is sent?

**Current approach:** Use `String::from_utf8_lossy()`, which replaces invalid sequences with the replacement character (U+FFFD, shown as `?` when printed).

**Verification:** Verify that byte sequences that don't form valid UTF-8 are handled gracefully without panicking.

### 4. **Null Bytes**
**Concern:** Could `%00` in the path cause issues?

**Current approach:** Decoded to actual null byte; the resulting string will contain a null byte, which may or may not cause issues with file operations.

**Recommendation:** Future enhancement could add validation to reject paths containing null bytes before routing.

### 5. **Whitespace and Special Characters**
Common encoded characters:
- `%20` (space)
- `%09` (tab)
- `%0A` (newline - problematic for headers)
- `%0D` (carriage return - problematic for headers)

These will be decoded correctly, but if any appear in the path, they may cause issues with file system operations (especially newlines and carriage returns). The server should ideally validate that decoded paths don't contain control characters.

### 6. **Reserved Characters That Shouldn't Be Encoded**
Per RFC 3986, some characters shouldn't be percent-encoded:
- `!`, `*`, `'`, `(`, `)`, `:`, `@`, `&`, `=`, `+`, `$`, `,`, `/`, `?`, `#`

Current approach: Decode them anyway. This is permissive and matches common server behavior.

---

## Implementation Checklist

- [ ] Add `percent_decode()` function to `http_request.rs`
- [ ] Modify `build_from_stream()` to call `percent_decode()` on the target
- [ ] Add all unit tests for `percent_decode()` function
- [ ] Add integration tests for request parsing with encoded URIs
- [ ] Run `cargo test` to ensure all tests pass
- [ ] Test with curl or similar tool to verify end-to-end behavior
- [ ] Test security edge cases (path traversal, double encoding, etc.)
- [ ] Verify no regression in existing tests
- [ ] Consider documenting the feature in comments or CLAUDE.md

---

## Alternative Approaches (Not Recommended)

### Approach 1: Lazy Decoding in `clean_route()`
Decode only when routing, not at parse time.
- **Pros:** Minimal changes, lazy evaluation
- **Cons:** Harder to test, decoding scattered across codebase, inconsistent state in `HttpRequest`

### Approach 2: Third-Party Crate
Use a crate like `urlencoding`.
- **Pros:** Vetted implementation, handles edge cases
- **Cons:** Violates "no external dependencies" constraint

### Approach 3: Inline Decoding in `main.rs`
Decode the target after retrieving it in `handle_connection()`.
- **Pros:** Keeps HTTP models simpler
- **Cons:** Decoding split across modules, inconsistent request representation

---

## Implementation Complexity Breakdown

| Task | Complexity | Effort |
|------|-----------|--------|
| Write `percent_decode()` function | 2/10 | 30 min |
| Integrate into `build_from_stream()` | 1/10 | 5 min |
| Add unit tests | 2/10 | 45 min |
| Add integration tests | 2/10 | 20 min |
| Security testing & verification | 3/10 | 30 min |
| **Total** | **~3/10** | **~2 hours** |

---

## Related Features

- **Query Parameter Handling:** If query strings are added in the future, they would need similar decoding (but with slightly different rules—`+` represents space, not just `%20`)
- **Request Headers Encoding:** Currently headers are stored as-is; future work could decode header values if needed
- **Safe Path Validation:** Adding checks to reject null bytes, control characters, etc. in decoded paths

---

## References

- [RFC 3986 - Uniform Resource Identifier (URI): Generic Syntax](https://tools.ietf.org/html/rfc3986#section-2.1) - Percent-Encoding specification
- [RFC 3987 - Internationalized Resource Identifiers (IRI)](https://tools.ietf.org/html/rfc3987) - Related standard
- Common HTTP server behaviors: Nginx, Apache, and others all percent-decode URIs before routing

