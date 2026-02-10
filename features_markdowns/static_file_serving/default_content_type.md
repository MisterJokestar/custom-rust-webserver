# Feature: Support serving files with no extension by defaulting to `application/octet-stream`

**Category:** Static File Serving
**Complexity:** 1/10
**Necessity:** 4/10

---

## Overview

Currently, the rcomm server only serves files with specific extensions (`.html`, `.css`, `.js`). Files without extensions are ignored during route building and requests to extension-less files will always return a 404.

This feature adds support for serving files with no extension by:
1. Including extension-less files in the route building process
2. Automatically setting the `Content-Type` header to `application/octet-stream` for any file without a recognized extension
3. Properly handling these files in the HTTP response pipeline

This is a low-complexity feature because:
- It requires minimal changes to route building logic
- Content-Type defaults are handled in a single location
- No new state management or complex parsing is needed
- Existing response handling automatically sets Content-Length headers

---

## Motivation

Some web applications need to serve files without extensions (e.g., `.well-known/acme-challenge/token` for ACME/Let's Encrypt verification, or custom binary/data files). Currently, these requests fail with 404 responses. By defaulting to `application/octet-stream`, the server becomes more flexible while maintaining security (the client will need to interpret the content appropriately).

---

## Files to Modify

### Primary Changes
1. **`/home/jwall/personal/rusty/rcomm/src/main.rs`** — Route building and content-type detection logic

### Testing
- **`/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`** — Add integration tests for extension-less files

### Test Fixtures
- **`/home/jwall/personal/rusty/rcomm/pages/`** — Add test files without extensions (e.g., `pages/test_file`, `pages/binary_data`)

---

## Step-by-Step Implementation

### Step 1: Extend Route Building to Include Extension-Less Files

**File:** `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Current code (lines 91-123):**
```rust
fn build_routes(route: String, directory: &Path) -> HashMap<String, PathBuf> {
    let mut routes: HashMap<String, PathBuf> = HashMap::new();

    for entry in fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let name = path.file_name().unwrap().to_str().unwrap();
        if path.is_dir() {
            routes.extend(
                build_routes(format!("{route}/{name}"), &path)
            );
        } else if path.is_file() {
            match path.extension().unwrap().to_str().unwrap() {
                "html" | "css" | "js" => {
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
                _ => {continue;}
            }
        }
    }

    routes
}
```

**Changes needed:**
- Check if a file has no extension using `path.extension().is_none()`
- If a file has no extension AND is not named "not_found.html", add it to the routes
- Add it at the route `/{route}/{name}` (same as for named files)

**New code:**
```rust
fn build_routes(route: String, directory: &Path) -> HashMap<String, PathBuf> {
    let mut routes: HashMap<String, PathBuf> = HashMap::new();

    for entry in fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let name = path.file_name().unwrap().to_str().unwrap();
        if path.is_dir() {
            routes.extend(
                build_routes(format!("{route}/{name}"), &path)
            );
        } else if path.is_file() {
            match path.extension() {
                Some(ext) => {
                    match ext.to_str().unwrap() {
                        "html" | "css" | "js" => {
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
                        _ => {continue;}
                    }
                }
                None => {
                    // No extension: add to routes and will serve with application/octet-stream
                    if name != "not_found.html" {
                        routes.insert(format!("{route}/{name}"), path);
                    }
                }
            }
        }
    }

    routes
}
```

### Step 2: Create a Helper Function to Determine Content-Type

**File:** `/home/jwall/personal/rusty/rcomm/src/main.rs`

Add this new function before `handle_connection()`:

```rust
fn get_content_type(filename: &str) -> String {
    match std::path::Path::new(filename).extension() {
        Some(ext) => {
            match ext.to_str().unwrap() {
                "html" => String::from("text/html"),
                "css" => String::from("text/css"),
                "js" => String::from("application/javascript"),
                // Add more MIME types as needed
                _ => String::from("application/octet-stream"),
            }
        }
        None => String::from("application/octet-stream"), // Default for no extension
    }
}
```

**Rationale:**
- Centralizes content-type logic in a single function
- Provides correct MIME types for known extensions
- Defaults to `application/octet-stream` for unknowns and extension-less files
- Easily extensible for future MIME types

### Step 3: Update `handle_connection()` to Use Content-Type Helper

**File:** `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Current code (lines 46-75):**
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

**New code:**
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
    let content_type = get_content_type(filename);
    response.add_header("Content-Type".to_string(), content_type);
    response.add_body(contents.into());

    println!("Response: {response}");
    stream.write_all(&response.as_bytes()).unwrap();
}
```

**Change summary:**
- Call `get_content_type(filename)` to determine the appropriate MIME type
- Add a `Content-Type` header to every response before adding the body
- This applies to both successful (200) and 404 responses

---

## Code Snippets Summary

### Refactored `build_routes()` function
The key insight is changing from direct pattern match to checking `Option<OsStr>` returned by `.extension()`:

```rust
fn build_routes(route: String, directory: &Path) -> HashMap<String, PathBuf> {
    let mut routes: HashMap<String, PathBuf> = HashMap::new();

    for entry in fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let name = path.file_name().unwrap().to_str().unwrap();
        if path.is_dir() {
            routes.extend(build_routes(format!("{route}/{name}"), &path));
        } else if path.is_file() {
            match path.extension() {
                Some(ext) => {
                    match ext.to_str().unwrap() {
                        "html" | "css" | "js" => {
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
                        _ => {continue;}
                    }
                }
                None => {
                    if name != "not_found.html" {
                        routes.insert(format!("{route}/{name}"), path);
                    }
                }
            }
        }
    }

    routes
}
```

### New `get_content_type()` helper
```rust
fn get_content_type(filename: &str) -> String {
    match std::path::Path::new(filename).extension() {
        Some(ext) => {
            match ext.to_str().unwrap() {
                "html" => String::from("text/html"),
                "css" => String::from("text/css"),
                "js" => String::from("application/javascript"),
                _ => String::from("application/octet-stream"),
            }
        }
        None => String::from("application/octet-stream"),
    }
}
```

### Updated `handle_connection()`
```rust
fn handle_connection(mut stream: TcpStream, routes: HashMap<String, PathBuf>) {
    // ... (error handling omitted for brevity) ...
    let (mut response, filename) = if routes.contains_key(&clean_target) {
        (HttpResponse::build(String::from("HTTP/1.1"), 200),
            routes.get(&clean_target).unwrap().to_str().unwrap())
    } else {
        (HttpResponse::build(String::from("HTTP/1.1"), 404),
            "pages/not_found.html")
    };

    let contents = fs::read_to_string(filename).unwrap();
    let content_type = get_content_type(filename);
    response.add_header("Content-Type".to_string(), content_type);
    response.add_body(contents.into());

    println!("Response: {response}");
    stream.write_all(&response.as_bytes()).unwrap();
}
```

---

## Testing Strategy

### Unit Tests
No new unit tests are needed since the logic is simple and the existing response and request models handle all the heavy lifting. The `get_content_type()` function is straightforward and could optionally be unit tested, but it's better tested via integration tests.

### Integration Tests

Add tests to `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`:

#### Test 1: File without extension returns 200
```rust
fn test_extensionless_file(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/test_file")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    assert_contains_or_err(&resp.body, "test content", "body")?;
    Ok(())
}
```

#### Test 2: Content-Type header is set for extension-less files
```rust
fn test_extensionless_file_content_type(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/test_file")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    let ct = resp.headers.get("content-type")
        .ok_or("missing Content-Type header")?;
    assert_eq_or_err(ct, &"application/octet-stream".to_string(), "content-type")?;
    Ok(())
}
```

#### Test 3: Content-Type is still correct for known extensions
```rust
fn test_html_content_type(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    let ct = resp.headers.get("content-type")
        .ok_or("missing Content-Type header")?;
    assert_eq_or_err(ct, &"text/html".to_string(), "content-type")?;
    Ok(())
}
```

#### Test 4: Unknown extensions default to application/octet-stream
```rust
fn test_unknown_extension_content_type(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/test.xyz")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    let ct = resp.headers.get("content-type")
        .ok_or("missing Content-Type header")?;
    assert_eq_or_err(ct, &"application/octet-stream".to_string(), "content-type")?;
    Ok(())
}
```

### Integration Test Fixtures

Create the following test files in the `pages/` directory:

1. **`pages/test_file`** (no extension)
   ```
   test content for extension-less file
   ```

2. **`pages/test.xyz`** (unknown extension)
   ```
   unknown extension test
   ```

3. **`pages/nested/binary_data`** (nested, no extension)
   ```
   binary data here
   ```

Add these test files to the `pages/` directory before running the integration tests.

---

## Edge Cases and Considerations

### Edge Case 1: Hidden Files or System Files
**Consideration:** Files starting with `.` (e.g., `.gitignore`, `.env`) will now be served if they're in the `pages/` directory. This is acceptable because:
- The `pages/` directory is meant for content serving
- Sensitive files should not be committed to version control if the repo is public
- The feature doesn't change security; these files were always accessible via the filesystem

**Recommendation:** Document this behavior in project documentation.

### Edge Case 2: Files with Multiple Dots
**Consideration:** Files like `backup.tar.gz` have multiple dots. The `.extension()` call will only return `gz`, so these will get `application/gzip` or `application/octet-stream`.

**Status:** This is acceptable. If more sophisticated MIME type detection is needed later, it can be added (e.g., checking for `.tar.gz`, `.tar.bz2`).

### Edge Case 3: Very Large Files
**Consideration:** Currently, `fs::read_to_string()` reads entire files into memory. This could be problematic for large binary files.

**Status:** This is a pre-existing issue, not introduced by this feature. A future optimization could use streaming/chunked transfer encoding, but that's out of scope.

### Edge Case 4: Content-Type for 404 Responses
**Consideration:** The `not_found.html` file will be served with `text/html` content-type because `.html` extension is recognized.

**Status:** This is correct behavior.

### Edge Case 5: Binary Files
**Consideration:** Extension-less files served with `application/octet-stream` should be treated as binary by clients.

**Status:** This is correct. Browsers will typically download such files rather than try to display them. If text content is needed, the extension should be `.txt`.

---

## Implementation Notes

### Order of Changes
1. Modify `build_routes()` first to start serving extension-less files
2. Create test fixtures in `pages/`
3. Add `get_content_type()` helper function
4. Update `handle_connection()` to use the helper
5. Add integration tests
6. Run full test suite to verify no regressions

### Testing Before Commit
```bash
# Build and test
cargo build
cargo test                           # Unit tests
cargo run --bin integration_test     # Integration tests

