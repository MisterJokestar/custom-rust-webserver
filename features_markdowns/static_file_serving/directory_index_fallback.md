# Directory Index Fallback Implementation Plan

## Overview

**Feature:** Add directory index fallback — serve `index.html` when a directory path is requested without a matching `page.html`

**Category:** Static File Serving

**Complexity:** 3/10

**Necessity:** 5/10

### Problem Statement

Currently, the rcomm server uses a convention-based routing system where:
- `pages/index.html` → `/` (root)
- `pages/howdy/page.html` → `/howdy`
- Other files are routed by their full relative path

This means:
- **Directories with `page.html`** are accessible as routes (e.g., `/howdy` serves `/pages/howdy/page.html`)
- **Directories with only `index.html`** (not at root) are NOT accessible as routes, only as direct file paths (e.g., `/some_dir/index.html` works, but `/some_dir/` returns 404)

**Desired behavior:** If a request comes in for `/some_dir/` or `/some_dir` and there is no `page.html` at that location, the server should fall back to serving `index.html` if it exists in that directory. This provides more flexible directory indexing consistent with web server conventions.

### Current Routing Logic

In `src/main.rs`, the `build_routes()` function scans the `pages/` directory and generates routes:

```rust
if name == "index.html" || name == "page.html" {
    if route == "" {
        routes.insert(String::from("/"), path);  // Only for root
    } else {
        routes.insert(route.clone(), path);
    }
}
```

This means:
- `index.html` only creates a route for `/` (at root)
- `page.html` creates a route for any directory path
- Nested `index.html` files are not routed at all

### Desired Behavior After Implementation

- `/some_dir/` or `/some_dir` with no `page.html` → serves `pages/some_dir/index.html` if it exists
- `/some_dir/` or `/some_dir` with `page.html` → still serves `page.html` (takes precedence)
- `/some_dir/` or `/some_dir` with neither → 404
- Trailing slashes normalized via `clean_route()`

---

## Files to Modify

### Primary Changes
1. **`src/main.rs`** — Core routing and connection handling logic
   - Modify `build_routes()` to register `index.html` files as directory routes (not just root)
   - Update `handle_connection()` fallback logic to check for index files
   - Possibly refactor route lookup to support fallback chain

---

## Step-by-Step Implementation

### Step 1: Understand Current Flow

**Route Registration Phase (`build_routes`):**
- Recursively walks `pages/` directory
- For `page.html` or `index.html`: creates a route at the directory level
- Currently `index.html` only inserted at root (`route == ""`)

**Request Handling Phase (`handle_connection`):**
- `clean_route()` normalizes the request path (strips empties, `.`, `..`)
- `routes.contains_key(&clean_target)` checks for exact match
- If not found: returns 404

**Limitation:** There's no fallback mechanism. The route must exist as an exact key in the HashMap.

### Step 2: Modify `build_routes()` to Register `index.html` Routes

**Current code (lines 105-110):**
```rust
if name == "index.html" || name == "page.html" {
    if route == "" {
        routes.insert(String::from("/"), path);
    } else {
        routes.insert(route.clone(), path);
    }
}
```

**Updated code:**
```rust
if name == "index.html" || name == "page.html" {
    let route_key = if route == "" {
        String::from("/")
    } else {
        route.clone()
    };
    routes.insert(route_key, path);
}
```

**Why this works:**
- Now `index.html` files at any level register themselves as routes
- Example: `pages/products/index.html` registers as `/products`
- Example: `pages/index.html` still registers as `/`
- This removes the `route == ""` gate-keeping for `index.html`

### Step 3: Handle Collision Between `page.html` and `index.html`

When both exist in the same directory, `page.html` should take precedence.

**Modification to `build_routes()` (line 99-115):**

Process `page.html` files first by separating the logic:

```rust
// Phase 1: Register page.html files (takes precedence)
for entry in fs::read_dir(directory).unwrap() {
    let entry = entry.unwrap();
    let path = entry.path();
    let name = path.file_name().unwrap().to_str().unwrap();
    if path.is_file() && name == "page.html" {
        match path.extension().unwrap().to_str().unwrap() {
            "html" => {
                let route_key = if route == "" {
                    String::from("/")
                } else {
                    route.clone()
                };
                routes.insert(route_key, path);
            }
            _ => {}
        }
    }
}

// Phase 2: Register index.html files (only if page.html didn't exist)
for entry in fs::read_dir(directory).unwrap() {
    let entry = entry.unwrap();
    let path = entry.path();
    let name = path.file_name().unwrap().to_str().unwrap();
    if path.is_file() && name == "index.html" {
        match path.extension().unwrap().to_str().unwrap() {
            "html" => {
                let route_key = if route == "" {
                    String::from("/")
                } else {
                    route.clone()
                };
                // Only insert if page.html didn't already create this route
                if !routes.contains_key(&route_key) {
                    routes.insert(route_key, path);
                }
            }
            _ => {}
        }
    }
}

// Phase 3: Recursively process subdirectories and other files
for entry in fs::read_dir(directory).unwrap() {
    let entry = entry.unwrap();
    let path = entry.path();
    let name = path.file_name().unwrap().to_str().unwrap();
    if path.is_dir() {
        routes.extend(build_routes(format!("{route}/{name}"), &path));
    } else if path.is_file() {
        match path.extension().unwrap().to_str().unwrap() {
            "html" | "css" | "js" => {
                if name != "page.html" && name != "index.html" && name != "not_found.html" {
                    routes.insert(format!("{route}/{name}"), path);
                }
            }
            _ => {}
        }
    }
}
```

**Alternative (Simpler) Approach:**

Instead of separating the logic, keep the current structure but simply register all `index.html` and `page.html` files as routes:

```rust
if name == "index.html" || name == "page.html" {
    let route_key = if route == "" {
        String::from("/")
    } else {
        route.clone()
    };
    // If page.html exists in the same directory, it will overwrite index.html
    // (HashMap insert with same key replaces the value)
    routes.insert(route_key, path);
}
```

**Pros:** Simpler, takes advantage of HashMap's natural overwrite behavior if we process in the right order
**Cons:** Requires sorting entries so `page.html` is processed after `index.html`, or processing them in two passes

**Recommendation:** Use the simpler approach with two separate passes for clarity. Reading the directory twice is negligible.

### Step 4: Update `handle_connection()` Response Logic

**Current code (lines 62-68):**
```rust
let (mut response, filename) = if routes.contains_key(&clean_target) {
    (HttpResponse::build(String::from("HTTP/1.1"), 200),
        routes.get(&clean_target).unwrap().to_str().unwrap())
} else {
    (HttpResponse::build(String::from("HTTP/1.1"), 404),
        "pages/not_found.html")
};
```

**No change needed here!**

Since `build_routes()` now registers `index.html` files as routes, they will be found by the `routes.contains_key()` check. The existing logic works without modification.

### Step 5: Verify Edge Cases

After implementation, test the following scenarios:

1. **Directory with only `index.html`** (no `page.html`)
   - `/newdir` → serves `pages/newdir/index.html` (NEW: will work)
   - `/newdir/` → serves `pages/newdir/index.html` (NEW: will work after normalization)

2. **Directory with both `page.html` and `index.html`**
   - `/existing` → serves `pages/existing/page.html` (unchanged: takes precedence)

3. **Directory with neither file**
   - `/empty` → 404 (unchanged)

4. **Nested structures**
   - `/a/b/c` with only `pages/a/b/c/index.html` → serves the index (NEW)

5. **Root directory**
   - `/` → still serves `pages/index.html` (unchanged)

---

## Code Snippets

### Complete Refactored `build_routes()` Function

```rust
fn build_routes(route: String, directory: &Path) -> HashMap<String, PathBuf> {
    let mut routes: HashMap<String, PathBuf> = HashMap::new();

    let entries: Vec<_> = fs::read_dir(directory)
        .unwrap()
        .map(|e| e.unwrap())
        .collect();

    // Phase 1: Register page.html files (takes precedence)
    for entry in &entries {
        let path = entry.path();
        let name = path.file_name().unwrap().to_str().unwrap();
        if path.is_file() && name == "page.html" {
            let route_key = if route == "" {
                String::from("/")
            } else {
                route.clone()
            };
            routes.insert(route_key, path);
        }
    }

    // Phase 2: Register index.html files (only if not already registered)
    for entry in &entries {
        let path = entry.path();
        let name = path.file_name().unwrap().to_str().unwrap();
        if path.is_file() && name == "index.html" {
            let route_key = if route == "" {
                String::from("/")
            } else {
                route.clone()
            };
            if !routes.contains_key(&route_key) {
                routes.insert(route_key, path);
            }
        }
    }

    // Phase 3: Register other files and recurse into subdirectories
    for entry in &entries {
        let path = entry.path();
        let name = path.file_name().unwrap().to_str().unwrap();
        if path.is_dir() {
            routes.extend(
                build_routes(format!("{route}/{name}"), &path)
            );
        } else if path.is_file() {
            match path.extension().unwrap().to_str().unwrap() {
                "html" | "css" | "js" => {
                    if name != "page.html" && name != "index.html" && name != "not_found.html" {
                        routes.insert(format!("{route}/{name}"), path);
                    }
                }
                _ => {}
            }
        }
    }

    routes
}
```

