# Integration Tests for Binary File Serving

**Category:** Testing
**Complexity:** 2/10
**Necessity:** 6/10
**Status:** Planning

---

## Overview

Add integration tests to `src/bin/integration_test.rs` that verify the server can correctly serve binary files (images, fonts, PDFs, etc.). Currently, the server uses `fs::read_to_string()` (src/main.rs, line 70), which fails or corrupts binary files. These tests will document the current limitation and serve as acceptance tests when binary file serving is implemented.

**Goal:** Validate that binary file responses preserve exact byte content and include correct `Content-Length` headers.

**Note:** These tests require placing test binary files in the `pages/` directory. The `build_routes()` function currently only routes `.html`, `.css`, and `.js` files (src/main.rs, line 104), so additional file type support must also be implemented for binary routes to exist.

---

## Current State

### File Reading (src/main.rs, line 70)

```rust
let contents = fs::read_to_string(filename).unwrap();
response.add_body(contents.into());
```

`read_to_string()` interprets file contents as UTF-8. Binary files containing invalid UTF-8 sequences will cause a panic (due to `unwrap()`), or corruption if the bytes happen to be valid UTF-8.

### Route Building (src/main.rs, line 103-104)

```rust
match path.extension().unwrap().to_str().unwrap() {
    "html" | "css" | "js" => {
```

Only `.html`, `.css`, and `.js` files are routed. Binary file types (`.png`, `.jpg`, `.woff2`, etc.) are silently ignored.

### Prerequisites

These tests depend on two features:
1. **Static File Serving > Binary File Serving** — replace `read_to_string()` with `read()` for `Vec<u8>` bodies
2. **Routing > Route Matching for More File Types** — extend `build_routes()` to include binary file extensions

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`

**Add:** Binary file test functions and a helper for binary comparison.
**Modify:** `main()` function to register new tests.

### 2. Test Fixture Files

**Add:** Small binary test files in the `pages/` directory:
- `pages/test.png` — a minimal 1x1 pixel PNG (67 bytes)
- `pages/test.bin` — a file with all 256 byte values (for round-trip testing)

---

## Step-by-Step Implementation

### Step 1: Create Test Fixture Files

Create a minimal PNG file programmatically in the test setup, or commit a tiny test file:

**Option A: Generate at test time**
```rust
fn create_test_binary_files() {
    use std::fs;
    // Minimal 1x1 transparent PNG (67 bytes)
    let png_bytes: Vec<u8> = vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG signature
        0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR chunk
        0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
        0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
        0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41,
        0x54, 0x78, 0x9C, 0x62, 0x00, 0x00, 0x00, 0x02,
        0x00, 0x01, 0xE5, 0x27, 0xDE, 0xFC, 0x00, 0x00,
        0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42,
        0x60, 0x82,
    ];
    let project_root = find_project_root();
    fs::write(project_root.join("pages/test.png"), &png_bytes)
        .expect("failed to create test.png");
}
```

**Option B: Commit a static test file** — Add a real 1x1 PNG to `pages/test.png` in the repository.

### Step 2: Add Binary Response Reader

The existing `read_response()` reads the body as a UTF-8 string. For binary tests, add a raw-byte variant:

```rust
struct TestResponseRaw {
    status_code: u16,
    status_phrase: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

fn read_response_raw(stream: &mut TcpStream) -> Result<TestResponseRaw, String> {
    let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);

    // Status line
    let mut status_line = String::new();
    reader
        .read_line(&mut status_line)
        .map_err(|e| format!("reading status line: {e}"))?;
    let parts: Vec<&str> = status_line.trim().splitn(3, ' ').collect();
    if parts.len() < 2 {
        return Err(format!("malformed status line: {status_line}"));
    }
    let status_code: u16 = parts[1]
        .parse()
        .map_err(|_| format!("bad status code: {}", parts[1]))?;
    let status_phrase = if parts.len() == 3 {
        parts[2].to_string()
    } else {
        String::new()
    };

    // Headers
    let mut headers = HashMap::new();
    loop {
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|e| format!("reading header: {e}"))?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some((key, val)) = trimmed.split_once(':') {
            headers.insert(key.trim().to_lowercase(), val.trim().to_string());
        }
    }

    // Body via Content-Length (raw bytes)
    let body = if let Some(cl) = headers.get("content-length") {
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

    Ok(TestResponseRaw {
        status_code,
        status_phrase,
        headers,
        body,
    })
}

