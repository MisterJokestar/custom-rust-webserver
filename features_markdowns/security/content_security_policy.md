# Content-Security-Policy Response Header Support

## Feature Overview

**Category:** Security
**Complexity:** 2/10
**Necessity:** 5/10

This feature adds comprehensive support for setting `Content-Security-Policy` (CSP) HTTP response headers in the rcomm web server. CSP is a security standard that allows servers to specify which content sources are allowed to be loaded by browsers, mitigating cross-site scripting (XSS), clickjacking, and other injection attacks.

### What is Content-Security-Policy?

The `Content-Security-Policy` header is an HTTP response header that instructs the browser to enforce a policy about which resources can be loaded. Common directives include:

- `default-src`: Default policy for all content types
- `script-src`: Which scripts can execute
- `style-src`: Which stylesheets can be loaded
- `img-src`: Which images can be loaded
- `font-src`: Which fonts can be loaded
- `connect-src`: Which origins can be connected to via XHR/Fetch/WebSocket
- `frame-src`: Which frames can be embedded
- `object-src`: Which plugins can be loaded

Example header value:
```
Content-Security-Policy: default-src 'self'; script-src 'self' https://cdn.example.com; style-src 'self' 'unsafe-inline'
```

### Rationale

1. **Security Hardening**: Reduces XSS and injection attack surface by restricting content sources
2. **Simple Integration**: Just requires adding headers to HTTP responses—no complex parsing or routing logic
3. **Low Maintenance**: CSP is a standard HTTP header; no custom protocols or dependencies needed
4. **Flexible Configuration**: Different routes can have different CSP policies

---

## Files to Modify

1. **`src/main.rs`**
   - Add a global or configurable CSP policy constant
   - Add CSP header to responses in `handle_connection()`
   - Optional: add environment variable support for CSP configuration

2. **`src/models/http_response.rs`**
   - No core changes needed (already supports arbitrary headers via `add_header()`)
   - Optional: add convenience method `add_csp_header()` for easier CSP application

3. **New Test File or Existing Tests**
   - Unit tests in `src/models/http_response.rs` for CSP header addition
   - Integration tests in `src/bin/integration_test.rs` to verify CSP headers in server responses

---

## Step-by-Step Implementation

### Phase 1: Core CSP Header Support in Main Server

#### Step 1.1: Add CSP Policy Configuration in `src/main.rs`

Define a default CSP policy at the top of the file. Start with a strict, permissive-by-default policy:

```rust
// Defines the default Content-Security-Policy header value
// This strict policy allows only same-origin resources
const DEFAULT_CSP_POLICY: &str = "default-src 'self'";
```

If environment variable support is desired, add a helper function:

```rust
fn get_csp_policy() -> String {
    std::env::var("RCOMM_CSP_POLICY").unwrap_or_else(|_| DEFAULT_CSP_POLICY.to_string())
}
```

#### Step 1.2: Apply CSP Header in Response Building

Modify the `handle_connection()` function to add the CSP header to all responses. Locate this section:

```rust
let (mut response, filename) = if routes.contains_key(&clean_target) {
    (HttpResponse::build(String::from("HTTP/1.1"), 200),
        routes.get(&clean_target).unwrap().to_str().unwrap())
} else {
    (HttpResponse::build(String::from("HTTP/1.1"), 404),
        "pages/not_found.html")
};
```

After building the response (and before reading the file), add:

```rust
let csp_policy = get_csp_policy();
response.add_header("Content-Security-Policy".to_string(), csp_policy);
```

This ensures all successful (200) and error (404) responses include the CSP header.

**Complete modified section:**

```rust
let (mut response, filename) = if routes.contains_key(&clean_target) {
    (HttpResponse::build(String::from("HTTP/1.1"), 200),
        routes.get(&clean_target).unwrap().to_str().unwrap())
} else {
    (HttpResponse::build(String::from("HTTP/1.1"), 404),
        "pages/not_found.html")
};

let csp_policy = get_csp_policy();
response.add_header("Content-Security-Policy".to_string(), csp_policy);

let contents = fs::read_to_string(filename).unwrap();
response.add_body(contents.into());
```

Also apply CSP to the 400 Bad Request error response in the error handling block:

```rust
Err(e) => {
    eprintln!("Bad request: {e}");
    let mut response = HttpResponse::build(String::from("HTTP/1.1"), 400);
    response.add_header("Content-Security-Policy".to_string(), get_csp_policy());
    let body = format!("Bad Request: {e}");
    response.add_body(body.into());
    let _ = stream.write_all(&response.as_bytes());
    return;
}
```

### Phase 2: Convenience Method in HTTP Response Model (Optional)

Add a helper method to `src/models/http_response.rs` to simplify CSP header addition:

```rust
pub fn add_csp_header(&mut self, policy: String) -> &mut HttpResponse {
    self.add_header("Content-Security-Policy".to_string(), policy);
    self
}
```

This allows usage like:

```rust
response.add_csp_header(csp_policy);
```

Instead of:

```rust
response.add_header("Content-Security-Policy".to_string(), csp_policy);
```

The convenience method is optional but improves code readability and makes CSP a first-class concern in the response builder.

---

## Code Snippets

### Complete `handle_connection()` with CSP Support

```rust
fn handle_connection(mut stream: TcpStream, routes: HashMap<String, PathBuf>) {
    let http_request = match HttpRequest::build_from_stream(&stream) {
        Ok(req) => req,
        Err(e) => {
            eprintln!("Bad request: {e}");
            let mut response = HttpResponse::build(String::from("HTTP/1.1"), 400);
            let csp_policy = get_csp_policy();
            response.add_header("Content-Security-Policy".to_string(), csp_policy);
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

    let csp_policy = get_csp_policy();
    response.add_header("Content-Security-Policy".to_string(), csp_policy);

    let contents = fs::read_to_string(filename).unwrap();
    response.add_body(contents.into());

    println!("Response: {response}");
    stream.write_all(&response.as_bytes()).unwrap();
}

fn get_csp_policy() -> String {
    std::env::var("RCOMM_CSP_POLICY").unwrap_or_else(|_| DEFAULT_CSP_POLICY.to_string())
}

const DEFAULT_CSP_POLICY: &str = "default-src 'self'";
```

### Convenience Method Addition to `src/models/http_response.rs`

```rust
impl HttpResponse {
    // ... existing methods ...

    pub fn add_csp_header(&mut self, policy: String) -> &mut HttpResponse {
        self.add_header("Content-Security-Policy".to_string(), policy);
        self
    }

    // ... rest of impl ...
}
```

---

## Testing Strategy

### Unit Tests in `src/models/http_response.rs`

Add tests to verify CSP header behavior:

```rust
#[test]
fn add_csp_header_stores_header() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
    resp.add_csp_header("default-src 'self'".to_string());
    let val = resp.try_get_header("Content-Security-Policy".to_string());
    assert_eq!(val, Some("default-src 'self'".to_string()));
}

#[test]
fn add_csp_header_returns_self() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
    let result = resp.add_csp_header("default-src 'self'".to_string());
    assert!(std::ptr::eq(result, &mut resp));
}

#[test]
fn add_csp_header_appears_in_output() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
    resp.add_csp_header("default-src 'self'; script-src 'unsafe-inline'".to_string());
    let output = format!("{resp}");
    assert!(output.contains("content-security-policy: default-src 'self'; script-src 'unsafe-inline'\r\n"));
}

#[test]
fn csp_header_survives_serialization() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
    resp.add_csp_header("script-src 'none'".to_string());
    resp.add_body(b"<h1>Test</h1>".to_vec());
    let bytes = resp.as_bytes();
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("content-security-policy: script-src 'none'\r\n"));
}
```

### Integration Tests in `src/bin/integration_test.rs`

Add tests to verify CSP headers are sent by the server in real HTTP responses:

```rust
#[test]
fn get_request_includes_csp_header() {
    let test_fn = |mut stream: TcpStream| -> TestResult {
        stream.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .map_err(|e| format!("Failed to write: {e}"))?;

        let response = read_response(&mut stream)
            .map_err(|e| format!("Failed to read response: {e}"))?;

        if let Some(csp) = response.headers.get("content-security-policy") {
            if !csp.is_empty() {
                TestResult::Pass
            } else {
                TestResult::Fail("CSP header is empty".to_string())
            }
        } else {
            TestResult::Fail("CSP header missing from response".to_string())
        }
    };

    run_test("GET request includes CSP header", test_fn);
}

#[test]
fn post_request_includes_csp_header() {
    let test_fn = |mut stream: TcpStream| -> TestResult {
        stream.write_all(b"POST /form HTTP/1.1\r\nHost: localhost\r\nContent-Length: 9\r\n\r\nkey=value")
            .map_err(|e| format!("Failed to write: {e}"))?;

        let response = read_response(&mut stream)
            .map_err(|e| format!("Failed to read response: {e}"))?;

        if let Some(csp) = response.headers.get("content-security-policy") {
            if !csp.is_empty() {
                TestResult::Pass
            } else {
                TestResult::Fail("CSP header is empty".to_string())
            }
        } else {
            TestResult::Fail("CSP header missing from POST response".to_string())
        }
    };

    run_test("POST request includes CSP header", test_fn);
}

#[test]
fn csp_header_default_value_is_correct() {
    let test_fn = |mut stream: TcpStream| -> TestResult {
        stream.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .map_err(|e| format!("Failed to write: {e}"))?;

        let response = read_response(&mut stream)
            .map_err(|e| format!("Failed to read response: {e}"))?;

        if let Some(csp) = response.headers.get("content-security-policy") {
            if csp.contains("default-src") && csp.contains("'self'") {
                TestResult::Pass
            } else {
                TestResult::Fail(format!("CSP policy missing expected directives. Got: {csp}"))
            }
        } else {
            TestResult::Fail("CSP header missing from response".to_string())
        }
    };

    run_test("CSP header has correct default policy", test_fn);
}

#[test]
fn csp_header_environment_variable_override() {
    let test_fn = |mut stream: TcpStream| -> TestResult {
        stream.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .map_err(|e| format!("Failed to write: {e}"))?;

        let response = read_response(&mut stream)
            .map_err(|e| format!("Failed to read response: {e}"))?;

        if let Some(csp) = response.headers.get("content-security-policy") {
            // When env var is set, it should contain custom policy
            if csp.contains("script-src") || csp.contains("img-src") || csp.contains("default-src") {
                TestResult::Pass
            } else {
                TestResult::Fail(format!("CSP header not set properly: {csp}"))
            }
        } else {
            TestResult::Fail("CSP header missing from response".to_string())
        }
    };

    run_test("Environment variable RCOMM_CSP_POLICY is respected", test_fn);
}

#[test]
fn csp_header_on_404_response() {
    let test_fn = |mut stream: TcpStream| -> TestResult {
        stream.write_all(b"GET /nonexistent/path HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .map_err(|e| format!("Failed to write: {e}"))?;

        let response = read_response(&mut stream)
            .map_err(|e| format!("Failed to read response: {e}"))?;

        if response.status_code != 404 {
            return TestResult::Fail(format!("Expected 404, got {}", response.status_code));
        }

        if let Some(csp) = response.headers.get("content-security-policy") {
            if !csp.is_empty() {
                TestResult::Pass
            } else {
                TestResult::Fail("CSP header is empty on 404".to_string())
            }
        } else {
            TestResult::Fail("CSP header missing from 404 response".to_string())
        }
    };

    run_test("404 responses include CSP header", test_fn);
}

#[test]
fn csp_header_on_400_response() {
    let test_fn = |mut stream: TcpStream| -> TestResult {
        // Send malformed request (no Host header on HTTP/1.1)
        stream.write_all(b"GET / HTTP/1.1\r\n\r\n")
            .map_err(|e| format!("Failed to write: {e}"))?;

        let response = read_response(&mut stream)
            .map_err(|e| format!("Failed to read response: {e}"))?;

        if response.status_code != 400 {
            return TestResult::Fail(format!("Expected 400, got {}", response.status_code));
        }

        if let Some(csp) = response.headers.get("content-security-policy") {
            if !csp.is_empty() {
                TestResult::Pass
            } else {
                TestResult::Fail("CSP header is empty on 400".to_string())
            }
        } else {
            TestResult::Fail("CSP header missing from 400 response".to_string())
        }
    };

    run_test("400 error responses include CSP header", test_fn);
}
```

### Manual Testing

1. **Start the server:**
   ```bash
   cargo run
   ```

2. **Check default CSP header:**
   ```bash
   curl -i http://127.0.0.1:7878/
   ```
   Expected output should include:
   ```
   Content-Security-Policy: default-src 'self'
   ```

3. **Override CSP with environment variable:**
   ```bash
   RCOMM_CSP_POLICY="script-src 'self' https://cdn.example.com" cargo run
   curl -i http://127.0.0.1:7878/
   ```
   Expected output should include:
   ```
   Content-Security-Policy: script-src 'self' https://cdn.example.com
   ```

4. **Verify on error responses:**
   ```bash
   curl -i http://127.0.0.1:7878/nonexistent
   ```
   Should still include CSP header on 404 response.

5. **Verify on bad requests:**
   ```bash
   (echo -e "GET / HTTP/1.1\r\n\r"; sleep 1) | nc localhost 7878
   ```
   Should include CSP header on 400 response.

---

## Edge Cases & Considerations

### 1. **Empty CSP Policy**
If `RCOMM_CSP_POLICY` is set to an empty string, the header will still be added with an empty value. This may cause the browser to reject it.

**Handling:**
- Document that the policy must be a valid CSP directive string
- Optionally validate in `get_csp_policy()` to ensure non-empty:

```rust
fn get_csp_policy() -> String {
    let policy = std::env::var("RCOMM_CSP_POLICY")
        .unwrap_or_else(|_| DEFAULT_CSP_POLICY.to_string());

    if policy.is_empty() {
        eprintln!("Warning: RCOMM_CSP_POLICY is empty, using default");
        DEFAULT_CSP_POLICY.to_string()
    } else {
        policy
    }
}
```

