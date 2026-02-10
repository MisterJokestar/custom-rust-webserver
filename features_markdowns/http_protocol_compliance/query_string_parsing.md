# Query String Parsing & Stripping Implementation Plan

## Overview

This feature addresses HTTP protocol compliance by implementing proper parsing and stripping of query strings (`?key=value`) from request URIs before route lookup. Currently, rcomm does not handle query strings, which means requests like `GET /page?foo=bar HTTP/1.1` will fail to route correctly because the `target` field includes the entire query string.

According to RFC 3986 and HTTP specifications, query strings are part of the request URI but should not participate in route matching. This feature will:

1. Parse and extract the query string component from the request URI
2. Store the query string separately in the `HttpRequest` struct for potential future use
3. Strip the query string from the `target` field before route matching in `handle_connection()`
4. Maintain backward compatibility with existing request handling

**Complexity**: 2/10
**Necessity**: 8/10 (Query strings are fundamental to HTTP)

---

## Files to Modify

1. **`src/models/http_request.rs`**
   - Add new field `query_string: Option<String>` to `HttpRequest` struct
   - Add method `try_get_query_string()` to access the parsed query string
   - Modify `build()` to initialize the new field
   - Refactor `build_from_stream()` to parse and separate query strings
   - Add unit tests for query string parsing edge cases

2. **`src/main.rs`**
   - Modify `handle_connection()` to use `clean_route()` on the URI (already done, no change needed)
   - Add integration test support for query string requests in `src/bin/integration_test.rs`

3. **`src/bin/integration_test.rs`** (optional, for testing)
   - Add test case for simple query strings
   - Add test case for multiple query parameters
   - Add test case for special characters in query values

---

## Step-by-Step Implementation

### Step 1: Add Query String Field to HttpRequest Struct

**File**: `src/models/http_request.rs`

Add a new field to store the parsed query string:

```rust
#[derive(Debug)]
pub struct HttpRequest {
    pub method: HttpMethods,
    pub target: String,          // URI path only, without query string
    pub version: String,
    headers: HashMap<String, String>,
    body: Option<Vec<u8>>,
    query_string: Option<String>, // NEW: Stores "key=value&foo=bar" (without the ?)
}
```

**Rationale**:
- Storing the query string separately allows future features (like form parsing, logging) to access it
- Keeping it optional handles requests without query strings cleanly
- The `target` field will now represent only the path component for route matching

---

### Step 2: Add Query String Accessor Method

**File**: `src/models/http_request.rs`

Add a public getter method (following the existing pattern):

```rust
impl HttpRequest {
    pub fn try_get_query_string(&self) -> Option<String> {
        self.query_string.clone()
    }
}
```

**Rationale**:
- Consistent with existing accessor methods like `try_get_header()` and `try_get_body()`
- Returns `Option<String>` to handle both present and absent query strings

---

### Step 3: Update HttpRequest::build() Constructor

**File**: `src/models/http_request.rs`

Update the `build()` method to initialize the new field:

```rust
pub fn build(method: HttpMethods, target: String, version: String) -> HttpRequest {
    let headers = HashMap::<String, String>::new();
    HttpRequest {
        method,
        target,
        version,
        headers,
        body: None,
        query_string: None, // NEW
    }
}
```

**Rationale**:
- Ensures all manually constructed `HttpRequest` objects are valid
- Default behavior is no query string (None), which is safe

---

### Step 4: Implement Query String Parsing in build_from_stream()

**File**: `src/models/http_request.rs`

Modify the request line parsing section to extract and strip the query string:

**Current Code (lines 54-68)**:
```rust
// Parse request line
let mut line = String::new();
buf_reader.read_line(&mut line).map_err(HttpParseError::IoError)?;
let line = line.trim_end_matches(|c| c == '\r' || c == '\n');

if line.len() > MAX_HEADER_LINE_LEN {
    return Err(HttpParseError::HeaderTooLong);
}

let mut iter = line.split_whitespace();
let method_str = iter.next().ok_or(HttpParseError::MalformedRequestLine)?;
let method = http_method_from_string(method_str).ok_or(HttpParseError::MalformedRequestLine)?;
let target = iter.next().ok_or(HttpParseError::MalformedRequestLine)?.to_string();
let version = iter.next().ok_or(HttpParseError::MalformedRequestLine)?.to_string();
let mut request = HttpRequest::build(method, target, version);
```

**New Code**:
```rust
// Parse request line
let mut line = String::new();
buf_reader.read_line(&mut line).map_err(HttpParseError::IoError)?;
let line = line.trim_end_matches(|c| c == '\r' || c == '\n');

if line.len() > MAX_HEADER_LINE_LEN {
    return Err(HttpParseError::HeaderTooLong);
}

let mut iter = line.split_whitespace();
let method_str = iter.next().ok_or(HttpParseError::MalformedRequestLine)?;
let method = http_method_from_string(method_str).ok_or(HttpParseError::MalformedRequestLine)?;
let full_uri = iter.next().ok_or(HttpParseError::MalformedRequestLine)?;
let version = iter.next().ok_or(HttpParseError::MalformedRequestLine)?.to_string();

// Split URI into path and query string
let (target, query_string) = if let Some(pos) = full_uri.find('?') {
    let (path, query) = full_uri.split_at(pos);
    (path.to_string(), Some(query[1..].to_string())) // Skip the '?' character
} else {
    (full_uri.to_string(), None)
};

let mut request = HttpRequest::build(method, target, version);
request.query_string = query_string;
```

