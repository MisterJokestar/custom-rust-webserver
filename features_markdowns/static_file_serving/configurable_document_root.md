# Configurable Document Root Directory Implementation Plan

## Overview

This feature adds **configurable document root directory** support to the rcomm web server, replacing the hardcoded `pages/` directory with an environment variable-based configuration system. Currently, the server always serves static files from `./pages/`, which limits deployment flexibility and multi-environment support.

### Why Configurable Document Root Matters

A configurable document root enables:
- **Deployment flexibility**: Different deployments can point to different directories without recompilation
- **Multi-environment support**: Development, staging, and production can use separate content directories
- **Container-friendly**: Docker deployments can mount different volumes and specify them via environment variables
- **Scalability**: Multiple server instances can serve different content sets
- **Testing simplicity**: Integration tests can use temporary directories without interfering with default content

### Current State

Today, the code hardcodes the document root:
```rust
// src/main.rs, line 30
let path = Path::new("./pages");
let routes = build_routes(String::from(""), path);
```

The 404 response also hardcodes the path:
```rust
// src/main.rs, line 67
(HttpResponse::build(String::from("HTTP/1.1"), 404),
    "pages/not_found.html")
```

### Design Approach

The implementation will follow the existing pattern for environment variables:
1. Add a new `get_document_root()` function similar to `get_port()` and `get_address()`
2. Default to `./pages` for backward compatibility
3. Accept `RCOMM_DOCUMENT_ROOT` environment variable for override
4. Normalize and validate the path to prevent directory traversal attacks
5. Update `build_routes()` to accept and use the configurable root
6. Update 404 handling to use the configurable root
7. Update integration tests to use temporary directories

---

## Files to Modify

### 1. **`src/main.rs`** (Core Changes)
- Add `get_document_root()` function
- Update `main()` to retrieve and pass document root
- Update `handle_connection()` to use document root for 404 path
- Update function signatures to accept `PathBuf` for document root

### 2. **`src/bin/integration_test.rs`** (Test Updates)
- Update test setup to create temporary document root directories
- Update server spawn to pass `RCOMM_DOCUMENT_ROOT` environment variable
- Add test cases for custom document root scenarios

### 3. **`CLAUDE.md`** (Documentation)
- Update Architecture section to document document root configuration
- Add new environment variable to the list
- Update convention-based routing section to reference document root instead of hardcoded `pages/`

---

## Step-by-Step Implementation

### Step 1: Add `get_document_root()` Function

**File: `/home/jwall/personal/rusty/rcomm/src/main.rs`**

Add this new function after the existing `get_address()` function (around line 20):

```rust
fn get_document_root() -> PathBuf {
    let root = std::env::var("RCOMM_DOCUMENT_ROOT")
        .unwrap_or_else(|_| String::from("./pages"));

    let path = Path::new(&root).to_path_buf();

    // Verify the directory exists
    if !path.exists() {
        eprintln!("Warning: Document root '{}' does not exist", root);
    }

    if !path.is_dir() {
        eprintln!("Warning: Document root '{}' is not a directory", root);
    }

    path
}
```

### Step 2: Update `main()` Function

**File: `/home/jwall/personal/rusty/rcomm/src/main.rs`**

Replace the existing `main()` function with this updated version:

```rust
fn main() {
    let port = get_port();
    let address = get_address();
    let document_root = get_document_root();

    let full_address = format!("{address}:{port}");
    let listener = TcpListener::bind(&full_address).unwrap();

    let pool = ThreadPool::new(4);

    let routes = build_routes(String::from(""), &document_root);

    println!("Routes:\n{routes:#?}\n\n");
    println!("Listening on {full_address}");
    println!("Document Root: {}\n", document_root.display());

    for stream in listener.incoming() {
        let routes_clone = routes.clone();
        let document_root_clone = document_root.clone();
        let stream = stream.unwrap();

        pool.execute(move || {
            handle_connection(stream, routes_clone, document_root_clone);
        });
    }
}
```

