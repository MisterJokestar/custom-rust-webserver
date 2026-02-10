# Cache-Control Response Header Implementation Plan

## Feature Overview

Implement support for the `Cache-Control` response header with a configurable `max-age` directive for static assets. This feature enables browsers and intermediate caches to understand how long static resources (HTML, CSS, JS) can be cached, reducing bandwidth consumption and improving page load times.

**Complexity:** 2/10
**Necessity:** 6/10

---

## Problem Statement

Currently, rcomm serves all static assets without any caching directives. Browsers must re-request all resources on every page load, even if they haven't changed. By implementing `Cache-Control: max-age=<seconds>`, we:

- Allow browsers to cache static assets for a configurable duration
- Reduce bandwidth usage
- Improve perceived performance
- Follow HTTP/1.1 best practices (RFC 7234)

---

## Architecture & Design

### Caching Strategy

```
Static Assets by Type:
  ├─ .html files     → max-age = 3600 (1 hour)  [short, pages may update]
  ├─ .css files      → max-age = 86400 (1 day)  [longer, styles stable]
  └─ .js files       → max-age = 86400 (1 day)  [longer, scripts stable]
```

### Configuration Approach

Support two levels of configuration:

1. **Environment variables** (simple, per-deployment):
   - `RCOMM_CACHE_MAX_AGE_HTML` (default: `3600`)
   - `RCOMM_CACHE_MAX_AGE_CSS` (default: `86400`)
   - `RCOMM_CACHE_MAX_AGE_JS` (default: `86400`)

2. **Code defaults** (fallback):
   - Hardcoded sensible defaults

This keeps complexity low (2/10) while providing flexibility.

### Header Format

According to RFC 7234, the `Cache-Control` header follows this format:

```
Cache-Control: max-age=3600, public
```

For static assets, we use:
- `max-age=<seconds>` — Cache duration
- `public` — Allow shared caches (CDNs, proxies) to cache

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Changes:**
- Add `CacheConfig` struct to hold cache durations per file type
- Add functions to read cache config from environment variables
- Determine file extension from the served file
- Add `Cache-Control` header based on file type before sending response

**New Structs:**
```rust
struct CacheConfig {
    html_max_age: u32,
    css_max_age: u32,
    js_max_age: u32,
}
```

**New Functions:**
- `load_cache_config() -> CacheConfig` — Load from env vars with defaults
- `get_cache_max_age(filename: &str, config: &CacheConfig) -> Option<u32>` — Determine max-age by file extension
- `add_cache_control_header(response: &mut HttpResponse, filename: &str, config: &CacheConfig)` — Apply header to response

---

### 2. `/home/jwall/personal/rusty/rcomm/src/models/http_response.rs`

**Changes:**
- Add convenience method for `Cache-Control` header (optional but clean)
- No structural changes needed; existing `add_header()` method is sufficient

**Optional Enhancement:**
```rust
pub fn add_cache_control(&mut self, max_age: u32) -> &mut HttpResponse {
    self.add_header(
        "Cache-Control".to_string(),
        format!("max-age={}, public", max_age)
    );
    self
}
```

---

## Step-by-Step Implementation

### Step 1: Create `CacheConfig` Struct & Loader

In `/home/jwall/personal/rusty/rcomm/src/main.rs`, at the top after imports:

```rust
struct CacheConfig {
    html_max_age: u32,
    css_max_age: u32,
    js_max_age: u32,
}

fn load_cache_config() -> CacheConfig {
    let html_max_age = std::env::var("RCOMM_CACHE_MAX_AGE_HTML")
        .unwrap_or_else(|_| String::from("3600"))
        .parse::<u32>()
        .unwrap_or(3600);

    let css_max_age = std::env::var("RCOMM_CACHE_MAX_AGE_CSS")
        .unwrap_or_else(|_| String::from("86400"))
        .parse::<u32>()
        .unwrap_or(86400);

    let js_max_age = std::env::var("RCOMM_CACHE_MAX_AGE_JS")
        .unwrap_or_else(|_| String::from("86400"))
        .parse::<u32>()
        .unwrap_or(86400);

    CacheConfig {
        html_max_age,
        css_max_age,
        js_max_age,
    }
}
```

