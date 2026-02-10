# Implementation Plan: Add `Strict-Transport-Security` (HSTS) Response Header

## Overview

This feature adds automatic inclusion of the `Strict-Transport-Security` (HSTS) response header to all HTTP responses sent by the rcomm server when the connection is established over TLS/HTTPS. The HSTS header instructs compatible browsers to enforce HTTPS-only communication with the server for a specified duration, preventing downgrade attacks from HTTPS to HTTP.

The feature is motivated by:
1. **Security Enhancement:** Protects against SSL stripping attacks and man-in-the-middle attacks
2. **Best Practice:** Recommended by OWASP and browser vendors
3. **Protocol Compliance:** Aligns with HTTP Strict Transport Security specification (RFC 6797)

**Complexity:** 1/10 (Straightforward addition, though requires TLS detection infrastructure)
**Necessity:** 5/10 (Important for production HTTPS services, not critical for current HTTP-only implementation)

---

## Current State Analysis

### TLS Support Status

**Current Limitation:** The rcomm server currently:
- Listens on plain TCP sockets via `std::net::TcpListener`
- Has no TLS/SSL support implemented
- Cannot detect whether a connection uses HTTPS or plain HTTP

**Impact on This Feature:**
- The HSTS header is only meaningful for HTTPS connections
- The implementation must include TLS support infrastructure
- This feature cannot be fully utilized until TLS support is added to the server

**Recommended Approach:** This plan documents:
1. The infrastructure changes needed to support TLS detection
2. The HSTS header implementation in `HttpResponse`
3. How to integrate the two components
4. Testing strategy for both TLS and HSTS

---

## Architecture Design

### Two-Phase Implementation

#### Phase 1: HSTS Header Support (No TLS)
- Add HSTS header generation to `HttpResponse`
- Add conditional logic to serialize header only when needed
- Create the foundation for TLS integration

#### Phase 2: TLS Integration (Future)
- Upgrade `TcpListener` to `TlsListener` (requires external crate like `rustls` or `native-tls`)
- Detect TLS connections at runtime
- Conditionally add HSTS header to responses

---

## Implementation Details

### Files to Modify

1. **`src/models/http_response.rs`** — Add HSTS header support
2. **`src/main.rs`** — Pass TLS information to request handler (future)
3. **`src/lib.rs`** — Define HSTS configuration constants
4. **`src/models/http_request.rs`** — Add optional TLS metadata (future)

### Design Decisions

#### 1. HSTS Header Value

The standard HSTS header format is:
```
Strict-Transport-Security: max-age=31536000; includeSubDomains; preload
```

**Components:**
- `max-age=31536000` — 1 year in seconds (standard production value)
- `includeSubDomains` — Apply HSTS to all subdomains
- `preload` — Allow inclusion in browser HSTS preload list

**Configuration Constants** (to be added to `src/lib.rs`):

```rust
// Default HSTS parameters
pub const HSTS_MAX_AGE: u32 = 31536000; // 1 year in seconds
pub const HSTS_INCLUDE_SUBDOMAINS: bool = true;
pub const HSTS_PRELOAD: bool = true;

// Full header value
pub fn hsts_header_value() -> String {
    let mut value = format!("max-age={}", HSTS_MAX_AGE);
    if HSTS_INCLUDE_SUBDOMAINS {
        value.push_str("; includeSubDomains");
    }
    if HSTS_PRELOAD {
        value.push_str("; preload");
    }
    value
}
```

#### 2. TLS Detection Pattern

Because the server currently uses `TcpStream`, we need a way to track whether a connection is TLS-secured. Options include:

**Option A: Pass TLS flag through request handling** (Recommended)
- Store a flag in `HttpRequest` indicating TLS status
- Add field: `pub tls_secure: bool`
- Pass this flag when building responses

**Option B: Environment variable control** (Interim)
- Add `RCOMM_USE_HSTS` environment variable
- Always add header if set (for testing)
- Replace with runtime detection once TLS is implemented

**Option C: Response-level configuration**
- Add method: `response.set_hsts_enabled(bool)`
- Users manually enable when appropriate

**Recommendation:** Use Option A (TLS flag in request) as it's the most accurate and prepares for real TLS support.

---

## Step-by-Step Implementation

### Phase 1: HSTS Header Support

#### Step 1: Define HSTS Configuration Constants

**File:** `/home/jwall/personal/rusty/rcomm/src/lib.rs`

Add constants and helper function at the top of the file:

