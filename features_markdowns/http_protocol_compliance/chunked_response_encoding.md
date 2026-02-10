# Chunked Transfer Encoding Implementation Plan

## Overview

Chunked transfer encoding (defined in RFC 7230 Section 4.1) allows HTTP responses to be sent in a series of "chunks" without knowing the total content length in advance. Each chunk is preceded by its size in hexadecimal bytes, followed by a CRLF. A zero-sized chunk signals the end of the message.

**Why This Feature?**
- Enables streaming responses (e.g., real-time data, large files, generated content)
- HTTP/1.1 spec allows omitting `Content-Length` in favor of chunked encoding
- Improves perceived latency by sending partial responses before generation completes
- Necessary for certain use cases like server-sent events (SSE) and progressive rendering

**Current State:**
The server currently requires knowing the full content length upfront via `add_body()`, which automatically sets the `Content-Length` header. Chunked encoding provides an alternative mechanism when the response size is unknown or streaming is desired.

---

## Files to Modify

### Primary Files
1. **`src/models/http_response.rs`** — Add chunked encoding support to the response model
2. **`src/main.rs`** — Modify `handle_connection()` to support chunked responses
3. **`src/models.rs`** (barrel file) — Export new chunked-related types if needed

### Testing Files
1. **`src/models/http_response.rs`** (test module) — Unit tests for chunked encoding methods
2. **`src/bin/integration_test.rs`** — Add integration tests for streaming responses

---

## Step-by-Step Implementation

### Step 1: Add Chunked Mode Support to `HttpResponse`

**File:** `/home/jwall/personal/rusty/rcomm/src/models/http_response.rs`

**Changes:**
- Add an internal state field to track chunked vs. non-chunked modes
- Add methods to enable chunked mode and add chunks incrementally
- Modify `as_bytes()` to handle chunked serialization
- Add validation to prevent mixing chunked and Content-Length modes

**Code Snippet:**

```rust
pub struct HttpResponse {
    version: String,
    status_code: u16,
    status_phrase: String,
    headers: HashMap<String, String>,
    body: Option<Vec<u8>>,
    chunks: Option<Vec<Vec<u8>>>,  // NEW: for chunked mode
    is_chunked: bool,              // NEW: flag to track if chunked
}

impl HttpResponse {
    // Existing build() method unchanged

    /// Enable chunked transfer encoding for this response.
    /// Cannot be called if add_body() has already been called.
    pub fn enable_chunked(&mut self) -> &mut HttpResponse {
        if self.body.is_some() {
            panic!("Cannot enable chunked encoding after add_body() has been called");
        }
        self.is_chunked = true;
        self.chunks = Some(Vec::new());
        // Remove Content-Length if it was set, add Transfer-Encoding header
        self.headers.remove("content-length");
        self.headers.insert("transfer-encoding".to_string(), "chunked".to_string());
        self
    }

    /// Add a chunk to the response. Only valid if chunked mode is enabled.
    pub fn add_chunk(&mut self, chunk: Vec<u8>) -> &mut HttpResponse {
        if !self.is_chunked {
            panic!("add_chunk() called on non-chunked response. Call enable_chunked() first.");
        }
        if let Some(ref mut chunks) = self.chunks {
            chunks.push(chunk);
        }
        self
    }

    /// Finalize and get chunks for transmission. Used internally.
    fn get_chunks_for_transmission(&self) -> Vec<Vec<u8>> {
        if !self.is_chunked {
            return vec![];
        }

        let mut result = Vec::new();

        // Add header bytes
        result.push(format!("{self}").as_bytes().to_vec());

        // Add each chunk with size prefix
        if let Some(chunks) = &self.chunks {
            for chunk in chunks {
                let size_hex = format!("{:x}", chunk.len());
                let chunk_line = format!("{}\r\n", size_hex).into_bytes();
                result.push(chunk_line);
                result.push(chunk.clone());
                result.push(b"\r\n".to_vec());
            }
        }

        // Add terminating zero chunk
        result.push(b"0\r\n\r\n".to_vec());

        result
    }
}

pub fn as_bytes(&self) -> Vec<u8> {
    if self.is_chunked {
        // For chunked responses, return all chunks concatenated
        let chunks = self.get_chunks_for_transmission();
        let mut result = Vec::new();
        for chunk in chunks {
            result.extend_from_slice(&chunk);
        }
        result
    } else if let Some(body) = &self.body {
        // Existing non-chunked logic
        let mut bytes = format!("{self}").as_bytes().to_vec();
        bytes.append(&mut body.clone());
        bytes
    } else {
        format!("{self}").as_bytes().to_vec()
    }
}
```