### Handle Connection (No Changes Required)

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

    // Routes now includes index.html files, so this works without changes
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

---

## Testing Strategy

### Unit Test: Route Building with `index.html`

Add to `src/main.rs` or create a test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    #[test]
    fn test_build_routes_with_index_html() {
        // Create temporary test directory structure
        let temp_dir = std::env::temp_dir().join("rcomm_test_index");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        // Create: temp_dir/products/index.html
        fs::create_dir(&temp_dir.join("products")).unwrap();
        fs::write(
            &temp_dir.join("products/index.html"),
            "<html>Products</html>"
        ).unwrap();

        // Build routes
        let routes = build_routes(String::from(""), &temp_dir);

        // Assert /products exists as a route
        assert!(routes.contains_key("/products"));

        // Cleanup
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_page_html_takes_precedence() {
        let temp_dir = std::env::temp_dir().join("rcomm_test_precedence");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        // Create: temp_dir/docs/page.html and temp_dir/docs/index.html
        fs::create_dir(&temp_dir.join("docs")).unwrap();
        fs::write(
            &temp_dir.join("docs/page.html"),
            "<html>Page</html>"
        ).unwrap();
        fs::write(
            &temp_dir.join("docs/index.html"),
            "<html>Index</html>"
        ).unwrap();

        // Build routes
        let routes = build_routes(String::from(""), &temp_dir);

        // Assert /docs route exists and points to page.html
        assert!(routes.contains_key("/docs"));
        let path = routes.get("/docs").unwrap();
        assert!(path.to_str().unwrap().contains("page.html"));

        // Cleanup
        let _ = fs::remove_dir_all(&temp_dir);
    }
}
```

### Integration Tests (in `src/bin/integration_test.rs`)

Add new test functions before the `main()` function:

```rust
fn test_directory_index_fallback(addr: &str) -> Result<(), String> {
    // Test: /products with only index.html should serve it
    let resp = send_request(addr, "GET", "/products")?;
    assert_eq_or_err(&resp.status_code, &200, "status for /products")?;
    assert_contains_or_err(&resp.body, "Products", "body should contain expected content")?;
    Ok(())
}

fn test_directory_index_with_trailing_slash(addr: &str) -> Result<(), String> {
    // Test: /products/ with only index.html should serve it
    let resp = send_request(addr, "GET", "/products/")?;
    assert_eq_or_err(&resp.status_code, &200, "status for /products/")?;
    assert_contains_or_err(&resp.body, "Products", "body should contain expected content")?;
    Ok(())
}

fn test_page_html_precedence(addr: &str) -> Result<(), String> {
    // Test: /docs with both page.html and index.html should serve page.html
    let resp = send_request(addr, "GET", "/docs")?;
    assert_eq_or_err(&resp.status_code, &200, "status for /docs")?;
    assert_contains_or_err(&resp.body, "Page Content", "body should contain page.html content")?;
    Ok(())
}

fn test_directory_without_index_404(addr: &str) -> Result<(), String> {
    // Test: /empty directory with no index or page should 404
    let resp = send_request(addr, "GET", "/empty")?;
    assert_eq_or_err(&resp.status_code, &404, "status for empty directory")?;
    Ok(())
}
```

Then add these to the test harness in `main()`:

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
    // NEW TESTS
    run_test("directory_index_fallback", || test_directory_index_fallback(&addr)),
    run_test("directory_index_with_trailing_slash", || test_directory_index_with_trailing_slash(&addr)),
    run_test("page_html_precedence", || test_page_html_precedence(&addr)),
    run_test("directory_without_index_404", || test_directory_without_index_404(&addr)),
];
```

### Test Pages Setup

For integration tests to pass, add new test pages to `pages/`:

**`pages/products/index.html`:**
```html
<!DOCTYPE html>
<html>
<head>
    <title>Products</title>
</head>
<body>
    <h1>Products</h1>
    <p>This is served via index.html fallback.</p>
</body>
</html>
```

**`pages/docs/page.html`:**
```html
<!DOCTYPE html>
<html>
<head>
    <title>Docs</title>
</head>
<body>
    <h1>Documentation</h1>
    <p>Page Content - served via page.html (takes precedence).</p>
</body>
</html>
```

**`pages/docs/index.html`:**
```html
<!DOCTYPE html>
<html>
<head>
    <title>Docs Index</title>
</head>
<body>
    <h1>Docs Index</h1>
    <p>This should NOT be served because page.html exists.</p>
</body>
</html>
```

**`pages/empty/`** (directory with no index.html or page.html)
- Just create the directory, leave it empty

---

## Edge Cases

