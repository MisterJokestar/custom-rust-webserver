# Transfer-Encoding: chunked for Request Bodies Implementation Plan

## 1. Overview of the Feature

The HTTP `Transfer-Encoding: chunked` mechanism is defined in RFC 7230 Section 4.1 as a method for transmitting request bodies when the sender does not know the content length in advance. This is commonly used by clients sending form data, streaming data, or dynamically generated content.

Currently, rcomm only supports reading request bodies via the `Content-Length` header. When a client sends a chunked-encoded request body (e.g., using HTTP/1.1 with `Transfer-Encoding: chunked`), the server cannot parse it and will either hang waiting for a `Content-Length` header or misinterpret the body.

**Goal**: Implement parsing and reassembly of chunked transfer encoding in request bodies, enabling the server to handle requests from clients that use chunked encoding.

**Scope**:
- Parse chunked request bodies (decoding chunk size headers and data)
- Reassemble chunks into a complete body buffer
- Store the reassembled body in `HttpRequest` as if it were from `Content-Length`
- Handle edge cases (empty chunks, chunk extensions, trailing headers)

**Standards Compliance**:
- RFC 7230 Section 4.1 (Transfer-Encoding and Chunked Encoding)
- RFC 7230 Section 3.3.3 (Message Body Length)
- Prerequisite: Must NOT have both `Content-Length` and `Transfer-Encoding: chunked` present

**Impact**:
- HTTP protocol compliance: Support modern client implementations and testing tools
- Interoperability: Accept requests from curl, wget, and other tools using chunked encoding
- Testing: Enable integration tests to send chunked payloads
- Future-proofing: Prepare for HTTP/2 and streaming scenarios

---

## 2. Files to be Modified or Created

### Modified Files

1. **`/home/jwall/personal/rusty/rcomm/src/models/http_request.rs`**
   - Extend `HttpParseError` enum with new error variants
   - Add chunked body parsing logic to `build_from_stream()`
   - Add helper method for decoding chunked data (or inline logic)

### New Files

2. **`/home/jwall/personal/rusty/rcomm/src/models/chunked_encoding.rs`** (recommended)
   - Create a dedicated module for chunked encoding/decoding
   - Provides `parse_chunked_body()` function
   - Handles chunk size parsing, validation, and reassembly
   - Unit tests for various chunk scenarios

3. **Update `/home/jwall/personal/rusty/rcomm/src/models.rs`**
   - Export the new `chunked_encoding` module

---

## 3. Step-by-Step Implementation Details

### Step 1: Extend HttpParseError Enum

**File**: `/home/jwall/personal/rusty/rcomm/src/models/http_request.rs`

Add new error variants to handle chunked encoding issues:

```rust
#[derive(Debug)]
pub enum HttpParseError {
    HeaderTooLong,
    MissingHostHeader,
    MalformedRequestLine,
    IoError(std::io::Error),
    InvalidChunkSize,           // Chunk size line is not valid hex
    InvalidChunkTerminator,     // Missing CRLF after chunk data
    TrailingHeadersNotSupported, // Trailer headers present (not yet supported)
    ChunkedWithContentLength,   // Both Transfer-Encoding and Content-Length present
}
```

Update the `Display` implementation to include error messages for new variants:

```rust
impl fmt::Display for HttpParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HttpParseError::HeaderTooLong => write!(f, "Header line exceeds maximum length"),
            HttpParseError::MissingHostHeader => write!(f, "Missing required Host header"),
            HttpParseError::MalformedRequestLine => write!(f, "Malformed request line"),
            HttpParseError::IoError(e) => write!(f, "IO error: {e}"),
            HttpParseError::InvalidChunkSize => write!(f, "Invalid chunk size format"),
            HttpParseError::InvalidChunkTerminator => write!(f, "Chunk not properly terminated with CRLF"),
            HttpParseError::TrailingHeadersNotSupported => write!(f, "Trailing headers in chunked encoding not supported"),
            HttpParseError::ChunkedWithContentLength => write!(f, "Cannot have both Content-Length and Transfer-Encoding: chunked"),
        }
    }
}
```

### Step 2: Create the Chunked Encoding Module

**File**: `/home/jwall/personal/rusty/rcomm/src/models/chunked_encoding.rs`

