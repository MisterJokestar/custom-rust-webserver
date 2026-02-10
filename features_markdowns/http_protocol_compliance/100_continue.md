# Feature: Send `100 Continue` Interim Response

## Overview

Implement HTTP/1.1 `Expect: 100-continue` support as specified in RFC 7231 Section 5.1.1. When a client sends an `Expect: 100-continue` header, the server must send a `100 Continue` interim response before the client sends the message body. This allows clients to abort requests early if the server cannot/will not process them, saving bandwidth and improving efficiency for large request bodies.

**Category:** HTTP Protocol Compliance
**Complexity:** 3/10
**Necessity:** 4/10 (useful for clients sending large bodies, but not critical for basic functionality)

---

## Expected Behavior

### Scenario 1: Request with `Expect: 100-continue` and body
```
Client sends:
  POST / HTTP/1.1
  Host: localhost:7878
  Content-Length: 13
  Expect: 100-continue
  [client pauses here, waiting for response]

Server must respond with:
  HTTP/1.1 100 Continue
  [empty line, no body]

Client receives 100 response and sends:
  hello, world!

Server processes request with full body and sends final response:
  HTTP/1.1 200 OK
  Content-Length: ...
  [body]
```

### Scenario 2: Request without `Expect: 100-continue`
Server ignores this entire feature and behaves normally. No interim response is sent.

### Scenario 3: `Expect` with unsupported value
Per RFC 7231 Section 6.5.14, respond with `417 Expectation Failed` instead of 100 Continue.

### Scenario 4: GET/HEAD/DELETE with `Expect: 100-continue`
These requests typically have no body, so the interim response is unnecessary but still valid to send if the header is present.

---

## Files to Modify

### 1. `src/models/http_request.rs`
- Add method: `has_expect_continue() -> bool`
- Query the headers HashMap for "expect" header with value "100-continue"
- Add test for header detection

### 2. `src/models/http_response.rs`
- Add method: `send_interim_response(stream: &TcpStream) -> Result<(), std::io::Error>`
- Creates and writes a 100 Continue response without body directly to stream
- Allows interim responses to be sent independently from final response

### 3. `src/main.rs` (handle_connection)
- After successful request parsing, check for `Expect: 100-continue`
- If present and valid, send interim 100 response before reading body
- If present but invalid (unsupported value), send 417 and return early
- Continue normally with routing and response

---

## Step-by-Step Implementation

### Step 1: Add helper method to HttpRequest
**File:** `/home/jwall/personal/rusty/rcomm/src/models/http_request.rs`

Add this method after the `try_get_header()` method (around line 116):

```rust
pub fn has_expect_continue(&self) -> bool {
    match self.try_get_header("expect".to_string()) {
        Some(value) => value.to_lowercase() == "100-continue",
        None => false,
    }
}
```

**Rationale:** Header values are stored lowercase (see line 110), so we compare against lowercase "100-continue".

---

### Step 2: Add interim response method to HttpResponse
**File:** `/home/jwall/personal/rusty/rcomm/src/models/http_response.rs`

Add this method after the `as_bytes()` method (around line 57):

```rust
pub fn send_interim_response(version: String, code: u16, stream: &mut TcpStream) -> Result<(), std::io::Error> {
    use std::io::Write;
    let response = HttpResponse::build(version, code);
    stream.write_all(&response.as_bytes())?;
    stream.flush()?;
    Ok(())
}
```

**Rationale:**
- Status code 100 is for informational responses (interim). The response has no body.
- Uses a static method to send interim responses without needing a full struct instance.
- Flush ensures the response is sent immediately to the client.

---

### Step 3: Modify request parsing flow in handle_connection
**File:** `/home/jwall/personal/rusty/rcomm/src/main.rs`