### Step 3: Update `handle_connection()` Function Signature

**File: `/home/jwall/personal/rusty/rcomm/src/main.rs`**

Replace the function signature to accept document root:

```rust
fn handle_connection(mut stream: TcpStream, routes: HashMap<String, PathBuf>, document_root: PathBuf) {
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
        let not_found_path = document_root.join("not_found.html");
        (HttpResponse::build(String::from("HTTP/1.1"), 404),
            not_found_path.to_str().unwrap())
    };

    let contents = fs::read_to_string(filename).unwrap();
    response.add_body(contents.into());

    println!("Response: {response}");
    stream.write_all(&response.as_bytes()).unwrap();
}
```

### Step 4: Update Integration Tests

**File: `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`**

Update the server spawn logic to pass the document root environment variable. Find the section where the server is spawned and add `RCOMM_DOCUMENT_ROOT`:

```rust
// In the spawn_server() or setup function, add:
let mut child = Command::new("cargo")
    .args(&["run", "--bin", "rcomm"])
    .env("RCOMM_PORT", port.to_string())
    .env("RCOMM_ADDRESS", "127.0.0.1")
    .env("RCOMM_DOCUMENT_ROOT", "./pages")  // Add this line
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
    .expect("Failed to spawn server");
```

Add test cases for custom document root (optional but recommended):

```rust
// Add these test functions to the integration tests
fn test_custom_document_root_serves_files() -> TestResult {
    // This test would require setting up a temporary directory
    // For now, verify the default document root works
    let response = send_request("GET", "/", None, None)?;
    if response.starts_with("HTTP/1.1 200") {
        TestResult::pass("Server serves files from default document root")
    } else {
        TestResult::fail("Server failed to serve from document root")
    }
}

fn test_not_found_uses_document_root() -> TestResult {
    let response = send_request("GET", "/nonexistent-page", None, None)?;
    if response.starts_with("HTTP/1.1 404") {
        TestResult::pass("404 response found and served from document root")
    } else {
        TestResult::fail("404 response not properly served")
    }
}
```

### Step 5: Update Imports (if needed)

**File: `/home/jwall/personal/rusty/rcomm/src/main.rs`**

Ensure `PathBuf` is imported at the top:

```rust
use std::{
    collections::HashMap,
    fs,
    io::prelude::*,
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},  // Ensure PathBuf is imported
};
```

It should already be present, but verify it's there.

### Step 6: Update Documentation

**File: `/home/jwall/personal/rusty/rcomm/CLAUDE.md`**

Update the "Build & Run Commands" section to document the new environment variable:

```markdown
The server port/address/document root can be overridden via environment variables:

- `RCOMM_PORT` (default: `7878`)
- `RCOMM_ADDRESS` (default: `127.0.0.1`)
- `RCOMM_DOCUMENT_ROOT` (default: `./pages`)
```

Update the "Convention-Based Routing" section:

```markdown
### Convention-Based Routing

Routes are auto-generated by recursively scanning the document root directory (default `pages/`, overridable via `RCOMM_DOCUMENT_ROOT`):

- `<DOCUMENT_ROOT>/index.html` → `/`
- `<DOCUMENT_ROOT>/howdy/page.html` → `/howdy`
- `<DOCUMENT_ROOT>/howdy/page.css` → `/howdy/page.css`
- `<DOCUMENT_ROOT>/not_found.html` → Used for 404 responses (not routed)

Pattern: Files named `index.html` or `page.html` become routes at their directory's path level. Other `.html`/`.css`/`.js` files are routed by their full relative path.
```

---

## Testing Strategy

### Unit Tests

No unit tests are strictly needed for this feature since it's a configuration mechanism. However, the existing tests should continue to pass.

### Integration Tests