**Rationale**:
- Uses `String::find()` for efficient query string detection
- Splits on the first `?` only (per RFC 3986, multiple `?` are invalid but we handle gracefully)
- Strips the leading `?` from the query string component
- Stores the result in the new field

---

### Step 5: Update the Display Trait Implementation

**File**: `src/models/http_request.rs`

The `Display` implementation (lines 140-152) currently outputs the original target. For logging clarity, consider whether to include query strings in display. **Recommended: Keep query string out of Display** because:
- The `target` field will now be path-only
- Keeping logs clean (query strings can contain sensitive data)
- HTTP wire format reconstruction can exclude query strings

**No change needed** unless you want to document this behavior:

```rust
impl fmt::Display for HttpRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Note: Displays path only, not query string
        // This is appropriate for logging (query strings may contain sensitive data)
        let _ = match write!(f, "{} {} {}\r\n", self.method, self.target, self.version) {
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
```

---

## Testing Strategy

### Unit Tests (in `src/models/http_request.rs`)

Add these tests to the existing `#[cfg(test)]` module:

#### Test 1: Parse simple query string
```rust
#[test]
fn build_from_stream_parses_query_string() {
    use std::io::Write;
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = std::thread::spawn(move || {
        let mut client = std::net::TcpStream::connect(addr).unwrap();
        client
            .write_all(b"GET /search?q=hello HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
    });

    let (stream, _) = listener.accept().unwrap();
    let req = HttpRequest::build_from_stream(&stream).unwrap();

    assert_eq!(req.method.to_string(), "GET");
    assert_eq!(req.target, "/search");
    assert_eq!(req.try_get_query_string(), Some("q=hello".to_string()));
    handle.join().unwrap();
}
```

#### Test 2: Parse multiple query parameters
```rust
#[test]
fn build_from_stream_parses_multiple_query_params() {
    use std::io::Write;
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = std::thread::spawn(move || {
        let mut client = std::net::TcpStream::connect(addr).unwrap();
        client
            .write_all(b"GET /page?foo=bar&baz=qux&id=123 HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
    });

    let (stream, _) = listener.accept().unwrap();
    let req = HttpRequest::build_from_stream(&stream).unwrap();

    assert_eq!(req.target, "/page");
    assert_eq!(req.try_get_query_string(), Some("foo=bar&baz=qux&id=123".to_string()));
    handle.join().unwrap();
}
```

#### Test 3: Handle no query string (backward compatibility)
```rust
#[test]
fn build_from_stream_handles_no_query_string() {
    use std::io::Write;
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = std::thread::spawn(move || {
        let mut client = std::net::TcpStream::connect(addr).unwrap();
        client
            .write_all(b"GET /page HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
    });

    let (stream, _) = listener.accept().unwrap();
    let req = HttpRequest::build_from_stream(&stream).unwrap();

    assert_eq!(req.target, "/page");
    assert_eq!(req.try_get_query_string(), None);
    handle.join().unwrap();
}
```

#### Test 4: Parse query string with special characters (URL-encoded)
```rust
#[test]
fn build_from_stream_parses_encoded_query_string() {
    use std::io::Write;
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = std::thread::spawn(move || {
        let mut client = std::net::TcpStream::connect(addr).unwrap();
        client
            .write_all(b"GET /search?q=hello%20world&lang=en-US HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
    });

    let (stream, _) = listener.accept().unwrap();
    let req = HttpRequest::build_from_stream(&stream).unwrap();

    assert_eq!(req.target, "/search");
    assert_eq!(req.try_get_query_string(), Some("q=hello%20world&lang=en-US".to_string()));
    handle.join().unwrap();
}
```

#### Test 5: Parse query string on root path
```rust
#[test]
fn build_from_stream_parses_query_on_root() {
    use std::io::Write;
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = std::thread::spawn(move || {
        let mut client = std::net::TcpStream::connect(addr).unwrap();
        client
            .write_all(b"GET /?theme=dark HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
    });

    let (stream, _) = listener.accept().unwrap();
    let req = HttpRequest::build_from_stream(&stream).unwrap();

    assert_eq!(req.target, "/");
    assert_eq!(req.try_get_query_string(), Some("theme=dark".to_string()));
    handle.join().unwrap();
}
```

### Integration Tests (in `src/bin/integration_test.rs`)

Add test cases to verify end-to-end routing with query strings:

#### Test 1: Route with query string returns 200
```rust
fn test_query_string_routing(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/?page=1")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    assert_contains_or_err(&resp.body, "Hello!", "body")?;
    Ok(())
}
```