### Step 2: Create Helper Function for Cache Duration

In `/home/jwall/personal/rusty/rcomm/src/main.rs`:

```rust
fn get_cache_max_age(filename: &str, config: &CacheConfig) -> Option<u32> {
    if filename.ends_with(".html") {
        Some(config.html_max_age)
    } else if filename.ends_with(".css") {
        Some(config.css_max_age)
    } else if filename.ends_with(".js") {
        Some(config.js_max_age)
    } else {
        None
    }
}
```

### Step 3: Create Helper Function to Add Cache Header

In `/home/jwall/personal/rusty/rcomm/src/main.rs`:

```rust
fn add_cache_control_header(
    response: &mut HttpResponse,
    filename: &str,
    config: &CacheConfig,
) {
    if let Some(max_age) = get_cache_max_age(filename, config) {
        let cache_control = format!("max-age={}, public", max_age);
        response.add_header("Cache-Control".to_string(), cache_control);
    }
}
```

### Step 4: Load Config in `main()` & Pass to Handler

Modify the `main()` function:

```rust
fn main() {
    let port = get_port();
    let address = get_address();
    let full_address = format!("{address}:{port}");
    let listener = TcpListener::bind(&full_address).unwrap();

    let pool = ThreadPool::new(4);

    let path = Path::new("./pages");
    let routes = build_routes(String::from(""), path);
    let cache_config = load_cache_config();  // ADD THIS LINE

    println!("Routes:\n{routes:#?}\n\n");
    println!("Listening on {full_address}");

    for stream in listener.incoming() {
        let routes_clone = routes.clone();
        let cache_config_clone = cache_config.clone();  // ADD THIS LINE
        let stream = stream.unwrap();

        pool.execute(move || {
            handle_connection(stream, routes_clone, cache_config_clone);  // MODIFY
        });
    }
}
```

This requires `CacheConfig` to implement `Clone`:

```rust
#[derive(Clone)]
struct CacheConfig {
    html_max_age: u32,
    css_max_age: u32,
    js_max_age: u32,
}
```

### Step 5: Update `handle_connection()` Signature

Modify the function signature and implementation:

```rust
fn handle_connection(
    mut stream: TcpStream,
    routes: HashMap<String, PathBuf>,
    cache_config: CacheConfig,  // ADD THIS PARAMETER
) {
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

    // ADD CACHE CONTROL HEADER
    add_cache_control_header(&mut response, filename, &cache_config);

    println!("Response: {response}");
    stream.write_all(&response.as_bytes()).unwrap();
}
```

### Step 6 (Optional): Add Convenience Method to HttpResponse

In `/home/jwall/personal/rusty/rcomm/src/models/http_response.rs`, add to the `impl HttpResponse` block:

```rust
pub fn add_cache_control(&mut self, max_age: u32) -> &mut HttpResponse {
    self.add_header(
        "Cache-Control".to_string(),
        format!("max-age={}, public", max_age)
    );
    self
}
```

Then in `handle_connection()`, you could use:

```rust
response.add_cache_control(get_cache_max_age(filename, &cache_config).unwrap_or(0));
```

However, the explicit `add_header()` call is fine and more transparent.

---

## Testing Strategy

### Unit Tests

**Location:** `/home/jwall/personal/rusty/rcomm/src/main.rs` (or separate tests module)