The integration tests should:
1. **Verify default behavior**: Server works with `./pages` when no environment variable is set
2. **Verify 404 handling**: 404 responses are served from the document root
3. **Verify environment variable override**: Server respects `RCOMM_DOCUMENT_ROOT`

### Manual Testing

```bash
# Test 1: Default behavior (should serve from ./pages)
cargo build
cargo run

# In another terminal:
curl -i http://127.0.0.1:7878/

# Test 2: Custom document root
mkdir -p /tmp/custom_pages
echo "<html><body>Custom Content</body></html>" > /tmp/custom_pages/index.html
echo "<html><body>Not Found</body></html>" > /tmp/custom_pages/not_found.html

RCOMM_DOCUMENT_ROOT=/tmp/custom_pages cargo run

# In another terminal:
curl -i http://127.0.0.1:7878/

# Test 3: Non-existent directory (should warn but continue)
RCOMM_DOCUMENT_ROOT=/nonexistent cargo run
# Should print warning but attempt to continue

# Test 4: Directory traversal attempt (should be safe due to build_routes logic)
curl -i http://127.0.0.1:7878/../../etc/passwd
# Should return 404
```

### Backward Compatibility

- Verify that existing tests pass without modification
- Verify that running without `RCOMM_DOCUMENT_ROOT` still serves from `./pages`
- Verify that existing deployments continue to work without changes

---

## Edge Cases & Considerations

### 1. **Relative vs Absolute Paths**
- **Issue**: Relative paths are resolved from the current working directory
- **Current Behavior**: `./pages` works when running from project root
- **Solution**: Document that paths are relative to current working directory; users can provide absolute paths
- **Example**: `RCOMM_DOCUMENT_ROOT=/var/www/content cargo run`

### 2. **Non-Existent Directory**
- **Scenario**: User specifies a directory that doesn't exist
- **Current Behavior**: Code prints warning but continues (as implemented in `get_document_root()`)
- **Outcome**: `build_routes()` will fail with `.unwrap()` when trying to read directory
- **Recommendation**: This is acceptable - operator error should fail fast
- **Alternative**: Could add more graceful error handling, but adds complexity

### 3. **Permission Issues**
- **Scenario**: Directory exists but is not readable by server process
- **Current Behavior**: `build_routes()` panics via `.unwrap()`
- **Recommendation**: Acceptable for this project; provides clear failure signal

### 4. **Symbolic Links**
- **Scenario**: Document root contains symbolic links (either files or subdirectories)
- **Current Behavior**: `Path::is_dir()`, `Path::is_file()`, and `fs::read_to_string()` follow symlinks
- **Security**: Symlinks are followed, so users can point to content outside document root
- **Consideration**: This is standard behavior for web servers; document root should be configured carefully

### 5. **Directory Traversal Attacks**
- **Scenario**: Attacker requests `GET /../../../etc/passwd`
- **Protection**: `clean_route()` function strips `..` segments (line 77-89 of src/main.rs)
- **Verification**: Routes are pre-computed at startup from actual filesystem; cannot be dynamically constructed to escape root
- **Conclusion**: Safe against directory traversal

### 6. **Dynamic Content Loading**
- **Limitation**: Routes are computed once at startup
- **Implication**: New files added to document root won't be served without server restart
- **Recommendation**: Document this as a known limitation; future feature could implement hot-reloading

### 7. **Special Characters in Paths**
- **Scenario**: Document root contains spaces or special characters
- **Current Behavior**: `Path` and `PathBuf` handle these correctly via `to_str().unwrap()`
- **Verification**: Shell quoting required when setting environment variable:
  ```bash
  RCOMM_DOCUMENT_ROOT="/path/with spaces/pages" cargo run
  ```
- **Conclusion**: Works correctly with proper quoting

### 8. **Performance Implications**
- **Route Building**: `build_routes()` scans entire directory tree once at startup
- **Per-Request Overhead**: Negligible - routes are in HashMap, O(1) lookup
- **Memory**: Document root path is cloned for each connection (small overhead)
- **Conclusion**: No significant performance impact

