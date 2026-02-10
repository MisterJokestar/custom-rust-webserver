# Binary File Serving Implementation Plan

## Overview

Currently, rcomm serves only text-based static files (HTML, CSS, JavaScript) via `fs::read_to_string()`, which fails for binary files like images, fonts, and PDFs. This plan outlines replacing string-based file serving with binary `Vec<u8>` bodies while maintaining backward compatibility with text files.

**Impact**: The HTTP response infrastructure is already binary-ready (`HttpResponse.body: Option<Vec<u8>>`), but the routing and file-serving layers need updates to handle non-UTF-8 files and set appropriate Content-Type headers.

**Key Changes**:
- Replace `fs::read_to_string()` with `fs::read()` in `handle_connection()`
- Extend `build_routes()` to include binary file extensions (`.png`, `.jpg`, `.gif`, `.svg`, `.woff`, `.pdf`, etc.)
- Implement automatic Content-Type detection based on file extension
- Update integration tests to validate binary file serving

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Current State**:
- `handle_connection()` at line 46 uses `fs::read_to_string()` (line 70)
- `build_routes()` at line 91 hardcodes extensions: `["html", "css", "js"]` (line 104)
- No Content-Type header is set; browsers guess from content

**Changes Required**:
- Replace `fs::read_to_string()` with `fs::read()` to support binary files
- Extend `build_routes()` to accept additional file extensions
- Add Content-Type header setting based on file extension
- Create helper function `get_content_type(extension: &str) -> String` to map extensions to MIME types

---

### 2. `/home/jwall/personal/rusty/rcomm/src/models/http_response.rs`

**Current State** (lines 7-56):
- `body: Option<Vec<u8>>` — already binary-capable
- `add_body(&mut self, body: Vec<u8>)` — already accepts binary data
- `as_bytes()` — correctly serializes headers + body

**No Changes Required**: The HTTP response model is already fully binary-compatible. The current String→Vec<u8> conversion in `main.rs` handles the conversion correctly.

---

### 3. `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`

**Current State** (lines 96-156):
- `TestResponse.body: String` — assumes text bodies
- `read_response()` decodes all bodies as UTF-8 (line 145)
- Tests only validate HTML/CSS content matching

**Changes Required**:
- Update integration tests to include binary file tests (e.g., a 1x1 pixel PNG or simple SVG)
- Keep `TestResponse.body: String` but add a new `TestResponse.body_bytes: Option<Vec<u8>>` or handle binary detection
- Add tests for Content-Type headers on binary files
- Add test to verify binary files are served without corruption

---

## Step-by-Step Implementation

### Step 1: Create Content-Type Mapping Function

**Location**: `src/main.rs` (before `handle_connection()`, around line 45)

**Purpose**: Map file extensions to HTTP Content-Type headers following HTTP standards.

**Implementation**:
```rust
fn get_content_type(extension: &str) -> String {
    match extension {
        // Text files (already supported)
        "html" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" => "application/javascript; charset=utf-8",
        "txt" => "text/plain; charset=utf-8",
        "json" => "application/json; charset=utf-8",

        // Images
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "webp" => "image/webp",

        // Fonts
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "eot" => "application/vnd.ms-fontobject",

        // Documents
        "pdf" => "application/pdf",
        "xml" => "application/xml; charset=utf-8",

        // Default
        _ => "application/octet-stream",
    }.to_string()
}
```

**Notes**:
- Use `application/octet-stream` as fallback for unknown types
- Include `charset=utf-8` for text-based MIME types
- Align with IANA MIME type registry

---

### Step 2: Update `build_routes()` Function

**Location**: `src/main.rs`, line 91

**Current Code**:
```rust
match path.extension().unwrap().to_str().unwrap() {
    "html" | "css" | "js" => {
        // ... route registration
    }
    _ => {continue;}
}
```

**New Code**:
```rust
match path.extension().unwrap().to_str().unwrap() {
    // Text files
    "html" | "css" | "js" | "txt" | "json" | "xml" => {
        // ... existing logic for index.html, page.html, not_found.html
    }
    // Images
    "png" | "jpg" | "jpeg" | "gif" | "svg" | "ico" | "webp" => {
        // Direct file serving (no index/page convention)
        routes.insert(format!("{route}/{name}"), path);
    }
    // Fonts
    "woff" | "woff2" | "ttf" | "otf" | "eot" => {
        routes.insert(format!("{route}/{name}"), path);
    }
    // Documents
    "pdf" | "docx" | "xlsx" => {
        routes.insert(format!("{route}/{name}"), path);
    }
    // Unknown extensions
    _ => {continue;}
}
```

