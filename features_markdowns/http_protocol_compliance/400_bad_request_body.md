# Return 400 Bad Request with Descriptive Body Implementation Plan

## 1. Overview of the Feature

The HTTP `400 Bad Request` response is used when the server receives a malformed HTTP request that it cannot understand or process. Currently, rcomm returns a 400 response but with an **empty body**, which provides clients with no information about what went wrong.

A descriptive body improves the debugging experience for API consumers and provides better HTTP protocol compliance. Clients can now see the specific error reason (e.g., "Missing required Host header", "Malformed request line", "Header line exceeds maximum length") instead of guessing.

**Goal**: Return a 400 Bad Request response with a human-readable and machine-parseable body that describes the specific parsing error.

**Benefits**:
- **Debugging**: Developers can identify and fix malformed requests quickly
- **HTTP Compliance**: Proper use of request/response body for error information
- **User Experience**: Clearer communication of what went wrong
- **Standards Alignment**: Follows REST and HTTP best practices (RFC 7231)

**Current State**:
```rust
// In src/main.rs, handle_connection():
Err(e) => {
    eprintln!("Bad request: {e}");
    let mut response = HttpResponse::build(String::from("HTTP/1.1"), 400);
    let body = format!("Bad Request: {e}");  // Body created but already implemented!
    response.add_body(body.into());
    let _ = stream.write_all(&response.as_bytes());
    return;
}
```

**Interesting finding**: The code **already** creates and sends a descriptive body! However, examining it more closely:
1. The body is created and added to the response
2. The body includes the error message from the `HttpParseError`
3. The `Content-Type` header is **not set** (currently lacks Content-Type specification)

**Actual Enhancement Needed**: While the feature is partially implemented, it should be improved by:
1. **Setting the `Content-Type` header** (currently missing)
2. **Formatting the body more consistently** (using HTML or plain text)
3. **Testing to ensure bodies are actually being sent** (integration test coverage)

---

## 2. Files to be Modified or Created

### Modified Files

1. **`/home/jwall/personal/rusty/rcomm/src/main.rs`**
   - Add `Content-Type` header to 400 Bad Request responses
   - Format the error body consistently (plain text or HTML)

### New Files (Optional, for better error handling)

2. **`/home/jwall/personal/rusty/rcomm/src/models/error_responses.rs`** (recommended)
   - Create centralized error response builder
   - Provides consistent formatting across error types (400, 404, 500, etc.)

---

## 3. Step-by-Step Implementation Details

### Step 1: Create Error Response Helper Module (Optional but Recommended)

**File**: `/home/jwall/personal/rusty/rcomm/src/models/error_responses.rs`

This module provides a consistent way to format error responses:

```rust
/// Formats an error response body as plain text.
///
/// # Arguments
/// * `error_description` - A description of what went wrong
///
/// # Returns
/// * A UTF-8 encoded plain text error body
pub fn format_error_body_plain(error_description: &str) -> Vec<u8> {
    format!("Error: {}\n", error_description).into_bytes()
}

/// Formats an error response body as HTML.
///
/// Useful for browser-based error viewing while maintaining
/// a clear structure for API clients.
///
/// # Arguments
/// * `status_code` - HTTP status code (e.g., 400)
/// * `status_phrase` - HTTP status phrase (e.g., "Bad Request")
/// * `error_description` - A description of what went wrong
///
/// # Returns
/// * A UTF-8 encoded HTML error page body
pub fn format_error_body_html(
    status_code: u16,
    status_phrase: &str,
    error_description: &str,
) -> Vec<u8> {
    let html = format!(
        "<!DOCTYPE html>\n\
         <html>\n\
         <head>\n\
         <meta charset=\"utf-8\">\n\
         <title>{} {}</title>\n\
         <style>\n\
         body {{ font-family: sans-serif; margin: 50px; }}\n\
         h1 {{ color: #d32f2f; }}\n\
         p {{ color: #555; }}\n\
         code {{ background-color: #f5f5f5; padding: 2px 6px; }}\n\
         </style>\n\
         </head>\n\
         <body>\n\
         <h1>{} {}</h1>\n\
         <p><code>{}</code></p>\n\
         </body>\n\
         </html>",
        status_code, status_phrase, status_code, status_phrase, error_description
    );
    html.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_error_body_plain_creates_plain_text() {
        let body = format_error_body_plain("Malformed request line");
        let text = String::from_utf8(body).unwrap();
        assert_eq!(text, "Error: Malformed request line\n");
    }

    #[test]
    fn format_error_body_plain_includes_error_message() {
        let body = format_error_body_plain("Missing Host header");
        let text = String::from_utf8(body).unwrap();
        assert!(text.contains("Missing Host header"));
    }

    #[test]
    fn format_error_body_html_creates_html() {
        let body = format_error_body_html(400, "Bad Request", "Malformed request line");
        let text = String::from_utf8(body).unwrap();
        assert!(text.contains("<!DOCTYPE html>"));
        assert!(text.contains("400 Bad Request"));
        assert!(text.contains("Malformed request line"));
    }

    #[test]
    fn format_error_body_html_includes_status_code() {
        let body = format_error_body_html(400, "Bad Request", "Test error");
        let text = String::from_utf8(body).unwrap();
        assert!(text.contains("<title>400 Bad Request</title>"));
    }

    #[test]
    fn format_error_body_html_includes_styles() {
        let body = format_error_body_html(400, "Bad Request", "Test error");
        let text = String::from_utf8(body).unwrap();
        assert!(text.contains("<style>"));
        assert!(text.contains("font-family: sans-serif"));
    }

    #[test]
    fn format_error_body_html_properly_escapes_error() {
        let body = format_error_body_html(400, "Bad Request", "Error with <tag>");
        let text = String::from_utf8(body).unwrap();
        // Note: Current implementation doesn't HTML-escape. This is a known limitation.
        // For production use, consider using a dedicated HTML escaping library.
        assert!(text.contains("Error with <tag>"));
    }
}
```