```rust
pub mod models;

// HSTS configuration
pub const HSTS_MAX_AGE: u32 = 31536000; // 1 year in seconds
pub const HSTS_INCLUDE_SUBDOMAINS: bool = true;
pub const HSTS_PRELOAD: bool = true;

/// Generate the Strict-Transport-Security header value
pub fn hsts_header_value() -> String {
    let mut value = format!("max-age={}", HSTS_MAX_AGE);
    if HSTS_INCLUDE_SUBDOMAINS {
        value.push_str("; includeSubDomains");
    }
    if HSTS_PRELOAD {
        value.push_str("; preload");
    }
    value
}
```

**Rationale:**
- Centralized configuration makes HSTS parameters easy to modify
- Helper function generates compliant header values
- Constants follow RFC 6797 recommendations for production servers
- Can be overridden if needed without code changes

#### Step 2: Add TLS Status Field to HttpRequest

**File:** `/home/jwall/personal/rusty/rcomm/src/models/http_request.rs`

Add a field to track whether the connection is TLS-secured:

```rust
#[derive(Debug)]
pub struct HttpRequest {
    pub method: HttpMethods,
    pub target: String,
    pub version: String,
    headers: HashMap<String, String>,
    body: Option<Vec<u8>>,
    pub is_tls: bool,  // NEW: Indicates if connection is over TLS
}
```

Update the `build()` method:

```rust
pub fn build(method: HttpMethods, target: String, version: String) -> HttpRequest {
    let headers = HashMap::<String, String>::new();
    HttpRequest {
        method,
        target,
        version,
        headers,
        body: None,
        is_tls: false,  // Default to false; set by connection handler
    }
}
```

Add a setter method:

```rust
pub fn set_tls(&mut self, is_tls: bool) -> &mut HttpRequest {
    self.is_tls = is_tls;
    self
}
```

Update the `build_from_stream()` method signature (optional, for documentation):

```rust
/// Build HttpRequest from a TCP stream.
///
/// Note: `is_tls` field defaults to false. The caller (in main.rs) is responsible
/// for setting this field based on the connection type.
pub fn build_from_stream(stream: &TcpStream) -> Result<HttpRequest, HttpParseError> {
    // ... existing implementation ...
}
```

**Rationale:**
- Tracks security context of the request
- Follows builder pattern consistency
- Allows connection handler to populate TLS status
- Future-proof: when real TLS is implemented, set this flag in TLS stream wrapper

#### Step 3: Add HSTS Header Methods to HttpResponse

**File:** `/home/jwall/personal/rusty/rcomm/src/models/http_response.rs`

Add field to track HSTS requirement:

```rust
pub struct HttpResponse {
    version: String,
    status_code: u16,
    status_phrase: String,
    headers: HashMap<String, String>,
    body: Option<Vec<u8>>,
    include_hsts: bool,  // NEW: Whether to include HSTS header
}
```

Update the `build()` method:

```rust
pub fn build(version: String, code: u16) -> HttpResponse {
    let headers = HashMap::<String, String>::new();
    let phrase = get_status_phrase(code);
    HttpResponse {
        version,
        status_code: code,
        status_phrase: phrase,
        headers,
        body: None,
        include_hsts: false,  // Default to false; caller enables if needed
    }
}
```

Add setter method:

```rust
pub fn enable_hsts(&mut self) -> &mut HttpResponse {
    self.include_hsts = true;
    self
}

pub fn disable_hsts(&mut self) -> &mut HttpResponse {
    self.include_hsts = false;
    self
}

pub fn is_hsts_enabled(&self) -> bool {
    self.include_hsts
}
```

Update the `fmt::Display` implementation to conditionally include HSTS header:

```rust
impl fmt::Display for HttpResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let _ = match write!(f, "{} {} {}\r\n", self.version, self.status_code, self.status_phrase) {
            Ok(result) => result,
            Err(e) => return Err(e),
        };

        // Add HSTS header if enabled (before other headers for visibility)
        if self.include_hsts {
            let hsts_value = crate::hsts_header_value();
            let _ = match write!(f, "strict-transport-security: {}\r\n", hsts_value) {
                Ok(result) => result,
                Err(e) => return Err(e),
            };
        }

        for (title, value) in &self.headers {
            let _ = match write!(f, "{}: {}\r\n", title, value) {
                Ok(result) => result,
                Err(e) => return Err(e),
            };
        }
        write!(f, "\r\n")
    }
}
```

**Rationale:**
- Non-intrusive design: HSTS is optional by default
- Follows builder pattern (enable/disable methods)
- Header generated dynamically from central configuration
- HSTS header written early for visibility in serialized output
- Maintains header lowercase convention