```rust
use std::io::{BufReader, prelude::*};
use std::net::TcpStream;
use super::http_request::HttpParseError;

const MAX_CHUNK_SIZE_LINE_LEN: usize = 8192;

/// Parses a chunked request body from a BufReader.
/// Returns the complete reassembled body as a Vec<u8>.
///
/// # Chunked Encoding Format (RFC 7230 Section 4.1)
/// chunk-size [ chunk-ext ] CRLF
/// chunk-data CRLF
/// ...
/// 0 CRLF
/// [ trailer ]
/// CRLF
pub fn parse_chunked_body(
    buf_reader: &mut BufReader<&TcpStream>,
) -> Result<Vec<u8>, HttpParseError> {
    let mut body = Vec::new();

    loop {
        // Read chunk size line
        let mut size_line = String::new();
        buf_reader
            .read_line(&mut size_line)
            .map_err(HttpParseError::IoError)?;

        let size_line = size_line.trim_end_matches(|c| c == '\r' || c == '\n');

        if size_line.len() > MAX_CHUNK_SIZE_LINE_LEN {
            return Err(HttpParseError::HeaderTooLong);
        }

        // Parse chunk size (may have chunk extensions, e.g., "1a;name=value")
        let chunk_size_str = size_line
            .split(';')
            .next()
            .ok_or(HttpParseError::InvalidChunkSize)?
            .trim();

        // Parse hex chunk size
        let chunk_size = u32::from_str_radix(chunk_size_str, 16)
            .map_err(|_| HttpParseError::InvalidChunkSize)? as usize;

        // If chunk size is 0, we've reached the end
        if chunk_size == 0 {
            // Handle optional trailing headers
            // For now, skip them until we read an empty line
            loop {
                let mut trailer_line = String::new();
                buf_reader
                    .read_line(&mut trailer_line)
                    .map_err(HttpParseError::IoError)?;

                let trailer_line = trailer_line.trim_end_matches(|c| c == '\r' || c == '\n');
                if trailer_line.is_empty() {
                    break; // End of message
                }
                // Skip trailer headers for now
                // Future: could parse and store trailer headers
            }
            break;
        }

        // Read chunk data
        let mut chunk_data = vec![0u8; chunk_size];
        buf_reader
            .read_exact(&mut chunk_data)
            .map_err(HttpParseError::IoError)?;

        body.append(&mut chunk_data);

        // Read trailing CRLF after chunk data
        let mut terminator = String::new();
        buf_reader
            .read_line(&mut terminator)
            .map_err(HttpParseError::IoError)?;

        let terminator = terminator.trim_end_matches(|c| c == '\r' || c == '\n');
        if !terminator.is_empty() {
            return Err(HttpParseError::InvalidChunkTerminator);
        }
    }

    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;

    #[test]
    fn parse_single_chunk() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let handle = std::thread::spawn(move || {
            let mut client = std::net::TcpStream::connect(addr).unwrap();
            // Single chunk: "hello" (5 bytes in hex = 0x5)
            client
                .write_all(b"5\r\nhello\r\n0\r\n\r\n")
                .unwrap();
            client.shutdown(std::net::Shutdown::Write).unwrap();
        });

        let (stream, _) = listener.accept().unwrap();
        let mut buf_reader = BufReader::new(&stream);
        let body = parse_chunked_body(&mut buf_reader).unwrap();

        assert_eq!(body, b"hello");
        handle.join().unwrap();
    }

    #[test]
    fn parse_multiple_chunks() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let handle = std::thread::spawn(move || {
            let mut client = std::net::TcpStream::connect(addr).unwrap();
            // Multiple chunks: "hel" + "lo"
            client
                .write_all(b"3\r\nhel\r\n2\r\nlo\r\n0\r\n\r\n")
                .unwrap();
            client.shutdown(std::net::Shutdown::Write).unwrap();
        });

        let (stream, _) = listener.accept().unwrap();
        let mut buf_reader = BufReader::new(&stream);
        let body = parse_chunked_body(&mut buf_reader).unwrap();

        assert_eq!(body, b"hello");
        handle.join().unwrap();
    }

    #[test]
    fn parse_empty_chunk_ends_message() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let handle = std::thread::spawn(move || {
            let mut client = std::net::TcpStream::connect(addr).unwrap();
            // Single chunk then end marker
            client
                .write_all(b"5\r\nhello\r\n0\r\n\r\n")
                .unwrap();
            client.shutdown(std::net::Shutdown::Write).unwrap();
        });

        let (stream, _) = listener.accept().unwrap();
        let mut buf_reader = BufReader::new(&stream);
        let body = parse_chunked_body(&mut buf_reader).unwrap();

        assert_eq!(body, b"hello");
        handle.join().unwrap();
    }

    #[test]
    fn parse_chunk_with_extension() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let handle = std::thread::spawn(move || {
            let mut client = std::net::TcpStream::connect(addr).unwrap();
            // Chunk with extension (name=value), should be ignored
            client
                .write_all(b"5;name=value\r\nhello\r\n0\r\n\r\n")
                .unwrap();
            client.shutdown(std::net::Shutdown::Write).unwrap();
        });

        let (stream, _) = listener.accept().unwrap();
        let mut buf_reader = BufReader::new(&stream);
        let body = parse_chunked_body(&mut buf_reader).unwrap();

        assert_eq!(body, b"hello");
        handle.join().unwrap();
    }

    #[test]
    fn parse_large_chunk_size() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let handle = std::thread::spawn(move || {
            let mut client = std::net::TcpStream::connect(addr).unwrap();
            // 256 bytes in hex = 0x100
            let data = vec![b'x'; 256];
            client.write_all(b"100\r\n").unwrap();
            client.write_all(&data).unwrap();
            client.write_all(b"\r\n0\r\n\r\n").unwrap();
            client.shutdown(std::net::Shutdown::Write).unwrap();
        });

        let (stream, _) = listener.accept().unwrap();
        let mut buf_reader = BufReader::new(&stream);
        let body = parse_chunked_body(&mut buf_reader).unwrap();

        assert_eq!(body.len(), 256);
        assert!(body.iter().all(|&b| b == b'x'));
        handle.join().unwrap();
    }

    #[test]
    fn invalid_chunk_size_returns_error() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let handle = std::thread::spawn(move || {
            let mut client = std::net::TcpStream::connect(addr).unwrap();
            // Invalid hex: "ZZ" is not valid hex
            client
                .write_all(b"ZZ\r\nhello\r\n")
                .unwrap();
            client.shutdown(std::net::Shutdown::Write).unwrap();
        });

        let (stream, _) = listener.accept().unwrap();
        let mut buf_reader = BufReader::new(&stream);
        let result = parse_chunked_body(&mut buf_reader);

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), HttpParseError::InvalidChunkSize));
        handle.join().unwrap();
    }

    #[test]
    fn missing_chunk_terminator_returns_error() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let handle = std::thread::spawn(move || {
            let mut client = std::net::TcpStream::connect(addr).unwrap();
            // Missing CRLF after chunk data - immediately send 0 size (which is read as terminator line)
            client
                .write_all(b"5\r\nhello")
                .unwrap();
            client.shutdown(std::net::Shutdown::Write).unwrap();
        });

        let (stream, _) = listener.accept().unwrap();
        let mut buf_reader = BufReader::new(&stream);
        let result = parse_chunked_body(&mut buf_reader);

        // This will actually hit an IO error when trying to read the terminator
        // because the stream closes unexpectedly
        assert!(result.is_err());
        handle.join().unwrap();
    }

    #[test]
    fn empty_body_with_only_terminator() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let handle = std::thread::spawn(move || {
            let mut client = std::net::TcpStream::connect(addr).unwrap();
            // Immediate terminator with no chunks
            client
                .write_all(b"0\r\n\r\n")
                .unwrap();
            client.shutdown(std::net::Shutdown::Write).unwrap();
        });

        let (stream, _) = listener.accept().unwrap();
        let mut buf_reader = BufReader::new(&stream);
        let body = parse_chunked_body(&mut buf_reader).unwrap();

        assert!(body.is_empty());
        handle.join().unwrap();
    }
}
```

