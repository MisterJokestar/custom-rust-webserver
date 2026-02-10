# Path Traversal Prevention: Canonicalization-Based Security

## Overview

This security feature implements path canonicalization validation to prevent path traversal attacks in the rcomm HTTP server. Currently, the `clean_route()` function removes empty segments, `.`, and `..` through string manipulation, which can be bypassed through encoding tricks or URL normalization edge cases. This feature implements a more robust, filesystem-aware approach using Rust's `std::fs::canonicalize()` to verify that resolved file paths remain within the `pages/` directory.

**Impact**: Prevents directory traversal attacks (e.g., requests to `/../../../etc/passwd`) that could escape the intended `pages/` root directory.

**Complexity**: 3/10
**Necessity**: 9/10

---

## Vulnerability Analysis

### Current Weakness

The existing `clean_route()` function (src/main.rs, lines 77-89):
```rust
fn clean_route(route: &String) -> String {
    let mut clean_route = String::from("");
    for part in route.split("/").collect::<Vec<_>>() {
        if part == "" || part == "." || part == ".." {
            continue;
        }
        clean_route.push_str(format!("/{part}").as_str());
    }
    if clean_route == "" {
        clean_route = String::from("/");
    }
    clean_route
}
```

**Problems**:
1. String-based filtering is susceptible to encoding bypasses (e.g., URL-encoded `%2e%2e` for `..`)
2. Double-encoded sequences (`%252e%252e`) might partially bypass the filter
3. No validation that the final resolved file path is actually within `pages/`
4. Relies on `build_routes()` pre-population for protection, which fails if routes can be constructed dynamically

### Attack Example

Although the current system pre-builds routes and validates against the hashmap, future changes or feature additions could introduce vulnerabilities. A canonicalization layer adds defense-in-depth.

---

## Solution Design

### Core Approach

1. **Canonicalization**: Convert requested file paths to their absolute, normalized form using `std::fs::canonicalize()`
2. **Base Directory Check**: Verify the canonical path starts with the canonical `pages/` directory path
3. **Fail-Safe Behavior**: Return 403 Forbidden or 404 Not Found if the path escapes the `pages/` directory

### Key Design Decisions

- **When to Validate**: In `handle_connection()` after the route is resolved to a file path but before reading the file
- **Base Directory**: Compute the canonical path of `pages/` once at startup for efficiency
- **Error Handling**: Gracefully handle symlinks and non-existent paths; return 403 if the check fails
- **No Changes to Routing**: The `build_routes()` and `clean_route()` functions remain largely unchanged; this is a **defense-in-depth** layer

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Changes Required:**
- Add a new function `validate_and_canonicalize_path()` to check if a resolved file path is within `pages/`
- Modify `handle_connection()` to call this validation function
- Optionally, compute and cache the canonical `pages/` path at startup for performance

**Rationale**: This is where file access decisions are made; validation must happen here before any file I/O.

---

## Step-by-Step Implementation

### Step 1: Add a Path Validation Helper Function

Add this function to `src/main.rs` before the `handle_connection()` function:

```rust
/// Validates that a file path is within the pages/ directory using canonicalization.
/// Returns Ok(canonical_path) if valid, or Err(reason) if the path escapes pages/.
fn validate_and_canonicalize_path(file_path: &str) -> Result<std::path::PathBuf, String> {
    let pages_dir = std::path::Path::new("./pages");

    // Canonicalize the pages directory
    let canonical_pages = pages_dir
        .canonicalize()
        .map_err(|e| format!("Failed to canonicalize pages directory: {}", e))?;

    // Canonicalize the requested file path
    let canonical_file = std::path::Path::new(file_path)
        .canonicalize()
        .map_err(|e| format!("Failed to canonicalize file path: {}", e))?;

    // Verify the canonical file path is within the canonical pages directory
    if !canonical_file.starts_with(&canonical_pages) {
        return Err(format!(
            "Path traversal attempt detected: {} is outside {}",
            canonical_file.display(),
            canonical_pages.display()
        ));
    }

    Ok(canonical_file)
}
```