#### Step 4: Update Connection Handler

**File:** `/home/jwall/personal/rusty/rcomm/src/main.rs`

Modify `handle_connection()` to use TLS status and HSTS:

```rust
fn handle_connection(mut stream: TcpStream, routes: HashMap<String, PathBuf>) {
    let http_request = match HttpRequest::build_from_stream(&stream) {
        Ok(req) => req,
        Err(e) => {
            eprintln!("Bad request: {e}");
            let mut response = HttpResponse::build(String::from("HTTP/1.1"), 400);
            // Note: HSTS not added to error responses until TLS is implemented
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

    // Enable HSTS if connection is over TLS (currently always false until TLS is implemented)
    if http_request.is_tls {
        response.enable_hsts();
    }

    let contents = fs::read_to_string(filename).unwrap();
    response.add_body(contents.into());

    println!("Response: {response}");
    stream.write_all(&response.as_bytes()).unwrap();
}
```

**Rationale:**
- HSTS only added when TLS is detected
- Prepares code for future TLS implementation
- No changes needed in response building logic
- Clean separation of concerns

#### Step 5: Update Unit Tests

**File:** `/home/jwall/personal/rusty/rcomm/src/models/http_response.rs`

Add tests for HSTS functionality:

```rust
#[test]
fn hsts_disabled_by_default() {
    let resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
    assert!(!resp.is_hsts_enabled());
}

#[test]
fn enable_hsts_sets_flag() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
    resp.enable_hsts();
    assert!(resp.is_hsts_enabled());
}

#[test]
fn disable_hsts_clears_flag() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
    resp.enable_hsts();
    resp.disable_hsts();
    assert!(!resp.is_hsts_enabled());
}

#[test]
fn hsts_header_not_included_when_disabled() {
    let resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
    let output = format!("{resp}");
    assert!(!output.contains("strict-transport-security"));
}

#[test]
fn hsts_header_included_when_enabled() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
    resp.enable_hsts();
    let output = format!("{resp}");
    assert!(output.contains("strict-transport-security"));
    assert!(output.contains("max-age=31536000"));
}

#[test]
fn hsts_header_format_includes_subdomains() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
    resp.enable_hsts();
    let output = format!("{resp}");
    assert!(output.contains("; includeSubDomains"));
}

#[test]
fn hsts_header_format_includes_preload() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
    resp.enable_hsts();
    let output = format!("{resp}");
    assert!(output.contains("; preload"));
}

#[test]
fn hsts_header_chaining() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
    resp.enable_hsts()
        .add_header("X-Custom".to_string(), "value".to_string());
    let output = format!("{resp}");
    assert!(output.contains("strict-transport-security"));
    assert!(output.contains("x-custom: value\r\n"));
}

#[test]
fn hsts_header_with_body() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
    resp.enable_hsts();
    resp.add_body(b"test body".to_vec());
    let bytes = resp.as_bytes();
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("strict-transport-security"));
    assert!(text.ends_with("test body"));
}

#[test]
fn hsts_header_different_status_codes() {
    for code in [200, 304, 400, 404, 500].iter() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), *code);
        resp.enable_hsts();
        let output = format!("{resp}");
        assert!(output.contains("strict-transport-security"),
                "HSTS header missing for status {}", code);
    }
}
```

Update existing tests that parse response output to account for HSTS header (when enabled):

```rust
#[test]
fn display_formats_status_line_and_headers() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
    resp.add_header("Server".to_string(), "rcomm".to_string());
    let output = format!("{resp}");
    assert!(output.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(output.contains("server: rcomm\r\n"));
    // HSTS not included unless explicitly enabled
    assert!(!output.contains("strict-transport-security"));
    assert!(output.ends_with("\r\n"));
}
```

**Rationale:**
- Comprehensive coverage of HSTS feature
- Tests both enabled and disabled states
- Verifies header format matches RFC 6797
- Tests method chaining (builder pattern)
- Tests various status codes
- Tests interaction with body serialization

#### Step 6: Update HttpRequest Tests

**File:** `/home/jwall/personal/rusty/rcomm/src/models/http_request.rs`

Add tests for TLS flag:

```rust
#[test]
fn build_initializes_tls_as_false() {
    let req = HttpRequest::build(
        HttpMethods::GET,
        "/".to_string(),
        "HTTP/1.1".to_string(),
    );
    assert!(!req.is_tls);
}

#[test]
fn set_tls_changes_flag() {
    let mut req = HttpRequest::build(
        HttpMethods::GET,
        "/".to_string(),
        "HTTP/1.1".to_string(),
    );
    req.set_tls(true);
    assert!(req.is_tls);
}

#[test]
fn set_tls_supports_chaining() {
    let mut req = HttpRequest::build(
        HttpMethods::GET,
        "/".to_string(),
        "HTTP/1.1".to_string(),
    );
    req.set_tls(true)
        .add_header("Host".to_string(), "example.com".to_string());
    assert!(req.is_tls);
    assert_eq!(req.try_get_header("Host".to_string()), Some("example.com".to_string()));
}
```

**Rationale:**
- Ensures TLS flag defaults correctly
- Tests setter method works
- Verifies builder pattern support

---

### Phase 2: TLS Integration (Future)

#### Infrastructure Setup

When implementing real TLS support:

1. **Add TLS Dependency:**
   - Update `Cargo.toml` to include `rustls` or similar
   - Note: This requires external dependencies, which breaks the "no external dependencies" design

2. **Create TLS Listener Wrapper:**
   ```rust
   // src/tls_listener.rs (new file)
   pub struct TlsListener { /* ... */ }
   pub struct TlsStream {
       inner: TcpStream,
       is_tls: bool,
   }
   ```

3. **Update Main Loop:**
   - Replace `TcpListener` with `TlsListener`
   - Detect TLS on connection accept
   - Pass TLS status to handler

4. **Update Handler Signature:**
   ```rust
   fn handle_connection(
       stream: TcpStream,
       is_tls: bool,  // NEW parameter
       routes: HashMap<String, PathBuf>
   ) {
       let mut http_request = HttpRequest::build_from_stream(&stream)?;
       http_request.set_tls(is_tls);
       // ... rest of handler
   }
   ```

#### Decision Point

**Important Note:** Adding TLS support violates the "no external dependencies" constraint stated in CLAUDE.md. Before implementing Phase 2:
1. Decide whether to add an external TLS crate
2. Alternatively, implement minimal TLS in pure Rust (extremely complex)
3. Or document that HSTS requires a separate TLS-enabled server variant

**Recommendation:** Phase 2 should be a separate feature request documenting TLS support.

---

## Testing Strategy

### Unit Tests (in `http_response.rs` and `http_request.rs`)

1. **HSTS Header Generation:**
   - Verify header is NOT included by default
   - Verify header IS included when enabled
   - Verify header format compliance (max-age, directives)

2. **HSTS with Different Status Codes:**
   - Test 200, 304, 400, 404, 500 responses
   - Verify header present on all codes when enabled

3. **HSTS with Headers and Body:**
   - Verify HSTS coexists with other headers
   - Verify HSTS appears before body content

4. **TLS Flag in HttpRequest:**
   - Default value is false
   - Can be set via `set_tls()`
   - Supports builder pattern chaining

### Integration Tests (in `src/bin/integration_test.rs`)

Add end-to-end tests that verify HSTS header behavior:

```rust
#[test]
fn hsts_header_not_present_in_plain_http() {
    // When TLS is not implemented, HSTS should not be in responses
    let (mut server, port) = spawn_server();

    let response = make_request(
        &format!("GET / HTTP/1.1\r\nHost: localhost:{port}\r\n\r\n"),
        port
    );
    assert!(!response.contains("strict-transport-security"));

    server.stop();
}

#[test]
fn hsts_header_present_on_200_response() {
    // Once TLS is implemented, test HSTS on 200 responses
    // This test is a placeholder for future TLS implementation
    let (mut server, port) = spawn_server_tls();  // Future function

    let response = make_request_tls(
        &format!("GET / HTTP/1.1\r\nHost: localhost:{port}\r\n\r\n"),
        port,
        true  // is_tls
    );
    assert!(response.contains("Strict-Transport-Security"));
    assert!(response.contains("max-age=31536000"));

    server.stop();
}

#[test]
fn hsts_header_includes_subdomains_and_preload() {
    // Future test once TLS is implemented
    let (mut server, port) = spawn_server_tls();

    let response = make_request_tls(
        &format!("GET / HTTP/1.1\r\nHost: example.com\r\n\r\n"),
        port,
        true
    );
    assert!(response.contains("includeSubDomains"));
    assert!(response.contains("preload"));

    server.stop();
}
```

**Note:** These tests are marked as future tests because they require TLS support.

### Manual Testing

**Current (HTTP Only):**
```bash
# Build the server
cargo build

# Run the server
cargo run &

# Make a request and check for HSTS
curl -i http://127.0.0.1:7878/

# Expected: No strict-transport-security header (as expected for HTTP)
```