### Step 3: Export the Chunked Encoding Module

**File**: `/home/jwall/personal/rusty/rcomm/src/models.rs`

Add the new module to the barrel export:

```rust
pub mod http_methods;
pub mod http_request;
pub mod http_response;
pub mod http_status_codes;
pub mod chunked_encoding;  // Add this line
```

### Step 4: Modify HttpRequest's build_from_stream

**File**: `/home/jwall/personal/rusty/rcomm/src/models/http_request.rs`

Update the body parsing section to detect and handle chunked encoding:

**Current Code** (lines 95-104):
```rust
// Parse body if Content-Length is present
if let Some(content_length) = request.headers.get("content-length") {
    if let Ok(len) = content_length.parse::<usize>() {
        if len > 0 {
            let mut body_buf = vec![0u8; len];
            buf_reader.read_exact(&mut body_buf).map_err(HttpParseError::IoError)?;
            request.add_body(body_buf);
        }
    }
}
```

**Updated Code**:
```rust
// Validate that both Content-Length and Transfer-Encoding are not present
let has_content_length = request.headers.contains_key("content-length");
let has_chunked_encoding = request
    .headers
    .get("transfer-encoding")
    .map(|v| v.to_lowercase().contains("chunked"))
    .unwrap_or(false);

if has_content_length && has_chunked_encoding {
    return Err(HttpParseError::ChunkedWithContentLength);
}

// Parse body based on Transfer-Encoding or Content-Length
if has_chunked_encoding {
    // Parse chunked transfer encoding
    let body = super::chunked_encoding::parse_chunked_body(&mut buf_reader)?;
    if !body.is_empty() {
        request.add_body(body);
    }
} else if let Some(content_length) = request.headers.get("content-length") {
    // Parse body if Content-Length is present
    if let Ok(len) = content_length.parse::<usize>() {
        if len > 0 {
            let mut body_buf = vec![0u8; len];
            buf_reader.read_exact(&mut body_buf).map_err(HttpParseError::IoError)?;
            request.add_body(body_buf);
        }
    }
}
```

