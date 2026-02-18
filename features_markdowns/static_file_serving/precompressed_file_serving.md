# Pre-compressed File Serving Implementation Plan

## Overview

Pre-compressed file serving allows rcomm to serve pre-built `.gz` (gzip) and `.br` (Brotli) compressed versions of static files when the client indicates support via the `Accept-Encoding` request header. Instead of compressing on-the-fly (which is CPU-intensive and requires implementing compression algorithms from scratch), the server checks for a pre-compressed variant on disk (e.g., `file.js.gz` alongside `file.js`) and serves it directly with the appropriate `Content-Encoding` header.

This is a common optimization for static file servers: developers run `gzip` or `brotli` at build time to produce compressed assets, and the server transparently serves the compressed version when possible.

**Complexity**: 4
**Necessity**: 3

**Key Changes**:
- Parse the `Accept-Encoding` request header to determine client-supported encodings
- Before reading the original file, check for `.br` and `.gz` variants on disk
- If a compressed variant exists and the client supports it, serve the compressed file with `Content-Encoding` header
- Add `Vary: Accept-Encoding` header to responses so caches key on encoding

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Current State**:
- `handle_connection()` (line 46) reads files with `fs::read_to_string()` (line 70) and serves them directly
- No `Accept-Encoding` header parsing
- No `Content-Encoding` or `Vary` response headers
- `HttpRequest` is constructed at line 47 and the target is cleaned at line 58

**Changes Required**:
- Add helper function `parse_accept_encoding()` to extract supported encodings from the request header
- Add helper function `find_compressed_variant()` to check disk for `.br`/`.gz` files
- Modify `handle_connection()` to check for compressed variants before reading the original file
- Set `Content-Encoding` and `Vary` headers when serving a compressed file

### 2. `/home/jwall/personal/rusty/rcomm/src/models/http_request.rs`

**Current State**: `HttpRequest` has a `headers: HashMap<String, String>` field with headers stored lowercase.

**No Changes Required**: The existing `headers` HashMap already provides access to `accept-encoding`.

### 3. `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`

**Changes Required**:
- Add integration tests that send requests with `Accept-Encoding: gzip` and verify compressed response
- Add tests for `Accept-Encoding: br` (Brotli)
- Add tests verifying the original file is served when no compressed variant exists
- Add tests verifying `Vary: Accept-Encoding` header is present

---

## Step-by-Step Implementation

### Step 1: Add `Accept-Encoding` Parsing Helper

**Location**: `src/main.rs`, insert before `handle_connection()` (before line 46)

```rust
/// Parse the Accept-Encoding header value into a list of supported encodings.
/// Returns encodings in preference order: br > gzip > deflate.
/// Example input: "gzip, deflate, br"
/// Example output: vec!["br", "gzip", "deflate"]
fn parse_accept_encoding(header_value: &str) -> Vec<String> {
    let mut encodings: Vec<(String, f32)> = header_value
        .split(',')
        .filter_map(|part| {
            let part = part.trim();
            if part.is_empty() {
                return None;
            }
            let mut iter = part.splitn(2, ";q=");
            let encoding = iter.next()?.trim().to_lowercase();
            let quality: f32 = iter
                .next()
                .and_then(|q| q.trim().parse().ok())
                .unwrap_or(1.0);
            if quality > 0.0 {
                Some((encoding, quality))
            } else {
                None
            }
        })
        .collect();

    // Sort by quality descending, prefer br over gzip at equal quality
    encodings.sort_by(|a, b| {
        b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
    });

    encodings.into_iter().map(|(enc, _)| enc).collect()
}
```

### Step 2: Add Compressed Variant Lookup Helper

**Location**: `src/main.rs`, insert after `parse_accept_encoding()`

```rust
/// Check if a pre-compressed variant of the file exists on disk.
/// Checks for .br first (better compression), then .gz.
/// Returns (compressed_path, encoding_name) if found, None otherwise.
fn find_compressed_variant(
    original_path: &str,
    accepted_encodings: &[String],
) -> Option<(PathBuf, &'static str)> {
    // Priority order: br > gzip
    let variants = [
        ("br", ".br"),
        ("gzip", ".gz"),
    ];

    for (encoding, suffix) in &variants {
        if accepted_encodings.iter().any(|e| e == encoding) {
            let compressed_path = PathBuf::from(format!("{}{}", original_path, suffix));
            if compressed_path.exists() {
                return Some((compressed_path, encoding));
            }
        }
    }

    None
}
```

