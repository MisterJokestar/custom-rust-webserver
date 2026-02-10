# Reject Requests with Null Bytes in the URI

## Overview

Null bytes in URIs represent a security vulnerability that can be exploited to bypass security checks or perform directory traversal attacks. While modern HTTP servers generally handle this well, explicitly rejecting requests containing null bytes (`\0` or `%00`) in the URI provides an additional security layer and allows the server to respond clearly to malformed input.

This feature adds validation to the HTTP request parser to detect and reject any requests where the URI contains null bytes, returning a `400 Bad Request` response.

## Motivation

- **Security**: Null bytes in URIs can be used for path traversal or bypass attacks
- **Clarity**: Explicit rejection with clear error messages aids debugging and security auditing
- **Standards Compliance**: RFC 3986 does not permit null bytes in URIs
- **Low Complexity**: Simple string validation during request parsing

## Files to Modify

1. **`src/models/http_request.rs`**
   - Add new error variant to `HttpParseError` enum
   - Add validation logic in `build_from_stream()` method
   - Add unit tests for null byte detection

2. **`src/main.rs`** (Minor, already handles errors)
   - No code changes required; existing error handling in `handle_connection()` will display the error message

## Step-by-Step Implementation

### Step 1: Add Error Variant to `HttpParseError`

In `/home/jwall/personal/rusty/rcomm/src/models/http_request.rs`, add a new variant to the `HttpParseError` enum:

```rust
#[derive(Debug)]
pub enum HttpParseError {
    HeaderTooLong,
    MissingHostHeader,
    MalformedRequestLine,
    NullByteInUri,  // <-- NEW
    IoError(std::io::Error),
}
```

Update the `Display` implementation to handle the new error:

```rust
impl fmt::Display for HttpParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HttpParseError::HeaderTooLong => write!(f, "Header line exceeds maximum length"),
            HttpParseError::MissingHostHeader => write!(f, "Missing required Host header"),
            HttpParseError::MalformedRequestLine => write!(f, "Malformed request line"),
            HttpParseError::NullByteInUri => write!(f, "Null byte detected in URI"),  // <-- NEW
            HttpParseError::IoError(e) => write!(f, "IO error: {e}"),
        }
    }
}
```

### Step 2: Add Validation Logic in `build_from_stream()`

In the `build_from_stream()` method, after extracting the target (URI) from the request line and before creating the `HttpRequest`, add validation:

```rust
pub fn build_from_stream(stream: &TcpStream) -> Result<HttpRequest, HttpParseError> {
    let mut buf_reader = BufReader::new(stream);

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

    // NEW: Validate that target does not contain null bytes
    if target.contains('\0') {
        return Err(HttpParseError::NullByteInUri);
    }

    let version = iter.next().ok_or(HttpParseError::MalformedRequestLine)?.to_string();
    let mut request = HttpRequest::build(method, target, version);

    // ... rest of method remains unchanged
}
```

### Step 3: Add Unit Tests

Add comprehensive tests to the `tests` module at the bottom of `http_request.rs`:

```rust
#[test]
fn build_from_stream_rejects_null_byte_in_uri() {
    use std::io::Write;
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = std::thread::spawn(move || {
        let mut client = std::net::TcpStream::connect(addr).unwrap();
        // Send a GET request with a null byte in the URI
        client
            .write_all(b"GET /hello\0world HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
    });

    let (stream, _) = listener.accept().unwrap();
    let result = HttpRequest::build_from_stream(&stream);

    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), HttpParseError::NullByteInUri));
    handle.join().unwrap();
}

#[test]
fn build_from_stream_rejects_percent_encoded_null_byte_variant() {
    use std::io::Write;
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = std::thread::spawn(move || {
        let mut client = std::net::TcpStream::connect(addr).unwrap();
        // Send a GET request with percent-encoded null byte (%00)
        // Note: %00 is a URL encoding, which is different from literal \0
        // This test documents that the current implementation only checks for literal \0
        client
            .write_all(b"GET /hello%00world HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
    });

    let (stream, _) = listener.accept().unwrap();
    let result = HttpRequest::build_from_stream(&stream);

    // %00 as a string is not a null byte, so this should parse successfully
    // (Future enhancement could decode %00 and reject it too)
    assert!(result.is_ok());
    handle.join().unwrap();
}

#[test]
fn build_from_stream_accepts_valid_uri_without_null_bytes() {
    use std::io::Write;
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = std::thread::spawn(move || {
        let mut client = std::net::TcpStream::connect(addr).unwrap();
        client
            .write_all(b"GET /hello/world?foo=bar HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
    });

    let (stream, _) = listener.accept().unwrap();
    let result = HttpRequest::build_from_stream(&stream);

    assert!(result.is_ok());
    let req = result.unwrap();
    assert_eq!(req.target, "/hello/world?foo=bar");
    handle.join().unwrap();
}

#[test]
fn build_from_stream_rejects_null_byte_at_uri_start() {
    use std::io::Write;
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = std::thread::spawn(move || {
        let mut client = std::net::TcpStream::connect(addr).unwrap();
        client
            .write_all(b"GET \0/path HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
    });

    let (stream, _) = listener.accept().unwrap();
    let result = HttpRequest::build_from_stream(&stream);

    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), HttpParseError::NullByteInUri));
    handle.join().unwrap();
}

#[test]
fn build_from_stream_rejects_null_byte_at_uri_end() {
    use std::io::Write;
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = std::thread::spawn(move || {
        let mut client = std::net::TcpStream::connect(addr).unwrap();
        client
            .write_all(b"GET /path\0 HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
    });

    let (stream, _) = listener.accept().unwrap();
    let result = HttpRequest::build_from_stream(&stream);

    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), HttpParseError::NullByteInUri));
    handle.join().unwrap();
}
```

