# Configurable Custom 404 Page Implementation Plan

## Overview

Currently, the 404 error page is hardcoded to `pages/not_found.html` in `handle_connection()` at line 67 of `src/main.rs`:

```rust
    } else {
        (HttpResponse::build(String::from("HTTP/1.1"), 404),
            "pages/not_found.html")
    };
```

This feature allows the 404 page path to be configured via an environment variable, so operators can customize the error page without modifying source code or adhering to the `not_found.html` naming convention.

**Complexity**: 2
**Necessity**: 4

**Key Changes**:
- Add `RCOMM_404_PAGE` environment variable
- Default to `pages/not_found.html` when not set
- Validate the configured path exists at startup
- Use the configured path in `handle_connection()`

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Current State**:
- Line 67: Hardcoded `"pages/not_found.html"` for 404 responses
- Line 111: `not_found.html` is skipped during route building (`continue`)
- The 404 page path is a string literal, not configurable

**Changes Required**:
- Add `get_404_page()` configuration function
- Pass the 404 page path into `handle_connection()` or store it alongside routes
- Update `build_routes()` to skip the configured 404 page file (not just `not_found.html`)
- Validate file exists at startup

### 2. `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`

**Changes Required**:
- Add test verifying 404 response still works with default configuration
- Existing 404 tests should continue to pass unchanged

---

## Step-by-Step Implementation

### Step 1: Add Configuration Function

**Location**: `src/main.rs`, after `get_address()` (after line 20)

```rust
fn get_404_page() -> String {
    std::env::var("RCOMM_404_PAGE").unwrap_or_else(|_| String::from("pages/not_found.html"))
}
```

### Step 2: Validate 404 Page at Startup

**Location**: `src/main.rs`, in `main()` after `let routes = build_routes(...)` (after line 31)

```rust
    let not_found_page = get_404_page();
    if !Path::new(&not_found_page).exists() {
        eprintln!("Warning: 404 page not found at '{not_found_page}'. 404 responses will have empty bodies.");
    }
```

### Step 3: Pass 404 Page Path to `handle_connection()`

**Option A**: Add it as a parameter to `handle_connection()`.

**Current Signature** (line 46):
```rust
fn handle_connection(mut stream: TcpStream, routes: HashMap<String, PathBuf>) {
```

**New Signature**:
```rust
fn handle_connection(mut stream: TcpStream, routes: HashMap<String, PathBuf>, not_found_page: &str) {
```

**Update call site** in `main()` (lines 37-42):
```rust
    for stream in listener.incoming() {
        let routes_clone = routes.clone();
        let not_found_page_clone = not_found_page.clone();
        let stream = stream.unwrap();

        pool.execute(move || {
            handle_connection(stream, routes_clone, &not_found_page_clone);
        });
    }
```

**Update 404 path usage** (line 67):
```rust
    } else {
        (HttpResponse::build(String::from("HTTP/1.1"), 404),
            not_found_page)
    };
```

### Step 4: Update `build_routes()` to Skip Configured 404 Page

**Location**: `src/main.rs`, line 111

**Current Code**:
```rust
                    } else if name == "not_found.html" {
                        continue;
```

**New Code** (pass `not_found_page` to `build_routes()` or compare against the full path):

The simplest approach is to keep the current `not_found.html` skip as-is, since the 404 page is expected to be in `pages/` and the `RCOMM_404_PAGE` variable points to the full path. Alternatively, compare the full path:

```rust
fn build_routes(route: String, directory: &Path, not_found_path: &str) -> HashMap<String, PathBuf> {
    // ... existing code ...

                    } else if path.to_str().map(|p| p == not_found_path).unwrap_or(false) {
                        continue;
```

### Step 5: Add Integration Tests

```rust
fn test_404_uses_custom_body(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/does-not-exist")?;
    assert_eq_or_err(&resp.status_code, &404, "status")?;
    // Body should contain content from not_found.html
    if resp.body.is_empty() {
        return Err("404 response body should not be empty".to_string());
    }
    Ok(())
}
```

---

## Edge Cases & Handling

### 1. Configured File Doesn't Exist
- **Behavior**: Warning printed at startup; `fs::read_to_string()` will panic at runtime
- **Future improvement**: Return a simple inline 404 message instead of reading a file
- **Status**: Warning issued; consistent with existing `.unwrap()` pattern

### 2. 404 Page Set to a File Outside `pages/`
- **Example**: `RCOMM_404_PAGE=/etc/custom_404.html`
- **Behavior**: Works (reads from the specified path)
- **Security note**: The operator controls this variable; no path traversal risk from clients

### 3. 404 Page Set to Empty String
- **Behavior**: `Path::new("").exists()` returns false; warning printed
- **Status**: Handled by validation

### 4. 404 Page Is Also a Route
- **Example**: `RCOMM_404_PAGE=pages/error.html` where `error.html` is not skipped in `build_routes()`
- **Behavior**: The file is both a route (`/error.html` → 200) and the 404 page. This is acceptable.
- **Status**: No conflict

---

## Implementation Checklist

- [ ] Add `get_404_page()` function
- [ ] Add startup validation for 404 page path
- [ ] Update `handle_connection()` signature to accept `not_found_page`
- [ ] Replace hardcoded `"pages/not_found.html"` with parameter
- [ ] Update `main()` to pass `not_found_page` to thread pool closures
- [ ] Update `build_routes()` to skip the configured 404 page
- [ ] Add/update integration tests
- [ ] Run `cargo test` and `cargo run --bin integration_test`

---

## Backward Compatibility

When `RCOMM_404_PAGE` is not set, behavior is identical to current. Default value `pages/not_found.html` matches the existing hardcoded path. All existing tests pass unchanged.