**Rationale**:
- Binary files are always served by their full relative path (not affected by `index.html`/`page.html` convention)
- Text files follow existing convention (directory-level routing for `index.html`/`page.html`)
- Extensions can be expanded without affecting existing logic

---

### Step 3: Update `handle_connection()` Function

**Location**: `src/main.rs`, lines 46–75

**Current Code** (lines 70–71):
```rust
let contents = fs::read_to_string(filename).unwrap();
response.add_body(contents.into());
```

**New Code**:
```rust
let binary_contents = fs::read(filename).unwrap();
response.add_body(binary_contents);

// Determine and set Content-Type header
let ext = std::path::Path::new(filename)
    .extension()
    .and_then(|e| e.to_str())
    .unwrap_or("txt");
let content_type = get_content_type(ext);
response.add_header("Content-Type".to_string(), content_type);
```

**Changes**:
- Replace `read_to_string()` with `read()` to handle all file types
- Extract file extension from the filename path
- Call `get_content_type()` to map extension to MIME type
- Set `Content-Type` header before sending response

**Error Handling**:
- File read errors still use `.unwrap()` (consistent with current codebase design)
- Extension extraction handles missing extensions gracefully with `.unwrap_or("txt")`

---

### Step 4: Add Unit Tests to `http_response.rs`

**Location**: `src/models/http_response.rs`, in the `#[cfg(test)]` module (after line 180)

**Purpose**: Verify that binary bodies are correctly serialized.

**New Tests**:
```rust
#[test]
fn add_body_with_binary_data() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
    let binary_data = vec![0xFF, 0xD8, 0xFF, 0xE0]; // JPEG header
    resp.add_body(binary_data.clone());
    assert_eq!(resp.try_get_body(), Some(binary_data));
}

#[test]
fn as_bytes_with_binary_body() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
    let binary_data = vec![0xFF, 0xD8, 0xFF, 0xE0]; // JPEG header
    resp.add_body(binary_data.clone());
    let bytes = resp.as_bytes();
    // Verify the binary data is present at the end of the response
    assert!(bytes.ends_with(&binary_data));
}

#[test]
fn add_header_content_type_binary() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
    resp.add_header("Content-Type".to_string(), "image/png".to_string());
    assert_eq!(
        resp.try_get_header("Content-Type".to_string()),
        Some("image/png".to_string())
    );
}
```

**Notes**:
- These tests verify that `Vec<u8>` bodies (including non-UTF-8 data) are preserved through serialization
- No changes to existing `HttpResponse` API; just additional test coverage

---

### Step 5: Update Integration Tests

**Location**: `src/bin/integration_test.rs`

#### 5a. Enhance `TestResponse` Structure

**Current** (lines 89–94):
```rust
struct TestResponse {
    status_code: u16,
    status_phrase: String,
    headers: HashMap<String, String>,
    body: String,
}
```

**New** (preserve backward compatibility):
```rust
struct TestResponse {
    status_code: u16,
    status_phrase: String,
    headers: HashMap<String, String>,
    body: String,
    body_bytes: Vec<u8>,  // Raw bytes for binary file validation
}
```

#### 5b. Update `read_response()` Function

**Location**: lines 96–156

**Key Changes**:
```rust
fn read_response(stream: &mut TcpStream) -> Result<TestResponse, String> {
    // ... (status line and headers reading unchanged)

    // Body via Content-Length
    let body_bytes = if let Some(cl) = headers.get("content-length") {
        let len: usize = cl
            .parse()
            .map_err(|_| format!("bad content-length: {cl}"))?;
        let mut buf = vec![0u8; len];
        reader
            .read_exact(&mut buf)
            .map_err(|e| format!("reading body: {e}"))?;
        buf
    } else {
        Vec::new()
    };

    // Attempt UTF-8 conversion, but don't fail on binary data
    let body = String::from_utf8_lossy(&body_bytes).to_string();

    Ok(TestResponse {
        status_code,
        status_phrase,
        headers,
        body,
        body_bytes,
    })
}
```

**Rationale**:
- Keep `body: String` for backward compatibility with existing tests
- Add `body_bytes: Vec<u8>` for binary file validation
- Use `from_utf8_lossy()` to handle mixed text/binary content gracefully

#### 5c. Create Test Binary File

Before running tests, create a minimal binary test file:

**File**: `pages/test.png`

