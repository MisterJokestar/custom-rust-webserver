# Trailing Slash Redirect Implementation Plan

## Overview

Currently, rcomm handles trailing slashes silently via `clean_route()` (line 77-89 in `src/main.rs`), which strips empty segments so that `/howdy/` and `/howdy` resolve to the same route. While functional, this causes SEO issues (duplicate content at two URLs) and inconsistency with HTTP conventions.

This feature adds proper trailing slash handling: when a request arrives with a trailing slash and the canonical route does not have one (or vice versa), the server responds with a `301 Moved Permanently` redirect to the canonical URL instead of serving the content directly.

**Complexity**: 3
**Necessity**: 4

**Key Changes**:
- Detect trailing slash in the original request target (before cleaning)
- After route lookup, if the request had a trailing slash but the canonical route does not, respond with `301` redirect
- Add `Location` header to redirect responses
- Preserve query strings across redirects

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Current State**:
- `clean_route()` (line 77) strips empty segments, making `/howdy/` equivalent to `/howdy`
- `handle_connection()` (line 46) uses `clean_target` for route lookup without distinguishing trailing slash
- No redirect support — no `Location` header logic

**Changes Required**:
- Add trailing slash detection before `clean_route()` is called
- After route lookup succeeds, check if the original target had a trailing slash
- If trailing slash was present on a non-root route, return `301` with `Location` pointing to the non-trailing-slash URL
- Root route `/` is exempt (trailing slash is canonical for root)

### 2. `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`

**Changes Required**:
- Update existing `trailing_slash` test to expect `301` instead of `200`
- Add test for redirect `Location` header
- Add test that root `/` is not redirected

---

## Step-by-Step Implementation

### Step 1: Add Trailing Slash Detection in `handle_connection()`

**Location**: `src/main.rs`, inside `handle_connection()`, after line 58 (`clean_target`)

```rust
    let clean_target = clean_route(&http_request.target);

    // Detect if original request had a trailing slash (and is not the root)
    let original_target = &http_request.target;
    let has_trailing_slash = original_target.len() > 1 && original_target.ends_with('/');
```

### Step 2: Add Redirect Logic Before Route Serving

**Location**: `src/main.rs`, after route lookup (line 62), before file reading (line 70)

```rust
    let (mut response, filename) = if routes.contains_key(&clean_target) {
        // Redirect if trailing slash was present on a non-root route
        if has_trailing_slash && clean_target != "/" {
            let mut redirect_response = HttpResponse::build(String::from("HTTP/1.1"), 301);
            redirect_response.add_header(
                "Location".to_string(),
                clean_target.clone(),
            );
            let body = format!(
                "<html><body>Moved permanently to <a href=\"{clean_target}\">{clean_target}</a></body></html>"
            );
            redirect_response.add_body(body.into());
            println!("Response: {redirect_response}");
            stream.write_all(&redirect_response.as_bytes()).unwrap();
            return;
        }
        (HttpResponse::build(String::from("HTTP/1.1"), 200),
            routes.get(&clean_target).unwrap().to_str().unwrap())
    } else {
        (HttpResponse::build(String::from("HTTP/1.1"), 404),
            "pages/not_found.html")
    };
```

### Step 3: Add 301 Status Phrase to `http_status_codes.rs`

**Location**: `src/models/http_status_codes.rs`

Check if `301` is already mapped. If not, add it:

```rust
301 => "Moved Permanently",
```

### Step 4: Update Integration Tests

**Location**: `src/bin/integration_test.rs`

**Update existing `test_trailing_slash`** to expect redirect:

```rust
fn test_trailing_slash_redirects(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/howdy/")?;
    assert_eq_or_err(&resp.status_code, &301, "status")?;
    let location = resp.headers.get("location")
        .ok_or("missing Location header")?;
    assert_eq_or_err(location, &"/howdy".to_string(), "redirect location")?;
    Ok(())
}

fn test_root_trailing_slash_no_redirect(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    Ok(())
}

fn test_no_trailing_slash_serves_normally(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/howdy")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    Ok(())
}
```

---

## Testing Strategy

### Unit Tests

```rust
#[cfg(test)]
mod trailing_slash_tests {
    #[test]
    fn detect_trailing_slash() {
        let target = "/howdy/";
        assert!(target.len() > 1 && target.ends_with('/'));
    }

    #[test]
    fn root_has_trailing_slash_but_is_exempt() {
        let target = "/";
        assert!(!(target.len() > 1 && target.ends_with('/')));
    }
}
```

### Integration Tests

| Test Name | What It Validates |
|-----------|-------------------|
| `trailing_slash_redirects` | `/howdy/` returns 301 with `Location: /howdy` |
| `root_trailing_slash_no_redirect` | `/` returns 200 (root is exempt) |
| `no_trailing_slash_serves_normally` | `/howdy` returns 200 |

### Manual Testing

```bash
cargo run &
curl -i http://127.0.0.1:7878/howdy/
# Expected: 301, Location: /howdy

curl -i http://127.0.0.1:7878/howdy
# Expected: 200

curl -i -L http://127.0.0.1:7878/howdy/
# Expected: follows redirect, returns 200
```

---

## Edge Cases & Handling

### 1. Root Route `/`
- **Behavior**: No redirect; `/` is the canonical form
- **Status**: Handled by `clean_target != "/"` check

### 2. Multiple Trailing Slashes (`/howdy///`)
- **Behavior**: `clean_route()` strips all empty segments, producing `/howdy`; redirect issued
- **Status**: Handled correctly

### 3. Query Strings (`/howdy/?page=1`)
- **Behavior**: Currently not handled (query string support is a separate feature)
- **Future**: When query string parsing is implemented, preserve query string in redirect `Location`
- **Status**: Deferred

### 4. Non-existent Routes (`/nonexistent/`)
- **Behavior**: `clean_route()` produces `/nonexistent`, route lookup fails, 404 returned (no redirect)
- **Status**: Correct — don't redirect to a non-existent URL

### 5. File Routes (`/index.css/`)
- **Behavior**: Route lookup for `/index.css` succeeds, redirect issued
- **Status**: Correct — canonical URL is `/index.css`

### 6. Existing `trailing_slash` Integration Test
- **Breaking Change**: The existing test (around line 300) expects 200 for `/howdy/`
- **Required Update**: Must update to expect 301
- **Status**: Address in implementation

---

## Implementation Checklist

- [ ] Add trailing slash detection in `handle_connection()`
- [ ] Add 301 redirect logic with `Location` header
- [ ] Verify `301` status phrase exists in `http_status_codes.rs`
- [ ] Update existing `trailing_slash` integration test to expect 301
- [ ] Add `root_trailing_slash_no_redirect` test
- [ ] Add `no_trailing_slash_serves_normally` test
- [ ] Run `cargo test` to verify unit tests
- [ ] Run `cargo run --bin integration_test` to verify integration tests
- [ ] Manual test with `curl -i`

---

## Backward Compatibility

### Breaking Change
- **`GET /path/`** now returns `301` instead of `200`. This is an intentional behavior change.
- Clients that follow redirects (browsers, `curl -L`) will see no difference in final content.
- API clients or scripts that expect `200` from trailing-slash URLs will need to be updated.

### Existing Tests
- The `trailing_slash` integration test must be updated to expect `301` instead of `200`.
- All other tests are unaffected.
