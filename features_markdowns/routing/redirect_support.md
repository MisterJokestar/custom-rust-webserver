# Redirect Support Implementation Plan

## Overview

HTTP redirects instruct clients to request a different URL. This feature adds configurable redirect rules that return `301`, `302`, `307`, or `308` status codes with a `Location` header, allowing operators to define URL redirections without code changes.

Unlike route aliases (which are server-side rewrites invisible to the client), redirects send an explicit HTTP response telling the client to navigate to a new URL. This is essential for URL migration, domain changes, and vanity URLs.

**Complexity**: 4
**Necessity**: 4

**Key Changes**:
- Define a redirect configuration format via environment variable
- Parse redirect rules at startup
- Check redirects before route lookup in `handle_connection()`
- Support `301` (permanent), `302` (found), `307` (temporary), and `308` (permanent, preserves method) status codes
- Include proper `Location` header and minimal HTML body

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Current State**:
- No redirect mechanism exists
- `handle_connection()` (line 46) only returns 200 or 404
- No `Location` header support (though `HttpResponse::add_header()` can set arbitrary headers)

**Changes Required**:
- Add redirect rule parsing
- Add redirect checking in `handle_connection()` before route lookup
- Build and send redirect responses with `Location` header

### 2. `/home/jwall/personal/rusty/rcomm/src/models/http_status_codes.rs`

**Changes Required**:
- Ensure status codes 301, 302, 307, 308 are mapped to their phrases

### 3. `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`

**Changes Required**:
- Add integration tests for redirect responses

---

## Step-by-Step Implementation

### Step 1: Define Redirect Configuration Format

Use `RCOMM_REDIRECTS` environment variable with semicolon-separated rules:

```bash
RCOMM_REDIRECTS="/old=/new:301;/temp=/other:302;/legacy=/modern:308"
```

Format: `source=target:status_code`

Default status code is `301` if omitted:
```bash
RCOMM_REDIRECTS="/old=/new"  # Defaults to 301
```

### Step 2: Add Redirect Rule Type and Parser

**Location**: `src/main.rs`, before `main()`

```rust
#[derive(Clone)]
struct RedirectRule {
    source: String,
    target: String,
    status_code: u16,
}

/// Parse redirect rules from RCOMM_REDIRECTS environment variable.
fn parse_redirects() -> Vec<RedirectRule> {
    let redirect_str = match std::env::var("RCOMM_REDIRECTS") {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    redirect_str
        .split(';')
        .filter_map(|rule| {
            let rule = rule.trim();
            if rule.is_empty() {
                return None;
            }

            // Split into source=target:status
            let parts: Vec<&str> = rule.splitn(2, '=').collect();
            if parts.len() != 2 {
                eprintln!("Warning: invalid redirect rule: '{rule}'");
                return None;
            }

            let source = parts[0].trim().to_string();
            let target_status = parts[1].trim();

            // Check for :status_code suffix
            let (target, status_code) = if let Some(colon_pos) = target_status.rfind(':') {
                let maybe_status = &target_status[colon_pos + 1..];
                if let Ok(code) = maybe_status.parse::<u16>() {
                    if [301, 302, 307, 308].contains(&code) {
                        (target_status[..colon_pos].to_string(), code)
                    } else {
                        eprintln!("Warning: unsupported redirect status {code}, using 301");
                        (target_status[..colon_pos].to_string(), 301)
                    }
                } else {
                    // Colon is part of the URL (e.g., https://example.com)
                    (target_status.to_string(), 301)
                }
            } else {
                (target_status.to_string(), 301)
            };

            Some(RedirectRule { source, target, status_code })
        })
        .collect()
}
```

### Step 3: Add Redirect Checking in `handle_connection()`

**Location**: `src/main.rs`, inside `handle_connection()`, after `clean_route()` (after line 58)

```rust
    let clean_target = clean_route(&http_request.target);

    // Check for redirects before route lookup
    for redirect in &redirects {
        if redirect.source == clean_target {
            let mut response = HttpResponse::build(
                String::from("HTTP/1.1"),
                redirect.status_code,
            );
            response.add_header("Location".to_string(), redirect.target.clone());
            let body = format!(
                "<html><body><p>Redirecting to <a href=\"{}\">{}</a></p></body></html>",
                redirect.target, redirect.target
            );
            response.add_body(body.into());
            println!("Response: {response}");
            stream.write_all(&response.as_bytes()).unwrap();
            return;
        }
    }
```