### 9. **Container / Deployment Scenarios**

#### Docker Example
```dockerfile
FROM rust:latest
WORKDIR /app
COPY . .
RUN cargo build --release

CMD ["sh", "-c", "RCOMM_DOCUMENT_ROOT=${DOCUMENT_ROOT:-./pages} ./target/release/rcomm"]
```

#### Kubernetes Example
```yaml
apiVersion: v1
kind: Pod
metadata:
  name: rcomm-server
spec:
  containers:
  - name: rcomm
    image: my-rcomm:latest
    env:
    - name: RCOMM_PORT
      value: "7878"
    - name: RCOMM_DOCUMENT_ROOT
      value: "/var/www/content"
    volumeMounts:
    - name: content
      mountPath: /var/www/content
  volumes:
  - name: content
    configMap:
      name: website-content
```

### 10. **Backward Compatibility**
- **Existing Deployments**: No change needed; will continue using `./pages`
- **New Deployments**: Can optionally specify `RCOMM_DOCUMENT_ROOT`
- **Migration Path**: Update deployments at own pace

---

## Implementation Checklist

- [ ] Add `get_document_root()` function to `src/main.rs`
- [ ] Update `main()` to call `get_document_root()` and pass to `build_routes()`
- [ ] Update `main()` to clone document root for each connection
- [ ] Update `handle_connection()` signature to accept `PathBuf`
- [ ] Update 404 path construction to use document root
- [ ] Update integration test server spawn to include environment variable
- [ ] Add manual test cases to integration tests
- [ ] Run full test suite: `cargo test`
- [ ] Run integration tests: `cargo run --bin integration_test`
- [ ] Manual testing with default `./pages` directory
- [ ] Manual testing with custom document root
- [ ] Manual testing with non-existent directory (verify warning)
- [ ] Update `CLAUDE.md` with environment variable documentation
- [ ] Update `CLAUDE.md` convention-based routing section
- [ ] Verify no regressions in existing tests
- [ ] Verify backward compatibility (no RCOMM_DOCUMENT_ROOT set)

---

## Estimated Effort

- **Code Implementation**: 30-45 minutes
- **Testing & Validation**: 30-45 minutes
- **Documentation & Review**: 15-20 minutes

**Total**: ~2 hours

---

## Code Changes Summary

### Before
```rust
// src/main.rs
let path = Path::new("./pages");
let routes = build_routes(String::from(""), path);
// ...
(HttpResponse::build(String::from("HTTP/1.1"), 404),
    "pages/not_found.html")
```

### After
```rust
// src/main.rs
let document_root = get_document_root();
let routes = build_routes(String::from(""), &document_root);
// ...
let not_found_path = document_root.join("not_found.html");
(HttpResponse::build(String::from("HTTP/1.1"), 404),
    not_found_path.to_str().unwrap())
```

---

## Related Features & Future Enhancements

1. **Hot Reloading**: Automatically detect new files in document root without restart
2. **Multiple Document Roots**: Support multiple document roots with path-based routing
3. **Configuration File**: Replace environment variables with TOML/YAML config file
4. **Virtual Hosts**: Support multiple virtual hosts with separate document roots
5. **Compressed Static Assets**: Serve pre-compressed `.gz` versions of files
6. **Cache Busting**: Generate fingerprinted asset names from document root
7. **Template Engine**: Support for dynamic content in document root

---

## References

- **Rust Path Documentation**: https://doc.rust-lang.org/std/path/struct.Path.html
- **Rust PathBuf Documentation**: https://doc.rust-lang.org/std/path/struct.PathBuf.html
- **Web Server Best Practices**: https://cheatsheetseries.owasp.org/cheatsheets/Nodejs_Security_Cheat_Sheet.html#file-upload-security
- **HTTP Status Codes (404)**: https://developer.mozilla.org/en-US/docs/Web/HTTP/Status/404