**Key Changes**:
- Check for presence of `Transfer-Encoding: chunked` header (case-insensitive)
- Validate that both headers are not present simultaneously
- Route to chunked parser or content-length parser accordingly
- Chunks are reassembled into a complete body before storing

---

## 4. Code Snippets and Pseudocode

### Chunked Encoding Parser

```
FUNCTION parse_chunked_body(buf_reader) -> Result<Vec<u8>, Error>
    body = []

    LOOP:
        READ line (chunk size line)
        PARSE hex number from line (may include extensions after ';')
        chunk_size = hex_value

        IF chunk_size == 0:
            BREAK (end of chunks)

        READ exactly chunk_size bytes
        APPEND bytes to body

        READ line (should be empty, terminator CRLF)
        IF line is not empty:
            RETURN Error(InvalidChunkTerminator)

    // Handle trailing headers (future)
    LOOP:
        READ line
        IF line is empty:
            BREAK
        ELSE:
            SKIP trailer header

    RETURN body
END FUNCTION
```

### Integration in Request Parser

```
FUNCTION build_from_stream(stream) -> Result<HttpRequest, Error>
    // ... parse request line and headers ...

    has_content_length = headers.contains("content-length")
    has_chunked = headers.get("transfer-encoding")?.contains("chunked")

    IF has_content_length AND has_chunked:
        RETURN Error(ChunkedWithContentLength)

    IF has_chunked:
        body = parse_chunked_body(buf_reader)?
        request.add_body(body)
    ELSE IF has_content_length:
        len = parse_int(headers.get("content-length"))
        body = read_exactly(buf_reader, len)
        request.add_body(body)

    RETURN request
END FUNCTION
```

---

## 5. Testing Strategy

### Unit Tests (in `chunked_encoding.rs`)

The `chunked_encoding.rs` module includes comprehensive unit tests:

1. **Single Chunk Test**: Verifies parsing of one complete chunk
2. **Multiple Chunks Test**: Verifies reassembly of multiple chunks into one body
3. **Chunk Extensions Test**: Verifies that chunk extensions (e.g., `5;name=value`) are ignored
4. **Large Chunk Test**: Verifies handling of chunks larger than typical buffer sizes
5. **Invalid Chunk Size Test**: Verifies error handling for malformed hex chunk size
6. **Missing Terminator Test**: Verifies error detection when CRLF is missing after chunk data
7. **Empty Body Test**: Verifies handling of messages with no chunks (only terminator)

