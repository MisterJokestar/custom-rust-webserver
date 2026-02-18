# Favicon Support Implementation Plan

## Overview

Browsers automatically request `/favicon.ico` on virtually every page load. Currently, rcomm has no special handling for this request, so it falls through to the 404 handler, returning the `pages/not_found.html` content with a 404 status code. This produces unnecessary error noise in server output and a broken favicon indicator in the browser tab.

This feature adds favicon.ico support by:
1. Allowing users to place a custom `favicon.ico` file in the `pages/` directory (convention-based, like other routes)
2. Providing a hardcoded minimal default favicon served when no custom file exists, so `/favicon.ico` always returns a 200 response
3. Serving the favicon with the correct `image/x-icon` Content-Type header

**Complexity**: 1 (minimal changes required)
**Necessity**: 3 (cosmetic improvement; eliminates noisy 404s in browser dev tools and server logs)

**Dependencies**: This plan assumes the binary file serving feature has been implemented first (replacing `fs::read_to_string()` with `fs::read()` and adding Content-Type headers). If binary file serving is not yet implemented, Step 2 below includes a self-contained fallback approach that works with the current text-based serving code.

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Current State**:
- `handle_connection()` at line 46 matches the request target against the `routes` HashMap (line 62). If the route is not found, it returns 404 with `pages/not_found.html` (lines 65-68).
- `build_routes()` at line 91 only registers routes for files with `.html`, `.css`, or `.js` extensions (line 104). A `favicon.ico` file placed in `pages/` would be silently ignored because `.ico` is not in the match arms.
- File content is read via `fs::read_to_string()` at line 70, which would fail on binary `.ico` files (they are not valid UTF-8).

**Changes Required**:
- Add a `DEFAULT_FAVICON` constant containing minimal valid ICO bytes
- Add special-case handling for `/favicon.ico` in `handle_connection()` before the route lookup, with fallback to the default when no custom file exists
- (Optional) Add `.ico` to the recognized extensions in `build_routes()` for general `.ico` routing

### 2. `/home/jwall/personal/rusty/rcomm/pages/` (optional)

**Current State**: Contains `index.html`, `index.css`, `not_found.html`, and `howdy/` subdirectory. No `favicon.ico` exists.

**Changes Required** (optional):
- Users may place a custom `favicon.ico` here. The implementation works correctly both with and without this file present.

### 3. `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`

**Current State**: 10+ integration tests covering routes, 404s, content-length, slash handling, and concurrency. No favicon-related tests.

**Changes Required**:
- Add integration test for `/favicon.ico` returning 200
- Add integration test verifying correct `Content-Type` header
- Add integration test verifying non-empty response body
- Register all new tests in the `main()` runner

---

## Step-by-Step Implementation

### Step 1: Create a Default Favicon Constant

**Location**: `src/main.rs`, insert before `fn main()` (before line 22)

**Purpose**: Provide a minimal valid `.ico` file as embedded bytes so that `/favicon.ico` always returns a valid response, even when no custom file exists in `pages/`.

**Implementation**:
```rust
/// Minimal 1x1 pixel transparent favicon in ICO format.
/// Served as a fallback when no custom pages/favicon.ico exists.
const DEFAULT_FAVICON: &[u8] = &[
    // ICO Header
    0x00, 0x00, // Reserved
    0x01, 0x00, // Type: ICO
    0x01, 0x00, // Count: 1 image
    // Directory Entry
    0x01,       // Width: 1
    0x01,       // Height: 1
    0x00,       // Color palette: 0 (no palette)
    0x00,       // Reserved
    0x01, 0x00, // Color planes: 1
    0x20, 0x00, // Bits per pixel: 32
    0x28, 0x00, 0x00, 0x00, // Size of BMP data: 40 + 4 = 44 bytes
    0x16, 0x00, 0x00, 0x00, // Offset to BMP data: 22 bytes (6 + 16)
    // BMP Info Header (BITMAPINFOHEADER)
    0x28, 0x00, 0x00, 0x00, // Header size: 40
    0x01, 0x00, 0x00, 0x00, // Width: 1
    0x02, 0x00, 0x00, 0x00, // Height: 2 (ICO doubles height for AND mask)
    0x01, 0x00,             // Planes: 1
    0x20, 0x00,             // Bits per pixel: 32
    0x00, 0x00, 0x00, 0x00, // Compression: none
    0x04, 0x00, 0x00, 0x00, // Image size: 4 bytes
    0x00, 0x00, 0x00, 0x00, // X pixels per meter
    0x00, 0x00, 0x00, 0x00, // Y pixels per meter
    0x00, 0x00, 0x00, 0x00, // Colors used
    0x00, 0x00, 0x00, 0x00, // Important colors
    // Pixel data (1 pixel, BGRA): transparent
    0x00, 0x00, 0x00, 0x00,
];
```