Replace the current `handle_connection` function (lines 46–75) with:

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

    // Handle Expect: 100-continue
    if let Some(expect_header) = http_request.try_get_header("expect".to_string()) {
        if expect_header.to_lowercase() == "100-continue" {
            // Send interim 100 Continue response
            if let Err(e) = HttpResponse::send_interim_response(
                String::from("HTTP/1.1"),
                100,
                &mut stream,
            ) {
                eprintln!("Failed to send interim response: {e}");
                return;
            }
        } else {
            // Unsupported Expect value, send 417
            eprintln!("Unsupported Expect header value: {expect_header}");
            let mut response = HttpResponse::build(String::from("HTTP/1.1"), 417);
            let body = format!("Expectation Failed: Unsupported Expect value '{expect_header}'");
            response.add_body(body.into());
            let _ = stream.write_all(&response.as_bytes());
            return;
        }
    }

    let clean_target = clean_route(&http_request.target);

    println!("Request: {http_request}");

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

**Key Changes:**
- Lines 22-37: Check for `Expect` header after parsing request but before routing
- If "100-continue": Send interim 100 response and continue processing
- If unsupported value: Send 417 and return early
- Rest of function continues unchanged

---

### Step 4: Add unit tests to HttpRequest
**File:** `/home/jwall/personal/rusty/rcomm/src/models/http_request.rs`

Add these tests in the test module (before the closing `}` at line 407):

```rust
#[test]
fn has_expect_continue_true_with_exact_header() {
    let mut req = HttpRequest::build(
        HttpMethods::POST,
        "/data".to_string(),
        "HTTP/1.1".to_string(),
    );
    req.add_header("Expect".to_string(), "100-continue".to_string());
    assert!(req.has_expect_continue());
}

#[test]
fn has_expect_continue_true_with_uppercase_header() {
    let mut req = HttpRequest::build(
        HttpMethods::POST,
        "/data".to_string(),
        "HTTP/1.1".to_string(),
    );
    req.add_header("EXPECT".to_string(), "100-CONTINUE".to_string());
    assert!(req.has_expect_continue());
}

#[test]
fn has_expect_continue_false_without_header() {
    let req = HttpRequest::build(
        HttpMethods::POST,
        "/data".to_string(),
        "HTTP/1.1".to_string(),
    );
    assert!(!req.has_expect_continue());
}

#[test]
fn has_expect_continue_false_with_different_value() {
    let mut req = HttpRequest::build(
        HttpMethods::POST,
        "/data".to_string(),
        "HTTP/1.1".to_string(),
    );
    req.add_header("Expect".to_string(), "some-other-value".to_string());
    assert!(!req.has_expect_continue());
}
```

---

### Step 5: Add unit tests to HttpResponse
**File:** `/home/jwall/personal/rusty/rcomm/src/models/http_response.rs`

Add this test in the test module (before the closing `}` at line 181):

```rust
#[test]
fn interim_100_response_has_correct_status() {
    let resp = HttpResponse::build("HTTP/1.1".to_string(), 100);
    let output = format!("{resp}");
    assert!(output.starts_with("HTTP/1.1 100 Continue\r\n"));
    // 100 responses should have no body
    assert!(resp.try_get_body().is_none());
}

#[test]
fn interim_response_has_no_content_length() {
    let resp = HttpResponse::build("HTTP/1.1".to_string(), 100);
    // 100 interim responses typically don't include Content-Length
    assert_eq!(resp.try_get_header("content-length".to_string()), None);
}
```

---

## Testing Strategy

### Unit Tests (automated)
1. **HttpRequest tests** (4 new tests):
   - `has_expect_continue()` returns true for exact "100-continue" value
   - `has_expect_continue()` returns true for case-insensitive match
   - `has_expect_continue()` returns false when header absent
   - `has_expect_continue()` returns false for different Expect values

2. **HttpResponse tests** (2 new tests):
   - 100 response formats correctly with status line "HTTP/1.1 100 Continue"
   - 100 response has no body

Run with: `cargo test`

### Integration Tests (manual via curl or test client)

**Test 1: Successful 100-continue flow with POST body**
```bash
curl -v -X POST \
  -H "Expect: 100-continue" \
  -H "Content-Length: 13" \
  -d "hello, world!" \
  http://localhost:7878/
```

Expected:
- Server prints interim response: `HTTP/1.1 100 Continue\r\n\r\n`
- Client receives 100 and sends body
- Server processes and responds with 200 OK