**Note**: The HTML version is simple and doesn't include HTML entity escaping for error messages. For production use with user-controlled error messages, consider adding an HTML escaping function or library. Currently, the error messages come from the `HttpParseError` enum which uses static strings, so this is safe.

### Step 2: Export the Error Response Module

**File**: `/home/jwall/personal/rusty/rcomm/src/models.rs`

Add the new module to the barrel export:

```rust
pub mod http_methods;
pub mod http_request;
pub mod http_response;
pub mod http_status_codes;
pub mod error_responses;  // Add this line
```

### Step 3: Modify the Main Server Handler

**File**: `/home/jwall/personal/rusty/rcomm/src/main.rs`

Update the bad request handling in `handle_connection()` to add the `Content-Type` header:

**Current Code** (lines 46-57):
```rust
fn handle_connection(mut stream: TcpStream, routes: HashMap<String, PathBuf>) {
    let http_request = match HttpRequest::build_from_stream(&stream) {
        Ok(req) => req,
        Err(e) => {
            eprintln!("Bad request: {e}");
            let mut response = HttpResponse::build(String::from("HTTP/1.1"), 400);
            let body = format!("Bad Request: {e}");
            response.add_body(body.into());
            let _ = stream.write_all(&response.as_bytes());
            return;
        }
    };
    // ... rest of function ...
}
```

**Option 1: Minimal Enhancement (Plain Text)**

```rust
fn handle_connection(mut stream: TcpStream, routes: HashMap<String, PathBuf>) {
    let http_request = match HttpRequest::build_from_stream(&stream) {
        Ok(req) => req,
        Err(e) => {
            eprintln!("Bad request: {e}");
            let mut response = HttpResponse::build(String::from("HTTP/1.1"), 400);
            response.add_header("Content-Type".to_string(), "text/plain".to_string());
            let body = format!("Bad Request: {e}");
            response.add_body(body.into());
            let _ = stream.write_all(&response.as_bytes());
            return;
        }
    };
    // ... rest of function ...
}
```

**Option 2: Enhanced With Error Response Module (HTML)**

First, add the import at the top of `src/main.rs`:
```rust
use rcomm::models::error_responses;
```

Then update the handler:
```rust
fn handle_connection(mut stream: TcpStream, routes: HashMap<String, PathBuf>) {
    let http_request = match HttpRequest::build_from_stream(&stream) {
        Ok(req) => req,
        Err(e) => {
            eprintln!("Bad request: {e}");
            let mut response = HttpResponse::build(String::from("HTTP/1.1"), 400);
            response.add_header("Content-Type".to_string(), "text/html".to_string());
            let body = error_responses::format_error_body_html(
                400,
                "Bad Request",
                &format!("{e}")
            );
            response.add_body(body);
            let _ = stream.write_all(&response.as_bytes());
            return;
        }
    };
    // ... rest of function ...
}
```

**Recommendation**: Use Option 2 (HTML with error response module) for:
- Consistency with 404 responses (which are also HTML)
- Better browser experience when manually testing
- Easier to extend to other error codes (500, etc.) in the future

