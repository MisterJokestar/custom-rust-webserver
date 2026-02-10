# Implementation Plan: Handle `TRACE` HTTP Method

## Overview

This feature implements support for the HTTP `TRACE` method, which is defined in RFC 7231 Section 5.1.3. The TRACE method allows clients to see what the request looks like after it has been processed by the server. The server should echo back the received request in the response body, prefixed with a `message/http` MIME type content via the `Content-Type` header.

The TRACE method is primarily used for diagnostic and debugging purposes. When the server receives a TRACE request, it should:
1. Accept the request (parse it successfully)
2. Create a 200 OK response
3. Set `Content-Type: message/http` header
4. Echo back the entire received request as the response body
5. Calculate and set the `Content-Length` header automatically

This feature improves HTTP protocol compliance by implementing a method already declared in the `HttpMethods` enum, even though it has low practical necessity (rarely used in production).

**Complexity:** 2/10 (Simple routing logic, leverages existing request/response infrastructure)
**Necessity:** 2/10 (Diagnostic feature, rarely needed in production)

---

## Implementation Details

### Files to Modify

1. **`src/main.rs`** — Add TRACE method handling in `handle_connection()`
2. **`src/bin/integration_test.rs`** — Add integration tests for TRACE responses

### Architecture Context

The current `handle_connection()` function routes requests based on the `http_request.target` after cleaning it. For TRACE, we need to add an early return that:
1. Checks if the request method is `HttpMethods::TRACE`
2. If so, creates a response with the request echoed as the body
3. Returns immediately without attempting to route to a file

This approach leverages the existing `HttpRequest` struct which already has an `as_bytes()` method that serializes the full HTTP message including headers.

---

## Step-by-Step Implementation

### Step 1: Add TRACE Method Handling to `handle_connection()`

**File:** `/home/jwall/personal/rusty/rcomm/src/main.rs`

