# Feature: Respond to OPTIONS Requests with Allow Header

## Overview

The HTTP OPTIONS method is used by clients to request information about communication options available for a given resource or server. This feature implements support for OPTIONS requests by responding with an `Allow` header that lists the supported HTTP methods.

**Status:** Not Implemented
**Complexity:** 2/10
**Necessity:** 5/10 (HTTP spec compliance, useful for CORS preflight scenarios)
**RFC Reference:** [RFC 7231, Section 4.3.7](https://tools.ietf.org/html/rfc7231#section-4.3.7)

---

## Current State

The rcomm server currently:
- Defines the `OPTIONS` variant in `HttpMethods` enum
- Parses OPTIONS from request lines (via `http_method_from_string()`)
- Treats OPTIONS requests like any other HTTP method in `handle_connection()` — attempting to serve file content
- Has no special handling to respond with allowed methods

The server only supports serving static files (GET requests) matching routes from the `pages/` directory structure.

---

## Goal

When an OPTIONS request arrives:
1. Respond with HTTP 200 OK
2. Include an `Allow` header listing supported methods for the requested resource
3. Return an empty body (per HTTP spec)
4. Handle both existing routes and non-existent routes

For rcomm's current implementation, the supported methods are:
- `GET` — fetch resource content
- `HEAD` — fetch headers only (not yet implemented, but declared)
- `OPTIONS` — query supported methods

All other methods (POST, PUT, DELETE, PATCH, TRACE, CONNECT) are not implemented.

---

## Architecture & Design Decisions

### Supported Methods Strategy

rcomm should report which methods are actually functional:

**Option A (Strict):** Only list `GET` and `OPTIONS` (the truly working methods)
**Option B (Forward-looking):** List `GET`, `HEAD`, and `OPTIONS` (declaring intent to support HEAD later)
**Chosen: Option A** — Only report methods that work today. Better for client expectations and standards compliance (RFC 7231: "The server SHOULD NOT respond with 405 for methods that are declared in Allow").

### Route-Specific Allow Headers

The Allow header should describe what's available at the **requested resource path**, not globally:

- `OPTIONS /` → `Allow: GET, HEAD, OPTIONS`
- `OPTIONS /nonexistent` → `Allow: GET, HEAD, OPTIONS` (if route supported GET)
- `OPTIONS /nonexistent` → `Allow: OPTIONS` (if route didn't exist, no GET possible)

**Chosen approach:** Return the same Allow header for all routes (`GET, OPTIONS`). This simplifies implementation and avoids routing complexity. A stricter approach would check route existence, but the core feature is to declare available methods uniformly.

### Response Structure

Per RFC 7231, an OPTIONS response:
- Status code: `200 OK`
- Headers: `Allow: GET, OPTIONS` (comma-separated list, case-insensitive per spec but we use uppercase for consistency)
- Body: Empty (no Content-Length needed for empty body per HTTP/1.1)

Example:
```
HTTP/1.1 200 OK
Allow: GET, OPTIONS
Content-Length: 0

```

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Current behavior:**
- `handle_connection()` treats all HTTP methods identically
- Attempts to load file content based on routing table

**Changes required:**
- After parsing request and cleaning route, check if method is OPTIONS
- If OPTIONS: build response with Allow header, empty body, return early
- If not OPTIONS: continue existing file-serving logic

**Key section:**
```rust
fn handle_connection(mut stream: TcpStream, routes: HashMap<String, PathBuf>) {
    let http_request = match HttpRequest::build_from_stream(&stream) {
        Ok(req) => req,
        Err(e) => { /* ... */ }
    };
    let clean_target = clean_route(&http_request.target);

    // NEW: Handle OPTIONS requests
    if http_request.method == HttpMethods::OPTIONS {
        let mut response = HttpResponse::build(String::from("HTTP/1.1"), 200);
        response.add_header(String::from("Allow"), String::from("GET, OPTIONS"));
        println!("Response: {response}");
        stream.write_all(&response.as_bytes()).unwrap();
        return;
    }

    // Existing file-serving logic continues...
}
```

---

## Step-by-Step Implementation

### Step 1: Import HttpMethods Enum

In `/home/jwall/personal/rusty/rcomm/src/main.rs`, ensure HttpMethods is imported:

```rust
use rcomm::models::{
    http_response::HttpResponse,
    http_request::HttpRequest,
    http_methods::HttpMethods,  // ADD THIS
};
```

**Rationale:** The OPTIONS method handling requires comparing `http_request.method == HttpMethods::OPTIONS`.

---

### Step 2: Add OPTIONS Check in handle_connection

In `/home/jwall/personal/rusty/rcomm/src/main.rs`, immediately after parsing and cleaning the route:

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

    // Handle OPTIONS requests
    if http_request.method == HttpMethods::OPTIONS {
        let mut response = HttpResponse::build(String::from("HTTP/1.1"), 200);
        response.add_header(String::from("Allow"), String::from("GET, OPTIONS"));
        println!("Response: {response}");
        stream.write_all(&response.as_bytes()).unwrap();
        return;
    }

    // Existing file-serving logic continues...
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

**Placement:** After `println!("Request: {http_request}");` and before the file-serving logic.

**Logic flow:**
1. Parse request (existing)
2. Clean route target (existing)
3. Log request (existing)
4. **NEW:** Check if method is OPTIONS → respond and return
5. Existing file-serving logic (unchanged)

---

### Step 3: Verify HttpResponse Behavior with Empty Body

No changes needed to `HttpResponse`. The existing implementation already:
- Stores `body: Option<Vec<u8>>`
- When body is `None`, `as_bytes()` returns headers only
- Does not add `Content-Length: 0` header automatically

Test this behavior with a unit test (see Testing section).

---

## Code Snippets

### Complete Updated handle_connection Function

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

    // Handle OPTIONS requests
    if http_request.method == HttpMethods::OPTIONS {
        let mut response = HttpResponse::build(String::from("HTTP/1.1"), 200);
        response.add_header(String::from("Allow"), String::from("GET, OPTIONS"));
        println!("Response: {response}");
        stream.write_all(&response.as_bytes()).unwrap();
        return;
    }

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

### Updated Imports in main.rs

```rust
use std::{
    collections::HashMap,
    fs,
    io::prelude::*,
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
};
use rcomm::ThreadPool;
use rcomm::models::{
    http_response::HttpResponse,
    http_request::HttpRequest,
    http_methods::HttpMethods,  // ADD THIS LINE
};
```

---

## Testing Strategy

### Unit Tests

Add unit tests to `/home/jwall/personal/rusty/rcomm/src/main.rs` to verify:

1. **OPTIONS request on root returns Allow header**
2. **OPTIONS request on non-existent route returns Allow header**
3. **Allow header contains GET and OPTIONS**
4. **Response has no body**
5. **Response status code is 200**

Note: `main.rs` currently has no unit tests (they're in library code). Tests should ideally be added to lib.rs if OPTIONS handling is extracted to a library function, or added as comments documenting expected behavior.

### Integration Tests

Add test cases to `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`:

```rust
fn test_options_root(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "OPTIONS", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;

    // Allow header should be present
    let allow = resp.headers.get("allow")
        .ok_or("Allow header missing")?;

    // Should contain both methods
    if !allow.contains("GET") {
        return Err("Allow header missing GET".to_string());
    }
    if !allow.contains("OPTIONS") {
        return Err("Allow header missing OPTIONS".to_string());
    }

    // Body should be empty
    if !resp.body.is_empty() {
        return Err(format!("Expected empty body, got {} bytes", resp.body.len()));
    }

    Ok(())
}

fn test_options_nonexistent_route(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "OPTIONS", "/does-not-exist")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;

    let allow = resp.headers.get("allow")
        .ok_or("Allow header missing")?;

    if !allow.contains("GET") || !allow.contains("OPTIONS") {
        return Err("Allow header incomplete".to_string());
    }

    if !resp.body.is_empty() {
        return Err(format!("Expected empty body, got {} bytes", resp.body.len()));
    }

    Ok(())
}

fn test_options_existing_route(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "OPTIONS", "/howdy")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;

    let allow = resp.headers.get("allow")
        .ok_or("Allow header missing")?;

    if !allow.contains("GET") || !allow.contains("OPTIONS") {
        return Err("Allow header incomplete".to_string());
    }

    Ok(())
}
```

Add to main test runner (in the `main()` function):

```rust
fn main() {
    let port = pick_free_port();
    let addr = format!("127.0.0.1:{port}");
    let mut server = start_server(port);

    if let Err(e) = wait_for_server(&addr, Duration::from_secs(5)) {
        eprintln!("{e}");
        let _ = server.kill();
        std::process::exit(1);
    }

    let mut results = vec![
        run_test("GET root route", || test_root_route(&addr)),
        run_test("GET index.css", || test_index_css(&addr)),
        run_test("GET howdy route", || test_howdy_route(&addr)),
        run_test("GET howdy/page.css", || test_howdy_page_css(&addr)),
        run_test("404 does not exist", || test_404_does_not_exist(&addr)),
        run_test("404 deep path", || test_404_deep_path(&addr)),
        run_test("Content-Length matches", || test_content_length_matches(&addr)),
        run_test("trailing slash", || test_trailing_slash(&addr)),
        run_test("double slash", || test_double_slash(&addr)),
        run_test("concurrent requests", || test_concurrent_requests(&addr)),
        run_test("OPTIONS root", || test_options_root(&addr)),
        run_test("OPTIONS non-existent", || test_options_nonexistent_route(&addr)),
        run_test("OPTIONS existing route", || test_options_existing_route(&addr)),
    ];

    // ... rest of test runner
}
```

### Manual Testing

Use curl to verify OPTIONS behavior:

```bash
# Start server
cargo run &
SERVER_PID=$!

# Test OPTIONS on root
curl -v -X OPTIONS http://127.0.0.1:7878/

# Expected output:
# < HTTP/1.1 200 OK
# < allow: GET, OPTIONS
# <
# [no body]

# Test OPTIONS on nonexistent route
curl -v -X OPTIONS http://127.0.0.1:7878/does-not-exist

# Test OPTIONS on nested route
curl -v -X OPTIONS http://127.0.0.1:7878/howdy

kill $SERVER_PID
```

---

## Edge Cases & Considerations

### 1. Empty vs Missing Body

**Case:** OPTIONS response with no Content-Length header

**Current behavior:** `HttpResponse.as_bytes()` doesn't add Content-Length when body is None.

**Expected:** HTTP/1.1 allows empty body without Content-Length in some cases. rcomm's implementation already handles this correctly.

**Test:** Verify response body is truly empty (0 bytes).

---

### 2. Case Sensitivity of Allow Header

**Case:** Different capitalization in the Allow header value

**RFC 7231:** Method tokens are case-sensitive, but convention is uppercase.

**Implementation:** Use uppercase `"GET, OPTIONS"` (consistent with how HttpMethods::Display works).

**Test:** Verify Allow header contains uppercase tokens.

---

### 3. Wildcard Allow (OPTIONS *)

**Case:** Some clients send `OPTIONS *` (requesting server-wide options, not resource-specific)

**Current approach:** Treat `*` like any other path. It won't match a route, but OPTIONS still returns the same Allow header.

**Alternative:** Could special-case `target == "*"` to skip route checking.

**Chosen:** Treat `*` as a path. This is simpler and acceptable for a static file server.

**Test:** Verify `OPTIONS *` also returns `200 OK` with Allow header.

---

### 4. HEAD Method Support

**Case:** Allow header declares `GET, OPTIONS`. Should it declare `HEAD`?

**Decision:** No. HEAD is not currently implemented. Only declare methods that actually work. Clients that see GET in Allow know they can use HEAD (per HTTP spec), so this is acceptable.

**Future:** When HEAD is implemented, add it: `"GET, HEAD, OPTIONS"`.

---

### 5. Other HTTP Methods

**Case:** Client sends POST, PUT, DELETE, PATCH, TRACE, CONNECT

**Current behavior:** Server attempts to fetch file, gets 404, returns not_found.html

**Post-OPTIONS:** Same behavior. These methods still aren't implemented. Per RFC 7231, if a method isn't in the Allow header, returning 405 Method Not Allowed is recommended, but 404 is acceptable for a static file server.

**Test:** Verify POST to existing route still returns 404 (not 405). This is a behavioral decision to keep simple.

---

### 6. OPTIONS Request with Body

**Case:** Client sends OPTIONS with a request body (unusual but allowed by spec)

**Current behavior:** `HttpRequest::build_from_stream()` parses body if Content-Length is present.

**Handler behavior:** Body is ignored; response is same.

**Test:** Verify OPTIONS with body still returns correct response (body is parsed but not used).

---

### 7. Multiple Routes with Different Files

**Case:** Should Allow header differ based on whether a route is GET-able?

**Example:**
- `OPTIONS /` (index.html exists) → Allow: GET, OPTIONS
- `OPTIONS /missing` (no file) → Allow: OPTIONS (only)

**Current choice:** Return same Allow header for all routes. This simplifies implementation and is acceptable (static server only serves GET anyway).

**Future enhancement:** Could check route existence and conditionally include GET.

---

### 8. Route Normalization

**Case:** Requests like `OPTIONS /howdy/` or `OPTIONS //howdy` (trailing/double slashes)

**Current behavior:** `clean_route()` normalizes these to `/howdy`.

**Handler behavior:** OPTIONS returns 200 regardless of route.

**Test:** Verify normalized paths still return OPTIONS correctly.

---

## Implementation Checklist

- [ ] Add `HttpMethods` import to `/home/jwall/personal/rusty/rcomm/src/main.rs`
- [ ] Add OPTIONS check in `handle_connection()` after route cleaning
- [ ] Build response with status 200 and Allow header
- [ ] Return early (don't serve files for OPTIONS)
- [ ] Test with curl manually
- [ ] Add integration tests
- [ ] Verify integration tests pass: `cargo run --bin integration_test`
- [ ] Run unit tests: `cargo test`
- [ ] Review code for style consistency

---

## Success Criteria

1. **HTTP/1.1 Compliance:** OPTIONS requests return 200 OK with Allow header
2. **Header Format:** Allow header lists "GET, OPTIONS" (comma-separated, uppercase)
3. **Empty Body:** Response body is empty (no Content-Length needed)
4. **Route-Agnostic:** Same response for all paths (root, existing, nonexistent)
5. **No Breaking Changes:** GET requests continue to work as before
6. **Tests Pass:** All integration and unit tests pass
7. **Clean Code:** No .unwrap() added; consistent style with existing code

---

## Code Diff Summary

**File: `/home/jwall/personal/rusty/rcomm/src/main.rs`**

```diff
use rcomm::models::{
    http_response::HttpResponse,
    http_request::HttpRequest,
+   http_methods::HttpMethods,
};

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

+   // Handle OPTIONS requests
+   if http_request.method == HttpMethods::OPTIONS {
+       let mut response = HttpResponse::build(String::from("HTTP/1.1"), 200);
+       response.add_header(String::from("Allow"), String::from("GET, OPTIONS"));
+       println!("Response: {response}");
+       stream.write_all(&response.as_bytes()).unwrap();
+       return;
+   }

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

---

## Future Enhancements

1. **Route-Specific Allow Headers:** Check if the requested route is GET-able before including GET in Allow header
2. **HEAD Method Implementation:** Add HEAD method support and declare in Allow header
3. **Method Not Allowed Responses:** Return 405 for unsupported methods (POST, PUT, DELETE, etc.) instead of 404
4. **OPTIONS for Specific Methods:** Handle `OPTIONS *` differently (server-wide options vs. resource-specific)
5. **Accept-* Headers:** Parse and respect client Accept headers in OPTIONS responses

---

## References

- [RFC 7231 - HTTP/1.1 Semantics, Section 4.3.7 (OPTIONS)](https://tools.ietf.org/html/rfc7231#section-4.3.7)
- [MDN - OPTIONS Method](https://developer.mozilla.org/en-US/docs/Web/HTTP/Methods/OPTIONS)
- [HTTP Allow Header](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Allow)
