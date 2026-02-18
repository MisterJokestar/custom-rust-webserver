# Return Structured Error Responses (HTML or JSON) for 4xx and 5xx Errors

**Feature**: Return structured error responses (HTML or JSON) for 4xx and 5xx errors
**Category**: Error Handling
**Complexity**: 3/10
**Necessity**: 5/10

---

## Overview

Currently, error responses from rcomm return plain-text bodies with minimal information. The 400 response includes `"Bad Request: {error}"` as a plain string, and the 404 response serves the full `pages/not_found.html` file. There is no 500 response at all (the server panics instead). This creates an inconsistent experience — some errors return styled HTML, others return raw text, and internal errors crash the process.

### Current State

**400 Bad Request** (`src/main.rs` lines 51-53):
```rust
let mut response = HttpResponse::build(String::from("HTTP/1.1"), 400);
let body = format!("Bad Request: {e}");
response.add_body(body.into());
```
- Plain text body, no Content-Type header
- No HTML structure

**404 Not Found** (`src/main.rs` lines 66-67):
```rust
(HttpResponse::build(String::from("HTTP/1.1"), 404),
    "pages/not_found.html")
```
- Serves full HTML page from `pages/not_found.html`
- Has styled HTML content

**500 Internal Server Error**: Does not exist — the server panics instead of responding.

### Desired State
- All error responses (400, 404, 405, 500, etc.) return well-structured HTML bodies
- HTML includes the status code, a human-readable message, and basic styling
- A `Content-Type: text/html` header is always set on error responses
- Error pages are generated programmatically (no dependency on file existence for error responses)

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/main.rs`

- Add an `error_page()` helper function that generates styled HTML error responses
- Update the 400 Bad Request handler to use `error_page()`
- Update the 404 Not Found handler to use `error_page()` as a fallback if `not_found.html` is unreadable
- Add 500 Internal Server Error responses using `error_page()` (depends on the file read unwrap fix)

### 2. `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`

- Update existing 404 tests to verify HTML structure in response body
- Add tests for 400 response body content

---

## Step-by-Step Implementation

### Step 1: Create Error Page Generator Function

Add to `src/main.rs`:

```rust
fn error_page(status_code: u16, message: &str) -> Vec<u8> {
    let phrase = rcomm::models::http_status_codes::get_status_phrase(status_code);
    format!(
        "<!DOCTYPE html>\
        <html><head>\
        <title>{status_code} {phrase}</title>\
        <style>\
        body {{ font-family: sans-serif; text-align: center; padding: 50px; background: #f5f5f5; }}\
        h1 {{ font-size: 48px; color: #333; }}\
        p {{ font-size: 18px; color: #666; }}\
        </style>\
        </head><body>\
        <h1>{status_code} {phrase}</h1>\
        <p>{message}</p>\
        </body></html>"
    ).into_bytes()
}
```

**Design decisions**:
- Returns `Vec<u8>` to match the `add_body()` signature
- Inline CSS so the error page is fully self-contained (no external stylesheet dependency)
- Uses `get_status_phrase()` from the existing status codes module for the phrase
- Takes a custom `message` parameter for additional context

### Step 2: Update 400 Bad Request Handler

**Current code** (lines 50-56):
```rust
eprintln!("Bad request: {e}");
let mut response = HttpResponse::build(String::from("HTTP/1.1"), 400);
let body = format!("Bad Request: {e}");
response.add_body(body.into());
let _ = stream.write_all(&response.as_bytes());
return;
```

**Updated code**:
```rust
eprintln!("Bad request: {e}");
let mut response = HttpResponse::build(String::from("HTTP/1.1"), 400);
response.add_header("content-type".to_string(), "text/html".to_string());
response.add_body(error_page(400, &format!("{e}")));
let _ = stream.write_all(&response.as_bytes());
return;
```

### Step 3: Update 404 with Fallback Error Page

The 404 case currently reads `pages/not_found.html`. Keep this as the primary behavior but add the generated error page as a fallback if the file is unreadable:

**Updated code** (after the file read unwrap fix is in place):
```rust
let (mut response, filename) = if let Some(path_buf) = routes.get(&clean_target) {
    // ... 200 case ...
} else {
    (HttpResponse::build(String::from("HTTP/1.1"), 404),
        "pages/not_found.html")
};