### Step 4: Update `handle_connection()` Signature

```rust
fn handle_connection(
    mut stream: TcpStream,
    routes: HashMap<String, PathBuf>,
    redirects: Vec<RedirectRule>,
) {
```

### Step 5: Update `main()`

```rust
    let redirects = parse_redirects();
    if !redirects.is_empty() {
        println!("Loaded {} redirect rule(s)", redirects.len());
    }

    for stream in listener.incoming() {
        let routes_clone = routes.clone();
        let redirects_clone = redirects.clone();
        let stream = stream.unwrap();

        pool.execute(move || {
            handle_connection(stream, routes_clone, redirects_clone);
        });
    }
```

### Step 6: Verify Status Code Phrases

**Location**: `src/models/http_status_codes.rs`

Ensure these entries exist:
```rust
301 => "Moved Permanently",
302 => "Found",
307 => "Temporary Redirect",
308 => "Permanent Redirect",
```

---

## Testing Strategy

### Unit Tests

```rust
#[cfg(test)]
mod redirect_tests {
    use super::*;

    #[test]
    fn redirect_rule_matches_source() {
        let rule = RedirectRule {
            source: "/old".to_string(),
            target: "/new".to_string(),
            status_code: 301,
        };
        assert_eq!(rule.source, "/old");
        assert_eq!(rule.target, "/new");
        assert_eq!(rule.status_code, 301);
    }
}
```

### Integration Tests

```rust
fn test_redirect_returns_301(addr: &str) -> Result<(), String> {
    // Requires RCOMM_REDIRECTS="/redirect-test=/howdy:301" on server
    let resp = send_request(addr, "GET", "/redirect-test")?;
    assert_eq_or_err(&resp.status_code, &301, "status")?;
    let location = resp.headers.get("location")
        .ok_or("missing Location header")?;
    assert_eq_or_err(location, &"/howdy".to_string(), "location")?;
    Ok(())
}
```

### Manual Testing

```bash
RCOMM_REDIRECTS="/old=/new:301;/temp=/other:302" cargo run &
curl -i http://127.0.0.1:7878/old
# Expected: 301 Moved Permanently, Location: /new

curl -i http://127.0.0.1:7878/temp
# Expected: 302 Found, Location: /other

curl -i -L http://127.0.0.1:7878/old
# Expected: follows redirect, returns /new content
```

---

## Edge Cases & Handling

### 1. Redirect Target is External URL
- **Example**: `RCOMM_REDIRECTS="/blog=https://blog.example.com:301"`
- **Behavior**: `Location` header set to external URL; works correctly
- **Note**: The `:301` suffix parser must handle URLs containing colons (e.g., `https:`) — the parser uses `rfind(':')` to find the last colon

### 2. Redirect Source Matches a Real Route
- **Behavior**: Redirect takes priority (checked before route lookup)
- **Status**: Intentional

### 3. Redirect Chain (A → B → C)
- **Behavior**: Client follows each redirect; server only handles one at a time
- **Status**: Correct — each request is independent

### 4. Redirect Loop (A → B → A)
- **Behavior**: Client detects loop and stops (browsers show "too many redirects" error)
- **Status**: Not preventable server-side; operator's responsibility

### 5. Missing Status Code
- **Behavior**: Defaults to `301`
- **Status**: Handled in parser

### 6. Unsupported Status Code
- **Example**: `RCOMM_REDIRECTS="/a=/b:303"`
- **Behavior**: Warning printed, defaults to `301`
- **Status**: Handled in parser

---

## Implementation Checklist

- [ ] Add `RedirectRule` struct
- [ ] Add `parse_redirects()` function
- [ ] Add redirect checking in `handle_connection()` before route lookup
- [ ] Update `handle_connection()` signature to accept redirects
- [ ] Update `main()` to parse and pass redirect rules
- [ ] Verify 301/302/307/308 status phrases in `http_status_codes.rs`
- [ ] Add unit tests for redirect rules
- [ ] Add integration tests
- [ ] Run `cargo test` and `cargo run --bin integration_test`

---

## Backward Compatibility

When `RCOMM_REDIRECTS` is not set, no redirects are loaded and behavior is identical to current. All existing tests pass unchanged.