**Key Design Decisions:**
- Use panics for invalid state transitions to prevent silent bugs
- Store chunks separately from the body to avoid ambiguity
- Serialize chunks on-demand in `as_bytes()` rather than eagerly
- Automatically strip `Content-Length` when chunked mode is enabled

---

### Step 2: Add Unit Tests for Chunked Encoding

**File:** `/home/jwall/personal/rusty/rcomm/src/models/http_response.rs` (test module)

**Code Snippet:**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enable_chunked_sets_transfer_encoding_header() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
        resp.enable_chunked();
        assert_eq!(
            resp.try_get_header("transfer-encoding".to_string()),
            Some("chunked".to_string())
        );
    }

    #[test]
    fn enable_chunked_removes_content_length() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
        resp.add_body(b"test".to_vec());
        let _ = resp.try_get_header("content-length".to_string()); // Set by add_body
        resp.enable_chunked();
        assert_eq!(resp.try_get_header("content-length".to_string()), None);
    }

    #[test]
    #[should_panic(expected = "Cannot enable chunked")]
    fn enable_chunked_panics_after_add_body() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
        resp.add_body(b"data".to_vec());
        resp.enable_chunked();
    }

    #[test]
    fn add_chunk_stores_chunk() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
        resp.enable_chunked();
        resp.add_chunk(b"Hello ".to_vec());
        resp.add_chunk(b"World".to_vec());
        // Verify serialization includes both chunks
        let bytes = resp.as_bytes();
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains("6\r\nHello \r\n"));  // "6" is hex for 6 bytes
        assert!(text.contains("5\r\nWorld\r\n"));   // "5" is hex for 5 bytes
    }

    #[test]
    #[should_panic(expected = "add_chunk")]
    fn add_chunk_panics_on_non_chunked_response() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
        resp.add_chunk(b"data".to_vec());
    }

    #[test]
    fn as_bytes_chunked_includes_terminating_chunk() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
        resp.enable_chunked();
        resp.add_chunk(b"test".to_vec());
        let bytes = resp.as_bytes();
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.ends_with("0\r\n\r\n"));
    }

    #[test]
    fn as_bytes_chunked_with_multiple_chunks() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
        resp.enable_chunked();
        resp.add_chunk(b"chunk1".to_vec());
        resp.add_chunk(b"chunk2".to_vec());
        resp.add_chunk(b"chunk3".to_vec());
        let bytes = resp.as_bytes();
        let text = String::from_utf8(bytes).unwrap();

        // Verify structure
        assert!(text.contains("HTTP/1.1 200 OK\r\n"));
        assert!(text.contains("transfer-encoding: chunked\r\n"));
        assert!(text.contains("6\r\nchunk1\r\n"));
        assert!(text.contains("6\r\nchunk2\r\n"));
        assert!(text.contains("6\r\nchunk3\r\n"));
        assert!(text.ends_with("0\r\n\r\n"));
    }

    #[test]
    fn as_bytes_chunked_empty_response() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
        resp.enable_chunked();
        let bytes = resp.as_bytes();
        let text = String::from_utf8(bytes).unwrap();
        // Should have headers and just the terminating chunk
        assert!(text.contains("transfer-encoding: chunked\r\n"));
        assert!(text.ends_with("0\r\n\r\n"));
    }

    #[test]
    fn as_bytes_chunked_with_non_ascii_bytes() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
        resp.enable_chunked();
        resp.add_chunk(vec![0xFF, 0xFE, 0xFD]);
        let bytes = resp.as_bytes();
        // Find the chunk size and verify the binary data follows
        let idx = bytes.windows(5).position(|w| w == b"3\r\n");
        assert!(idx.is_some());
    }

    #[test]
    fn enable_chunked_allows_chaining() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
        resp.enable_chunked()
            .add_chunk(b"data1".to_vec())
            .add_chunk(b"data2".to_vec());
        let bytes = resp.as_bytes();
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains("5\r\ndata1\r\n"));
        assert!(text.contains("5\r\ndata2\r\n"));
    }
}
```

**Test Coverage:**
- Correct header setup
- Panic prevention on invalid state transitions
- Serialization with proper hex-encoded chunk sizes
- Terminating zero chunk
- Multiple chunks and empty responses
- Binary data handling
- Method chaining

---

### Step 3: Modify Server Request Handling

**File:** `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Changes:**
- Create a new variant of the connection handler or add a parameter for chunked responses
- Optionally add environment variable or query parameter to trigger chunked encoding for testing