**Future (HTTPS):**
```bash
# Once TLS is implemented:
curl -i https://127.0.0.1:7878/

# Expected output includes:
# Strict-Transport-Security: max-age=31536000; includeSubDomains; preload
```

### Test Execution

```bash
# Run all unit tests
cargo test

# Run specific unit tests
cargo test hsts

# Run integration tests
cargo run --bin integration_test

# Run tests with output
cargo test -- --nocapture
```

---

## Edge Cases & Considerations

### 1. HSTS Header Format Compliance

**RFC 6797 Requirements:**
- Directive names are case-insensitive (implementation uses lowercase)
- Directives are separated by semicolons
- Optional whitespace around directives is allowed
- Unrecognized directives should be ignored by clients

**Our Implementation:**
```
Strict-Transport-Security: max-age=31536000; includeSubDomains; preload
```

**Compliance:** Fully compliant ✓

**Test Case:**
```rust
#[test]
fn hsts_header_rfc6797_compliant() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
    resp.enable_hsts();
    let output = format!("{resp}");
    // Must have exact format
    assert!(output.contains("strict-transport-security: max-age=31536000; includeSubDomains; preload"));
}
```

### 2. Minimum max-age Requirement

**RFC 6797 Recommendation:**
- Minimum `max-age` should be 18 hours (64800 seconds)
- For maximum security, 1 year (31536000 seconds)

**Our Implementation:** Uses 31536000 (1 year) ✓

**Consideration:** If configuration becomes dynamic, validate min-age >= 64800

### 3. HSTS with Redirects (HTTP to HTTPS)

**Scenario:** Server returns 301/307 redirect from HTTP to HTTPS

**RFC 6797 Guidance:**
- HSTS header should NOT be sent over plain HTTP
- Only send HSTS over secure connections

**Our Implementation:**
- Currently sends HSTS based on TLS flag
- Future implementation will only set flag on TLS connections
- Redirect responses will only have HSTS if over TLS ✓

**Test Case:**
```rust
#[test]
fn hsts_not_added_to_non_tls_responses() {
    // Simulate HTTP request
    let mut req = HttpRequest::build(HttpMethods::GET, "/".to_string(), "HTTP/1.1".to_string());
    req.set_tls(false);  // Explicitly HTTP

    // Response should not include HSTS
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 301);
    if req.is_tls {
        resp.enable_hsts();
    }

    let output = format!("{resp}");
    assert!(!output.contains("strict-transport-security"));
}
```

### 4. Wildcard Domains and includeSubDomains

**Scenario:** Server serves `*.example.com`

**RFC 6797 Behavior:**
- `includeSubDomains` applies HSTS to all subdomains
- Includes wildcard subdomains

**Our Implementation:** Always includes `includeSubDomains` directive

**Consideration:** If making this configurable, document implications

### 5. Max-Age Zero (Policy Removal)

**Use Case:** To remove an HSTS policy, set `max-age=0`

**Current Implementation:** Uses hardcoded 1-year value

**Future Enhancement:**
```rust
pub fn disable_hsts_policy() -> String {
    format!("max-age=0; includeSubDomains")
}
```

**Note:** Not required for Phase 1

### 6. Preload Directive Implications

**RFC 6797 Supplement - HSTS Preload List:**
- Browsers maintain a built-in list of HSTS preload domains
- Domains must:
  - Send `Strict-Transport-Security` header with `preload` directive
  - Serve all subdomains over HTTPS
  - Ensure max-age >= 31536000 (1 year)
  - Have valid SSL certificate

**Our Implementation:** Includes `preload` directive

**Implications:**
- Server operators must ensure HTTPS is properly configured
- All subdomains must be HTTPS
- Certificate must be valid (checked by TLS layer)

**Documentation:** Should note these requirements for users

### 7. Interaction with CSP and Other Security Headers

**Complementary Headers:**
- `Content-Security-Policy` — Prevents injection attacks
- `X-Content-Type-Options: nosniff` — Prevents MIME type sniffing
- `X-Frame-Options` — Clickjacking protection

**HSTS Role:** Transport-level security, complements application-level headers

**Test Case:**
```rust
#[test]
fn hsts_works_with_other_security_headers() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
    resp.enable_hsts();
    resp.add_header("X-Frame-Options".to_string(), "DENY".to_string());
    resp.add_header("X-Content-Type-Options".to_string(), "nosniff".to_string());

    let output = format!("{resp}");
    assert!(output.contains("strict-transport-security"));
    assert!(output.contains("x-frame-options: DENY"));
    assert!(output.contains("x-content-type-options: nosniff"));
}
```

