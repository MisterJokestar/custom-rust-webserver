# Route Matching for More File Types Implementation Plan

## Overview

Currently, `build_routes()` in `src/main.rs` (line 103-104) only routes files with `.html`, `.css`, or `.js` extensions:

```rust
match path.extension().unwrap().to_str().unwrap() {
    "html" | "css" | "js" => {
```

All other file types placed in `pages/` are silently ignored. This feature extends the route builder to include additional common web file types: `.json`, `.xml`, `.svg`, `.wasm`, `.txt`, `.ico`, `.png`, `.jpg`, `.gif`, `.webp`, `.woff`, `.woff2`, `.ttf`, `.otf`, `.pdf`, and others.

**Complexity**: 2
**Necessity**: 7

**Key Changes**:
- Expand the extension match arms in `build_routes()` to include more file types
- Non-HTML files are always routed by their full relative path (they never use the `index.html`/`page.html` convention)
- The `index.html`/`page.html`/`not_found.html` convention remains unchanged

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Current State**:
- `build_routes()` at line 91 recursively scans `pages/`
- Lines 103-117: Only `"html" | "css" | "js"` are matched; all others hit `_ => {continue;}`
- Convention-based routing: `index.html`/`page.html` map to directory-level routes, `not_found.html` is skipped

**Changes Required**:
- Add match arms for additional file extensions
- Non-HTML file types use simple path-based routing (no convention logic)

### 2. `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`

**Changes Required**:
- Add test files for new types (e.g., `pages/data.json`, `pages/feed.xml`)
- Add integration tests verifying these files are routable

---

## Step-by-Step Implementation

### Step 1: Expand Extension Match in `build_routes()`

**Location**: `src/main.rs`, lines 102-118

**Current Code**:
```rust
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
```

**New Code**:
```rust
        } else if path.is_file() {
            let extension = match path.extension().and_then(|e| e.to_str()) {
                Some(ext) => ext,
                None => continue, // Skip extensionless files
            };

            match extension {
                // Text/markup files — use convention-based routing for HTML
                "html" => {
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
                // All other recognized file types — route by full relative path
                "css" | "js" | "json" | "xml" | "txt" | "csv"
                | "svg" | "png" | "jpg" | "jpeg" | "gif" | "ico" | "webp" | "avif"
                | "woff" | "woff2" | "ttf" | "otf" | "eot"
                | "pdf" | "wasm"
                | "mp3" | "mp4" | "webm" | "ogg"
                | "map" => {
                    routes.insert(format!("{route}/{name}"), path);
                }
                _ => { continue; }
            }
```

**Key Design Decisions**:
- Only `html` files participate in the `index.html`/`page.html`/`not_found.html` convention
- `css` and `js` no longer share the HTML convention arm (they were never affected by it since they can't be named `index.html`, but separating makes intent clearer)
- New file types are grouped by category for readability
- `.map` is included for source map files (common in JS/CSS build toolchains)
- The extensionless file case now uses `.and_then()` instead of `.unwrap()` to avoid panics

### Step 2: Create Test Files

Create test files in `pages/` for integration testing:

```bash
echo '{"test": true}' > pages/data.json
echo '<?xml version="1.0"?><root/>' > pages/feed.xml
echo 'plain text content' > pages/readme.txt
```

### Step 3: Add Integration Tests

**Location**: `src/bin/integration_test.rs`

```rust
fn test_json_file_served(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/data.json")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    Ok(())
}

fn test_xml_file_served(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/feed.xml")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    Ok(())
}

fn test_txt_file_served(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/readme.txt")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    Ok(())
}
```

Register in test runner:
```rust
    run_test("json_file_served", || test_json_file_served(&addr)),
    run_test("xml_file_served", || test_xml_file_served(&addr)),
    run_test("txt_file_served", || test_txt_file_served(&addr)),
```

---

## Testing Strategy

### Unit Tests

No new unit tests needed — the `build_routes()` function is tested via integration tests. Optionally, add a unit test that builds routes from a temporary directory containing various file types.

### Integration Tests

| Test Name | What It Validates |
|-----------|-------------------|
| `json_file_served` | `.json` files are routable and return 200 |
| `xml_file_served` | `.xml` files are routable and return 200 |
| `txt_file_served` | `.txt` files are routable and return 200 |

### Manual Testing

```bash
echo '{"ok":true}' > pages/api.json
cargo run &
curl -i http://127.0.0.1:7878/api.json   # Should return 200
curl -i http://127.0.0.1:7878/unknown.xyz  # Should return 404
```

---

## Edge Cases & Handling

### 1. Extensionless Files
- **Behavior**: Skipped with `continue` (safe `.and_then()` instead of `.unwrap()`)
- **Status**: Fixes existing latent panic bug

### 2. Unknown Extensions
- **Behavior**: Still skipped (hit `_ => { continue; }` arm)
- **Status**: Consistent with current behavior; can be expanded later

### 3. Binary Files with `fs::read_to_string()`
- **Issue**: `handle_connection()` uses `fs::read_to_string()` (line 70) which fails on binary files
- **Dependency**: Binary file types (`.png`, `.jpg`, etc.) require the Binary File Serving feature first
- **Interim**: These routes will be registered but panic if accessed before binary serving is implemented
- **Recommendation**: Implement Binary File Serving before or alongside this feature

### 4. CSS/JS Convention Change
- **Behavior**: CSS and JS files no longer share the `html` match arm
- **Impact**: None — CSS/JS files never matched `index.html`/`page.html`/`not_found.html` names, so the convention logic never applied to them

---

## Implementation Checklist

- [ ] Refactor `build_routes()` extension match to separate HTML convention logic from other file types
- [ ] Add match arms for `.json`, `.xml`, `.txt`, `.csv`, `.svg`, `.png`, `.jpg`, `.jpeg`, `.gif`, `.ico`, `.webp`, `.avif`, `.woff`, `.woff2`, `.ttf`, `.otf`, `.eot`, `.pdf`, `.wasm`, `.mp3`, `.mp4`, `.webm`, `.ogg`, `.map`
- [ ] Fix extensionless file handling (replace `.unwrap()` with `.and_then()`)
- [ ] Create test files for integration testing
- [ ] Add integration tests for new file types
- [ ] Run `cargo test` to verify unit tests pass
- [ ] Run `cargo run --bin integration_test` to verify integration tests pass
- [ ] Clean up test files if not needed permanently

---

## Backward Compatibility

All existing routes and tests are unaffected. The HTML convention logic is preserved exactly. The only semantic change is that CSS/JS are now in a separate match arm, but since they never triggered convention logic, behavior is identical.