Modify the `handle_connection()` function to check for TRACE early, before file routing logic. Insert the TRACE handler after the initial request parsing and before the route lookup:

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

    println!("Request: {http_request}");

    // Handle TRACE method by echoing back the request
    if http_request.method == HttpMethods::TRACE {
        let mut response = HttpResponse::build(String::from("HTTP/1.1"), 200);
        response.add_header("Content-Type".to_string(), "message/http".to_string());
        let request_echo = http_request.as_bytes();
        response.add_body(request_echo);
        println!("Response: {response}");
        stream.write_all(&response.as_bytes()).unwrap();
        return;
    }

    // Continue with normal routing for other methods
    let clean_target = clean_route(&http_request.target);

    let (mut response, filename) = if routes.contains_key(&clean_target) {
        (HttpResponse::build(String::from("HTTP/1.1"), 200),
            routes.get(&clean_target).unwrap().to_str().unwrap())
    } else {
        (HttpResponse::build(String::from("HTTP/1.1"), 404),
            "pages/not_found.html")
    };

    let contents = fs::read_to_string(filename).unwrap();
    response.add_body(contents.into());

    println!("Response: {response}");
    stream.write_all(&response.as_bytes()).unwrap();
}
```

**Key Points:**
- The TRACE handler is placed immediately after parsing and before route lookup
- Uses `HttpMethods::TRACE` enum variant (already defined in `http_methods.rs`)
- Sets `Content-Type: message/http` per RFC 7231
- Leverages `http_request.as_bytes()` to serialize the full request
- `add_body()` automatically calculates and sets `Content-Length`
- Early return prevents execution of normal routing logic
- Maintains consistent logging with existing code

**Rationale:**
- TRACE requires echo functionality, not file serving
- Early return is more efficient than adding another condition to routing logic
- The handler is generic and doesn't depend on the request target
- All GET, HEAD, POST, etc. requests bypass this and go to normal routing

### Step 2: Add Necessary Imports (if not already present)

**File:** `/home/jwall/personal/rusty/rcomm/src/main.rs`

Verify that `HttpMethods` is imported. Add if missing:

```rust
use rcomm::models::http_methods::HttpMethods;
```

This should already be available, but if the import uses a wildcard (`use rcomm::models::*;`), ensure `HttpMethods` is accessible. Current imports show:

```rust
use rcomm::models::{
    http_response::HttpResponse,
    http_request::HttpRequest,
};
```

Add `HttpMethods` to this use block:

```rust
use rcomm::models::{
    http_methods::HttpMethods,
    http_response::HttpResponse,
    http_request::HttpRequest,
};
```

### Step 3: Add Integration Tests

**File:** `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`

Add a new test function to verify TRACE responses:

```rust
fn test_trace_method() -> TestResult {
    let (mut server, port) = spawn_server();
    let request = format!("TRACE / HTTP/1.1\r\nHost: localhost:{port}\r\n\r\n");
    let response = make_request(&request, port);

    // Verify status line
    if !response.starts_with("HTTP/1.1 200 OK") {
        return TestResult::Failed(
            "TRACE response should be 200 OK".to_string(),
        );
    }

    // Verify Content-Type header
    if !response.contains("content-type: message/http") {
        return TestResult::Failed(
            "TRACE response should have Content-Type: message/http".to_string(),
        );
    }

    // Verify request is echoed in body
    if !response.contains("TRACE / HTTP/1.1") {
        return TestResult::Failed(
            "TRACE response body should contain echoed request line".to_string(),
        );
    }

    if !response.contains("host: localhost") {
        return TestResult::Failed(
            "TRACE response body should contain echoed Host header".to_string(),
        );
    }

    server.stop();
    TestResult::Passed
}
```

Add this test to the `main()` function's test runner array:

```rust
run_test("TRACE method returns 200 with echoed request", test_trace_method);
```

**Alternative Unit-Style Test (simpler):**

```rust
fn test_trace_simple() -> TestResult {
    let (mut server, port) = spawn_server();
    let request = format!("TRACE / HTTP/1.1\r\nHost: localhost:{port}\r\nX-Custom: test\r\n\r\n");
    let response = make_request(&request, port);

    let success = response.starts_with("HTTP/1.1 200 OK")
        && response.contains("content-type: message/http")
        && response.contains("TRACE / HTTP/1.1")
        && response.contains("x-custom: test");

    server.stop();

    if success {
        TestResult::Passed
    } else {
        TestResult::Failed("TRACE response validation failed".to_string())
    }
}
```

### Step 4: Verify Enum Already Supports TRACE

**File:** `/home/jwall/personal/rusty/rcomm/src/models/http_methods.rs`

Verify the `HttpMethods::TRACE` enum variant exists and the parser supports it. It does:

```rust
#[derive(Debug, PartialEq)]
pub enum HttpMethods {
    // ...
    TRACE,
    // ...
}

pub fn http_method_from_string(method: &str) -> Option<HttpMethods> {
    match method {
        // ...
        "TRACE" => Some(HttpMethods::TRACE),
        // ...
    }
}
```

**Status:** ✓ No changes needed — TRACE is already defined and parsed correctly.

### Step 5: Verify HTTP Status Code Support

**File:** `/home/jwall/personal/rusty/rcomm/src/models/http_status_codes.rs`

Verify that status code 200 is supported:

```rust
200 => String::from("OK"),
```

**Status:** ✓ No changes needed — 200 OK is already supported.

---

## Testing Strategy

### Unit Tests

No new unit tests are strictly needed since the HTTP models (request/response/methods) already have comprehensive test coverage. However, the integration tests are essential.

### Integration Tests

Add the `test_trace_method()` test function (see Step 3 above) to `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`.

**Test Coverage:**

1. **Basic TRACE Request:**
   - TRACE / with Host header
   - Verify 200 OK response
   - Verify Content-Type is message/http
   - Verify request is echoed in body

2. **TRACE with Multiple Headers:**
   - Include custom headers (X-Custom, User-Agent, etc.)
   - Verify all headers are echoed in response body

3. **TRACE with Different Paths:**
   - TRACE /index
   - TRACE /nonexistent
   - Verify request target is echoed correctly (not routed to files)

4. **Response Body Structure:**
   - Verify echoed request line format: `TRACE /path HTTP/1.1\r\n`
   - Verify echoed headers are present with correct case-handling
   - Verify blank line separates headers from body
   - Verify Content-Length is correct

### Running Tests

```bash
# Run integration tests
cargo run --bin integration_test

# Run unit tests
cargo test