#### Test 1: Cache Config Loading with Defaults
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_cache_config_uses_defaults_when_unset() {
        // Ensure env vars are not set
        std::env::remove_var("RCOMM_CACHE_MAX_AGE_HTML");
        std::env::remove_var("RCOMM_CACHE_MAX_AGE_CSS");
        std::env::remove_var("RCOMM_CACHE_MAX_AGE_JS");

        let config = load_cache_config();
        assert_eq!(config.html_max_age, 3600);
        assert_eq!(config.css_max_age, 86400);
        assert_eq!(config.js_max_age, 86400);
    }

    #[test]
    fn load_cache_config_respects_env_vars() {
        std::env::set_var("RCOMM_CACHE_MAX_AGE_HTML", "1800");
        std::env::set_var("RCOMM_CACHE_MAX_AGE_CSS", "43200");
        std::env::set_var("RCOMM_CACHE_MAX_AGE_JS", "43200");

        let config = load_cache_config();
        assert_eq!(config.html_max_age, 1800);
        assert_eq!(config.css_max_age, 43200);
        assert_eq!(config.js_max_age, 43200);

        // Cleanup
        std::env::remove_var("RCOMM_CACHE_MAX_AGE_HTML");
        std::env::remove_var("RCOMM_CACHE_MAX_AGE_CSS");
        std::env::remove_var("RCOMM_CACHE_MAX_AGE_JS");
    }

    #[test]
    fn get_cache_max_age_returns_correct_values() {
        let config = CacheConfig {
            html_max_age: 3600,
            css_max_age: 86400,
            js_max_age: 86400,
        };

        assert_eq!(get_cache_max_age("index.html", &config), Some(3600));
        assert_eq!(get_cache_max_age("style.css", &config), Some(86400));
        assert_eq!(get_cache_max_age("app.js", &config), Some(86400));
        assert_eq!(get_cache_max_age("favicon.ico", &config), None);
    }

    #[test]
    fn get_cache_max_age_handles_nested_paths() {
        let config = CacheConfig {
            html_max_age: 3600,
            css_max_age: 86400,
            js_max_age: 86400,
        };

        assert_eq!(get_cache_max_age("pages/about/index.html", &config), Some(3600));
        assert_eq!(get_cache_max_age("assets/css/main.css", &config), Some(86400));
        assert_eq!(get_cache_max_age("js/vendor/lib.js", &config), Some(86400));
    }
}
```

### Integration Tests

**Location:** `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`

Add new test cases to the integration test suite:

```rust
// In the test_suite() function, add these test results:

TestResult {
    name: "HTML files include Cache-Control header with max-age",
    test: Box::new(|| {
        let mut stream = establish_connection()?;
        send_http_request(&mut stream, "GET / HTTP/1.1")?;
        let response = read_response(&mut stream)?;

        if response.contains("Cache-Control: max-age=3600, public") {
            Ok(())
        } else {
            Err("Cache-Control header not found or incorrect".to_string())
        }
    }),
},

TestResult {
    name: "CSS files include Cache-Control header with max-age",
    test: Box::new(|| {
        let mut stream = establish_connection()?;
        send_http_request(&mut stream, "GET /style.css HTTP/1.1")?;
        let response = read_response(&mut stream)?;

        if response.contains("Cache-Control: max-age=86400, public") {
            Ok(())
        } else {
            Err("Cache-Control header not found for CSS".to_string())
        }
    }),
},

TestResult {
    name: "JS files include Cache-Control header with max-age",
    test: Box::new(|| {
        let mut stream = establish_connection()?;
        send_http_request(&mut stream, "GET /app.js HTTP/1.1")?;
        let response = read_response(&mut stream)?;

        if response.contains("Cache-Control: max-age=86400, public") {
            Ok(())
        } else {
            Err("Cache-Control header not found for JS".to_string())
        }
    }),
},