# Manual verification
cargo run &
# Request: curl -v http://127.0.0.1:7878/test_file
# Expected: 200 OK, Content-Type: application/octet-stream
```

### Potential Regressions
- The change to `build_routes()` from pattern matching on `.unwrap()` to checking `Option<_>` is safer but must be verified to not break existing routes
- All existing HTML/CSS/JS files should still be served with correct content-types
- The `not_found.html` special case must still work

---

## Future Enhancements

### Content-Type Expansion
Add more common MIME types in `get_content_type()`:
```rust
"json" => String::from("application/json"),
"xml" => String::from("application/xml"),
"png" => String::from("image/png"),
"jpg" | "jpeg" => String::from("image/jpeg"),
"gif" => String::from("image/gif"),
"svg" => String::from("image/svg+xml"),
"txt" => String::from("text/plain"),
"woff" => String::from("font/woff"),
"woff2" => String::from("font/woff2"),
```

### Content-Type Detection Library
If more sophisticated detection is needed, consider adding a dependency like `mime_guess` or `infer`. This is out of scope for this feature but documented here for future reference.

### Configuration-Based Content Types
Allow overriding MIME types via environment variables or a configuration file (e.g., `.rcommrc` or environment variables like `RCOMM_MIME_TYPES`).

---

## Summary

This feature is straightforward to implement with minimal risk:

1. **Route Building:** Extend `build_routes()` to include extension-less files by checking `path.extension().is_none()`
2. **Content-Type:** Create `get_content_type()` helper that defaults to `application/octet-stream`
3. **Response Handling:** Call `get_content_type()` in `handle_connection()` and set the header
4. **Testing:** Add integration tests to verify content-types are correct for extension-less and known file types

Total lines of code changed: ~30 lines (mostly structural, not complex logic).
Total new functions: 1 (`get_content_type()`).
Risk level: Very low (no breaking changes, purely additive feature).
