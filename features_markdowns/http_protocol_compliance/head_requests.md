# Implementation Plan: Handle HEAD Requests Correctly

## Overview

The HTTP HEAD method is semantically identical to GET, except the server must not return a message body in the response. The server MUST include all the headers it would include in a GET response, including Content-Length, so clients can determine resource metadata without transferring the full content.

**Current State**: The server's `handle_connection()` function in `src/main.rs` only explicitly handles GET requests. While the HTTP method is parsed correctly (HEAD is defined in `src/models/http_methods.rs`), the handler treats all non-GET requests the same way—returning a 404 or sending the full body regardless of method.

**Desired State**: HEAD requests should be processed identically to GET requests but with the response body stripped before transmission, allowing clients to efficiently query resource metadata.

## Files to Modify

1. **`src/main.rs`** — Primary changes
   - Modify `handle_connection()` to track the HTTP method
   - Conditionally omit the body from response bytes for HEAD requests
   - Pattern: Determine Content-Length and headers as if GET, then strip body before `write_all()`

2. **`src/models/http_response.rs`** — Secondary changes (helper method)
   - Add a new method `as_bytes_headers_only()` to return status line + headers without body
   - Alternative: Add conditional logic to `as_bytes()` (less clean)
   - Recommended: New dedicated method for clarity and testability

3. **`src/models/http_methods.rs`** — No changes needed
   - Already defines HEAD enum variant and parses it correctly
   - Verify parsing in existing tests

## Step-by-Step Implementation

### Step 1: Add Helper Method to HttpResponse

**File**: `src/models/http_response.rs`

Add a new public method `as_bytes_headers_only()` that returns only headers without the body:

```rust
pub fn as_bytes_headers_only(&self) -> Vec<u8> {
    format!("{self}").as_bytes().to_vec()
}
```

This leverages the existing `Display` implementation (lines 60–74) which intentionally excludes the body. The method is simple but improves code clarity in the caller.

**Tests to add** (after line 181 in `http_response.rs`):

```rust
#[test]
fn as_bytes_headers_only_excludes_body() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
    resp.add_header("Content-Type".to_string(), "text/html".to_string());
    resp.add_body(b"<h1>Test</h1>".to_vec());

    let headers_only = resp.as_bytes_headers_only();
    let text = String::from_utf8(headers_only).unwrap();

    // Should include status and headers
    assert!(text.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(text.contains("content-type: text/html\r\n"));
    assert!(text.contains("content-length: 14\r\n"));
    // Should NOT include body
    assert!(!text.contains("Test"));
    assert!(text.ends_with("\r\n"));
}

#[test]
fn as_bytes_headers_only_with_no_body() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 404);
    resp.add_header("Server".to_string(), "rcomm".to_string());

    let headers_only = resp.as_bytes_headers_only();
    let text = String::from_utf8(headers_only).unwrap();

    assert!(text.starts_with("HTTP/1.1 404 Not Found\r\n"));
    assert!(text.contains("server: rcomm\r\n"));
    assert!(text.ends_with("\r\n"));
}
```

### Step 2: Modify handle_connection() to Respect HEAD Method

**File**: `src/main.rs`

Modify the `handle_connection()` function (lines 46–75) to:
1. Extract the HTTP method from the request
2. Prepare the response with headers and body as normal
3. Use different serialization method for HEAD vs. GET

**Current code (lines 46–75)**:
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

**Replace with**:
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
    let clean_target = clean_route(&http_request.target);

    println!("Request: {http_request}");

    // Track the request method for later serialization decision
    let request_method = http_request.method.clone();

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

    // For HEAD requests, send headers only; for all others (GET, etc.), send full response
    let response_bytes = match request_method {
        rcomm::models::http_methods::HttpMethods::HEAD => response.as_bytes_headers_only(),
        _ => response.as_bytes(),
    };
    stream.write_all(&response_bytes).unwrap();
}
```

**Rationale**:
- Stores `request_method` early to make the intent explicit
- Prepare the response normally (including body and Content-Length header) to maintain correctness
- Only the serialization differs based on the method
- The `as_bytes_headers_only()` call includes Content-Length, allowing clients to know the size

### Step 3: Ensure HttpMethods Implements Clone

**File**: `src/models/http_methods.rs`

The `HttpMethods` enum must be `Clone` to move it in the handler. Verify or add derive:

```rust
#[derive(Debug, PartialEq, Clone)]  // Add Clone if not present
pub enum HttpMethods {
    // ...
}
```

Check line 4 and add `Clone` to the derive list if missing.

### Step 4: Update Unit Tests in http_response.rs

Run the new tests:

```bash
cargo test http_response::tests::as_bytes_headers_only -v
```

Expected: 2 new tests pass.

### Step 5: Integration Tests

**File**: `src/bin/integration_test.rs`

Add integration tests for HEAD requests. Use the existing `send_request()` helper (line 158) with method name `"HEAD"`.

Add after the last test function (around line 300+):

```rust
fn test_head_root_route(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "HEAD", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;

    // Should have Content-Length header
    assert!(resp.headers.contains_key("content-length"), "missing content-length");

    // Should have no body
    assert_eq_or_err(&resp.body, &"".to_string(), "HEAD should have empty body")?;

    // For reference, get the actual size with GET
    let get_resp = send_request(addr, "GET", "/")?;
    let content_length: usize = resp
        .headers
        .get("content-length")
        .ok_or("no content-length")?
        .parse()
        .map_err(|e: std::num::ParseIntError| e.to_string())?;
    assert_eq_or_err(&(get_resp.body.len() as u64), &(content_length as u64), "content-length mismatch")?;

    Ok(())
}