**Code Snippet:**

```rust
// In handle_connection(), after serving a file, conditionally use chunked encoding:

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

    // Check if chunked encoding is requested (e.g., via query parameter)
    let use_chunked = http_request.target.contains("?chunked=true");

    if use_chunked {
        // Stream response in chunks (demonstration: split by lines)
        response.enable_chunked();
        for line in contents.lines() {
            response.add_chunk(format!("{}\n", line).into_bytes());
        }
    } else {
        // Traditional Content-Length approach
        response.add_body(contents.into());
    }

    println!("Response: {response}");
    stream.write_all(&response.as_bytes()).unwrap();
}
```

**Alternative Design (More Complex):**
If the feature should automatically stream large files, add a threshold:

```rust
const CHUNK_THRESHOLD: usize = 1024 * 1024; // 1MB

if contents.len() > CHUNK_THRESHOLD {
    response.enable_chunked();
    for chunk in contents.as_bytes().chunks(4096) {
        response.add_chunk(chunk.to_vec());
    }
} else {
    response.add_body(contents.into());
}
```

---

### Step 4: Add Integration Tests

**File:** `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`

**Code Snippet:**

```rust
// Add to the test suite:

fn test_chunked_transfer_encoding() -> TestResult {
    let server_port = start_test_server();

    // Send GET request with chunked flag
    let response_bytes = send_request("127.0.0.1", server_port, b"GET /?chunked=true HTTP/1.1\r\nHost: localhost\r\n\r\n");
    let response_text = String::from_utf8(response_bytes).unwrap();

    // Verify transfer-encoding header
    if !response_text.contains("transfer-encoding: chunked") {
        return TestResult::fail("Response missing transfer-encoding: chunked header");
    }

    // Verify chunked format
    if !response_text.contains("\r\n") || !response_text.ends_with("0\r\n\r\n") {
        return TestResult::fail("Response does not have valid chunked format");
    }

    TestResult::pass()
}

fn test_chunked_multiple_chunks() -> TestResult {
    let server_port = start_test_server();

    let response_bytes = send_request("127.0.0.1", server_port, b"GET /?chunked=true HTTP/1.1\r\nHost: localhost\r\n\r\n");
    let response_text = String::from_utf8(response_bytes).unwrap();

    // Should contain multiple chunk size markers (hex)
    let chunk_markers: Vec<_> = response_text.matches("\r\n").collect();

    if chunk_markers.len() < 3 {  // At least header CRLF, chunk CRLF, terminator
        return TestResult::fail("Response does not appear to have multiple chunks");
    }

    TestResult::pass()
}

fn test_chunked_content_integrity() -> TestResult {
    let server_port = start_test_server();

    let response_bytes = send_request("127.0.0.1", server_port, b"GET /?chunked=true HTTP/1.1\r\nHost: localhost\r\n\r\n");
    let response_text = String::from_utf8(response_bytes).unwrap();

    // Extract body from chunked response (simple parsing)
    let mut body = String::new();
    let parts: Vec<&str> = response_text.split("\r\n\r\n").collect();
    if parts.len() < 2 {
        return TestResult::fail("Could not parse response headers");
    }

    let chunked_body = parts[1];
    let mut in_chunk_data = false;
    let mut current_chunk = String::new();

    for line in chunked_body.split("\r\n") {
        if line.is_empty() {
            continue;
        }

        // Try to parse as hex chunk size
        if let Ok(_) = usize::from_str_radix(line, 16) {
            in_chunk_data = true;
        } else if in_chunk_data && line != "0" {
            body.push_str(line);
            body.push('\n');
            in_chunk_data = false;
        }
    }

    if body.is_empty() {
        return TestResult::fail("Could not extract content from chunked response");
    }

    TestResult::pass()
}
```

