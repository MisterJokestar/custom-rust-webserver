# Configurable Custom Error Pages Implementation Plan

## Overview

Currently, rcomm only has a custom page for 404 errors (`pages/not_found.html`, hardcoded at line 67 of `src/main.rs`). Other error responses (400 Bad Request at line 52, and the implicit 500 errors from `.unwrap()` panics) either return plain text bodies or crash the worker thread.

This feature adds configurable custom error pages for any HTTP status code (400, 403, 404, 500, 502, 503, etc.), allowing operators to provide branded, user-friendly HTML error pages.

**Complexity**: 3
**Necessity**: 4

**Key Changes**:
- Define a convention for error pages: `pages/errors/{status_code}.html` (e.g., `pages/errors/404.html`, `pages/errors/500.html`)
- Scan for error pages at startup and build an error page map
- Create a helper function `get_error_page(status_code) -> Option<Vec<u8>>` used throughout `handle_connection()`
- Fall back to a simple inline HTML body if no custom error page exists

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Current State**:
- Line 52-54: 400 response uses inline `format!("Bad Request: {e}")` body
- Line 66-67: 404 response uses hardcoded `pages/not_found.html`
- No 500 error handling (`.unwrap()` panics the thread)
- No convention for error page files

**Changes Required**:
- Add `build_error_pages()` function that scans `pages/errors/` directory
- Add `get_error_body()` helper that returns error page content for a status code
- Update 400 handler to use custom error page if available
- Update 404 handler to use `get_error_body()` instead of hardcoded path
- Pass error pages map to `handle_connection()`

### 2. `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`

**Changes Required**:
- Add tests for custom error pages

---

## Step-by-Step Implementation

### Step 1: Define Error Pages Directory Convention

Error pages live in `pages/errors/` with filenames matching HTTP status codes:

```
pages/
  errors/
    400.html
    403.html
    404.html    ← replaces pages/not_found.html
    500.html
    503.html
```

### Step 2: Add `build_error_pages()` Function

**Location**: `src/main.rs`, before `main()`

```rust
/// Scan pages/errors/ directory and load error page content by status code.
/// Returns a map of status_code -> file content (as bytes).
fn build_error_pages(error_dir: &Path) -> HashMap<u16, Vec<u8>> {
    let mut error_pages: HashMap<u16, Vec<u8>> = HashMap::new();

    if !error_dir.exists() || !error_dir.is_dir() {
        return error_pages;
    }

    for entry in fs::read_dir(error_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();

        if path.is_file() {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                if let Ok(status_code) = stem.parse::<u16>() {
                    let content = fs::read_to_string(&path).unwrap();
                    error_pages.insert(status_code, content.into());
                }
            }
        }
    }

    error_pages
}
```

### Step 3: Add `get_error_body()` Helper

```rust
/// Get the error page body for a status code.
/// Falls back to a simple inline HTML page if no custom page exists.
fn get_error_body(status_code: u16, error_pages: &HashMap<u16, Vec<u8>>) -> Vec<u8> {
    if let Some(body) = error_pages.get(&status_code) {
        body.clone()
    } else {
        let phrase = rcomm::models::http_status_codes::get_status_phrase(status_code);
        format!(
            "<html><head><title>{status_code} {phrase}</title></head>\
             <body><h1>{status_code} {phrase}</h1></body></html>"
        ).into()
    }
}
```

### Step 4: Update `main()` to Build Error Pages

**Location**: `src/main.rs`, in `main()` after `build_routes()` (after line 31)

```rust
    let error_pages_dir = Path::new("./pages/errors");
    let error_pages = build_error_pages(error_pages_dir);

    // Also check for legacy not_found.html if no errors/404.html exists
    if !error_pages.contains_key(&404) {
        let legacy_path = Path::new("./pages/not_found.html");
        if legacy_path.exists() {
            // Maintain backward compatibility
            let content = fs::read_to_string(legacy_path).unwrap();
            error_pages.insert(404, content.into());
        }
    }
```

### Step 5: Update `handle_connection()` Signature

```rust
fn handle_connection(
    mut stream: TcpStream,
    routes: HashMap<String, PathBuf>,
    error_pages: HashMap<u16, Vec<u8>>,
) {
```