**Key Points:**
- Uses `Path::canonicalize()` which resolves symlinks and normalizes path separators
- Compares the canonical paths to ensure the file is truly within `pages/`
- Returns descriptive error messages for logging
- Handles both existing and non-existent files (both will be caught by `canonicalize()` failing)

### Step 2: Modify `handle_connection()` Function

Update the `handle_connection()` function to validate paths:

**Current Code (lines 46-75):**
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

**Updated Code:**
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

    // Validate the file path is within pages/ directory
    match validate_and_canonicalize_path(filename) {
        Ok(_canonical_path) => {
            // Path is valid; proceed with file read
            let contents = fs::read_to_string(filename).unwrap();
            response.add_body(contents.into());

            println!("Response: {response}");
            stream.write_all(&response.as_bytes()).unwrap();
        }
        Err(e) => {
            // Path validation failed; return 403 Forbidden
            eprintln!("Path validation failed: {}", e);
            let mut forbidden_response = HttpResponse::build(String::from("HTTP/1.1"), 403);
            let body = "Forbidden: Access denied".to_string();
            forbidden_response.add_body(body.into());

            println!("Response: {forbidden_response}");
            let _ = stream.write_all(&forbidden_response.as_bytes());
        }
    }
}
```

**Key Changes:**
- Validation happens after route resolution but before file I/O
- Returns 403 Forbidden instead of 500 Internal Server Error
- Logs the validation failure for security auditing
- Gracefully handles errors without panicking

### Step 3: Handle the not_found.html Special Case

The `not_found.html` file is served for 404 responses and should also be validated:

**Current Pattern**: 404 responses serve `"pages/not_found.html"` directly

**Recommendation**: Ensure `not_found.html` exists in `pages/` and validate it the same way:

```rust
let (mut response, filename) = if routes.contains_key(&clean_target) {
    (HttpResponse::build(String::from("HTTP/1.1"), 200),
        routes.get(&clean_target).unwrap().to_str().unwrap())
} else {
    (HttpResponse::build(String::from("HTTP/1.1"), 404),
        "pages/not_found.html")
};

// Both 200 and 404 responses must validate their file paths
match validate_and_canonicalize_path(filename) {
    Ok(_) => {
        // Serve the file
        let contents = fs::read_to_string(filename).unwrap();
        response.add_body(contents.into());
        println!("Response: {response}");
        stream.write_all(&response.as_bytes()).unwrap();
    }
    Err(e) => {
        eprintln!("Path validation failed: {}", e);
        let mut forbidden_response = HttpResponse::build(String::from("HTTP/1.1"), 403);
        forbidden_response.add_body("Forbidden: Access denied".into());
        println!("Response: {forbidden_response}");
        let _ = stream.write_all(&forbidden_response.as_bytes());
    }
}
```

---

## Testing Strategy

### Unit Tests

Add tests in `src/main.rs` within a `#[cfg(test)]` module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    #[test]
    fn validate_and_canonicalize_valid_file() {
        // Ensure pages/index.html exists (it should in the test environment)
        assert!(Path::new("./pages/index.html").exists());

        let result = validate_and_canonicalize_path("./pages/index.html");
        assert!(result.is_ok());
        let canonical = result.unwrap();
        assert!(canonical.to_string_lossy().contains("pages"));
    }

    #[test]
    fn validate_and_canonicalize_traversal_attempt() {
        // Attempt to escape the pages/ directory
        let result = validate_and_canonicalize_path("./pages/../src/main.rs");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("outside"));
    }

    #[test]
    fn validate_and_canonicalize_direct_traversal() {
        // Attempt direct path traversal
        let result = validate_and_canonicalize_path("./pages/../../etc/passwd");
        assert!(result.is_err());
    }

    #[test]
    fn validate_and_canonicalize_nonexistent_file_in_pages() {
        // Non-existent files within pages/ should still fail canonicalization
        // (canonicalize() requires the path to exist)
        let result = validate_and_canonicalize_path("./pages/nonexistent.html");
        assert!(result.is_err());
    }
}
```

### Integration Tests

Add tests to `src/bin/integration_test.rs` to validate end-to-end behavior:

```rust
// Add this test to the integration test suite