**Integration Test Checklist:**
- Response includes `transfer-encoding: chunked` header
- Response includes chunk size markers in hexadecimal
- Response ends with terminating zero chunk `0\r\n\r\n`
- Multiple chunks are properly formatted
- Content can be reconstructed from chunks
- Binary data in chunks is preserved
- Server correctly handles clients that don't request chunked encoding

---

## Testing Strategy

### Unit Tests (in `http_response.rs`)
Run with:
```bash
cargo test http_response
```

**Coverage areas:**
1. Enabling/disabling chunked mode
2. Header management (transfer-encoding, content-length)
3. Chunk serialization with correct hex sizes
4. Terminating chunk format
5. Empty responses
6. Single and multiple chunks
7. Binary data preservation
8. State transition validation

### Integration Tests (in `integration_test.rs`)
Run with:
```bash
cargo run --bin integration_test
```

**Coverage areas:**
1. End-to-end chunked response transmission
2. Client-side reception and reassembly
3. Mixed chunked and non-chunked routes
4. Large file streaming
5. Error responses with chunked encoding

### Manual Testing
```bash
# Terminal 1: Start server
cargo run

# Terminal 2: Test with curl (will automatically decode chunked responses)
curl -v http://127.0.0.1:7878/?chunked=true

# Test with netcat to see raw chunked format
echo -e "GET /?chunked=true HTTP/1.1\r\nHost: localhost\r\n\r\n" | nc localhost 7878
```

---

## Edge Cases & Robustness

### 1. Chunk Size Boundaries
- **Issue:** Very large chunks could cause memory issues
- **Solution:** Add optional `add_chunk_limit()` method that validates chunk size

```rust
pub fn add_chunk_limited(&mut self, chunk: Vec<u8>, max_size: usize) -> Result<&mut HttpResponse, String> {
    if chunk.len() > max_size {
        return Err(format!("Chunk size {} exceeds limit {}", chunk.len(), max_size));
    }
    self.add_chunk(chunk);
    Ok(self)
}
```

### 2. Hex Size Encoding
- **Issue:** Need to correctly format chunk sizes in hexadecimal
- **Current implementation:** Uses `format!("{:x}", chunk.len())` which is correct
- **Test case:** Verify 255 bytes encodes as `ff`, 256 as `100`, etc.

### 3. Empty Chunks
- **Issue:** Should we allow empty chunks? HTTP spec allows but not typical
- **Decision:** Allow them; they're harmless and simplify code paths

```rust
#[test]
fn add_empty_chunk() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
    resp.enable_chunked();
    resp.add_chunk(vec![]);  // Empty chunk
    let bytes = resp.as_bytes();
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("0\r\n\r\n")); // Only terminator
}
```

### 4. Chunk Trailer Headers (RFC 7230 Section 4.1.1)
- **Issue:** HTTP/1.1 chunked encoding can include trailer headers after chunks
- **Decision (Phase 1):** Not implemented; add as future enhancement
- **Note:** Client support varies; most ignore them

### 5. Invalid State Transitions
- **Issue:** Prevent mixing chunked and non-chunked modes
- **Solution:** Use panic for immediate feedback in development; could be improved with Result types in future

```rust
#[test]
#[should_panic]
fn cannot_add_body_after_chunked() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
    resp.enable_chunked();
    resp.add_body(b"data".to_vec()); // Should panic
}
```

### 6. Concurrent Chunk Addition (Thread Safety)
- **Issue:** Current design is single-threaded; no synchronization
- **Decision:** Add documentation noting that `HttpResponse` is not thread-safe
- **Future:** Could add Mutex wrapper for thread-safe variant

---

## Performance Considerations