**Notes**:
- Using a `const` byte slice means no heap allocation and no file I/O for the default case
- The favicon is intentionally minimal (transparent 1x1 pixel) so it does not visually interfere

---

### Step 2: Add Favicon Handling in `handle_connection()`

**Location**: `src/main.rs`, inside `handle_connection()`, between the `println!("Request: {http_request}");` call (line 60) and the route lookup (line 62)

**New Code** (insert after line 60, before line 62):
```rust
    // Special handling for favicon.ico
    if clean_target == "/favicon.ico" {
        let mut response = HttpResponse::build(String::from("HTTP/1.1"), 200);
        let favicon_path = Path::new("./pages/favicon.ico");
        let body = if favicon_path.exists() {
            fs::read(favicon_path).unwrap()
        } else {
            DEFAULT_FAVICON.to_vec()
        };
        response.add_body(body);
        response.add_header("Content-Type".to_string(), "image/x-icon".to_string());
        println!("Response: {response}");
        stream.write_all(&response.as_bytes()).unwrap();
        return;
    }
```

The rest of `handle_connection()` (lines 62-74) remains unchanged.

**Key Design Decisions**:
1. The favicon check happens **before** the route lookup, so it short-circuits early without needing to register `/favicon.ico` as a route
2. A custom `pages/favicon.ico` file takes priority over the default -- checked at request time via `Path::exists()` so the user can add/remove the file without restarting the server
3. The handler uses `fs::read()` (binary read) instead of `fs::read_to_string()` because `.ico` files are binary
4. The `return` statement exits `handle_connection()` early after serving the favicon, keeping the rest of the function unchanged

---

### Step 3: (Optional) Register `.ico` in `build_routes()`

**Location**: `src/main.rs`, lines 103-118

Add an `"ico"` arm after the `"html" | "css" | "js"` arm:

```rust
    "ico" => {
        routes.insert(format!("{route}/{name}"), path);
    }
```

**Note**: This alone is not sufficient because the server currently uses `fs::read_to_string()` on line 70 which would fail on binary `.ico` files. The special handling in Step 2 is the primary mechanism. This step only matters if binary file serving is already implemented and you want `.ico` files in subdirectories to be routable.

---

### Step 4: Add Integration Tests

**Location**: `src/bin/integration_test.rs`

#### 4a. Add Favicon Test Functions:

```rust
fn test_favicon_returns_200(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/favicon.ico")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    Ok(())
}

fn test_favicon_content_type(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/favicon.ico")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    let content_type = resp
        .headers
        .get("content-type")
        .ok_or("missing Content-Type header")?;
    assert_eq_or_err(content_type, &"image/x-icon".to_string(), "content-type")?;
    Ok(())
}

fn test_favicon_has_body(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/favicon.ico")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    let cl: usize = resp
        .headers
        .get("content-length")
        .ok_or("missing Content-Length header")?
        .parse()
        .map_err(|_| "Content-Length not a number".to_string())?;
    if cl == 0 {
        return Err("favicon body is empty (Content-Length: 0)".to_string());
    }
    Ok(())
}
```

#### 4b. Register Tests in Main Runner:

```rust
    // Favicon tests
    run_test("favicon_returns_200", || test_favicon_returns_200(&addr)),
    run_test("favicon_content_type", || test_favicon_content_type(&addr)),
    run_test("favicon_has_body", || test_favicon_has_body(&addr)),
```

---

## Testing Strategy

### Unit Tests

No new unit tests required in `src/models/http_response.rs` because the `HttpResponse` body handling already supports `Vec<u8>`. Optionally, sanity tests for the `DEFAULT_FAVICON` constant can be added to `src/main.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_favicon_has_valid_ico_header() {
        assert!(DEFAULT_FAVICON.len() >= 6);
        assert_eq!(DEFAULT_FAVICON[0..4], [0x00, 0x00, 0x01, 0x00]);
    }

    #[test]
    fn default_favicon_is_not_empty() {
        assert!(DEFAULT_FAVICON.len() > 0);
    }
}
```

### Integration Tests