**Run unit tests**:
```bash
cargo test chunked_encoding
```

### Unit Tests (in `http_request.rs`)

Add new tests to verify the integration with `build_from_stream()`:

```rust
#[test]
fn build_from_stream_parses_chunked_post() {
    use std::io::Write;
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = std::thread::spawn(move || {
        let mut client = std::net::TcpStream::connect(addr).unwrap();
        let msg = b"POST /submit HTTP/1.1\r\n\
                    Host: localhost\r\n\
                    Transfer-Encoding: chunked\r\n\
                    \r\n\
                    5\r\n\
                    hello\r\n\
                    6\r\n\
                     world\r\n\
                    0\r\n\
                    \r\n";
        client.write_all(msg).unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
    });

    let (stream, _) = listener.accept().unwrap();
    let req = HttpRequest::build_from_stream(&stream).unwrap();

    assert_eq!(req.method.to_string(), "POST");
    assert_eq!(req.target, "/submit");
    assert_eq!(req.try_get_body(), Some(b"hello world".to_vec()));
    handle.join().unwrap();
}

#[test]
fn build_from_stream_rejects_chunked_with_content_length() {
    use std::io::Write;
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = std::thread::spawn(move || {
        let mut client = std::net::TcpStream::connect(addr).unwrap();
        let msg = b"POST /submit HTTP/1.1\r\n\
                    Host: localhost\r\n\
                    Content-Length: 5\r\n\
                    Transfer-Encoding: chunked\r\n\
                    \r\n";
        client.write_all(msg).unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
    });

    let (stream, _) = listener.accept().unwrap();
    let result = HttpRequest::build_from_stream(&stream);

    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), HttpParseError::ChunkedWithContentLength));
    handle.join().unwrap();
}

#[test]
fn build_from_stream_parses_chunked_with_extensions() {
    use std::io::Write;
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let handle = std::thread::spawn(move || {
        let mut client = std::net::TcpStream::connect(addr).unwrap();
        let msg = b"POST /submit HTTP/1.1\r\n\
                    Host: localhost\r\n\
                    Transfer-Encoding: chunked\r\n\
                    \r\n\
                    5;name=value\r\n\
                    hello\r\n\
                    0\r\n\
                    \r\n";
        client.write_all(msg).unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
    });

    let (stream, _) = listener.accept().unwrap();
    let req = HttpRequest::build_from_stream(&stream).unwrap();

    assert_eq!(req.try_get_body(), Some(b"hello".to_vec()));
    handle.join().unwrap();
}
```

**Run integrated HTTP request tests**:
```bash
cargo test build_from_stream
```

### Integration Tests (in `src/bin/integration_test.rs`)

Add new test cases to verify chunked requests work end-to-end:

```rust
fn test_chunked_post_request(addr: &str) -> Result<(), String> {
    // Build a chunked request manually
    let mut stream = std::net::TcpStream::connect(addr)
        .map_err(|e| format!("Connection failed: {e}"))?;

    let request = b"POST /submit HTTP/1.1\r\n\
                    Host: localhost\r\n\
                    Transfer-Encoding: chunked\r\n\
                    Content-Type: application/x-www-form-urlencoded\r\n\
                    \r\n\
                    8\r\n\
                    key=hello\r\n\
                    6\r\n\
                    &value\r\n\
                    0\r\n\
                    \r\n";

    stream.write_all(request)
        .map_err(|e| format!("Request write failed: {e}"))?;
    stream.shutdown(std::net::Shutdown::Write)
        .map_err(|e| format!("Shutdown failed: {e}"))?;

    // Read response
    let mut response = String::new();
    std::io::Read::read_to_string(&mut stream, &mut response)
        .map_err(|e| format!("Response read failed: {e}"))?;

    if !response.contains("HTTP/1.1 200") && !response.contains("HTTP/1.1 404") {
        return Err(format!("Unexpected response: {}", response));
    }

    Ok(())
}

fn test_chunked_with_extension(addr: &str) -> Result<(), String> {
    let mut stream = std::net::TcpStream::connect(addr)
        .map_err(|e| format!("Connection failed: {e}"))?;

    // Chunked request with extension
    let request = b"POST / HTTP/1.1\r\n\
                    Host: localhost\r\n\
                    Transfer-Encoding: chunked\r\n\
                    \r\n\
                    5;ext=data\r\n\
                    hello\r\n\
                    0\r\n\
                    \r\n";

    stream.write_all(request)
        .map_err(|e| format!("Request write failed: {e}"))?;
    stream.shutdown(std::net::Shutdown::Write)
        .map_err(|e| format!("Shutdown failed: {e}"))?;

    let mut response = String::new();
    std::io::Read::read_to_string(&mut stream, &mut response)
        .map_err(|e| format!("Response read failed: {e}"))?;

    if !response.starts_with("HTTP/1.1") {
        return Err("Response missing HTTP status line".to_string());
    }

    Ok(())
}
```