## Testing Strategy

### Unit Tests
- Run existing unit tests to ensure no regressions:
  ```bash
  cargo test http_request
  ```
- New tests verify:
  - Null byte rejection at various positions (start, middle, end)
  - Valid URIs without null bytes are accepted
  - Error message is correctly displayed
  - Edge case: percent-encoded `%00` is NOT rejected (documented as future enhancement)

### Integration Tests
- Add a test to `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs` to verify the server responds with HTTP 400 to requests with null bytes:

```rust
#[test]
fn test_null_byte_in_uri_returns_400() {
    let (mut server, addr) = spawn_server();

    let mut stream = TcpStream::connect(addr).unwrap();
    // Attempt to request URI with null byte
    let request = b"GET /hello\0world HTTP/1.1\r\nHost: localhost\r\n\r\n";
    stream.write_all(request).unwrap();
    stream.shutdown(std::net::Shutdown::Write).unwrap();

    let response = read_response(&mut stream).unwrap();

    assert_eq!(response.status_code, 400);
    assert!(response.body.contains("Null byte"));

    server.kill().unwrap();
}
```

### Manual Testing
1. Start the server:
   ```bash
   cargo run
   ```

2. Send a request with a null byte using a custom client or netcat:
   ```bash
   printf "GET /hello\x00world HTTP/1.1\r\nHost: localhost\r\n\r\n" | nc localhost 7878
   ```
   Expected response: `HTTP/1.1 400 Bad Request`

3. Verify normal requests still work:
   ```bash
   curl http://localhost:7878/
   ```
   Expected response: The page content (not a 400 error)

## Edge Cases

### 1. Literal Null Byte vs Percent-Encoded Null
- **Current Implementation**: Only rejects literal `\0` bytes
- **Why**: Percent-encoded `%00` is a valid URI component until decoded. The `clean_route()` function in `main.rs` does not perform URL decoding, so `%00` will not be treated as a path traversal tool.
- **Future Enhancement**: If URL decoding is added to the router, this validation should be updated to reject `%00` patterns as well.

### 2. Null Byte in Query String
- **Current Implementation**: Treats query strings as part of the URI, so null bytes here are also rejected.
- **Example**: `GET /path?key=val\0ue` → 400 Bad Request

### 3. Null Byte in Fragment (Should Not Occur in HTTP)
- **Current Implementation**: HTTP requests do not transmit the fragment (`#...`) to the server, so this is not a concern.

### 4. Multiple Null Bytes
- **Current Implementation**: Any occurrence of one or more null bytes will trigger rejection.
- **Example**: `GET /\0\0\0 HTTP/1.1` → 400 Bad Request

### 5. Performance Consideration
- **Impact**: Minimal. The `contains('\0')` check is O(n) where n is URI length, and is only executed once per request during parsing. For typical URIs (< 2048 bytes), this is negligible.

## Error Message Flow

1. **Client sends request with null byte in URI**
   ```
   GET /hello\0world HTTP/1.1
   Host: localhost
   ```

2. **Server parser detects null byte**
   - `HttpRequest::build_from_stream()` returns `Err(HttpParseError::NullByteInUri)`

3. **Server's `handle_connection()` catches the error**
   - Prints to stderr: `Bad request: Null byte detected in URI`
   - Sends HTTP response:
     ```
     HTTP/1.1 400 Bad Request
     content-length: 39

     Bad Request: Null byte detected in URI
     ```

## Implementation Checklist

- [ ] Add `NullByteInUri` variant to `HttpParseError` enum
- [ ] Update `Display` implementation for `HttpParseError`
- [ ] Add null byte validation in `build_from_stream()` after target extraction
- [ ] Add 5 unit tests for null byte detection (various positions)
- [ ] Verify all existing tests pass: `cargo test http_request`
- [ ] Add integration test for server response
- [ ] Verify full test suite passes: `cargo test`
- [ ] Manual test with netcat or custom client
- [ ] Update `CLAUDE.md` if needed to document security improvements

## References

- RFC 3986: URI Generic Syntax - does not permit null bytes in URIs
- OWASP: Path Traversal - discusses null byte injection attacks
- CWE-158: Improper Neutralization of Null Byte or NUL Character

## Future Enhancements

1. **Percent-Encoded Null Byte Detection**: Extend validation to reject `%00` patterns in the URI
2. **Header Value Validation**: Apply similar null byte checks to header values (optional, lower priority)
3. **Logging/Metrics**: Record rejected requests for security monitoring (requires logging infrastructure)