### Step 3: Modify `handle_connection()` to Use Compressed Variants

**Location**: `src/main.rs`, lines 62-74

**Current Code** (lines 62-74):
```rust
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
```

**New Code**:
```rust
    let (mut response, filename) = if routes.contains_key(&clean_target) {
        (HttpResponse::build(String::from("HTTP/1.1"), 200),
            routes.get(&clean_target).unwrap().to_str().unwrap())
    } else {
        (HttpResponse::build(String::from("HTTP/1.1"), 404),
            "pages/not_found.html")
    };

    // Check for pre-compressed variants
    let accepted_encodings = http_request
        .headers
        .get("accept-encoding")
        .map(|v| parse_accept_encoding(v))
        .unwrap_or_default();

    let (file_to_read, encoding) =
        match find_compressed_variant(filename, &accepted_encodings) {
            Some((compressed_path, enc)) => (compressed_path, Some(enc)),
            None => (PathBuf::from(filename), None),
        };

    let contents = fs::read_to_string(file_to_read.to_str().unwrap()).unwrap();
    response.add_body(contents.into());

    // Set Content-Encoding if serving compressed variant
    if let Some(enc) = encoding {
        response.add_header("Content-Encoding".to_string(), enc.to_string());
    }

    // Always add Vary header so caches differentiate by encoding
    response.add_header("Vary".to_string(), "Accept-Encoding".to_string());

    println!("Response: {response}");
    stream.write_all(&response.as_bytes()).unwrap();
```

**Note**: When binary file serving is implemented (`fs::read()` instead of `fs::read_to_string()`), the compressed file read should use `fs::read()` since `.gz`/`.br` files are binary.

---

### Step 4: Add Integration Tests

**Location**: `src/bin/integration_test.rs`

#### 4a. Create Test Compressed File

Before running tests, create a gzip-compressed test file. In the integration test setup or as a build step:

```bash
# Create a compressed version of index.html for testing
cd pages && gzip -k index.html  # produces index.html.gz
```

Or create a minimal `.gz` file programmatically in the test binary's setup.

#### 4b. Add Test Functions

```rust
fn test_precompressed_gzip_served(addr: &str) -> Result<(), String> {
    // Send request with Accept-Encoding: gzip
    let mut stream = TcpStream::connect(addr)
        .map_err(|e| format!("connect: {e}"))?;
    let request = format!(
        "GET / HTTP/1.1\r\nHost: {}\r\nAccept-Encoding: gzip\r\nConnection: close\r\n\r\n",
        addr
    );
    stream.write_all(request.as_bytes())
        .map_err(|e| format!("write: {e}"))?;
    let resp = read_response(&mut stream)?;

    // Should get Content-Encoding: gzip if .gz file exists
    if let Some(encoding) = resp.headers.get("content-encoding") {
        assert_eq_or_err(encoding, &"gzip".to_string(), "content-encoding")?;
    }
    // Vary header should always be present
    let vary = resp.headers.get("vary")
        .ok_or("missing Vary header")?;
    assert_eq_or_err(vary, &"Accept-Encoding".to_string(), "vary")?;
    Ok(())
}

fn test_no_encoding_serves_original(addr: &str) -> Result<(), String> {
    // Send request WITHOUT Accept-Encoding
    let resp = send_request(addr, "GET", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    // Should NOT have Content-Encoding header
    if resp.headers.contains_key("content-encoding") {
        return Err("Content-Encoding should not be present without Accept-Encoding".to_string());
    }
    Ok(())
}

fn test_vary_header_always_present(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    let vary = resp.headers.get("vary")
        .ok_or("missing Vary header")?;
    assert_eq_or_err(vary, &"Accept-Encoding".to_string(), "vary")?;
    Ok(())
}
```

---

## Testing Strategy

### Unit Tests

Add tests for the `parse_accept_encoding()` helper in `src/main.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accept_encoding_simple() {
        let result = parse_accept_encoding("gzip, deflate, br");
        assert!(result.contains(&"gzip".to_string()));
        assert!(result.contains(&"br".to_string()));
        assert!(result.contains(&"deflate".to_string()));
    }

    #[test]
    fn parse_accept_encoding_with_quality() {
        let result = parse_accept_encoding("gzip;q=0.8, br;q=1.0");
        assert_eq!(result[0], "br");
        assert_eq!(result[1], "gzip");
    }

    #[test]
    fn parse_accept_encoding_excludes_zero_quality() {
        let result = parse_accept_encoding("gzip;q=0, br");
        assert!(!result.contains(&"gzip".to_string()));
        assert!(result.contains(&"br".to_string()));
    }

    #[test]
    fn parse_accept_encoding_empty() {
        let result = parse_accept_encoding("");
        assert!(result.is_empty());
    }
}
```

