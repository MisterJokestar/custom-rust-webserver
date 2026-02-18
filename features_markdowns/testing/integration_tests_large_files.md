# Integration Tests for Large File Responses

**Category:** Testing
**Complexity:** 3/10
**Necessity:** 5/10
**Status:** Planning

---

## Overview

Add integration tests to `src/bin/integration_test.rs` that verify the server correctly handles large file responses. The current test files in `pages/` are small HTML/CSS files (a few hundred bytes). Large file tests exercise the TCP buffering, content-length accuracy, and memory handling for files in the kilobyte-to-megabyte range.

**Goal:** Ensure the server can serve files of various sizes without corruption, truncation, or crashes.

---

## Current State

### File Serving (src/main.rs, line 70-71)

```rust
let contents = fs::read_to_string(filename).unwrap();
response.add_body(contents.into());
```

The server reads the entire file into memory as a `String`, converts it to bytes via `add_body()`, and writes the full response in a single `stream.write_all()` call. This means:
- The entire file is buffered in memory
- `Content-Length` is set by `add_body()` based on the byte length
- `write_all()` handles TCP fragmentation internally

### Response Writing (src/main.rs, line 74)

```rust
stream.write_all(&response.as_bytes()).unwrap();
```

`write_all()` loops until all bytes are written, but the entire response (headers + body) is serialized into one `Vec<u8>` first. For very large files, this means double memory usage (file content + serialized response).

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`

**Add:** Large file test functions and test file generation.
**Modify:** `main()` function to register new tests.

### 2. Test Fixture Files

**Generate at test time:** Create temporary large HTML files in `pages/` before starting the server, clean up after.

---

## Step-by-Step Implementation

### Step 1: Add Test File Generation

Generate large test files before starting the server:

```rust
fn create_large_test_files() {
    let project_root = find_project_root();
    let pages_dir = project_root.join("pages");

    // 10 KB file
    let content_10k = format!(
        "<html><body>{}</body></html>",
        "A".repeat(10_000)
    );
    fs::write(pages_dir.join("large_10k.html"), &content_10k)
        .expect("failed to create large_10k.html");

    // 100 KB file
    let content_100k = format!(
        "<html><body>{}</body></html>",
        "B".repeat(100_000)
    );
    fs::write(pages_dir.join("large_100k.html"), &content_100k)
        .expect("failed to create large_100k.html");

    // 1 MB file
    let content_1m = format!(
        "<html><body>{}</body></html>",
        "C".repeat(1_000_000)
    );
    fs::write(pages_dir.join("large_1m.html"), &content_1m)
        .expect("failed to create large_1m.html");
}

fn cleanup_large_test_files() {
    let project_root = find_project_root();
    let pages_dir = project_root.join("pages");
    let _ = fs::remove_file(pages_dir.join("large_10k.html"));
    let _ = fs::remove_file(pages_dir.join("large_100k.html"));
    let _ = fs::remove_file(pages_dir.join("large_1m.html"));
}
```

### Step 2: Add Test Functions

```rust
fn test_serve_10k_file(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/large_10k")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;

    // Verify Content-Length matches body
    let cl: usize = resp
        .headers
        .get("content-length")
        .ok_or("missing Content-Length")?
        .parse()
        .map_err(|_| "bad Content-Length".to_string())?;
    assert_eq_or_err(&resp.body.len(), &cl, "body length matches Content-Length")?;

    // Verify content is from the generated file
    assert_contains_or_err(&resp.body, "AAAAAAA", "body contains expected content")?;
    Ok(())
}

fn test_serve_100k_file(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/large_100k")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;

    let cl: usize = resp
        .headers
        .get("content-length")
        .ok_or("missing Content-Length")?
        .parse()
        .map_err(|_| "bad Content-Length".to_string())?;
    assert_eq_or_err(&resp.body.len(), &cl, "body length matches Content-Length")?;
    assert_contains_or_err(&resp.body, "BBBBBBB", "body contains expected content")?;
    Ok(())
}

fn test_serve_1m_file(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/large_1m")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;

    let cl: usize = resp
        .headers
        .get("content-length")
        .ok_or("missing Content-Length")?
        .parse()
        .map_err(|_| "bad Content-Length".to_string())?;
    assert_eq_or_err(&resp.body.len(), &cl, "body length matches Content-Length")?;
    assert_contains_or_err(&resp.body, "CCCCCCC", "body contains expected content")?;
    Ok(())
}

fn test_large_file_exact_content(addr: &str) -> Result<(), String> {
    // Read the original file and compare byte-for-byte
    let project_root = find_project_root();
    let original = fs::read_to_string(project_root.join("pages/large_10k.html"))
        .map_err(|e| format!("reading original: {e}"))?;

    let resp = send_request(addr, "GET", "/large_10k")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    assert_eq_or_err(&resp.body.len(), &original.len(), "body length")?;

    if resp.body != original {
        return Err("Large file content mismatch".to_string());
    }
    Ok(())
}