fn test_path_traversal_protection() -> TestResult {
    let port = pick_free_port();
    let mut server = start_server(port);
    let addr = format!("127.0.0.1:{}", port);

    if let Err(e) = wait_for_server(&addr, Duration::from_secs(5)) {
        return TestResult::Failed(e);
    }

    // Test 1: Attempt to read ../src/main.rs via path traversal
    if let Ok(mut stream) = TcpStream::connect(&addr) {
        let request = "GET /../src/main.rs HTTP/1.1\r\nHost: localhost\r\n\r\n";
        if stream.write_all(request.as_bytes()).is_ok() {
            if let Ok(response) = read_response(&mut stream) {
                // Should return 404 or 403, NOT 200 with file contents
                if response.status_code == 200 {
                    return TestResult::Failed(
                        "Path traversal attack succeeded! Got 200 for /../src/main.rs".to_string()
                    );
                }
            }
        }
    }

    // Test 2: Attempt encoded traversal (%2e%2e)
    if let Ok(mut stream) = TcpStream::connect(&addr) {
        let request = "GET /%2e%2e/src/main.rs HTTP/1.1\r\nHost: localhost\r\n\r\n";
        if stream.write_all(request.as_bytes()).is_ok() {
            if let Ok(response) = read_response(&mut stream) {
                if response.status_code == 200 {
                    return TestResult::Failed(
                        "URL-encoded path traversal attack succeeded!".to_string()
                    );
                }
            }
        }
    }

    // Test 3: Verify legitimate requests still work
    if let Ok(mut stream) = TcpStream::connect(&addr) {
        let request = "GET / HTTP/1.1\r\nHost: localhost\r\n\r\n";
        if stream.write_all(request.as_bytes()).is_ok() {
            if let Ok(response) = read_response(&mut stream) {
                if response.status_code != 200 {
                    return TestResult::Failed(
                        format!("Legitimate request failed with status {}", response.status_code)
                    );
                }
            }
        }
    }

    server.kill().ok();
    TestResult::Passed
}
```

### Manual Testing

```bash
# Build the project
cargo build

# Run unit tests
cargo test validate_and_canonicalize

# Run integration tests
cargo run --bin integration_test

# Manual curl tests
curl http://127.0.0.1:7878/              # Should return 200 (index.html)
curl http://127.0.0.1:7878/../src/main.rs # Should return 403 or 404
curl http://127.0.0.1:7878/howdy          # Should return 200 (howdy/page.html)
```

---

## Edge Cases & Considerations

### 1. Symlinks in pages/ Directory

**Scenario**: A symlink within `pages/` points to a file outside `pages/`

**Behavior**: `canonicalize()` resolves symlinks, so the canonical path will be outside `pages/`, triggering a validation failure.

**Decision**: This is the **correct** behavior for security. If symlinks to external files are needed, they should be copied into `pages/` or the policy should be explicitly reconsidered.

**Alternative**: Use `std::fs::metadata()` with `symlink_metadata()` to allow symlinks, but this weakens security.

### 2. Case-Insensitive Filesystems (macOS, Windows)

**Scenario**: On case-insensitive filesystems, `./pages` and `./Pages` are the same directory.

**Behavior**: `canonicalize()` normalizes to the actual filesystem case, so comparisons are consistent.

**Resolution**: No action needed; `canonicalize()` handles this automatically.

### 3. Non-Existent Files

**Scenario**: A legitimate route points to a file that doesn't exist (e.g., due to a race condition).

**Behavior**: `canonicalize()` fails for non-existent paths.

**Decision**: Return 403 Forbidden. This is conservative but safe; the file should exist after `build_routes()`.

**Alternative**: Check file existence separately before canonicalization, but this reintroduces TOCTOU (Time-of-Check-Time-of-Use) races.

### 4. Performance

**Scenario**: Every request canonicalizes file paths, adding filesystem I/O overhead.

**Optimization**: Cache the canonical `pages/` path at startup:

```rust
use std::sync::OnceLock;