### 8. Performance Impact

**Analysis:**
- Minimal: One string allocation per response (header value generation)
- Boolean flag check on response serialization
- Header written once during Display trait

**Benchmark Note:** Not expected to impact performance measurably

### 9. Case Sensitivity in Header Name

**Standard:** Header names are case-insensitive in HTTP

**Our Implementation:**
- Stored as `"strict-transport-security"` (lowercase)
- Serialized as-is in Display impl
- Clients handle case-insensitively ✓

**Test Case:**
```rust
#[test]
fn hsts_header_name_lowercase() {
    let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
    resp.enable_hsts();
    let output = format!("{resp}");
    // Must be lowercase in our implementation
    assert!(output.contains("strict-transport-security:"));
    assert!(!output.contains("Strict-Transport-Security:"));
}
```

### 10. Error Responses (400, 403, 500)

**Question:** Should HSTS be included in error responses?

**RFC 6797:** No specific guidance, but best practice suggests:
- Include on 4xx client errors (not the server's fault)
- Include on 5xx server errors (still over HTTPS)
- Especially important for 400 (malformed request)

**Our Implementation:** Adds HSTS to all responses when TLS is detected

**Rationale:** Client should still enforce HTTPS regardless of response status

**Test Case:**
```rust
#[test]
fn hsts_included_on_error_responses() {
    for code in [400, 403, 404, 500].iter() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), *code);
        resp.enable_hsts();
        let output = format!("{resp}");
        assert!(output.contains("strict-transport-security"),
                "HSTS missing on {} response", code);
    }
}
```

---

## Implementation Checklist

### Phase 1: HSTS Header Support

- [ ] Add HSTS constants to `src/lib.rs` (`HSTS_MAX_AGE`, `HSTS_INCLUDE_SUBDOMAINS`, `HSTS_PRELOAD`)
- [ ] Add `hsts_header_value()` function to `src/lib.rs`
- [ ] Add `is_tls: bool` field to `HttpRequest` struct in `src/models/http_request.rs`
- [ ] Update `HttpRequest::build()` to initialize `is_tls` to false
- [ ] Add `set_tls()` method to `HttpRequest` in `src/models/http_request.rs`
- [ ] Add unit tests for `HttpRequest::set_tls()` in `src/models/http_request.rs`
- [ ] Add `include_hsts: bool` field to `HttpResponse` struct in `src/models/http_response.rs`
- [ ] Update `HttpResponse::build()` to initialize `include_hsts` to false
- [ ] Add `enable_hsts()` method to `HttpResponse` in `src/models/http_response.rs`
- [ ] Add `disable_hsts()` method to `HttpResponse` in `src/models/http_response.rs`
- [ ] Add `is_hsts_enabled()` method to `HttpResponse` in `src/models/http_response.rs`
- [ ] Update `fmt::Display` implementation to include HSTS header when enabled
- [ ] Update `handle_connection()` in `src/main.rs` to conditionally enable HSTS based on TLS flag
- [ ] Add unit tests for HSTS header generation (8+ tests)
- [ ] Add unit tests for HSTS format compliance (3+ tests)
- [ ] Add unit tests for HSTS with different status codes
- [ ] Run `cargo test` to verify all tests pass and no regressions
- [ ] Add integration test placeholder for future TLS implementation
- [ ] Update CLAUDE.md to mention HSTS support (if needed)

### Phase 2: TLS Integration (Future)

- [ ] Decide on TLS library (rustls, native-tls, or pure Rust alternative)
- [ ] Update `Cargo.toml` with TLS dependency
- [ ] Create TLS listener wrapper or adapter
- [ ] Update main server loop to handle TLS connections
- [ ] Set `is_tls` flag on TLS-secured requests
- [ ] Implement integration tests for HSTS over HTTPS
- [ ] Document HSTS preload requirements
- [ ] Create configuration for HSTS parameters (optional)

---

## Code Summary

### Complete Changes for Phase 1

#### File: `src/lib.rs`

Add at the top of the file:

```rust
pub mod models;

use std::{
    sync::{Arc, Mutex, mpsc},
    thread,
};

// ============ HSTS Configuration ============
pub const HSTS_MAX_AGE: u32 = 31536000; // 1 year in seconds
pub const HSTS_INCLUDE_SUBDOMAINS: bool = true;
pub const HSTS_PRELOAD: bool = true;

/// Generate the Strict-Transport-Security header value
pub fn hsts_header_value() -> String {
    let mut value = format!("max-age={}", HSTS_MAX_AGE);
    if HSTS_INCLUDE_SUBDOMAINS {
        value.push_str("; includeSubDomains");
    }
    if HSTS_PRELOAD {
        value.push_str("; preload");
    }
    value
}

// ============ Thread Pool Implementation ============
// ... rest of existing code ...
```

#### File: `src/models/http_request.rs`

Add field to struct:

```rust
#[derive(Debug)]
pub struct HttpRequest {
    pub method: HttpMethods,
    pub target: String,
    pub version: String,
    headers: HashMap<String, String>,
    body: Option<Vec<u8>>,
    pub is_tls: bool,  // NEW: Indicates if connection is over TLS
}
```

Update `build()`:

```rust
pub fn build(method: HttpMethods, target: String, version: String) -> HttpRequest {
    let headers = HashMap::<String, String>::new();
    HttpRequest {
        method,
        target,
        version,
        headers,
        body: None,
        is_tls: false,  // Default to false; set by connection handler
    }
}
```

Add method:

```rust
pub fn set_tls(&mut self, is_tls: bool) -> &mut HttpRequest {
    self.is_tls = is_tls;
    self
}
```

#### File: `src/models/http_response.rs`

Add field to struct:

```rust
pub struct HttpResponse {
    version: String,
    status_code: u16,
    status_phrase: String,
    headers: HashMap<String, String>,
    body: Option<Vec<u8>>,
    include_hsts: bool,  // NEW: Whether to include HSTS header
}
```

Update `build()`:

```rust
pub fn build(version: String, code: u16) -> HttpResponse {
    let headers = HashMap::<String, String>::new();
    let phrase = get_status_phrase(code);
    HttpResponse {
        version,
        status_code: code,
        status_phrase: phrase,
        headers,
        body: None,
        include_hsts: false,  // Default to false; caller enables if needed
    }
}
```

Add methods:

```rust
pub fn enable_hsts(&mut self) -> &mut HttpResponse {
    self.include_hsts = true;
    self
}

pub fn disable_hsts(&mut self) -> &mut HttpResponse {
    self.include_hsts = false;
    self
}

pub fn is_hsts_enabled(&self) -> bool {
    self.include_hsts
}
```

Update `fmt::Display`:

```rust
impl fmt::Display for HttpResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let _ = match write!(f, "{} {} {}\r\n", self.version, self.status_code, self.status_phrase) {
            Ok(result) => result,
            Err(e) => return Err(e),
        };

        // Add HSTS header if enabled
        if self.include_hsts {
            let hsts_value = crate::hsts_header_value();
            let _ = match write!(f, "strict-transport-security: {}\r\n", hsts_value) {
                Ok(result) => result,
                Err(e) => return Err(e),
            };
        }

        for (title, value) in &self.headers {
            let _ = match write!(f, "{}: {}\r\n", title, value) {
                Ok(result) => result,
                Err(e) => return Err(e),
            };
        }
        write!(f, "\r\n")
    }
}
```

#### File: `src/main.rs`

Update `handle_connection()`:

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

    // Enable HSTS if connection is over TLS
    if http_request.is_tls {
        response.enable_hsts();
    }

    let contents = fs::read_to_string(filename).unwrap();
    response.add_body(contents.into());

    println!("Response: {response}");
    stream.write_all(&response.as_bytes()).unwrap();
}
```

---

## Expected Behavior After Implementation

### Before Implementation

```
GET / HTTP/1.1
Host: localhost:7878