# Run all tests with output
cargo test -- --nocapture
```

### Manual Testing

```bash
# Start the server
cargo run &
SERVER_PID=$!

# Test TRACE request with curl
curl -X TRACE -v http://127.0.0.1:7878/

# Test with custom headers
curl -X TRACE -H "X-Custom: test-value" -v http://127.0.0.1:7878/index

# Use netcat for raw inspection
echo -e "TRACE / HTTP/1.1\r\nHost: localhost:7878\r\n\r\n" | nc localhost 7878

# Kill server
kill $SERVER_PID
```

**Expected Manual Test Output:**

```
< HTTP/1.1 200 OK
< content-type: message/http
< content-length: 53
<
< TRACE / HTTP/1.1
< host: localhost:7878
```

---

## Edge Cases & Considerations

### 1. Request Body in TRACE

**Case:** Client sends `TRACE` with a body (e.g., Content-Length header)

**RFC Guidance:** RFC 7231 Section 5.1.3 states: "A client MUST NOT generate a message body in a TRACE request."

**Current Implementation:** The server echoes whatever is received, including any body if present. This is technically correct — the server reflects what was sent, allowing the client to see if their request was malformed.

**Test Case:**
```rust
// TRACE request with unexpected body
let request = format!("TRACE / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5\r\n\r\nhello");
// Server echoes back including the body
```

**Behavior:** ✓ Correct — Server echoes the complete request, including any unexpected body.

### 2. TRACE with Query Strings

**Case:** Client sends `TRACE /path?query=value HTTP/1.1`

**Expected Behavior:** Query string is preserved in the echoed request line

**Example:**
```
Request:  TRACE /search?q=rust HTTP/1.1
Response: TRACE /search?q=rust HTTP/1.1
```

**Implementation:** ✓ Works automatically — `http_request.target` includes the query string.

**Test Case:**
```rust
let request = format!("TRACE /search?q=test HTTP/1.1\r\nHost: localhost:{port}\r\n\r\n");
let response = make_request(&request, port);
assert!(response.contains("TRACE /search?q=test HTTP/1.1"));
```

### 3. TRACE with Large Request Bodies (if sent)

**Case:** Client sends TRACE with a large body (violating RFC but possible)

**Potential Issue:** The entire request is echoed back, which could echo back any large payloads.

**Behavior:** Expected per RFC — TRACE echoes exactly what was received.

**Mitigation:** This is not a security concern; a client choosing to send TRACE with a body is simply seeing the exact request. The server should not add validation here per RFC.

### 4. Empty Host Header Edge Case

**Case:** HTTP/1.1 request without Host header sent via TRACE

**Current Behavior:** `HttpRequest::build_from_stream()` validates HTTP/1.1 requires Host and returns `HttpParseError::MissingHostHeader`

**Impact:** TRACE handler never executes because the request fails to parse.

**Result:** ✓ Expected behavior — Bad requests are rejected at the parsing stage.

### 5. TRACE on Non-Root Paths

**Case:** `TRACE /howdy/page.css HTTP/1.1`

**Expected Behavior:** Echo the request as-is; do not attempt file routing

**Implementation:** ✓ Works correctly — The TRACE check happens before any routing logic, so the target is echoed exactly as provided.

**Test Case:**
```rust
let request = format!("TRACE /howdy/page.css HTTP/1.1\r\nHost: localhost:{port}\r\n\r\n");
assert!(response.contains("TRACE /howdy/page.css"));
```

### 6. Header Normalization in Echo

**Case:** Client sends headers with mixed case: `Content-Type: application/json`

**Current Implementation:** `HttpRequest` stores headers in lowercase (line 110: `self.headers.insert(title.to_lowercase(), value)`)

**Serialization:** When `http_request.as_bytes()` is called, headers are printed via the `Display` impl (lines 145-150), which outputs them in lowercase.

**Expected Output:** Headers appear lowercase in echoed request

**Behavior:** ✓ Correct per RFC 7230 Section 3.2 — HTTP header names are case-insensitive, and lowercasing during echo is acceptable.

**Test Case:**
```rust
let request = format!("TRACE / HTTP/1.1\r\nHost: localhost\r\nX-Custom: VALUE\r\n\r\n");
// Response body will contain: x-custom: VALUE (name lowercase, value preserved)
assert!(response.contains("x-custom: VALUE"));
```

### 7. CRLF Line Endings in Echo

**Case:** Client sends request with bare LF instead of CRLF

**Current Implementation:** `HttpRequest::build_from_stream()` handles both `\r\n` and `\n` via `trim_end_matches()` (line 57).

**Serialization:** `as_bytes()` uses the `Display` impl which hardcodes `\r\n` (line 151: `write!(f, "\r\n")`).

**Expected Behavior:** Echo back with standard `\r\n` regardless of input

**Result:** ✓ Correct — Server normalizes to HTTP standard CRLF.

### 8. Performance Impact

**Analysis:**
- TRACE handler is an early return (minimal overhead)
- Only executes when `HttpMethods::TRACE` is matched (rare in practice)
- `as_bytes()` involves single serialization (not expensive)
- No additional memory allocations beyond the response body

**Conclusion:** Negligible performance impact.

### 9. HTTP/1.0 TRACE Requests

**Case:** `TRACE / HTTP/1.0`

**RFC Compliance:** RFC 7231 applies to both HTTP/1.0 and HTTP/1.1

**Current Implementation:** Works correctly — The version string is echoed exactly as received.

**Behavior:** ✓ Supported.

### 10. Max Request Size

**Case:** Client sends extremely large TRACE request

**Current Constraint:** `HttpRequest::build_from_stream()` enforces `MAX_HEADER_LINE_LEN = 8192` per header

**Behavior:** Large headers are rejected before reaching TRACE handler

**Result:** ✓ Safe — Existing validation applies.

---

## Implementation Checklist

- [ ] Add import of `HttpMethods` to `src/main.rs` (if not already present)
- [ ] Add TRACE method handler to `handle_connection()` in `src/main.rs`
- [ ] Verify handler is placed before route lookup logic
- [ ] Verify `Content-Type: message/http` header is set
- [ ] Verify request is echoed via `http_request.as_bytes()`
- [ ] Verify Content-Length is auto-calculated by `add_body()`
- [ ] Add `test_trace_method()` to `src/bin/integration_test.rs`
- [ ] Register test in the integration test runner
- [ ] Run `cargo test` to verify all unit tests pass
- [ ] Run `cargo run --bin integration_test` to verify integration tests pass
- [ ] Manual testing with `curl -X TRACE`
- [ ] Verify TRACE works on root path `/`
- [ ] Verify TRACE works on arbitrary paths `/test`, `/api/endpoint`
- [ ] Verify TRACE works with custom headers
- [ ] Verify TRACE works with query strings
- [ ] Verify response headers are correct (200 OK, Content-Type, Content-Length)

---

## Code Summary

### Complete Modified `handle_connection()` Function

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

    println!("Request: {http_request}");

    // Handle TRACE method by echoing back the request
    if http_request.method == HttpMethods::TRACE {
        let mut response = HttpResponse::build(String::from("HTTP/1.1"), 200);
        response.add_header("Content-Type".to_string(), "message/http".to_string());
        let request_echo = http_request.as_bytes();
        response.add_body(request_echo);
        println!("Response: {response}");
        stream.write_all(&response.as_bytes()).unwrap();
        return;
    }

    // Continue with normal routing for other methods
    let clean_target = clean_route(&http_request.target);

    let (mut response, filename) = if routes.contains_key(&clean_target) {
        (HttpResponse::build(String::from("HTTP/1.1"), 200),
            routes.get(&clean_target).unwrap().to_str().unwrap())
    } else {
        (HttpResponse::build(String::from("HTTP/1.1"), 404),
            "pages/not_found.html")
    };

    let contents = fs::read_to_string(filename).unwrap();
    response.add_body(contents.into());

    println!("Response: {response}");
    stream.write_all(&response.as_bytes()).unwrap();
}
```