**Content** (1x1 red pixel PNG, 68 bytes in hex):
```
89 50 4E 47 0D 0A 1A 0A 00 00 00 0D 49 48 44 52
00 00 00 01 00 00 00 01 08 02 00 00 00 90 77 53
DE 00 00 00 0C 49 44 41 54 08 99 63 F8 CF C0 00
00 00 03 00 01 FB 62 A0 0F 00 00 00 00 49 45 4E
44 AE 42 60 82
```

Or use a simple SVG instead (text-based but semantically binary):
```xml
<!-- pages/test.svg -->
<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1">
  <rect width="1" height="1" fill="red"/>
</svg>
```

#### 5d. Add Binary File Tests

**New Test Functions** (after line 322):

```rust
fn test_png_image(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/test.png")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    assert_eq_or_err(
        resp.headers.get("content-type").ok_or("missing Content-Type")?,
        &"image/png".to_string(),
        "content-type"
    )?;
    // PNG files start with magic bytes: 89 50 4E 47
    assert_eq_or_err(
        &resp.body_bytes[0..4],
        &[0x89, 0x50, 0x4E, 0x47],
        "PNG magic bytes"
    )?;
    Ok(())
}

fn test_svg_image(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/test.svg")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    assert_eq_or_err(
        resp.headers.get("content-type").ok_or("missing Content-Type")?,
        &"image/svg+xml".to_string(),
        "content-type"
    )?;
    assert_contains_or_err(&resp.body, "<svg", "SVG tag")?;
    Ok(())
}

fn test_content_type_on_html(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    assert_eq_or_err(
        resp.headers.get("content-type").ok_or("missing Content-Type")?,
        &"text/html; charset=utf-8".to_string(),
        "content-type"
    )?;
    Ok(())
}

fn test_content_type_on_css(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/index.css")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    assert_eq_or_err(
        resp.headers.get("content-type").ok_or("missing Content-Type")?,
        &"text/css; charset=utf-8".to_string(),
        "content-type"
    )?;
    Ok(())
}

fn test_binary_integrity(addr: &str) -> Result<(), String> {
    // Request PNG twice and verify byte-for-byte identity
    let resp1 = send_request(addr, "GET", "/test.png")?;
    let resp2 = send_request(addr, "GET", "/test.png")?;
    assert_eq_or_err(&resp1.body_bytes, &resp2.body_bytes, "binary integrity")?;
    Ok(())
}
```

#### 5e. Update Main Test Runner

**Location**: `main()` function, lines 342–354

**Current**:
```rust
let results = vec![
    run_test("root_route", || test_root_route(&addr)),
    run_test("index_css", || test_index_css(&addr)),
    // ... existing tests
];
```

**Updated** (add binary file tests):
```rust
let results = vec![
    run_test("root_route", || test_root_route(&addr)),
    run_test("index_css", || test_index_css(&addr)),
    run_test("howdy_route", || test_howdy_route(&addr)),
    run_test("howdy_page_css", || test_howdy_page_css(&addr)),
    run_test("404_does_not_exist", || test_404_does_not_exist(&addr)),
    run_test("404_deep_path", || test_404_deep_path(&addr)),
    run_test("content_length_matches", || test_content_length_matches(&addr)),
    run_test("trailing_slash", || test_trailing_slash(&addr)),
    run_test("double_slash", || test_double_slash(&addr)),
    run_test("concurrent_requests", || test_concurrent_requests(&addr)),
    // NEW: Binary file serving tests
    run_test("png_image", || test_png_image(&addr)),
    run_test("svg_image", || test_svg_image(&addr)),
    run_test("content_type_on_html", || test_content_type_on_html(&addr)),
    run_test("content_type_on_css", || test_content_type_on_css(&addr)),
    run_test("binary_integrity", || test_binary_integrity(&addr)),
];
```

**Expected Result**: 15 passing tests (10 existing + 5 new).

---

## Complete Code Changes Summary

### `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Change 1: Add Content-Type Helper** (insert before `fn main()` at line 22)
```rust
fn get_content_type(extension: &str) -> String {
    match extension {
        // Text files
        "html" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" => "application/javascript; charset=utf-8",
        "txt" => "text/plain; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "xml" => "application/xml; charset=utf-8",

        // Images
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "webp" => "image/webp",

        // Fonts
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "eot" => "application/vnd.ms-fontobject",

        // Documents
        "pdf" => "application/pdf",

        // Default
        _ => "application/octet-stream",
    }.to_string()
}
```

**Change 2: Update `handle_connection()` Function** (lines 70–71)
```rust
// OLD:
// let contents = fs::read_to_string(filename).unwrap();
// response.add_body(contents.into());

// NEW:
let binary_contents = fs::read(filename).unwrap();
response.add_body(binary_contents);

let ext = std::path::Path::new(filename)
    .extension()
    .and_then(|e| e.to_str())
    .unwrap_or("txt");
let content_type = get_content_type(ext);
response.add_header("Content-Type".to_string(), content_type);
```