TestResult {
    name: "404 responses do not include Cache-Control",
    test: Box::new(|| {
        let mut stream = establish_connection()?;
        send_http_request(&mut stream, "GET /nonexistent HTTP/1.1")?;
        let response = read_response(&mut stream)?;

        if !response.contains("Cache-Control:") {
            Ok(())
        } else {
            Err("Cache-Control header should not be in 404 responses".to_string())
        }
    }),
},
```

### Manual Testing

1. **Start the server:**
   ```bash
   cargo run
   ```

2. **Request a static asset and inspect headers:**
   ```bash
   curl -i http://127.0.0.1:7878/
   ```

   Expected output (contains):
   ```
   HTTP/1.1 200 OK
   ...
   cache-control: max-age=3600, public
   ...
   ```

3. **Test with custom max-age values:**
   ```bash
   RCOMM_CACHE_MAX_AGE_HTML=1800 cargo run
   curl -i http://127.0.0.1:7878/
   ```

   Expected: `cache-control: max-age=1800, public`

4. **Test CSS file:**
   ```bash
   curl -i http://127.0.0.1:7878/style.css
   ```

   Expected: `cache-control: max-age=86400, public`

---

## Edge Cases & Considerations

### 1. 404 Responses (Not Found)

**Current behavior:** Serves `pages/not_found.html` as a 404 response.

**Decision:** Do NOT add Cache-Control header to 404s. Users should retry frequently since the resource might be added later.

**Implementation:** Check response status code before adding header:

```rust
// Only add cache header for successful responses
if response.status_code == 200 {
    add_cache_control_header(&mut response, filename, &cache_config);
}
```

Or more flexibly:

```rust
// Add cache header only for cacheable status codes
let cacheable_status_codes = vec![200, 206];  // 206 for partial content (future)
if cacheable_status_codes.contains(&response.status_code) {
    add_cache_control_header(&mut response, filename, &cache_config);
}
```

### 2. Malformed Environment Variables

**Edge case:** User sets `RCOMM_CACHE_MAX_AGE_HTML=invalid_number`

**Current behavior:** Falls back to default (via `.unwrap_or()`)

**Improvement:** Log warning message:

```rust
let html_max_age = match std::env::var("RCOMM_CACHE_MAX_AGE_HTML") {
    Ok(val) => {
        match val.parse::<u32>() {
            Ok(n) => n,
            Err(_) => {
                eprintln!("Warning: RCOMM_CACHE_MAX_AGE_HTML='{}' is invalid, using default 3600", val);
                3600
            }
        }
    }
    Err(_) => 3600,
};
```

### 3. Non-Static Files (Future Concern)

If rcomm ever serves non-static resources (API responses, dynamically generated pages), they should NOT have Cache-Control headers. Current implementation is safe because:

1. Only files in `pages/` directory are routed
2. Only `.html`, `.css`, `.js` extensions are handled
3. Extension check in `get_cache_max_age()` prevents other file types

### 4. File Not Found During Read

**Edge case:** File exists in routes map but is deleted between request handling and file read.

**Current behavior:** `.unwrap()` on `fs::read_to_string()` panics.

**Note:** This is existing behavior unrelated to Cache-Control. The header would never be added because `fs::read_to_string()` fails first.

### 5. Case Sensitivity of Headers

**Current behavior:** Headers stored lowercase in response (`headers.insert(title.to_lowercase(), value)`)

**Impact:** Cache-Control header sent as `cache-control: max-age=3600, public`

**Correctness:** Per HTTP spec, header names are case-insensitive. Browsers accept both `Cache-Control` and `cache-control`.

**Consistency:** Current approach is consistent with how response headers are already stored internally.

### 6. Header Duplication

**Edge case:** What if someone manually adds Cache-Control header, then we add it again?

**Current behavior:** HashMap insert would overwrite (last write wins)

**Risk:** Low, because we control the `handle_connection()` flow. No external code can add headers before us.

### 7. Zero or Negative Max-Age

**Edge case:** User sets `RCOMM_CACHE_MAX_AGE_HTML=0`

**Current behavior:** Accepted and sent as `Cache-Control: max-age=0, public`

**Semantics:** Valid per HTTP spec. `max-age=0` means "immediately stale," effectively no caching.

**Decision:** Allow this. It's a valid and useful configuration for debugging.

### 8. Very Large Max-Age Values

**Edge case:** User sets `RCOMM_CACHE_MAX_AGE_HTML=99999999`

**Current behavior:** Parsed and sent as-is

**Risk:** None. Cache duration is advisory; browsers may enforce their own limits.

**Decision:** Allow this. No validation needed.

### 9. Public vs. Private vs. No Directive

**Current approach:** Use `max-age=<seconds>, public`

**Rationale:**
- `public` — Allows shared caches (CDNs, proxies) to cache, good for static assets
- `private` — Only browser cache, not CDN (not used here)
- If omitted — Defaults to private in many implementations

**Security:** Static assets in `pages/` directory are intentionally served, so `public` is safe.

---

## Implementation Checklist

- [ ] Add `#[derive(Clone)]` to `CacheConfig` struct
- [ ] Implement `load_cache_config()` function
- [ ] Implement `get_cache_max_age()` function
- [ ] Implement `add_cache_control_header()` function
- [ ] Update `main()` to instantiate and clone `CacheConfig`
- [ ] Update `handle_connection()` signature to accept `cache_config` parameter
- [ ] Call `add_cache_control_header()` in `handle_connection()` for 200 responses
- [ ] Add unit tests for `load_cache_config()`
- [ ] Add unit tests for `get_cache_max_age()`
- [ ] Add integration tests for Cache-Control headers
- [ ] Manual test with curl to verify headers
- [ ] Manual test with environment variables
- [ ] Run full test suite: `cargo test`
- [ ] Run integration tests: `cargo run --bin integration_test`