### 2. **Invalid CSP Directives**
The server does not validate CSP syntax. Invalid directives will be passed to the browser, which will silently ignore them.

**Handling:**
- Document that users are responsible for valid CSP syntax
- Point to [MDN CSP Reference](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Content-Security-Policy) for valid directives
- Consider future enhancement: add optional CSP validation

### 3. **Multiple CSP Headers**
HTTP allows multiple `Content-Security-Policy` headers. The current implementation uses a single header.

**Handling:**
- Single header approach is sufficient for most use cases
- Future enhancement could support semicolon-separated policies or multiple `add_header()` calls

### 4. **CSP Report-Only Mode**
CSP also has a report-only variant using `Content-Security-Policy-Report-Only` header, which logs violations without blocking content.

**Future Enhancement:** Add support via `RCOMM_CSP_REPORT_ONLY` environment variable:

```rust
const DEFAULT_CSP_POLICY: &str = "default-src 'self'";
const DEFAULT_CSP_REPORT_ONLY: bool = false;

fn apply_csp_headers(response: &mut HttpResponse) {
    let policy = get_csp_policy();
    let report_only = get_csp_report_only();

    if report_only {
        response.add_header("Content-Security-Policy-Report-Only".to_string(), policy);
    } else {
        response.add_header("Content-Security-Policy".to_string(), policy);
    }
}
```

### 5. **Case Sensitivity**
The header name "Content-Security-Policy" is stored in lowercase ("content-security-policy") internally due to the `add_header()` implementation converting keys to lowercase. This is correct per HTTP spec.

**Handling:**
- No action needed; existing behavior is correct
- Tests verify this works correctly

### 6. **Character Encoding**
CSP directives can contain only ASCII characters. Non-ASCII values will cause issues.

**Handling:**
- Document that CSP policy must be ASCII-only
- No validation needed for MVP; users responsible for valid input

### 7. **Performance Impact**
Calling `std::env::var()` on every request could have minimal overhead. For high-traffic servers, consider caching the policy at startup.

**Optimization (future enhancement):**
```rust
use std::sync::OnceLock;

fn get_csp_policy() -> String {
    static CSP_CACHE: OnceLock<String> = OnceLock::new();
    CSP_CACHE.get_or_init(|| {
        std::env::var("RCOMM_CSP_POLICY")
            .unwrap_or_else(|_| DEFAULT_CSP_POLICY.to_string())
    }).clone()
}
```

This avoids environment variable lookup on every request.

---

## Implementation Checklist

- [ ] Add `DEFAULT_CSP_POLICY` constant to `src/main.rs`
- [ ] Add `get_csp_policy()` function to `src/main.rs`
- [ ] Apply CSP header to successful responses in `handle_connection()`
- [ ] Apply CSP header to 404 error responses in `handle_connection()`
- [ ] Apply CSP header to 400 error responses in `handle_connection()`
- [ ] (Optional) Add `add_csp_header()` convenience method to `HttpResponse`
- [ ] Add unit tests to `src/models/http_response.rs`
- [ ] Add integration tests to `src/bin/integration_test.rs`
- [ ] Manual testing with `curl` and environment variable overrides
- [ ] Document CSP policy format and valid directives in comments
- [ ] Verify all tests pass: `cargo test`
- [ ] Verify integration tests pass: `cargo run --bin integration_test`
- [ ] Update CLAUDE.md with CSP support info (optional)

---

## Configuration Examples

### Default Policy (Most Restrictive)
```bash
cargo run
# Uses: default-src 'self'
```

### Allow CDN Resources
```bash
RCOMM_CSP_POLICY="default-src 'self'; script-src 'self' https://cdn.example.com; style-src 'self' https://cdn.example.com" cargo run
```

### Development-Friendly Policy
```bash
RCOMM_CSP_POLICY="default-src 'self' 'unsafe-inline' 'unsafe-eval'" cargo run
```

### Strict Policy with Nonces
```bash
RCOMM_CSP_POLICY="default-src 'none'; script-src 'nonce-abc123'; style-src 'nonce-abc123'" cargo run
```

---

## Related Standards & References

- [MDN: Content-Security-Policy](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Content-Security-Policy)
- [CSP Level 3 Specification](https://w3c.github.io/webappsec-csp/)
- [OWASP: Content Security Policy](https://cheatsheetseries.owasp.org/cheatsheets/Content_Security_Policy_Cheat_Sheet.html)
- [Helmet.js CSP Middleware](https://helmetjs.github.io/#csp) (reference for inspiration)

---

## Summary

This feature adds essential security hardening to rcomm by enabling Content-Security-Policy headers on all HTTP responses. The implementation is minimal (under 20 lines of new code), relies on existing `HttpResponse` infrastructure, and provides full flexibility through environment variable configuration. The feature protects against XSS and injection attacks while maintaining the server's lightweight, dependency-free design.