fn send_request_raw(addr: &str, method: &str, path: &str) -> Result<TestResponseRaw, String> {
    let mut stream = TcpStream::connect(addr).map_err(|e| format!("connect: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| format!("set timeout: {e}"))?;
    let request = format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("write: {e}"))?;
    read_response_raw(&mut stream)
}
```

### Step 3: Add Test Functions

```rust
fn test_serve_png_file(addr: &str) -> Result<(), String> {
    let resp = send_request_raw(addr, "GET", "/test.png")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;

    // Verify PNG signature (first 8 bytes)
    if resp.body.len() < 8 {
        return Err(format!("body too short: {} bytes", resp.body.len()));
    }
    let png_sig = &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    if &resp.body[..8] != png_sig {
        return Err("PNG signature not found in response body".to_string());
    }
    Ok(())
}

fn test_binary_content_length_matches(addr: &str) -> Result<(), String> {
    let resp = send_request_raw(addr, "GET", "/test.png")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;

    let cl: usize = resp
        .headers
        .get("content-length")
        .ok_or("missing Content-Length header")?
        .parse()
        .map_err(|_| "Content-Length not a number".to_string())?;
    assert_eq_or_err(&resp.body.len(), &cl, "content-length vs body")?;
    Ok(())
}

fn test_binary_file_exact_roundtrip(addr: &str) -> Result<(), String> {
    // Read the original file from disk
    let project_root = find_project_root();
    let original = std::fs::read(project_root.join("pages/test.png"))
        .map_err(|e| format!("reading original: {e}"))?;

    let resp = send_request_raw(addr, "GET", "/test.png")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    assert_eq_or_err(&resp.body.len(), &original.len(), "body length")?;

    if resp.body != original {
        return Err("Binary content mismatch: response body differs from original file".to_string());
    }
    Ok(())
}
```

### Step 4: Register Tests in `main()`

```rust
// Binary file serving tests
// Note: These require binary file serving and extended route matching to be implemented
run_test("serve_png_file", || test_serve_png_file(&addr)),
run_test("binary_content_length", || test_binary_content_length_matches(&addr)),
run_test("binary_exact_roundtrip", || test_binary_file_exact_roundtrip(&addr)),
```

---

## Edge Cases & Considerations

### 1. Test File Setup / Cleanup

**Scenario:** Tests create files in `pages/` that might persist.

**Recommendation:** Either commit the test files to the repo (simplest) or generate and clean them up in the test harness. Since the server builds routes at startup, files must exist before the server starts.

### 2. Route Registration

**Prerequisite:** `build_routes()` must be extended to include `.png` (and other binary extensions). If this hasn't been implemented, the tests will get 404 responses.

**Mitigation:** The tests can be written now and will fail with a clear message ("expected 200, got 404") until the prerequisites are met.

### 3. Large Binary Files

**Scenario:** Serving a multi-megabyte image.

**Not covered here:** Large file tests are a separate testing feature. These tests use minimal files (~67 bytes).

### 4. Content-Type Header

**Scenario:** Binary files should have appropriate Content-Type (e.g., `image/png`).

**Not tested here:** Content-Type is a separate feature (HTTP Protocol > Content-Type header). These tests focus on binary content integrity.

---

## Implementation Checklist

- [ ] Create test binary file(s) in `pages/` directory
- [ ] Add `TestResponseRaw` struct and `read_response_raw()` / `send_request_raw()` helpers
- [ ] Add `test_serve_png_file()` test
- [ ] Add `test_binary_content_length_matches()` test
- [ ] Add `test_binary_file_exact_roundtrip()` test
- [ ] Register all tests in `main()`
- [ ] Run `cargo build` — no compiler errors
- [ ] Run `cargo run --bin integration_test` — all tests pass (once prerequisites are met)

---

## Dependencies

- **Static File Serving > Binary File Serving** — must replace `read_to_string()` with `read()`
- **Routing > Route Matching for More File Types** — must extend `build_routes()` to include `.png`

---

## References

- [Rust std::fs::read](https://doc.rust-lang.org/std/fs/fn.read.html)
- [PNG Specification](http://www.libpng.org/pub/png/spec/1.2/PNG-Structure.html)