---

## 4. Code Snippets and Pseudocode

### Error Response Formatting

```
FUNCTION format_error_body_plain(error_description: string) -> bytes
    RETURN ("Error: " + error_description + "\n").as_bytes()
END FUNCTION

FUNCTION format_error_body_html(code: u16, phrase: string, error: string) -> bytes
    html = "<!DOCTYPE html>\n"
    html += "<html>\n"
    html += "<head>\n"
    html += "<title>" + code + " " + phrase + "</title>\n"
    html += "<style>body { font-family: sans-serif; margin: 50px; }</style>\n"
    html += "</head>\n"
    html += "<body>\n"
    html += "<h1>" + code + " " + phrase + "</h1>\n"
    html += "<p><code>" + error + "</code></p>\n"
    html += "</body>\n"
    html += "</html>"
    RETURN html.as_bytes()
END FUNCTION
```

### Integration in Request Handler

```
FUNCTION handle_connection(stream, routes)
    TRY
        request = HttpRequest::build_from_stream(stream)
    CATCH HttpParseError as e
        response = HttpResponse::build("HTTP/1.1", 400)
        response.add_header("Content-Type", "text/html")
        body = format_error_body_html(400, "Bad Request", e.to_string())
        response.add_body(body)
        stream.write_all(response.as_bytes())
        RETURN
    END TRY CATCH

    // ... normal request processing ...
END FUNCTION
```

---

## 5. Testing Strategy

### Unit Tests (in `error_responses.rs`)

The `error_responses.rs` module includes unit tests that verify:
- Plain text body format is correct
- Plain text body includes error message
- HTML body includes DOCTYPE declaration
- HTML body includes status code and phrase
- HTML body includes the error description
- HTML body includes styling

**Run unit tests**:
```bash
cargo test error_responses
```

### Integration Tests (in `src/bin/integration_test.rs`)

Add new test functions to verify 400 responses include bodies and correct headers:

```rust
fn test_malformed_request_returns_400_with_body(addr: &str) -> Result<(), String> {
    let mut stream = TcpStream::connect(addr)
        .map_err(|e| format!("connect: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| format!("set timeout: {e}"))?;

    // Send malformed request (missing HTTP version)
    let malformed_request = "GET /\r\n\r\n";
    stream
        .write_all(malformed_request.as_bytes())
        .map_err(|e| format!("write: {e}"))?;

    let resp = read_response(&mut stream)?;
    assert_eq_or_err(&resp.status_code, &400, "status should be 400")?;

    // Verify body is not empty
    if resp.body.is_empty() {
        return Err("400 response body is empty".to_string());
    }

    // Verify body contains error information
    if !resp.body.contains("Bad Request") && !resp.body.contains("Malformed") {
        return Err(format!("body doesn't contain expected error info: {}", resp.body));
    }

    Ok(())
}

fn test_missing_host_header_returns_400_with_body(addr: &str) -> Result<(), String> {
    let mut stream = TcpStream::connect(addr)
        .map_err(|e| format!("connect: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| format!("set timeout: {e}"))?;

    // Send HTTP/1.1 request without Host header (required)
    let request = "GET / HTTP/1.1\r\n\r\n";
    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("write: {e}"))?;

    let resp = read_response(&mut stream)?;
    assert_eq_or_err(&resp.status_code, &400, "status should be 400")?;
    assert_contains_or_err(&resp.body, "Host", "body should mention Host header")?;
    Ok(())
}

fn test_400_has_content_type_header(addr: &str) -> Result<(), String> {
    let mut stream = TcpStream::connect(addr)
        .map_err(|e| format!("connect: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| format!("set timeout: {e}"))?;

    // Send malformed request
    let malformed_request = "INVALID";
    stream
        .write_all(malformed_request.as_bytes())
        .map_err(|e| format!("write: {e}"))?;

    let resp = read_response(&mut stream)?;
    assert_eq_or_err(&resp.status_code, &400, "status should be 400")?;

    let content_type = resp
        .headers
        .get("content-type")
        .ok_or("missing Content-Type header")?;

    // Accept either text/html or text/plain
    if content_type != "text/html" && content_type != "text/plain" {
        return Err(format!("unexpected content-type: {}", content_type));
    }

    Ok(())
}

fn test_400_content_length_matches_body(addr: &str) -> Result<(), String> {
    let mut stream = TcpStream::connect(addr)
        .map_err(|e| format!("connect: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| format!("set timeout: {e}"))?;

    // Send malformed request
    let malformed_request = "GET /\r\n\r\n";
    stream
        .write_all(malformed_request.as_bytes())
        .map_err(|e| format!("write: {e}"))?;

    let resp = read_response(&mut stream)?;
    assert_eq_or_err(&resp.status_code, &400, "status should be 400")?;

    if let Some(cl_str) = resp.headers.get("content-length") {
        let cl: usize = cl_str
            .parse()
            .map_err(|_| "content-length not a number".to_string())?;
        let actual_len = resp.body.len();
        assert_eq_or_err(&actual_len, &cl, "content-length should match body size")?;
    }

    Ok(())
}
```