Add these to the test list in integration_test.rs `main()`:
```rust
let results = vec![
    // ... existing tests ...
    run_test("chunked_post_request", || test_chunked_post_request(&addr)),
    run_test("chunked_with_extension", || test_chunked_with_extension(&addr)),
];
```

**Run integration tests**:
```bash
cargo run --bin integration_test
```

### Manual Testing with curl

Test chunked requests using curl's `--data-binary` flag:

```bash
# Start server
cargo run &

# Test with chunked encoding
curl -X POST \
     --data-binary @/path/to/file \
     -H "Transfer-Encoding: chunked" \
     http://127.0.0.1:7878/

# Test with verbose output
curl -v -X POST \
     --data "hello world" \
     -H "Transfer-Encoding: chunked" \
     http://127.0.0.1:7878/
```

---

## 6. Edge Cases to Consider

### Case 1: Both Content-Length and Transfer-Encoding Present
**Scenario**: A malformed or malicious request includes both headers
**Desired Behavior**: Reject with error `ChunkedWithContentLength`
**Handling**: Check both headers before parsing body
**Code**:
```rust
let has_content_length = request.headers.contains_key("content-length");
let has_chunked_encoding = request
    .headers
    .get("transfer-encoding")
    .map(|v| v.to_lowercase().contains("chunked"))
    .unwrap_or(false);

if has_content_length && has_chunked_encoding {
    return Err(HttpParseError::ChunkedWithContentLength);
}
```

### Case 2: Case-Insensitive Header Matching
**Scenario**: Client sends `Transfer-Encoding: CHUNKED` or `transfer-encoding: Chunked`
**Current Behavior**: Headers are normalized to lowercase on insertion
**Expected Result**: Should match correctly
**Verification**: The `.to_lowercase()` call ensures case-insensitive comparison

### Case 3: Chunk Extensions (RFC 7230 Section 4.1.1)
**Scenario**: Chunk size line includes extensions like `5;name=value;foo=bar`
**Current Behavior**: Extensions are ignored (everything after first `;` is discarded)
**Expected Result**: Parse hex chunk size successfully, skip extension data
**Code**:
```rust
let chunk_size_str = size_line
    .split(';')
    .next()
    .ok_or(HttpParseError::InvalidChunkSize)?
    .trim();
```

### Case 4: Trailing Headers in Chunked Encoding (RFC 7230 Section 4.1.2)
**Scenario**: Message includes trailer headers after the zero-size chunk
**Current Behavior**: Skipped/ignored (lines are consumed but not parsed)
**Future Enhancement**: Could store and validate trailer headers
**Note**: Currently not supported; will consume but discard them
**Code**:
```rust
loop {
    let mut trailer_line = String::new();
    buf_reader.read_line(&mut trailer_line)
        .map_err(HttpParseError::IoError)?;

    let trailer_line = trailer_line.trim_end_matches(|c| c == '\r' || c == '\n');
    if trailer_line.is_empty() {
        break; // End of message
    }
    // Skip trailer headers for now
}
```

### Case 5: Chunk Size with Mixed Case Hex (e.g., "1aF")
**Scenario**: Chunk size uses uppercase hex digits
**Current Behavior**: Rust's `u32::from_str_radix(_, 16)` accepts both cases
**Expected Result**: Parse correctly
**Verification**: `from_str_radix` is case-insensitive for hex

### Case 6: Invalid Hex in Chunk Size
**Scenario**: Client sends `ZZ\r\n` as chunk size
**Current Behavior**: `from_str_radix` returns an error
**Expected Result**: Return `HttpParseError::InvalidChunkSize`
**Code**:
```rust
let chunk_size = u32::from_str_radix(chunk_size_str, 16)
    .map_err(|_| HttpParseError::InvalidChunkSize)? as usize;
```