### Integration Tests

| Test Name | What It Validates |
|-----------|-------------------|
| `precompressed_gzip_served` | `.gz` file served with `Content-Encoding: gzip` when `Accept-Encoding: gzip` is present |
| `no_encoding_serves_original` | Original file served when no `Accept-Encoding` header |
| `vary_header_always_present` | `Vary: Accept-Encoding` always present in response |

### Manual Testing

```bash
# Create pre-compressed file
gzip -k pages/index.html

# Start server
cargo run &

# Request with gzip support
curl -i -H "Accept-Encoding: gzip" http://127.0.0.1:7878/
# Should see Content-Encoding: gzip header

# Request without encoding
curl -i http://127.0.0.1:7878/
# Should see original uncompressed content

# Cleanup
rm pages/index.html.gz
```

---

## Edge Cases & Handling

### 1. No Compressed Variant Exists
- **Behavior**: Serves original file as normal, no `Content-Encoding` header
- **Status**: Handled by `find_compressed_variant()` returning `None`

### 2. Client Doesn't Send `Accept-Encoding`
- **Behavior**: `accepted_encodings` is empty, original file served
- **Status**: Handled by `.unwrap_or_default()`

### 3. Both `.br` and `.gz` Exist
- **Behavior**: `.br` (Brotli) preferred over `.gz` (gzip) because it has better compression ratios
- **Status**: Handled by ordering in `find_compressed_variant()`

### 4. Quality Values in `Accept-Encoding`
- **Behavior**: Respects `q=0` (encoding rejected), sorts by quality value
- **Status**: Handled by `parse_accept_encoding()`

### 5. Compressed File Deleted After Server Start
- **Behavior**: `find_compressed_variant()` checks `exists()` per-request, so removal is detected
- **Status**: Safe; falls back to original file

### 6. Compressed File Exists but Original Doesn't
- **Behavior**: The route is built from original files only, so the compressed file is served for the original's route
- **Status**: Works correctly — the route lookup finds the original path, then the compressed variant is found

### 7. TOCTOU Between `exists()` and `read()`
- **Behavior**: If compressed file deleted between check and read, `unwrap()` panics
- **Status**: Consistent with existing codebase patterns

### 8. Caching Proxies
- **Behavior**: `Vary: Accept-Encoding` header ensures proxies cache different representations separately
- **Status**: Correctly implemented

### 9. Content-Length with Compressed Files
- **Behavior**: `add_body()` auto-sets `Content-Length` to the compressed body size, which is correct
- **Status**: Works correctly — the Content-Length should reflect the transmitted (compressed) size

---

## Implementation Checklist

- [ ] Add `parse_accept_encoding()` function to `src/main.rs`
- [ ] Add `find_compressed_variant()` function to `src/main.rs`
- [ ] Modify `handle_connection()` to check for compressed variants
- [ ] Add `Content-Encoding` header when serving compressed files
- [ ] Add `Vary: Accept-Encoding` header to all responses
- [ ] Add unit tests for `parse_accept_encoding()`
- [ ] Create test compressed files (e.g., `pages/index.html.gz`)
- [ ] Add integration tests for compressed file serving
- [ ] Run `cargo test` to verify all unit tests pass
- [ ] Run `cargo run --bin integration_test` to verify all integration tests pass
- [ ] Manual test with `curl -H "Accept-Encoding: gzip"`

---

## Backward Compatibility

### Existing Tests
All existing tests pass without modification. The pre-compressed serving is purely additive — it only activates when both (a) the client sends `Accept-Encoding` and (b) a compressed variant exists on disk.

### Behavioral Changes
- Responses now include `Vary: Accept-Encoding` header (new, but standard)
- No change to response content when no compressed files exist

### Performance Impact
- **Per-request overhead**: One `Path::exists()` check per supported encoding (up to 2: `.br`, `.gz`)
- **Benefit**: Significantly reduced bandwidth for pre-compressed assets
- **No CPU overhead for compression**: Files are compressed at build time, not at runtime
