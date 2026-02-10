# Content-Type Header Implementation Plan

## 1. Overview of the Feature

The HTTP `Content-Type` response header is essential for HTTP protocol compliance. It informs clients of the media type of the response body, enabling proper parsing and rendering by browsers and other HTTP clients.

Currently, rcomm serves static HTML, CSS, and JavaScript files without setting the `Content-Type` header, which can cause browsers to misinterpret content and fail to render stylesheets or execute scripts properly.

**Goal**: Automatically set the `Content-Type` response header based on the file extension of the requested resource.

**Supported MIME Types**:
- `text/html` — for `.html` files
- `text/css` — for `.css` files
- `application/javascript` — for `.js` files

**Impact**:
- HTTP protocol compliance: Browsers will correctly handle CSS styling and JavaScript execution
- Client-side functionality: Stylesheets will apply correctly, scripts will execute
- Standards adherence: Aligns with RFC 7231 (HTTP/1.1 Semantics and Content)

---

## 2. Files to be Modified or Created

### Modified Files

1. **`/home/jwall/personal/rusty/rcomm/src/main.rs`**
   - Modify `handle_connection()` to determine MIME type from file extension
   - Set `Content-Type` header on the response before sending

### New Files

2. **`/home/jwall/personal/rusty/rcomm/src/models/mime_types.rs`** (optional, recommended)
   - Create a utility module for MIME type mapping
   - Provides a function `get_mime_type(extension: &str) -> &'static str`
   - Centralizes MIME type definitions for maintainability

---

## 3. Step-by-Step Implementation Details

### Step 1: Create the MIME Types Utility Module

**File**: `/home/jwall/personal/rusty/rcomm/src/models/mime_types.rs`

Create a new module with a public function that maps file extensions to MIME types:

```rust
/// Returns the MIME type for a given file extension.
///
/// # Arguments
/// * `extension` - The file extension (without the leading dot)
///
/// # Returns
/// * A string slice with the MIME type, defaults to "application/octet-stream" for unknown types
pub fn get_mime_type(extension: &str) -> &'static str {
    match extension.to_lowercase().as_str() {
        "html" => "text/html",
        "css" => "text/css",
        "js" => "application/javascript",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_extension_returns_text_html() {
        assert_eq!(get_mime_type("html"), "text/html");
    }

    #[test]
    fn css_extension_returns_text_css() {
        assert_eq!(get_mime_type("css"), "text/css");
    }

    #[test]
    fn js_extension_returns_application_javascript() {
        assert_eq!(get_mime_type("js"), "application/javascript");
    }

    #[test]
    fn unknown_extension_returns_octet_stream() {
        assert_eq!(get_mime_type("unknown"), "application/octet-stream");
    }

    #[test]
    fn case_insensitive() {
        assert_eq!(get_mime_type("HTML"), "text/html");
        assert_eq!(get_mime_type("Css"), "text/css");
        assert_eq!(get_mime_type("JS"), "application/javascript");
    }
}
```

### Step 2: Export the MIME Types Module

**File**: `/home/jwall/personal/rusty/rcomm/src/models.rs`

Add the new module to the barrel export:

```rust
pub mod http_methods;
pub mod http_request;
pub mod http_response;
pub mod http_status_codes;
pub mod mime_types;  // Add this line
```

### Step 3: Modify the Main Server Handler

**File**: `/home/jwall/personal/rusty/rcomm/src/main.rs`

Update the `handle_connection()` function to:
1. Extract the file extension from the filename
2. Determine the MIME type using `get_mime_type()`
3. Add the `Content-Type` header to the response

**Current Code** (lines 46-75):
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