fn test_head_howdy_route(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "HEAD", "/howdy")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;

    // Should have Content-Length header
    assert!(resp.headers.contains_key("content-length"), "missing content-length");

    // Should have no body
    assert_eq_or_err(&resp.body, &"".to_string(), "HEAD should have empty body")?;

    Ok(())
}

fn test_head_nonexistent_route(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "HEAD", "/nonexistent")?;
    assert_eq_or_err(&resp.status_code, &404, "status")?;

    // Even 404 should include Content-Length and headers
    assert!(resp.headers.contains_key("content-length"), "missing content-length");

    // Should have no body
    assert_eq_or_err(&resp.body, &"".to_string(), "HEAD should have empty body")?;

    Ok(())
}

fn test_head_css_file(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "HEAD", "/index.css")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;

    // Should have Content-Length
    assert!(resp.headers.contains_key("content-length"), "missing content-length");

    // Should have no body
    assert_eq_or_err(&resp.body, &"".to_string(), "HEAD should have empty body")?;

    Ok(())
}
```

Register these tests in the `main()` function by adding them to the `results` vector collection (typically around line 350+):

```rust
results.push(run_test("HEAD root route", || test_head_root_route(&addr)));
results.push(run_test("HEAD /howdy route", || test_head_howdy_route(&addr)));
results.push(run_test("HEAD nonexistent route", || test_head_nonexistent_route(&addr)));
results.push(run_test("HEAD CSS file", || test_head_css_file(&addr)));
```

## Testing Strategy

### Unit Tests

1. **Test `as_bytes_headers_only()` in http_response.rs**:
   - Verify that headers (including Content-Length) are present
   - Verify that the body is NOT present
   - Test both success (200) and error (404) responses
   - Verify Content-Length is correctly calculated

2. **Run existing unit tests**:
   ```bash
   cargo test models::http_response
   ```
   Expected: All existing tests + 2 new tests pass

### Integration Tests

1. **HEAD to valid routes**:
   - HEAD /  (should return 200 with index.html headers, no body)
   - HEAD /howdy  (should return 200 with page.html headers, no body)
   - HEAD /index.css  (should return 200 with CSS headers, no body)

2. **HEAD to nonexistent routes**:
   - HEAD /missing  (should return 404 with not_found.html headers, no body)

3. **Header verification**:
   - Content-Length must equal the size of the GET response body
   - Status code must match GET
   - All headers except body must match GET

4. **Run integration tests**:
   ```bash
   cargo run --bin integration_test
   ```
   Expected: 4 new tests pass, all existing tests still pass (12 + 4 = 16 total)

### Manual Testing

```bash
cargo build
cargo run &
sleep 2
# Test with curl
curl -I http://127.0.0.1:7878/  # HEAD request (curl -I)
curl -I http://127.0.0.1:7878/howdy
curl -I http://127.0.0.1:7878/nonexistent
# Compare with GET
curl http://127.0.0.1:7878/  # Full response
# Verify Content-Length matches body size
```

Expected behavior:
- HEAD response has no body (`\r\n` immediately after headers)
- Content-Length equals the size of GET response body
- Status codes match GET
- All headers match GET

## Edge Cases

### 1. **HEAD on 404 Responses**
- **Scenario**: Client requests HEAD on nonexistent resource
- **Expected Behavior**: Return 404 status with not_found.html headers + Content-Length, but no body
- **Implementation**: Already handled—404 path goes through same response preparation (lines 66–68)
- **Test**: `test_head_nonexistent_route()`

### 2. **Content-Length Header Correctness**
- **Scenario**: Resource body changes; Content-Length must reflect current size
- **Expected Behavior**: Content-Length reflects the actual file size on disk
- **Implementation**: `add_body()` calculates length from actual bytes (http_response.rs:38–40)
- **Test**: `test_head_root_route()` compares HEAD Content-Length with GET body size

### 3. **Large Files**
- **Scenario**: Client uses HEAD to check size of large file before GET
- **Expected Behavior**: Server calculates Content-Length without streaming file to client
- **Implementation**: Already correct—file is read and added to body (main.rs:70), then only headers are sent
- **Test**: Manual test with large files in pages/ directory

### 4. **Empty Resources**
- **Scenario**: Client requests HEAD on empty file or directory index
- **Expected Behavior**: Return 200 with Content-Length: 0, no body
- **Implementation**: `add_body()` handles empty vectors correctly
- **Test**: Add test for empty index.html if applicable

### 5. **Method Case Sensitivity**
- **Scenario**: Client sends "head" or "Head" instead of "HEAD"
- **Expected Behavior**: Server rejects as malformed (per HTTP spec and existing behavior)
- **Implementation**: http_request.rs:65 uses `http_method_from_string()` which requires uppercase
- **Test**: Existing behavior; verify in parsing tests (http_methods.rs:60–64)

### 6. **Response Status Codes**
- **Scenario**: HEAD should return same status codes as GET (200, 404, 500, etc.)
- **Expected Behavior**: Status code depends on route resolution, not method
- **Implementation**: Response preparation is identical to GET; only body is stripped
- **Test**: `test_head_nonexistent_route()` verifies 404; others verify 200

### 7. **Custom Headers in Response**
- **Scenario**: Server adds custom headers (e.g., Content-Type, Cache-Control)
- **Expected Behavior**: All headers must be present in HEAD response
- **Implementation**: Headers are prepared before method-specific serialization
- **Test**: Verify all headers are present in `test_head_*` tests

### 8. **Concurrent HEAD Requests**
- **Scenario**: Multiple clients send HEAD requests simultaneously
- **Expected Behavior**: Thread pool handles them independently; no interference
- **Implementation**: `handle_connection()` is thread-safe; each thread gets own request/response objects
- **Test**: Manual test with ab (Apache Bench) or similar

## Checklist

- [ ] Add `Clone` derive to `HttpMethods` if missing
- [ ] Add `as_bytes_headers_only()` method to `HttpResponse`
- [ ] Add unit tests for `as_bytes_headers_only()` (2 tests)
- [ ] Modify `handle_connection()` to track request method and conditionally serialize
- [ ] Add import for `HttpMethods` if needed in main.rs
- [ ] Add 4 integration tests for HEAD requests
- [ ] Run `cargo test` (all unit tests pass)
- [ ] Run `cargo run --bin integration_test` (all integration tests pass)
- [ ] Manual testing with curl -I
- [ ] Verify Content-Length correctness in HEAD responses
- [ ] Verify no body in HEAD responses
- [ ] Test edge cases (404, empty files, large files)

## Success Criteria

1. **All Tests Pass**:
   - `cargo test` shows 36 unit tests (34 + 2 new)
   - `cargo run --bin integration_test` shows 16 tests (12 + 4 new), all passing

2. **Protocol Compliance**:
   - HEAD requests receive same headers as GET
   - HEAD responses contain Content-Length but no body
   - Status codes match corresponding GET requests

3. **Client Behavior**:
   - `curl -I http://localhost:7878/` succeeds with headers only
   - Response headers match `curl http://localhost:7878/` (except body)
   - Content-Length value matches actual body size

4. **Code Quality**:
   - No `.unwrap()` added in response serialization path
   - Method intent is clear from code (use of `as_bytes_headers_only()`)
   - No breaking changes to existing API

## Implementation Difficulty: 2/10

**Rationale**:
- HttpMethods enum already supports HEAD parsing
- HttpResponse already has all infrastructure (headers, body separate)
- Only 3 new method calls needed in main handler
- No complex logic—just conditional serialization
- Well-scoped with minimal side effects

## Risk Assessment: Low

- **Backward Compatibility**: Existing GET requests unaffected
- **Performance**: Negligible overhead (one enum match)
- **Correctness**: Logic mirrors HEAD RFC 7231 specification exactly
- **Testing**: Comprehensive test coverage for all cases