HTTP/1.1 200 OK
content-length: 42

<html>...</html>
```

### After Phase 1 Implementation (HTTP only)

Same as above — HSTS header not included (because `is_tls` defaults to false and is not set)

### After Phase 2 Implementation (with TLS)

Over HTTPS:
```
GET / HTTP/1.1
Host: localhost:7878

HTTP/1.1 200 OK
strict-transport-security: max-age=31536000; includeSubDomains; preload
content-length: 42

<html>...</html>
```

Over HTTP:
```
GET / HTTP/1.1
Host: localhost:7878

HTTP/1.1 200 OK
content-length: 42

<html>...</html>
```

---

## Rollback Plan

### Phase 1 Rollback

If HSTS support needs to be removed:

1. Remove `HSTS_MAX_AGE`, `HSTS_INCLUDE_SUBDOMAINS`, `HSTS_PRELOAD` constants from `src/lib.rs`
2. Remove `hsts_header_value()` function from `src/lib.rs`
3. Remove `include_hsts` field from `HttpResponse` struct
4. Remove `enable_hsts()`, `disable_hsts()`, `is_hsts_enabled()` methods
5. Remove HSTS header writing from `fmt::Display` implementation
6. Remove `is_tls` field from `HttpRequest` struct
7. Remove `set_tls()` method
8. Revert `handle_connection()` changes (remove HSTS enabling logic)
9. Remove all HSTS-related tests
10. Run `cargo test` to verify no regressions

The change is completely reversible with no database migrations or configuration needed.

### Phase 2 Rollback

If TLS integration needs to be reverted:

1. Revert TLS listener changes
2. Restore plain TCP listener
3. Remove TLS crate from `Cargo.toml`
4. Remove TLS detection logic
5. Remove TLS integration tests
6. Keep Phase 1 HSTS infrastructure (now unusable but harmless)
7. Ensure backward compatibility

---

## Dependency Considerations

### Current State

rcomm explicitly has **no external dependencies** (as stated in CLAUDE.md).

### Phase 1 Impact

No impact — Phase 1 uses only Rust standard library (`std::collections::HashMap`, `fmt`, etc.)

### Phase 2 Impact

**BREAKING CHANGE:** Adding TLS support requires external dependency

**Options:**
1. **rustls** — Pure Rust TLS implementation, recommended
   - No system library dependencies
   - Good performance, actively maintained

2. **native-tls** — Wraps OS TLS (OpenSSL/SecureTransport)
   - System dependencies required
   - Generally faster

3. **Pure Rust TLS** — Implement minimal TLS from scratch
   - Aligns with "no dependencies" philosophy
   - Extremely complex, not recommended

**Recommendation:** Accept external dependency for TLS if Phase 2 is approved. Update CLAUDE.md and documentation.

---

## Future Enhancements

### 1. Configurable HSTS Parameters

Allow environment variables to override defaults:

```rust
pub fn hsts_header_value() -> String {
    let max_age = std::env::var("HSTS_MAX_AGE")
        .unwrap_or_else(|_| HSTS_MAX_AGE.to_string())
        .parse::<u32>()
        .unwrap_or(HSTS_MAX_AGE);

    let include_subdomains = std::env::var("HSTS_INCLUDE_SUBDOMAINS")
        .unwrap_or_else(|_| HSTS_INCLUDE_SUBDOMAINS.to_string())
        .parse::<bool>()
        .unwrap_or(HSTS_INCLUDE_SUBDOMAINS);

    // ... build header value ...
}
```

**Environment variables:**
- `HSTS_MAX_AGE` (default: 31536000)
- `HSTS_INCLUDE_SUBDOMAINS` (default: true)
- `HSTS_PRELOAD` (default: true)
- `HSTS_ENABLED` (default: true when TLS detected)

### 2. Per-Domain HSTS Configuration

For multi-domain servers:

```rust
pub struct HstsConfig {
    pub domain: String,
    pub max_age: u32,
    pub include_subdomains: bool,
    pub preload: bool,
}