let contents = match fs::read_to_string(filename) {
    Ok(c) => c.into_bytes(),
    Err(e) => {
        eprintln!("Error reading file {filename}: {e}");
        if response.status_code == 404 {
            // Fallback: generate a 404 page if not_found.html is unreadable
            error_page(404, "The requested page was not found.")
        } else {
            error_page(500, "An internal server error occurred.")
        }
    }
};
response.add_body(contents);
```

Note: This requires exposing `status_code` on `HttpResponse` or tracking it separately. Currently `status_code` is a private field. A simpler approach is to handle the status separately:

```rust
let contents = match fs::read_to_string(filename) {
    Ok(c) => c.into_bytes(),
    Err(e) => {
        eprintln!("Error reading file {filename}: {e}");
        // Build a 500 response since the file read failed
        let mut error_response = HttpResponse::build(String::from("HTTP/1.1"), 500);
        error_response.add_header("content-type".to_string(), "text/html".to_string());
        error_response.add_body(error_page(500, "An internal server error occurred."));
        let _ = stream.write_all(&error_response.as_bytes());
        return;
    }
};
response.add_body(contents);
```

### Step 4: Add Content-Type Header to Error Responses

Ensure all error responses set `Content-Type: text/html`:

```rust
// In the error_page usage pattern:
response.add_header("content-type".to_string(), "text/html".to_string());
response.add_body(error_page(status_code, message));
```

---

## Code Snippets

### Complete Error Page Helper

```rust
fn error_page(status_code: u16, message: &str) -> Vec<u8> {
    let phrase = rcomm::models::http_status_codes::get_status_phrase(status_code);
    format!(
        "<!DOCTYPE html>\
        <html><head>\
        <title>{status_code} {phrase}</title>\
        <style>\
        body {{ font-family: sans-serif; text-align: center; padding: 50px; background: #f5f5f5; }}\
        h1 {{ font-size: 48px; color: #333; }}\
        p {{ font-size: 18px; color: #666; }}\
        </style>\
        </head><body>\
        <h1>{status_code} {phrase}</h1>\
        <p>{message}</p>\
        </body></html>"
    ).into_bytes()
}
```

### Example Error Response Build Pattern

```rust
fn send_error(stream: &mut TcpStream, status_code: u16, message: &str) {
    let mut response = HttpResponse::build(String::from("HTTP/1.1"), status_code);
    response.add_header("content-type".to_string(), "text/html".to_string());
    response.add_body(error_page(status_code, message));
    let _ = stream.write_all(&response.as_bytes());
}
```

This helper could be used to simplify all error paths:
```rust
// 400
send_error(&mut stream, 400, &format!("{e}"));
return;

// 500
send_error(&mut stream, 500, "An internal server error occurred.");
return;
```

---

## Edge Cases

### 1. Error Message Contains HTML Characters
**Scenario**: A parse error includes `<` or `>` characters (e.g., `"unexpected token '<'"`)
**Handling**: The message should be HTML-escaped before embedding. Use a simple escape function:
```rust
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
     .replace('<', "&lt;")
     .replace('>', "&gt;")
     .replace('"', "&quot;")
}
```
**Risk**: Without escaping, a crafted request could inject HTML into the error page. This is a minor XSS concern since the error page is not served from a domain with cookies, but best practice is to escape.

### 2. Very Long Error Messages
**Scenario**: A parse error produces a very long message.
**Handling**: Truncate the message in the HTML to a reasonable length (e.g., 200 characters). The full message is still logged via `eprintln!()`.

### 3. Client Expects JSON
**Scenario**: An API client sends `Accept: application/json` and expects JSON error responses.
**Handling**: Out of scope for initial implementation. HTML is the default. Content negotiation could be added later by checking the `Accept` header.

### 4. not_found.html Missing
**Scenario**: The `pages/not_found.html` file doesn't exist or is unreadable.
**Handling**: The generated `error_page()` serves as a fallback, so the client always gets a well-formed error page even if the custom 404 template is missing.

---

## Testing Strategy

### Integration Tests

```rust
fn test_400_returns_html_body(addr: &str) -> Result<(), String> {
    // Send a malformed request
    let mut stream = TcpStream::connect(addr).map_err(|e| format!("connect: {e}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| format!("timeout: {e}"))?;
    stream.write_all(b"BADREQUEST\r\n\r\n")
        .map_err(|e| format!("write: {e}"))?;
    let resp = read_response(&mut stream)?;
    assert_eq_or_err(&resp.status_code, &400, "status")?;
    assert_contains_or_err(&resp.body, "<!DOCTYPE html>", "html structure")?;
    assert_contains_or_err(&resp.body, "400", "status code in body")?;
    Ok(())
}

fn test_404_returns_html_body(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/nonexistent")?;
    assert_eq_or_err(&resp.status_code, &404, "status")?;
    // Body should contain HTML (either from not_found.html or generated)
    assert_contains_or_err(&resp.body, "<", "html content")?;
    Ok(())
}
```

---

## Implementation Checklist

- [ ] Add `error_page()` function to `src/main.rs`
- [ ] Add `html_escape()` helper for safe message embedding
- [ ] Update 400 handler to use structured HTML body
- [ ] Add `Content-Type: text/html` header to all error responses
- [ ] Update 500 handler to use structured HTML body (after file read fix)
- [ ] Add fallback for missing `not_found.html`
- [ ] Consider extracting `send_error()` helper to reduce duplication
- [ ] Add integration tests for error response HTML structure
- [ ] Run `cargo test` — no regressions
- [ ] Run `cargo run --bin integration_test` — all tests pass

---

## Backward Compatibility

- **404 responses**: Still serve `pages/not_found.html` when available. Existing tests pass unchanged.
- **400 responses**: Body changes from plain text to HTML. No existing integration tests check the 400 body format.
- **New 500 responses**: Previously the server panicked; now it returns HTML. Strictly better.

---

## Related Features

- **Error Handling > handle_connection File Read 500**: Must be implemented first (or concurrently) since 500 responses don't exist yet
- **Routing > Custom 404 Page**: The generated error page serves as a fallback when the custom 404 file is unavailable
- **Routing > Custom Error Pages**: This feature provides the foundation for configurable error pages per status code
- **HTTP Protocol > Content-Type Header**: Error responses should set Content-Type, which this feature addresses