**Updated Code**:
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

    // Determine and set Content-Type header based on file extension
    let extension = Path::new(filename)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("html");
    let mime_type = rcomm::models::mime_types::get_mime_type(extension);
    response.add_header("Content-Type".to_string(), mime_type.to_string());

    let contents = fs::read_to_string(filename).unwrap();
    response.add_body(contents.into());

    println!("Response: {response}");
    stream.write_all(&response.as_bytes()).unwrap();
}
```

**Key Changes**:
- Use `Path::new(filename)` to extract the extension
- Call `get_mime_type()` with the extension
- Add the header using `response.add_header()` before setting the body
- Uses method chaining pattern already established in the codebase

---

## 4. Code Snippets and Pseudocode

### MIME Type Lookup Function

```
FUNCTION get_mime_type(extension: string) -> string
    SWITCH extension.lowercase() DO
        CASE "html":
            RETURN "text/html"
        CASE "css":
            RETURN "text/css"
        CASE "js":
            RETURN "application/javascript"
        DEFAULT:
            RETURN "application/octet-stream"
    END SWITCH
END FUNCTION
```

### Integration in Request Handler

```
FUNCTION handle_connection(stream, routes)
    // ... existing request parsing and routing logic ...

    LET filename = get_file_path(clean_target, routes)
    LET extension = extract_extension(filename)
    LET mime_type = get_mime_type(extension)

    response.add_header("Content-Type", mime_type)
    response.add_body(read_file(filename))

    send_response(stream, response)
END FUNCTION
```

---

## 5. Testing Strategy

### Unit Tests (in `mime_types.rs`)

The `mime_types.rs` module includes unit tests that verify:
- Correct MIME type for `.html` files → `text/html`
- Correct MIME type for `.css` files → `text/css`
- Correct MIME type for `.js` files → `application/javascript`
- Default MIME type for unknown extensions → `application/octet-stream`
- Case-insensitive extension matching

**Run unit tests**:
```bash
cargo test mime_types
```

### Integration Tests (in `src/bin/integration_test.rs`)

Add new test cases to verify the header is present in actual HTTP responses:

```rust
fn test_html_content_type(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    let content_type = resp
        .headers
        .get("content-type")
        .ok_or("missing Content-Type header")?;
    assert_eq_or_err(content_type, &"text/html".to_string(), "content-type")?;
    Ok(())
}

fn test_css_content_type(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/index.css")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    let content_type = resp
        .headers
        .get("content-type")
        .ok_or("missing Content-Type header")?;
    assert_eq_or_err(content_type, &"text/css".to_string(), "content-type")?;
    Ok(())
}

fn test_javascript_content_type(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/app.js")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    let content_type = resp
        .headers
        .get("content-type")
        .ok_or("missing Content-Type header")?;
    assert_eq_or_err(content_type, &"application/javascript".to_string(), "content-type")?;
    Ok(())
}

fn test_404_has_content_type(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/nonexistent")?;
    assert_eq_or_err(&resp.status_code, &404, "status")?;
    let content_type = resp
        .headers
        .get("content-type")
        .ok_or("missing Content-Type header")?;
    assert_eq_or_err(content_type, &"text/html".to_string(), "404 content-type")?;
    Ok(())
}
```

Add these tests to the `main()` function's test list:
```rust
let results = vec![
    // ... existing tests ...
    run_test("html_content_type", || test_html_content_type(&addr)),
    run_test("css_content_type", || test_css_content_type(&addr)),
    run_test("javascript_content_type", || test_javascript_content_type(&addr)),
    run_test("404_has_content_type", || test_404_has_content_type(&addr)),
];
```

**Run integration tests**:
```bash
cargo run --bin integration_test
```

### Manual Browser Testing

1. Start the server: `cargo run`
2. Open browser to `http://127.0.0.1:7878/`
3. Open browser Developer Tools (F12)
4. Check the Network tab for response headers
5. Verify:
   - CSS files have `Content-Type: text/css` header
   - HTML files have `Content-Type: text/html` header
   - JavaScript files have `Content-Type: application/javascript` header
   - CSS stylesheets apply correctly
   - JavaScript code executes

---

## 6. Edge Cases to Consider

### Case 1: Files Without Extension
**Scenario**: A file named `robots` with no extension is served
**Current Behavior**: `Path::new("robots").extension()` returns `None`
**Handling**: Default to `html` extension assumption (or `application/octet-stream`)
**Code**:
```rust
let extension = Path::new(filename)
    .extension()
    .and_then(|ext| ext.to_str())
    .unwrap_or("html");  // Defaults to "html" if no extension
```