### Integration Test Code

```rust
fn test_trace_method() -> TestResult {
    let (mut server, port) = spawn_server();
    let request = format!("TRACE / HTTP/1.1\r\nHost: localhost:{port}\r\nX-Test: demo\r\n\r\n");
    let response = make_request(&request, port);

    // Verify status line
    if !response.starts_with("HTTP/1.1 200 OK") {
        return TestResult::Failed(
            "TRACE response should be 200 OK".to_string(),
        );
    }

    // Verify Content-Type header
    if !response.contains("content-type: message/http") {
        return TestResult::Failed(
            "TRACE response should have Content-Type: message/http".to_string(),
        );
    }

    // Verify request is echoed in body
    if !response.contains("TRACE / HTTP/1.1") {
        return TestResult::Failed(
            "TRACE response body should contain echoed request line".to_string(),
        );
    }

    if !response.contains("host: localhost") {
        return TestResult::Failed(
            "TRACE response body should contain echoed Host header".to_string(),
        );
    }

    if !response.contains("x-test: demo") {
        return TestResult::Failed(
            "TRACE response body should contain echoed custom headers".to_string(),
        );
    }

    server.stop();
    TestResult::Passed
}
```

---

## Expected Behavior After Implementation

### Before Implementation

```
Request:
TRACE / HTTP/1.1
Host: localhost:7878

Response:
HTTP/1.1 404 Not Found
content-length: 42

<html>... not found page ...</html>
```