### Step 6: Update Error Response Paths

**400 Bad Request** (lines 50-55):
```rust
        Err(e) => {
            eprintln!("Bad request: {e}");
            let mut response = HttpResponse::build(String::from("HTTP/1.1"), 400);
            response.add_body(get_error_body(400, &error_pages));
            let _ = stream.write_all(&response.as_bytes());
            return;
        }
```

**404 Not Found** (lines 65-68):
```rust
    } else {
        let mut response = HttpResponse::build(String::from("HTTP/1.1"), 404);
        response.add_body(get_error_body(404, &error_pages));
        println!("Response: {response}");
        stream.write_all(&response.as_bytes()).unwrap();
        return;
    };
```

### Step 7: Update Thread Pool Call Site

```rust
    for stream in listener.incoming() {
        let routes_clone = routes.clone();
        let error_pages_clone = error_pages.clone();
        let stream = stream.unwrap();

        pool.execute(move || {
            handle_connection(stream, routes_clone, error_pages_clone);
        });
    }
```

### Step 8: Exclude Error Pages from Regular Routes

Update `build_routes()` to skip the `errors/` directory (or any file within it):

```rust
        if path.is_dir() {
            let dir_name = name;
            if dir_name == "errors" && route == "" {
                continue; // Skip error pages directory at top level
            }
            routes.extend(
                build_routes(format!("{route}/{name}"), &path)
            );
```

---

## Testing Strategy

### Integration Tests

```rust
fn test_404_returns_custom_error_page(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/does-not-exist")?;
    assert_eq_or_err(&resp.status_code, &404, "status")?;
    if resp.body.is_empty() {
        return Err("404 body should not be empty".to_string());
    }
    Ok(())
}

fn test_error_page_not_routed(addr: &str) -> Result<(), String> {
    // Error pages should not be accessible as normal routes
    let resp = send_request(addr, "GET", "/errors/404.html")?;
    assert_eq_or_err(&resp.status_code, &404, "status")?;
    Ok(())
}
```

### Manual Testing

```bash
mkdir -p pages/errors
echo '<h1>Custom 404</h1>' > pages/errors/404.html
echo '<h1>Server Error</h1>' > pages/errors/500.html
cargo run &
curl -i http://127.0.0.1:7878/nonexistent  # Should show "Custom 404"
```

---

## Edge Cases & Handling

### 1. No `pages/errors/` Directory
- **Behavior**: `build_error_pages()` returns empty map; inline fallback HTML used
- **Status**: Handled gracefully

### 2. Legacy `pages/not_found.html` Still Exists
- **Behavior**: Used as 404 page if `pages/errors/404.html` doesn't exist (backward compatible)
- **Status**: Migration path preserved

### 3. Non-Numeric Filenames in `pages/errors/`
- **Example**: `pages/errors/custom.html`
- **Behavior**: `stem.parse::<u16>()` fails, file is skipped
- **Status**: Handled by `.ok()` on parse

### 4. Error Pages Loaded at Startup (Not Hot-Reloaded)
- **Behavior**: Changes to error pages require server restart
- **Status**: Consistent with route building behavior

### 5. Large Error Pages
- **Behavior**: Loaded into memory at startup; cloned per connection
- **Mitigation**: Error pages are typically small HTML files
- **Future**: Could use `Arc` to share without cloning

---

## Implementation Checklist

- [ ] Create `pages/errors/` directory convention
- [ ] Add `build_error_pages()` function
- [ ] Add `get_error_body()` helper function
- [ ] Add backward-compatible fallback to `pages/not_found.html`
- [ ] Update `handle_connection()` to accept and use error pages map
- [ ] Update 400 and 404 handlers to use `get_error_body()`
- [ ] Exclude `errors/` directory from regular routing
- [ ] Update thread pool closure to clone error pages
- [ ] Add integration tests
- [ ] Run `cargo test` and `cargo run --bin integration_test`

---

## Backward Compatibility

- When no `pages/errors/` directory exists, behavior is identical to current
- Legacy `pages/not_found.html` is still recognized as fallback for 404
- The 400 error response now returns HTML instead of plain text (minor behavioral change)
- All existing tests pass without modification