### Memory Efficiency
- **Current:** Stores all chunks in a `Vec<Vec<u8>>` before serialization
- **Trade-off:** Simple implementation vs. memory overhead
- **Alternative:** Stream chunks directly to socket (requires restructuring)

### Serialization Cost
- **Current:** `as_bytes()` allocates new buffers and concatenates
- **Optimization:** Could buffer fewer times or use `write!` directly to socket
- **Decision:** Start with simple approach; optimize if profiling shows issues

### Suggested Optimizations (Post-MVP)
1. Stream chunks directly to `TcpStream` instead of collecting in memory
2. Add optional compression (gzip) for chunked responses
3. Implement lazy chunk iteration for large files

---

## Implementation Checklist

### Phase 1: Core Implementation
- [ ] Add `is_chunked` and `chunks` fields to `HttpResponse`
- [ ] Implement `enable_chunked()` method
- [ ] Implement `add_chunk()` method
- [ ] Update `as_bytes()` to handle chunked serialization
- [ ] Add hex encoding for chunk sizes
- [ ] Write 10+ unit tests
- [ ] All existing tests still pass

### Phase 2: Server Integration
- [ ] Modify `handle_connection()` to support chunked mode
- [ ] Add query parameter flag for testing chunked encoding
- [ ] Document usage with examples
- [ ] Write 3+ integration tests
- [ ] Manual testing with curl/netcat

### Phase 3: Documentation & Refinement
- [ ] Update `CLAUDE.md` with chunked encoding details
- [ ] Add code comments explaining chunked format
- [ ] Consider error handling improvements (Result types)
- [ ] Optional: Add performance benchmarks

---

## Code Review Checklist

Before merging, verify:
1. All tests pass: `cargo test`
2. No panics in normal operation (only on invalid API usage)
3. Hex encoding is correct for all chunk sizes
4. Terminating chunk is always present
5. Headers properly set/cleared
6. No memory leaks (chunks cleared appropriately)
7. Response integrity: content unchanged by chunking
8. Backward compatibility: existing non-chunked routes unaffected

---

## Future Enhancements

1. **Trailer Headers** — Allow headers after chunks (HTTP/1.1 spec allows)
2. **Streaming API** — Direct socket writes instead of buffering chunks
3. **Chunk Compression** — Optional gzip on the fly
4. **Async Support** — When async is added to main codebase
5. **Configurable Chunk Size** — Optimize for different use cases
6. **Metrics** — Track chunk transmission times, total response sizes
7. **Error Handling** — Use `Result` types instead of panics
8. **Trailer Validation** — Verify trailer header syntax if supported

---

## References

- **RFC 7230 Section 4.1:** HTTP/1.1 Chunked Transfer Encoding
- **RFC 7231:** HTTP/1.1 Semantics and Content
- **MDN:** Transfer-Encoding (https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Transfer-Encoding)

---

## Example Usage

### Basic Chunked Response
```rust
let mut response = HttpResponse::build("HTTP/1.1".to_string(), 200);
response.enable_chunked();
response.add_chunk(b"Hello ".to_vec());
response.add_chunk(b"World".to_vec());
stream.write_all(&response.as_bytes()).unwrap();

// Transmits:
// HTTP/1.1 200 OK\r\n
// transfer-encoding: chunked\r\n
// \r\n
// 6\r\n
// Hello \r\n
// 5\r\n
// World\r\n
// 0\r\n
// \r\n
```

### Streaming Large File
```rust
let mut response = HttpResponse::build("HTTP/1.1".to_string(), 200);
response.enable_chunked();

for chunk in large_file_buffer.chunks(4096) {
    response.add_chunk(chunk.to_vec());
}

stream.write_all(&response.as_bytes()).unwrap();
```

---

## Summary

This implementation plan provides a straightforward, test-driven approach to adding chunked transfer encoding to rcomm. The feature is isolated in the `HttpResponse` model, maintains backward compatibility, and includes comprehensive testing. The phased approach allows for incremental development and validation.

**Estimated Effort:** 2-4 hours for core implementation and testing
**Complexity Rating:** 6/10 (moderate — requires HTTP protocol knowledge but straightforward in practice)
**Risk Level:** Low (isolated changes, well-tested, no external dependencies)