### Case 7: Missing CRLF After Chunk Data
**Scenario**: Chunk data not followed by `\r\n`
**Current Behavior**: `read_line()` will read a non-empty line where empty is expected
**Expected Result**: Return `HttpParseError::InvalidChunkTerminator`
**Code**:
```rust
let terminator = terminator.trim_end_matches(|c| c == '\r' || c == '\n');
if !terminator.is_empty() {
    return Err(HttpParseError::InvalidChunkTerminator);
}
```

### Case 8: Very Large Chunk Sizes
**Scenario**: Client sends a chunk size of `FFFFFFFF` (max 32-bit value)
**Current Behavior**: Will allocate a vector of that size or error if memory is unavailable
**Risk**: DoS vulnerability (memory exhaustion)
**Mitigation**: Consider adding a maximum chunk size limit (e.g., 1GB per chunk)
**Future Enhancement**: Add configurable `MAX_CHUNK_SIZE` constant

```rust
const MAX_CHUNK_SIZE: usize = 1024 * 1024 * 1024; // 1GB per chunk

// In parser:
if chunk_size > MAX_CHUNK_SIZE {
    return Err(HttpParseError::InvalidChunkSize); // or new error variant
}
```

### Case 9: Multiple Transfer-Encoding Values
**Scenario**: Request has `Transfer-Encoding: gzip, chunked`
**Current Behavior**: Current check uses `.contains("chunked")`
**Expected Result**: Will detect "chunked" in the comma-separated list
**Note**: Full support for stacked encodings (gzip, deflate, etc.) is out of scope

### Case 10: Empty Chunks (Size 0 but Not Terminal)
**Scenario**: Malformed message with `0\r\n\r\n` in the middle
**Current Behavior**: Treated as end-of-chunks marker
**Expected Result**: Parsing stops, remaining data is ignored
**Note**: This is technically correct per RFC; zero chunk always terminates

### Case 11: Bare LF Instead of CRLF in Chunk Lines
**Scenario**: Chunk size line or terminator uses only `\n` instead of `\r\n`
**Current Behavior**: `read_line()` accepts both `\r\n` and bare `\n`
**Expected Result**: Should handle gracefully
**Verification**: `.trim_end_matches(|c| c == '\r' || c == '\n')` handles both

### Case 12: Very Small Buffer Reads
**Scenario**: Network sends data byte-by-byte
**Current Behavior**: `read_line()` and `read_exact()` handle buffering internally
**Expected Result**: Will work correctly but may be slower
**Note**: BufReader handles the underlying socket properly

---

## 7. Implementation Checklist

- [ ] Add new error variants to `HttpParseError` enum in `/home/jwall/personal/rusty/rcomm/src/models/http_request.rs`
- [ ] Update `Display` implementation for new `HttpParseError` variants
- [ ] Create `/home/jwall/personal/rusty/rcomm/src/models/chunked_encoding.rs`:
  - [ ] Implement `parse_chunked_body()` function
  - [ ] Add unit tests for single chunk, multiple chunks, extensions, large chunks, error cases
  - [ ] Add test for empty body
- [ ] Export `chunked_encoding` module in `/home/jwall/personal/rusty/rcomm/src/models.rs`
- [ ] Modify `build_from_stream()` in `/home/jwall/personal/rusty/rcomm/src/models/http_request.rs`:
  - [ ] Add validation for conflicting headers
  - [ ] Add routing to chunked parser when `Transfer-Encoding: chunked` is detected
  - [ ] Add unit tests for chunked request parsing
- [ ] Add integration tests to `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`:
  - [ ] `test_chunked_post_request()`
  - [ ] `test_chunked_with_extension()`
  - [ ] Test for conflicting headers (if testing responses)
- [ ] Run all tests: `cargo test`
- [ ] Manual testing with curl and real clients
- [ ] Verify no performance regression with existing Content-Length requests

---

## 8. Complexity and Risk Analysis

**Complexity**: 6/10
- Hex parsing and chunk reassembly logic is moderately complex
- Requires understanding of RFC 7230 chunked encoding format
- Multiple edge cases to handle (chunk extensions, trailing headers, terminators)
- Integration with existing stream parsing requires careful sequencing