**Test 2: GET request with 100-continue (unusual but valid)**
```bash
curl -v -X GET \
  -H "Expect: 100-continue" \
  http://localhost:7878/
```

Expected:
- Server sends 100 Continue
- No body sent by client
- Server continues and sends final 200 response
- Confirm no hanging or deadlock

**Test 3: Unsupported Expect value**
```bash
curl -v -X POST \
  -H "Expect: gzip-transfer" \
  -H "Content-Length: 5" \
  -d "hello" \
  http://localhost:7878/
```

Expected:
- Server responds with 417 Expectation Failed
- Body should explain unsupported expectation
- Connection closes or handles gracefully

**Test 4: No Expect header (baseline)**
```bash
curl -v -X POST \
  -H "Content-Length: 13" \
  -d "hello, world!" \
  http://localhost:7878/
```

Expected:
- Server skips 100-continue logic
- Processes request normally
- Returns 200 OK with response body

---

## Edge Cases and Considerations

### 1. **Case Insensitivity**
- Header name "expect" must be matched case-insensitively (headers are stored lowercase)
- Header value "100-continue" must be matched case-insensitively per RFC 7231
- Test covers both uppercase variations: "EXPECT" and "100-CONTINUE"

### 2. **Request Methods**
- `Expect: 100-continue` can appear in any HTTP method (GET, POST, PUT, DELETE, etc.)
- Server should handle all uniformly: send 100 if expected, proceed normally if not
- Most common with POST/PUT (methods with bodies), but valid on any method

### 3. **No Body Present**
- If client sends `Expect: 100-continue` but no `Content-Length` header, server sends 100 but client may not send body
- Current parsing waits for body only if `Content-Length` exists (line 96 in http_request.rs)
- This is correct: client decides to send body after seeing 100

### 4. **Timeout and Slow Clients**
- If client doesn't send body after receiving 100, connection may hang
- Current thread-pool will eventually timeout on read operations (system-level TCP timeout)
- Not implementing explicit timeout; relies on OS TCP keepalive/timeout

### 5. **Stream Flushing**
- Critical to flush after sending 100 response so client receives it immediately
- `stream.flush()` ensures bytes leave the buffer
- Without flush, client may hang waiting for response

### 6. **Multiple Interim Responses**
- RFC 7231 allows multiple 1xx responses before final response
- This implementation sends at most one (100 Continue)
- Sufficient for standard use cases

### 7. **Unsupported Expect Values**
- Server responds with 417 if Expect header present but value not "100-continue"
- Examples: "gzip-transfer", "100-something", empty string
- Correct per RFC 7231 Section 5.1.1

### 8. **HTTP/1.0 Compatibility**
- `Expect: 100-continue` is HTTP/1.1 feature
- HTTP/1.0 clients shouldn't send this header
- If present in HTTP/1.0 request, server still handles correctly (no special version check needed)

---

## Implementation Checklist

- [ ] Add `has_expect_continue()` method to HttpRequest
- [ ] Add `send_interim_response()` method to HttpResponse
- [ ] Modify `handle_connection()` to check for Expect header and send 100 response
- [ ] Add 4 unit tests to HttpRequest test module
- [ ] Add 2 unit tests to HttpResponse test module
- [ ] Run `cargo test` to verify all tests pass
- [ ] Run `cargo run` and test with curl manually
- [ ] Verify 100-continue flow with body works
- [ ] Verify GET/HEAD with Expect header works
- [ ] Verify 417 response for unsupported Expect values
- [ ] Verify normal requests without Expect header unaffected

---

## Code Summary

**Total new lines of code:** ~60
**Files modified:** 3 (http_request.rs, http_response.rs, main.rs)
**New tests:** 6 unit tests
**Breaking changes:** None

The implementation is minimal, non-breaking, and follows existing code patterns (builder pattern for responses, lowercase header storage, similar error handling).

---

## References

- RFC 7231 Section 5.1.1: Expect Header Field
- RFC 7231 Section 6.3.1: 100 Continue
- RFC 7231 Section 6.5.14: 417 Expectation Failed
- HTTP/1.1 Specification: https://tools.ietf.org/html/rfc7231