| Test Name | What It Validates |
|-----------|-------------------|
| `favicon_returns_200` | `/favicon.ico` returns HTTP 200 instead of 404 |
| `favicon_content_type` | Response includes `Content-Type: image/x-icon` |
| `favicon_has_body` | Response body is non-empty (Content-Length > 0) |

### Manual Testing

```bash
cargo build && cargo run &
curl -i http://127.0.0.1:7878/favicon.ico
curl -s http://127.0.0.1:7878/favicon.ico -o /tmp/favicon.ico && file /tmp/favicon.ico
cargo test
cargo run --bin integration_test
```

---

## Edge Cases & Handling

### 1. No Custom `favicon.ico` Exists
- **Behavior**: Serves the `DEFAULT_FAVICON` constant (embedded bytes)
- **Status**: Handled by `Path::exists()` check

### 2. Custom `favicon.ico` Exists in `pages/`
- **Behavior**: Reads and serves the custom file via `fs::read()`
- **Status**: Handled by `if favicon_path.exists()` branch

### 3. Custom Favicon Replaced/Deleted While Server Is Running
- **Behavior**: Changes take effect on the next request (file read per-request, not cached at startup)
- **Status**: Works correctly

### 4. Request for `/favicon.ico/` (Trailing Slash)
- **Behavior**: `clean_route()` normalizes to `/favicon.ico`
- **Status**: Handled correctly

### 5. Request for `/FAVICON.ICO` (Case Sensitivity)
- **Behavior**: Case-sensitive; would NOT match and returns 404
- **Decision**: Consistent with the rest of the routing. Browsers always request lowercase `/favicon.ico`
- **Status**: Acceptable

### 6. Large Custom Favicon File
- **Behavior**: Entire file loaded into memory (consistent with all other file serving)
- **Status**: No special handling needed

### 7. `fs::read()` Fails on Custom Favicon
- **Behavior**: `.unwrap()` panics the worker thread
- **Status**: Consistent with existing codebase patterns

### 8. Race Condition Between `exists()` and `read()`
- **Behavior**: If file deleted between `exists()` returning true and `read()`, `.unwrap()` panics
- **Status**: Known TOCTOU race; consistent with codebase. Future error handling improvements would address this.

---

## Implementation Checklist

- [ ] Add `DEFAULT_FAVICON` byte constant to `src/main.rs` (before `fn main()`)
- [ ] Add favicon early-return handler in `handle_connection()` before route lookup (between lines 60 and 62)
- [ ] Add `test_favicon_returns_200` integration test function
- [ ] Add `test_favicon_content_type` integration test function
- [ ] Add `test_favicon_has_body` integration test function
- [ ] Register all 3 new tests in the integration test runner `main()`
- [ ] Run `cargo build` to verify compilation
- [ ] Run `cargo test` to verify all existing unit tests pass
- [ ] Run `cargo run --bin integration_test` to verify all integration tests pass
- [ ] Manual test: verify default favicon served when no custom file exists
- [ ] Manual test: place custom `favicon.ico` in `pages/` and verify it is served
- [ ] (Optional) Add `.ico` extension to `build_routes()` match arms (line 104)
- [ ] (Optional) Add unit tests for `DEFAULT_FAVICON` ICO header validation

---

## Backward Compatibility

### Existing Tests
All existing integration tests pass without modification. The favicon handler is an early-return code path that only activates for `/favicon.ico` and cannot interfere with existing routing.

### Behavioral Changes
- **`GET /favicon.ico`**: Changes from 404 to 200. This is the intended improvement.
- **All other routes**: Completely unchanged.

### API / Structural Changes
- No changes to `HttpResponse` or `HttpRequest` APIs
- No changes to the `routes: HashMap<String, PathBuf>` structure
- No changes to the thread pool, `clean_route()`, or `build_routes()`
- No new dependencies

### Performance Impact
- **Negligible**: One additional string comparison (`== "/favicon.ico"`) per request
- **Default favicon**: Zero I/O; served from a compile-time constant
- **Custom favicon**: One `Path::exists()` + one `fs::read()` per favicon request

---

## References

- Favicon specification: https://developer.mozilla.org/en-US/docs/Glossary/Favicon
- ICO file format: https://en.wikipedia.org/wiki/ICO_(file_format)
- Browser favicon request behavior: https://developer.mozilla.org/en-US/docs/Web/HTML/Attributes/rel#icon
- MIME type for ICO: `image/x-icon` (registered) or `image/vnd.microsoft.icon` (IANA standard)