#### Test 2: Non-existent route with query string still 404
```rust
fn test_query_string_404(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/notfound?search=test")?;
    assert_eq_or_err(&resp.status_code, &404, "status")?;
    Ok(())
}
```

#### Test 3: Multiple parameters
```rust
fn test_query_string_multiple_params(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/howdy?name=cowboy&greeting=yippee")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    assert_contains_or_err(&resp.body, "Howdy!", "body")?;
    Ok(())
}
```

Add these to the `main()` function's test vector:
```rust
let results = vec![
    // ... existing tests ...
    run_test("query_string_routing", || test_query_string_routing(&addr)),
    run_test("query_string_404", || test_query_string_404(&addr)),
    run_test("query_string_multiple_params", || test_query_string_multiple_params(&addr)),
];
```

---

## Edge Cases & Handling

| Edge Case | Current Behavior | Expected Behavior | Implementation |
|-----------|-----------------|-------------------|-----------------|
| No query string | Works | Works | Already handled by Optional field |
| Empty query string (`/?`) | Path parsed as `/` | Path parsed as `/`, query string is empty string | Check: `if let Some(pos) = full_uri.find('?')` will split, `query[1..]` will be `""` → `Some("")` |
| Multiple `?` chars (`/?a=1?b=2`) | Entire string becomes path | Only split on first `?`, rest is part of query | `find('?')` returns first occurrence, so we get path `/` and query `a=1?b=2` ✓ |
| Fragment identifier (`/?a=1#section`) | Fragment included in target | Fragment is rare in server requests, but we handle it | HTTP requests don't include fragments, but if sent, treated as part of query string (safe, though unusual) |
| Special characters in query (`/?name=John%20Doe`) | Preserved as-is | Preserved as-is, no decoding in HTTP layer | We don't decode; that's for application layer ✓ |
| Query string with spaces (invalid but sent) | Preserved | Preserved | Same as above ✓ |
| Very long query string | Checked against MAX_HEADER_LINE_LEN | Rejected if > 8192 bytes | Existing `line.len()` check applies to entire request line ✓ |
| Semicolon parameters (`/?a=1;b=2`) | Preserved | Preserved | Not split on `;`, entire string kept together ✓ |

---

## Code Changes Summary

### File: `src/models/http_request.rs`

**Changes**:
1. Add `query_string: Option<String>` field to struct (line ~36)
2. Update `build()` to initialize `query_string: None` (line ~47)
3. Add `try_get_query_string()` method (after `try_get_body()`)
4. Refactor `build_from_stream()` request line parsing (lines 54-68) to split on `?`
5. Add 5 new unit tests to test module (at end of file)

**Estimated lines changed**: ~30 lines added, ~5 lines modified

### File: `src/main.rs`

**Changes**: None required. The existing `clean_route()` function and route lookup logic will work correctly since `http_request.target` no longer includes the query string.

### File: `src/bin/integration_test.rs`

**Changes**:
1. Add 3 new test functions (optional, for comprehensive testing)
2. Add them to the `results` vector in `main()`

**Estimated lines added**: ~20 lines

---

## Verification Checklist

- [ ] All new unit tests pass (`cargo test http_request::tests`)
- [ ] Integration tests pass (`cargo run --bin integration_test`)
- [ ] Request to `/path?query=value` serves `/path` content with 200 status
- [ ] Request to `/invalid?query=value` returns 404
- [ ] Request to `/path` (no query) still works (backward compatibility)
- [ ] `cargo test` passes all 34+ existing tests
- [ ] No `.unwrap()` added in new code (use `map_err` for errors)
- [ ] Query string is not displayed in request logs (privacy)

---

## Backward Compatibility

This change is **fully backward compatible**:
- Existing requests without query strings continue to work (query_string is None)
- The `target` field still contains the path, minus the query string
- Existing route matching in `handle_connection()` works identically
- The `HttpRequest::build()` constructor maintains the same signature
- Existing tests do not require modification

---

## Future Enhancements

Once this feature is implemented, future work could include:

1. **Query String Parsing**: Add a method to parse `query_string` into a `HashMap<String, Vec<String>>` for easier access
2. **URL Decoding**: Decode percent-encoded characters in query values
3. **Form POST Parsing**: Parse `application/x-www-form-urlencoded` request bodies
4. **Logging Redaction**: Exclude query strings from logs if they contain sensitive data (e.g., `password`, `token`)
5. **Route Parameters**: Support dynamic routes like `/user/:id?expand=details`

---

## Implementation Order

1. Modify `HttpRequest` struct and add new field (Step 1)
2. Update `build()` constructor (Step 3)
3. Implement `try_get_query_string()` accessor (Step 2)
4. Refactor `build_from_stream()` for query string parsing (Step 4)
5. Add unit tests (Testing Strategy section)
6. Add integration tests (Testing Strategy section)
7. Run full test suite and verify
8. No changes needed in `main.rs`

---

## Estimated Effort

- **Implementation**: 30-45 minutes
- **Testing**: 30-45 minutes
- **Code Review**: 15-20 minutes
- **Total**: ~1.5 hours

Complexity is low due to straightforward string splitting with no external dependencies.