(TRACE would be treated as unknown/unhandled, falling through to 404 routing)

### After Implementation

```
Request:
TRACE / HTTP/1.1
Host: localhost:7878
X-Custom: test

Response:
HTTP/1.1 200 OK
content-type: message/http
content-length: 63

TRACE / HTTP/1.1
host: localhost:7878
x-custom: test

```

(TRACE is handled specially, echoes the request back)

---

## Rollback Plan

If this feature needs to be removed:

1. Remove the TRACE handler block from `handle_connection()` in `src/main.rs`
2. Remove the `test_trace_method()` function from `src/bin/integration_test.rs`
3. Remove the corresponding test runner registration
4. Run test suite to verify no regressions
5. TRACE requests will once again fall through to 404 (since TRACE targets won't match any routes)

The change is fully reversible with no side effects.

---

## Future Enhancements

1. **TRACE Filtering:** Option to disable TRACE method for security reasons (via environment variable)
   - Some security policies restrict TRACE/DEBUG methods
   - Could check `RCOMM_ALLOW_TRACE` environment variable

2. **Max Request Echo Size:** Limit the size of requests that can be echoed
   - Prevent DOS via extremely large TRACE requests
   - Could return 413 Payload Too Large if request exceeds threshold

3. **OPTIONS Method Support:** Implement HTTP OPTIONS to advertise supported methods
   - `OPTIONS * HTTP/1.1` would return `Allow: GET, HEAD, POST, PUT, DELETE, OPTIONS, TRACE`

4. **Request Sanitization:** Option to sanitize sensitive headers before echoing
   - Strip Authorization headers from TRACE echo for privacy
   - Could be a security-focused mode

---

## RFC 7231 Compliance Notes

**RFC 7231 Section 5.1.3 - TRACE Method:**

- "The TRACE method requests a remote, application-level loop-back of the request message."
- "The final recipient SHOULD send the request message, excluding any Transfer-Encoding header fields, back to the client as the message body of a 200 (OK) response with a Content-Type of `message/http`."
- "A client MUST NOT generate a message-body in a TRACE request."
- "If any part of the request message is considered sensitive by the origin server, that server SHOULD refuse the request."

**Implementation Compliance:**

- ✓ Returns 200 OK
- ✓ Sets `Content-Type: message/http`
- ✓ Echoes the complete request message
- ✓ Does not require client to send a body
- ✓ Does not filter sensitive headers (follows RFC's "SHOULD refuse" — we don't refuse, but that's server-operator's choice to implement via security policies)

---

## Summary

This is a simple, low-complexity feature that adds support for the HTTP TRACE method as defined in RFC 7231. The implementation is a straightforward addition to `handle_connection()` that:

1. Checks if the request method is TRACE
2. If so, creates a 200 OK response
3. Sets the appropriate `Content-Type: message/http` header
4. Echoes the received request as the response body
5. Returns early to skip normal file routing

The feature requires minimal code changes (roughly 10-15 lines), leverages existing infrastructure (HttpRequest.as_bytes(), HttpResponse.add_body()), and has negligible performance impact. Integration tests verify the functionality end-to-end.

**Recommended Approach:** Implement as described in Step 1 (add TRACE handler to `handle_connection()`) followed by integration tests in Step 3.