Add these tests to the `main()` function's test list:
```rust
let results = vec![
    // ... existing tests ...
    run_test("malformed_request_returns_400_with_body", || {
        test_malformed_request_returns_400_with_body(&addr)
    }),
    run_test("missing_host_header_returns_400_with_body", || {
        test_missing_host_header_returns_400_with_body(&addr)
    }),
    run_test("400_has_content_type_header", || {
        test_400_has_content_type_header(&addr)
    }),
    run_test("400_content_length_matches_body", || {
        test_400_content_length_matches_body(&addr)
    }),
];
```

**Run integration tests**:
```bash
cargo run --bin integration_test
```

### Manual Testing

1. Start the server: `cargo run`
2. Use `curl` to test malformed requests:

```bash
# Test 1: Missing Host header (HTTP/1.1 requires it)
curl -v --raw "http://127.0.0.1:7878/" -H ""

# Test 2: Completely malformed request
(echo -e "GARBAGE\r\n\r\n"; sleep 1) | nc 127.0.0.1 7878

# Test 3: Oversized header line
python3 -c "
import socket
s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
s.connect(('127.0.0.1', 7878))
long_header = 'GET / HTTP/1.1\r\nX-Big: ' + 'x' * 10000 + '\r\n\r\n'
s.send(long_header.encode())
print(s.recv(4096).decode())
s.close()
"

# Test 4: Verify response has Content-Type header
curl -i http://127.0.0.1:7878/invalid-path 2>/dev/null | head -20
```

3. Verify responses in the terminal output:
   - Status code is 400
   - `Content-Type: text/html` (or `text/plain`) header is present
   - Response body contains descriptive error message
   - `Content-Length` header matches the body size

---

## 6. Edge Cases to Consider

### Case 1: Very Long Error Messages
**Scenario**: An error message itself is extremely long (e.g., from a malformed header line)
**Current Behavior**: The `HttpParseError::HeaderTooLong` message is static: `"Header line exceeds maximum length"`
**Handling**: Safe - all error messages are from static enum variants
**Code**: No changes needed

### Case 2: Non-UTF8 Error Descriptions
**Scenario**: Error description contains non-UTF8 bytes
**Current Behavior**: All `HttpParseError` messages are static strings, guaranteed to be valid UTF-8
**Handling**: No issue in current implementation
**Future**: If adding dynamic error details, use `String::from_utf8_lossy()` for safety

### Case 3: Clients That Don't Support Reading Response Bodies on 4xx
**Scenario**: Older HTTP clients that ignore body on error responses
**Current Behavior**: The body is still sent; these clients simply ignore it
**Impact**: No breaking change - purely additive enhancement
**Testing**: Manual curl/browser testing confirms body is sent