### Case 2: Mixed Case Extensions
**Scenario**: User requests a file with uppercase or mixed case: `Style.CSS`, `Script.Js`
**Current Behavior**: Extensions should be normalized to lowercase in the MIME type function
**Handling**: The `get_mime_type()` function uses `.to_lowercase()` for comparison
**Code**:
```rust
pub fn get_mime_type(extension: &str) -> &'static str {
    match extension.to_lowercase().as_str() {
        // ... case-insensitive matching ...
    }
}
```

### Case 3: 404 Responses with HTML Body
**Scenario**: A 404 error is served with `pages/not_found.html`
**Current Behavior**: The filename is `pages/not_found.html`, extension is `.html`
**Expected Result**: Should have `Content-Type: text/html` header
**Verification**: Will be tested by `test_404_has_content_type()` integration test

### Case 4: Multiple Dots in Filename
**Scenario**: A file named `bundle.min.js` is requested
**Current Behavior**: `Path::new("bundle.min.js").extension()` returns `Some("js")`
**Expected Result**: Correctly identified as JavaScript
**Rust Path Behavior**: `extension()` returns the final extension only, which is correct

### Case 5: Hidden Files
**Scenario**: Files like `.htaccess` or `.env` (though not typically served by rcomm)
**Current Behavior**: These files wouldn't be routed by the `build_routes()` function (only `.html`, `.css`, `.js` are routed)
**Impact**: Non-issue for current architecture

### Case 6: Symbolic Links or Malformed Paths
**Current Code**: Already uses `.unwrap()` extensively, no graceful handling of edge cases
**Impact**: Consistent with existing codebase pattern
**Future Improvement**: Could implement proper error handling (noted as a known issue in CLAUDE.md)

### Case 7: Default MIME Type
**Scenario**: A file type not in the supported list (e.g., `.txt`, `.json`, `.xml`)
**Current Behavior**: Returns `"application/octet-stream"`
**Reasoning**: Conservative default that tells clients to treat as binary data
**Future Extensibility**: Easy to add more MIME types to the `get_mime_type()` function

---

## 7. Implementation Checklist

- [ ] Create `/home/jwall/personal/rusty/rcomm/src/models/mime_types.rs` with MIME type function and unit tests
- [ ] Update `/home/jwall/personal/rusty/rcomm/src/models.rs` to export `mime_types` module
- [ ] Modify `/home/jwall/personal/rusty/rcomm/src/main.rs`:
  - [ ] Add import for `Path` from `std::path` (if not already imported)
  - [ ] Add import for `rcomm::models::mime_types`
  - [ ] Update `handle_connection()` to extract extension and set `Content-Type` header
- [ ] Add integration tests to `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`:
  - [ ] `test_html_content_type()`
  - [ ] `test_css_content_type()`
  - [ ] `test_javascript_content_type()`
  - [ ] `test_404_has_content_type()`
- [ ] Run unit tests: `cargo test mime_types`
- [ ] Run integration tests: `cargo run --bin integration_test`
- [ ] Manual browser testing to verify CSS and JavaScript work correctly

---

## 8. Complexity and Risk Analysis

**Complexity**: 2/10
- Simple string matching function
- Single location where header is added
- No changes to core architecture or HTTP models

**Risk**: Very Low
- Pure additive feature (doesn't break existing functionality)
- MIME type determination is deterministic and testable
- Standard HTTP header with well-defined semantics
- No performance impact (minimal string comparison per request)

**Dependencies**: None
- Uses only standard library (`std::path::Path`)
- No external crates required
- Aligns with project's no-external-dependencies constraint

---

## 9. Future Enhancements

1. **Configurable MIME Types**: Allow users to define custom MIME types via configuration file
2. **Content Encoding**: Add `Content-Encoding` header for gzipped responses
3. **Character Sets**: Add charset parameter to `Content-Type` (e.g., `text/html; charset=utf-8`)
4. **More File Types**: Extend support to `.json`, `.svg`, `.txt`, `.xml`, etc.
5. **MIME Type Configuration File**: External mapping for extensibility without code changes