static CANONICAL_PAGES_DIR: OnceLock<std::path::PathBuf> = OnceLock::new();

fn get_canonical_pages_dir() -> &'static std::path::PathBuf {
    CANONICAL_PAGES_DIR.get_or_init(|| {
        std::path::Path::new("./pages")
            .canonicalize()
            .expect("Failed to canonicalize pages directory at startup")
    })
}

fn validate_and_canonicalize_path(file_path: &str) -> Result<std::path::PathBuf, String> {
    let canonical_pages = get_canonical_pages_dir();

    let canonical_file = std::path::Path::new(file_path)
        .canonicalize()
        .map_err(|e| format!("Failed to canonicalize file path: {}", e))?;

    if !canonical_file.starts_with(canonical_pages) {
        return Err(format!(
            "Path traversal attempt detected: {} is outside {}",
            canonical_file.display(),
            canonical_pages.display()
        ));
    }

    Ok(canonical_file)
}
```

This eliminates the per-request overhead of canonicalizing `pages/`.

### 5. Relative vs. Absolute Paths

**Current Code**: Uses `"./pages"` relative to the current working directory.

**Behavior**: `canonicalize()` converts this to an absolute path, making comparisons robust even if `cwd` changes.

**Resolution**: No changes needed; the implementation is already safe.

### 6. Windows Path Separators

**Scenario**: Running on Windows, where paths use `\` instead of `/`.

**Behavior**: `canonicalize()` uses the OS-native path separator, and `starts_with()` compares normalized paths.

**Resolution**: Works automatically; no special handling needed.

---

## Implementation Checklist

- [ ] Add `validate_and_canonicalize_path()` function to `src/main.rs`
- [ ] Update `handle_connection()` to call validation before file I/O
- [ ] Add unit tests for path validation edge cases
- [ ] Add integration tests for path traversal attempts
- [ ] Update README or documentation (optional) to mention the security layer
- [ ] Test on multiple platforms (Linux, macOS, Windows) if applicable
- [ ] Performance test: measure impact of canonicalization on request latency
- [ ] Code review: verify the validation logic is comprehensive

---

## Rollout Plan

1. **Phase 1 (Immediate)**: Implement the validation function and integrate it into `handle_connection()` with detailed error logging.
2. **Phase 2 (Testing)**: Run the full test suite (`cargo test`) and integration tests (`cargo run --bin integration_test`).
3. **Phase 3 (Verification)**: Manual testing with curl and path traversal payloads to confirm blocking behavior.
4. **Phase 4 (Optimization)**: If performance tests show impact, implement the `OnceLock` caching strategy.
5. **Phase 5 (Documentation)**: Document the security feature in CLAUDE.md under the "Known Issues" or "Security" section.

---

## Future Enhancements

1. **Allowlist Validation**: Pre-compute and cache the canonical paths of all routes in `build_routes()`, then validate against the allowlist instead of re-canonicalizing every request.
2. **Configurable Base Directory**: Allow the base directory (`pages/`) to be configured via environment variable, with validation at startup.
3. **Audit Logging**: Log all path traversal attempts to a separate audit log for security monitoring.
4. **Rate Limiting**: Combine with request rate limiting to mitigate brute-force path enumeration attacks.

---

## References

- [OWASP Path Traversal](https://owasp.org/www-community/attacks/Path_Traversal)
- [Rust std::fs::canonicalize](https://doc.rust-lang.org/std/fs/fn.canonicalize.html)
- [Rust std::path::Path](https://doc.rust-lang.org/std/path/struct.Path.html)
- [CWE-22: Improper Limitation of a Pathname to a Restricted Directory](https://cwe.mitre.org/data/definitions/22.html)