---

## Deployment & Configuration

### Default Behavior (No Configuration)

```bash
cargo run
# Serves all .html with Cache-Control: max-age=3600, public
# Serves all .css with Cache-Control: max-age=86400, public
# Serves all .js with Cache-Control: max-age=86400, public
```

### Custom Configuration

#### Short-lived HTML, Long-lived Assets

```bash
RCOMM_CACHE_MAX_AGE_HTML=1800 \
RCOMM_CACHE_MAX_AGE_CSS=604800 \
RCOMM_CACHE_MAX_AGE_JS=604800 \
cargo run
```

#### Development Mode (No Caching)

```bash
RCOMM_CACHE_MAX_AGE_HTML=0 \
RCOMM_CACHE_MAX_AGE_CSS=0 \
RCOMM_CACHE_MAX_AGE_JS=0 \
cargo run
```

#### Production Mode (Maximum Caching)

```bash
RCOMM_CACHE_MAX_AGE_HTML=3600 \
RCOMM_CACHE_MAX_AGE_CSS=31536000 \
RCOMM_CACHE_MAX_AGE_JS=31536000 \
cargo run
```

(31536000 = 1 year, common for versioned assets)

---

## Future Enhancements

1. **Etag/Last-Modified Headers** — Implement conditional requests (`304 Not Modified`)
2. **Versioned Asset Names** — Support cache busting via filenames (e.g., `app-a1b2c3.js`)
3. **Configuration File** — Load cache settings from `.toml` or `.json` file instead of just env vars
4. **Immutable Directive** — Add `immutable` flag for versioned assets: `Cache-Control: max-age=31536000, public, immutable`
5. **Compression** — Add `Content-Encoding: gzip` alongside caching
6. **Cache Validation Headers** — Implement `Etag` and `Last-Modified` for 304 responses

---

## References

- [RFC 7234 - HTTP Caching](https://tools.ietf.org/html/rfc7234)
- [MDN - Cache-Control](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Cache-Control)
- [HTTP Caching Best Practices](https://developers.google.com/web/fundamentals/performance/optimizing-content-efficiency/http-caching)