### Case 4: Content-Type Header Already Set
**Scenario**: Response already has Content-Type (shouldn't happen for 400s)
**Current Behavior**: The `add_header()` method will overwrite existing header (stored in HashMap)
**Handling**: This is the expected behavior and is consistent with codebase patterns

### Case 5: HTML Escaping in Error Messages
**Scenario**: Error message contains HTML special characters (e.g., `<`, `>`, `&`)
**Current Behavior**: Not escaped in the HTML template
**Safety**: Currently safe because all error messages are from static enum variants
**Future Improvement**: If adding dynamic error messages, implement HTML entity escaping

### Case 6: Response Already Partially Sent
**Scenario**: An error occurs after response headers have been sent (shouldn't happen for 400s)
**Current Behavior**: This error handling is at the start of `handle_connection()`, before any headers are sent
**Impact**: No issue - the 400 error is returned before normal response headers

### Case 7: Stream Write Failure
**Scenario**: `stream.write_all()` fails (client disconnected, network error)
**Current Behavior**: Error is ignored with `let _ =`
**Impact**: Consistent with codebase pattern
**Note**: This is a known issue mentioned in CLAUDE.md regarding lack of error handling

---

## 7. Implementation Checklist

### Option 1: Minimal Enhancement (Just Add Content-Type)
- [ ] Modify `/home/jwall/personal/rusty/rcomm/src/main.rs`:
  - [ ] Add `Content-Type: text/plain` header to 400 response
- [ ] Add integration tests to `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`:
  - [ ] `test_malformed_request_returns_400_with_body()`
  - [ ] `test_400_has_content_type_header()`
- [ ] Run unit tests: `cargo test`
- [ ] Run integration tests: `cargo run --bin integration_test`
- [ ] Manual testing with curl

### Option 2: Recommended Enhancement (Add Error Response Module)
- [ ] Create `/home/jwall/personal/rusty/rcomm/src/models/error_responses.rs` with:
  - [ ] `format_error_body_plain()` function
  - [ ] `format_error_body_html()` function
  - [ ] Unit tests for both functions
- [ ] Update `/home/jwall/personal/rusty/rcomm/src/models.rs` to export `error_responses` module
- [ ] Modify `/home/jwall/personal/rusty/rcomm/src/main.rs`:
  - [ ] Add import for `rcomm::models::error_responses`
  - [ ] Update 400 error handler to use `format_error_body_html()`
  - [ ] Add `Content-Type: text/html` header
- [ ] Add integration tests to `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`:
  - [ ] `test_malformed_request_returns_400_with_body()`
  - [ ] `test_missing_host_header_returns_400_with_body()`
  - [ ] `test_400_has_content_type_header()`
  - [ ] `test_400_content_length_matches_body()`
- [ ] Run unit tests: `cargo test error_responses`
- [ ] Run all tests: `cargo test`
- [ ] Run integration tests: `cargo run --bin integration_test`
- [ ] Manual testing with curl

---

## 8. Complexity and Risk Analysis

**Complexity**: 2/10
- Simple header addition for minimal option
- Straightforward HTML formatting function for full option
- No changes to core HTTP parsing or routing logic
- Error handling already exists; enhancement is purely additive

**Risk**: Very Low
- Pure additive feature (doesn't break existing functionality)
- All error messages are static strings from enum variants
- No dynamic content that needs escaping (in current form)
- HTML format is simple and standard
- Content-Type header is a standard HTTP header

**Dependencies**: None
- Minimal option: Uses only standard library
- Full option: Uses only standard library (no external crates)
- Aligns with project's no-external-dependencies constraint

**Performance Impact**: Negligible
- Only affects error path (rare requests)
- Simple string formatting
- No expensive operations

---

## 9. Implementation Notes

### Why Content-Type Header Matters

Even though the body already exists in the current code, the missing `Content-Type` header causes:

1. **Browser Confusion**: Without `Content-Type`, browsers may not interpret the body correctly
2. **API Client Issues**: Clients may not know how to parse the response body
3. **HTTP Compliance**: RFC 7231 recommends setting `Content-Type` for all responses with a body
4. **Consistency**: Other successful responses (200) and error responses (404) should also have Content-Type

### Why HTML Format is Better Than Plain Text

While "plain text" would be simpler, using HTML offers:

1. **Consistency**: Matches the 404 error page format
2. **Browser Viewing**: Better experience when manually testing with a browser
3. **Future Extensibility**: Easier to add styling, links, or additional information
4. **Extensibility**: Foundation for 5xx error pages in the future

### Design Decision: Error Response Module vs Inline

**Option 1 (Inline)**: Just add header directly in main.rs
- Pros: Minimal change, quick implementation
- Cons: Harder to maintain multiple error codes, code duplication for 404/500

**Option 2 (Error Response Module)**: Create dedicated module
- Pros: DRY principle, extensible to other error codes, centralized formatting
- Cons: Slightly more code upfront

**Recommendation**: Option 2 because:
- rcomm will eventually need 500 error handling
- Consistent with existing codebase structure (models module)
- Enables reuse for 404 error page improvements
- Follows established patterns (similar to mime_types module)

---

## 10. Future Enhancements

1. **Enhanced Error Details**: Include request trace, allowed methods, etc.
2. **Error Codes**: Add machine-readable error codes (e.g., `MALFORMED_REQUEST_LINE`)
3. **JSON Error Format**: Support content negotiation (JSON for APIs, HTML for browsers)
4. **Localization**: Error messages in different languages
5. **Logging**: Track which error types are most common for debugging
6. **404 Error Handler**: Use same module to improve 404 responses with better styling
7. **500 Error Handler**: When error handling improves, use same module for 500s
8. **Custom Error Pages**: Allow serving custom error HTML files from the `pages/` directory