fn test_multiple_large_file_requests(addr: &str) -> Result<(), String> {
    // Serve the same large file multiple times to test consistency
    for i in 0..5 {
        let resp = send_request(addr, "GET", "/large_100k")?;
        assert_eq_or_err(&resp.status_code, &200, &format!("request {i} status"))?;
        let cl: usize = resp
            .headers
            .get("content-length")
            .ok_or(format!("request {i} missing Content-Length"))?
            .parse()
            .map_err(|_| format!("request {i} bad Content-Length"))?;
        assert_eq_or_err(&resp.body.len(), &cl, &format!("request {i} body length"))?;
    }
    Ok(())
}
```

### Step 3: Update `main()` Flow

```rust
fn main() {
    // Create test files before starting the server
    create_large_test_files();

    let port = pick_free_port();
    let addr = format!("127.0.0.1:{port}");

    println!("Starting server on {addr}...");
    let mut server = start_server(port);

    if let Err(e) = wait_for_server(&addr, Duration::from_secs(5)) {
        eprintln!("ERROR: {e}");
        cleanup_large_test_files();
        let _ = server.kill();
        std::process::exit(1);
    }
    println!("Server is ready.\n");

    let results = vec![
        // ... existing tests ...
        run_test("serve_10k_file", || test_serve_10k_file(&addr)),
        run_test("serve_100k_file", || test_serve_100k_file(&addr)),
        run_test("serve_1m_file", || test_serve_1m_file(&addr)),
        run_test("large_file_exact_content", || test_large_file_exact_content(&addr)),
        run_test("multiple_large_file_requests", || test_multiple_large_file_requests(&addr)),
    ];

    // ... print results ...

    let _ = server.kill();
    let _ = server.wait();

    // Clean up test files
    cleanup_large_test_files();

    if failed > 0 {
        std::process::exit(1);
    }
}
```

---

## Edge Cases & Considerations

### 1. Read Timeout on Large Files

**Scenario:** The 5-second read timeout in `send_request()` may be too short for 1 MB responses on slow systems.

**Mitigation:** Increase the timeout for large file tests or use a longer global timeout:

```rust
stream.set_read_timeout(Some(Duration::from_secs(30)))
```

### 2. Memory Usage

**Scenario:** The server loads the entire file + serialized response into memory. A 1 MB file uses ~2 MB of memory per concurrent request.

**Test:** `test_multiple_large_file_requests` sends 5 sequential requests to verify the server doesn't leak memory.

### 3. Test File Routing

**Important:** The generated `.html` files must be named with `page.html` or `index.html` convention to match routes, OR they need unique names. With the current routing:
- `pages/large_10k.html` → route `/large_10k.html` (not `/large_10k`)

**Alternatives:**
- Name them `pages/large_10k/page.html` → route `/large_10k`
- Use `/large_10k.html` as the test request path
- Create directories for cleaner routes

**Recommendation:** Use the `.html` extension in the request path for simplicity:

```rust
let resp = send_request(addr, "GET", "/large_10k.html")?;
```

### 4. Cleanup on Failure

**Scenario:** If the test binary crashes before cleanup, test files remain in `pages/`.

**Mitigation:** The cleanup function uses `let _ = fs::remove_file()` which ignores missing files. Running the test again will overwrite and clean up.

### 5. Server Restart Required

Since `build_routes()` runs at startup, the test files must exist **before** the server starts. The `create_large_test_files()` call must come before `start_server()`.

---

## Testing Strategy

### Running the Tests

```bash
cargo build && cargo run --bin integration_test
```

### Expected Results

All large file tests pass. `Content-Length` matches body length for all file sizes. Multiple sequential requests return consistent results.

---

## Implementation Checklist

- [ ] Add `create_large_test_files()` function
- [ ] Add `cleanup_large_test_files()` function
- [ ] Add `test_serve_10k_file()` test
- [ ] Add `test_serve_100k_file()` test
- [ ] Add `test_serve_1m_file()` test
- [ ] Add `test_large_file_exact_content()` test
- [ ] Add `test_multiple_large_file_requests()` test
- [ ] Update `main()` with setup/teardown and test registration
- [ ] Verify read timeouts are sufficient for large files
- [ ] Run `cargo build` — no compiler errors
- [ ] Run `cargo run --bin integration_test` — all tests pass

---

## Dependencies

- **No new external dependencies**
- No feature prerequisites (tests use `.html` files which are already routed)

---

## References

- [Rust std::io::Write::write_all](https://doc.rust-lang.org/std/io/trait.Write.html#method.write_all)
- [TCP Window Size and Buffering](https://en.wikipedia.org/wiki/TCP_window_scale_option)