// Return different header based on Host header
fn get_hsts_for_domain(host: &str) -> Option<String> { /* ... */ }
```

### 3. HSTS Reporting

Implement `report-uri` directive for HSTS violations:

```
Strict-Transport-Security: max-age=31536000; includeSubDomains; preload; report-uri "https://example.com/report"
```

### 4. HSTS Preload Validation

Add utility to validate prerequisites for HSTS preload list:

```rust
pub fn validate_hsts_preload_ready(domain: &str, cert_valid: bool) -> Result<(), String> {
    // Check max-age >= 31536000
    // Check includeSubDomains directive
    // Check preload directive
    // Check certificate validity
}
```

### 5. Security Headers Suite

Extend HSTS with other security headers:

- `Content-Security-Policy`
- `X-Frame-Options`
- `X-Content-Type-Options`
- `Referrer-Policy`
- `Permissions-Policy`

---

## Summary

This implementation plan provides:

1. **Phase 1:** Complete HSTS header support infrastructure in pure Rust, ready for TLS integration
   - Minimal complexity (1/10)
   - Foundation for security improvements
   - No external dependencies
   - Fully testable without TLS

2. **Phase 2:** TLS integration (future work)
   - Requires external TLS library
   - Enables full HSTS functionality
   - Solves HTTP-to-HTTPS security issues

3. **Comprehensive testing:** Unit, integration, and edge case coverage

4. **RFC 6797 compliance:** Implementation matches HTTP Strict Transport Security specification

5. **Future extensibility:** Design supports configuration and multi-domain scenarios

**Recommended Approach:** Implement Phase 1 now to establish the foundation, then schedule Phase 2 when TLS support is approved and dependency constraints are relaxed.