### 1. Trailing Slashes
- **Current behavior:** `clean_route()` normalizes `/products/` to `/products`
- **Impact:** Already handled—no additional logic needed
- **Test:** `test_directory_index_with_trailing_slash()`

### 2. Multiple Path Segments
- **Scenario:** `/a/b/c/index.html` should be served for `/a/b/c`
- **Implementation:** Recursive `build_routes()` naturally handles this
- **Test:** Create nested structure and verify

### 3. `page.html` Precedence
- **Scenario:** If both `page.html` and `index.html` exist, `page.html` wins
- **Implementation:** Two-phase registration in `build_routes()`
- **Test:** `test_page_html_precedence()`

### 4. Root Directory Remains Unchanged
- **Scenario:** `/` always serves `pages/index.html`
- **Implementation:** Special casing in `build_routes()` (line: `if route == ""`)
- **Test:** Existing `test_root_route()` should still pass

### 5. Non-Existent Directory Paths
- **Scenario:** `/nonexistent` or `/nonexistent/` should 404
- **Implementation:** No route registered → default 404 logic applies
- **Test:** Existing tests cover this

### 6. Case Sensitivity
- **Current:** Filenames are case-sensitive (Unix default)
- **Impact:** `INDEX.html` and `Page.html` won't be recognized
- **Recommendation:** Document this limitation or enforce lowercase in the CLAUDE.md

### 7. Deeply Nested Directories
- **Scenario:** `/a/b/c/d/e/index.html` should be served for `/a/b/c/d/e`
- **Implementation:** Recursive nature of `build_routes()` handles this
- **Test:** Create deep structure and verify

### 8. Symbolic Links
- **Current:** `fs::read_dir()` follows symlinks by default
- **Behavior:** Will treat symlinked directories as regular directories
- **Risk:** Potential for directory traversal if symlinks aren't managed carefully
- **Recommendation:** Consider validating canonicalized paths; document current behavior

---

## Implementation Checklist

- [ ] Read and understand current `build_routes()` implementation
- [ ] Refactor `build_routes()` to register `index.html` files as routes
- [ ] Implement two-phase registration (page.html first, then index.html)
- [ ] Verify `handle_connection()` requires no changes
- [ ] Create test pages in `pages/` directory:
  - [ ] `pages/products/index.html`
  - [ ] `pages/docs/page.html`
  - [ ] `pages/docs/index.html`
  - [ ] `pages/empty/` (empty directory)
- [ ] Add unit tests to `src/main.rs`
- [ ] Add integration tests to `src/bin/integration_test.rs`
- [ ] Run all tests: `cargo test && cargo run --bin integration_test`
- [ ] Verify existing tests still pass
- [ ] Manual testing with browser/curl:
  - [ ] GET `/products` returns 200 with Products content
  - [ ] GET `/products/` returns 200 with Products content
  - [ ] GET `/docs` returns 200 with Page content (not Index)
  - [ ] GET `/empty` returns 404
  - [ ] GET `/` returns 200 with root index.html
- [ ] Document behavior in CLAUDE.md (update routing section)

---

## Performance Considerations

### Directory Reading Overhead
- **Current:** `build_routes()` reads directory once during startup
- **After:** Reads directory twice per level (Phase 1 for page.html, Phase 2 for index.html)
- **Impact:** Negligible (still O(n) where n = number of files), only during startup
- **Optimization:** Could collect entries once and process twice (implemented in code snippet)

### Route Lookup
- **Current:** HashMap lookup is O(1)
- **After:** Still O(1)—no change to request handling performance

### Memory
- **Current:** Routes map contains one entry per routable file
- **After:** Routes map contains one entry per routable file (index.html files are now routable)
- **Impact:** Minimal—fewer wasted `index.html` files that weren't previously routed

---

## Rollback Plan

If the feature introduces issues:

1. **Revert `build_routes()` to original logic** — Comment out Phase 1 & 2, restore original code
2. **Revert integration tests** — Remove new test functions
3. **Remove test pages** — Delete `pages/products/`, `pages/docs/`, `pages/empty/`
4. **Verify with existing tests** — `cargo test && cargo run --bin integration_test`

The change is isolated to `build_routes()` function, making rollback straightforward.

---

## Future Enhancements

1. **Config-Driven Fallback:** Allow users to disable index.html fallback via config file
2. **Directory Listing:** Generate HTML directory listings if no index.html exists
3. **Custom Index Names:** Allow configuring alternative index filenames (e.g., `default.html`)
4. **Cache Route Metadata:** Store route type (page.html vs index.html) for debug/analytics
5. **Conditional Redirect:** Option to redirect `/some_dir` to `/some_dir/` or vice versa