**Change 3: Extend `build_routes()` Function** (lines 103–118)
```rust
// OLD:
// match path.extension().unwrap().to_str().unwrap() {
//     "html" | "css" | "js" => {
//         // ... logic
//     }
//     _ => {continue;}
// }

// NEW:
match path.extension().unwrap().to_str().unwrap() {
    // Text files (existing convention-based routing)
    "html" | "css" | "js" | "txt" | "json" | "xml" => {
        if name == "index.html" || name == "page.html" {
            if route == "" {
                routes.insert(String::from("/"), path);
            } else {
                routes.insert(route.clone(), path);
            }
        } else if name == "not_found.html" {
            continue;
        } else {
            routes.insert(format!("{route}/{name}"), path);
        }
    }
    // Images
    "png" | "jpg" | "jpeg" | "gif" | "svg" | "ico" | "webp" => {
        routes.insert(format!("{route}/{name}"), path);
    }
    // Fonts
    "woff" | "woff2" | "ttf" | "otf" | "eot" => {
        routes.insert(format!("{route}/{name}"), path);
    }
    // Documents
    "pdf" => {
        routes.insert(format!("{route}/{name}"), path);
    }
    // Unknown
    _ => {continue;}
}
```

---

## Testing Strategy

### Unit Tests

1. **HTTP Response Tests** (already exist, verify binary support):
   - `as_bytes_with_binary_body()` — Verify non-UTF-8 data survives serialization
   - `add_body_with_binary_data()` — Test with JPEG magic bytes

2. **Content-Type Function Tests** (new):
   ```rust
   #[test]
   fn get_content_type_returns_correct_mime_types() {
       assert_eq!(get_content_type("png"), "image/png");
       assert_eq!(get_content_type("html"), "text/html; charset=utf-8");
       assert_eq!(get_content_type("unknown"), "application/octet-stream");
   }
   ```

### Integration Tests (12 → 17 tests)

**Binary File Tests**:
- Serve a PNG file and verify magic bytes (`89 50 4E 47`)
- Serve an SVG file and verify Content-Type
- Verify Content-Type headers on text files (HTML, CSS)
- Verify binary data integrity across multiple requests

**Existing Tests** (should still pass):
- Root route HTML
- CSS files
- Named routes (howdy)
- 404 responses
- Content-Length header accuracy
- Trailing slash handling
- Double slash handling
- Concurrent requests

### Manual Testing

```bash
# Build and run
cargo build
cargo run &

# Test HTML (text)
curl -i http://127.0.0.1:7878/
# Verify: Content-Type: text/html; charset=utf-8

# Test CSS (text)
curl -i http://127.0.0.1:7878/index.css
# Verify: Content-Type: text/css; charset=utf-8

# Test SVG (binary-safe text)
curl -i http://127.0.0.1:7878/test.svg
# Verify: Content-Type: image/svg+xml

# Test PNG (binary)
curl -i http://127.0.0.1:7878/test.png -o /tmp/test.png
file /tmp/test.png
# Verify: PNG image data

# Run integration tests
cargo run --bin integration_test
# Expected: 17 passed, 0 failed
```

---

## Edge Cases & Handling

### 1. **Missing File Extension**
- **Issue**: File with no extension (e.g., `Makefile`)
- **Solution**: `get_content_type()` returns `application/octet-stream`
- **Status**: Handled by `.unwrap_or("txt")` + default case in match

### 2. **Unknown/Unsupported Binary Formats**
- **Issue**: User adds `.wasm` or `.webm` file
- **Solution**: Return `application/octet-stream`; add to whitelist as needed
- **Status**: Handled by default case

### 3. **Large Binary Files**
- **Issue**: Loading entire file into memory via `fs::read()`
- **Concern**: No streaming; file size limited by available RAM
- **Mitigation**: Document as limitation; future enhancement for streaming responses
- **Status**: Defer to future; current design matches read_to_string() behavior

### 4. **Binary Files Containing NULL Bytes**
- **Issue**: Integration tests use `String` for body; NULL bytes break UTF-8 conversion
- **Solution**: Use `String::from_utf8_lossy()` and add separate `body_bytes: Vec<u8>` field
- **Status**: Handled in updated `TestResponse` structure

### 5. **File Not Found During Service**
- **Issue**: File deleted between route building and request handling
- **Current Behavior**: `.unwrap()` panics (consistent with existing design)
- **Future**: Should return 500 Internal Server Error gracefully
- **Status**: Documented as known issue (existing behavior preserved)