**Risk**: Medium
- **Blocking Risk**: Could hang if stream format is malformed and parser waits indefinitely
  - Mitigation: Comprehensive error handling for IO errors
- **Memory Risk**: Large chunks could exhaust memory (DoS)
  - Mitigation: Could add configurable max chunk size limit
- **Correctness Risk**: RFC compliance requires precise CRLF handling
  - Mitigation: Extensive unit tests with various chunk formats
- **Backward Compatibility**: Low risk (additive feature, doesn't change existing request types)

**Dependencies**: None
- Uses only `std::io::BufReader` and standard library
- No external crates required
- Aligns with project's no-external-dependencies constraint

---

## 9. Future Enhancements

1. **Maximum Chunk Size Limit**: Add configurable limit to prevent DoS
   - Add constant `MAX_CHUNK_SIZE: usize`
   - Return error if chunk exceeds limit

2. **Trailer Headers**: Parse and validate trailer headers
   - Extract trailer header name-value pairs
   - Validate trailer headers are in allow list (per RFC 7230)

3. **Stacked Encodings**: Support `Transfer-Encoding: gzip, chunked`
   - Parse `Transfer-Encoding` header value as list
   - Apply decodings in reverse order
   - Requires additional dependencies (flate2 or similar)

4. **Streaming API**: Allow consuming chunks as they arrive
   - Implement iterator pattern for chunks
   - Useful for large file uploads

5. **Chunked Response Encoding**: Support `Transfer-Encoding: chunked` on responses
   - Add method to `HttpResponse` for chunked body encoding
   - Useful for streaming responses

6. **Request Body Size Metrics**: Track total bytes received
   - Could be useful for logging and monitoring
   - Help identify unusual request sizes

7. **Request Timeout**: Implement timeout for body parsing
   - Use `set_read_timeout()` on TcpStream
   - Prevent hanging on slow clients
   - Already handled by thread pool's graceful shutdown

---

## 10. RFC 7230 Compliance Notes

This implementation follows RFC 7230 Section 4.1 (Transfer-Encoding):

- **Chunk Format**: `chunk-size [ chunk-ext ] CRLF chunk-data CRLF`
- **Chunk Size**: Hex representation without leading zeros (parser accepts any valid hex)
- **Last Chunk**: Zero-size chunk (`0 CRLF`) marks end of chunks
- **Trailer Section**: Optional headers after last chunk (not yet implemented)
- **Termination**: Final CRLF after trailers (or after `0 CRLF` if no trailers)

Not Implemented:
- Chunked trailers with header validation
- Stacked transfer encodings (e.g., `chunked;gzip`)
- Chunk extensions beyond basic parsing
- Server-side chunked response encoding

---

## 11. Example Request/Response Flow

### Request with Chunked Encoding
```
Client sends:
POST /api/data HTTP/1.1
Host: localhost
Transfer-Encoding: chunked
Content-Type: application/json

1e
{"key": "hello", "value": "
1f
world"}
0

(blank line)

Server parsing:
1. Read "POST /api/data HTTP/1.1\r\n"
2. Read headers, find "Transfer-Encoding: chunked"
3. Call parse_chunked_body():
   - Read "1e\r\n" -> chunk size = 30 bytes
   - Read 30 bytes: {"key": "hello", "value": "
   - Read "\r\n" (terminator)
   - Read "1f\r\n" -> chunk size = 31 bytes
   - Read 31 bytes: world"}
   - Read "\r\n" (terminator)
   - Read "0\r\n" -> chunk size = 0, end
   - Read "\r\n" (final terminator)
4. Reassembled body: {"key": "hello", "value": "world"}
5. Request processed normally

Server response:
HTTP/1.1 200 OK
Content-Length: 15
Content-Type: text/html

Success
```

### Comparison: Content-Length vs Chunked

**Content-Length** (current support):
- Client knows body size in advance
- Single size header + exact body bytes
- Simpler to parse

**Transfer-Encoding: chunked** (new feature):
- Client doesn't know body size (streaming, dynamic generation)
- Multiple chunks with intermediate metadata
- More flexible for streaming scenarios
- Slightly more complex parsing