### 6. **Content-Type Charset for Binary**
- **Issue**: Should binary files include `charset=utf-8`?
- **Decision**: No; only text MIME types include charset
- **Rationale**: Browser treats binary Content-Types as-is; charset is meaningless
- **Status**: Implemented correctly in `get_content_type()`

### 7. **Routing Conflicts**
- **Issue**: What if `pages/test.html` and `pages/test.png` both exist?
- **Solution**: Both are routed separately: `/test.html` and `/test.png`
- **Status**: No conflict; different routes, handled correctly

### 8. **Directory Traversal via Binary Files**
- **Issue**: Could attacker use `../` in filename to escape pages/ directory?
- **Current Protection**: `clean_route()` removes `..` and `.` segments
- **Scope**: Applies only to route matching; file paths come from `build_routes()` scan
- **Status**: Protected by existing `build_routes()` recursive scan (only scans `pages/` and subdirs)

---

## Backward Compatibility

### Existing Tests
All 10 existing integration tests should pass without modification:
- `root_route` — Still serves HTML as text
- `index_css` — Still serves CSS as text
- `howdy_route` — Still serves HTML
- `howdy_page_css` — Still serves CSS
- `404_does_not_exist` — Still returns 404 HTML
- `404_deep_path` — Still returns 404 HTML
- `content_length_matches` — Content-Length still auto-set by `add_body()`
- `trailing_slash` — Route normalization unchanged
- `double_slash` — Route normalization unchanged
- `concurrent_requests` — No locking changes; thread pool unchanged

### API Changes
- `HttpResponse.add_body()` signature unchanged (takes `Vec<u8>`)
- `main.rs` functions (thread pool integration) unchanged
- HTTP request/response format unchanged
- Route structure (`HashMap<String, PathBuf>`) unchanged

### Semantic Guarantee
Text files (HTML, CSS, JS) will automatically get correct `Content-Type` headers, improving browser behavior without breaking existing clients.

---

## Implementation Checklist

- [ ] Add `get_content_type()` function to `src/main.rs`
- [ ] Replace `fs::read_to_string()` with `fs::read()` in `handle_connection()`
- [ ] Add Content-Type header setting in `handle_connection()`
- [ ] Extend `build_routes()` to include binary file extensions
- [ ] Add binary-body unit tests to `src/models/http_response.rs`
- [ ] Create test binary files (`pages/test.png` or `pages/test.svg`)
- [ ] Update `TestResponse` struct to include `body_bytes`
- [ ] Update `read_response()` to capture raw bytes
- [ ] Add 5 new integration tests (PNG, SVG, Content-Type, integrity)
- [ ] Run `cargo test` and verify all unit tests pass
- [ ] Run `cargo run --bin integration_test` and verify 17 tests pass
- [ ] Manual test with `curl` to verify binary files serve correctly

---

## Performance Considerations

| Factor | Impact | Notes |
|--------|--------|-------|
| **File I/O** | Minimal | `read()` vs `read_to_string()` has negligible difference for small files |
| **Memory** | Linear with file size | Entire file loaded into memory; no streaming (future enhancement) |
| **CPU** | Reduced | No UTF-8 validation overhead for binary files (was inherent to `read_to_string()`) |
| **Network** | Unchanged | HTTP serialization identical; headers sent once per response |

---

## Future Enhancements

1. **Streaming Responses**: Replace `fs::read()` with streaming for large files
2. **Compression**: Auto-gzip for compressible file types (text, SVG)
3. **Caching Headers**: Set `Cache-Control`, `ETag` for static files
4. **Range Requests**: Support HTTP 206 Partial Content for large files
5. **MIME Type Detection**: Use file magic bytes instead of extensions (e.g., `magic` crate)
6. **Graceful Error Handling**: Return 500 errors instead of panicking on file read failure

---

## References

- HTTP Content-Type Header: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Content-Type
- IANA Media Types: https://www.iana.org/assignments/media-types/
- Rust `std::fs::read()`: https://doc.rust-lang.org/std/fs/fn.read.html
- RFC 7231 (HTTP/1.1 Semantics & Content): https://tools.ietf.org/html/rfc7231#section-3.1

---

## Related Issues / Known Limitations

- **No graceful error handling**: `.unwrap()` throughout codebase (file not found panics)
- **No streaming**: Large files must fit in memory
- **Limited extension support**: Only explicitly whitelisted extensions are served
- **No directory listings**: Directories return 404 even if they exist
