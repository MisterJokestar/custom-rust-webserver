# Integration Tests for Path Traversal Attempts

**Category:** Testing
**Complexity:** 2/10
**Necessity:** 9/10
**Status:** Planning

---

## Overview

Add integration tests to `src/bin/integration_test.rs` that verify the server rejects path traversal attacks. These tests ensure that requests like `GET /../etc/passwd` or `GET /..%2F..%2Fetc/passwd` do not return 200 or leak file contents from outside the `pages/` directory.

**Goal:** Validate that the server's routing and path cleaning logic prevents directory escape, and that no file contents from outside `pages/` are ever served.

**Note:** The current `clean_route()` function (src/main.rs, lines 77-89) strips `..` segments, and `build_routes()` only pre-populates routes from within `pages/`. The server currently relies on the route hashmap for safety (unknown paths return 404). These tests confirm that this defense holds under adversarial input.

---

## Current State

### Route Cleaning (src/main.rs, lines 77-89)

```rust
fn clean_route(route: &String) -> String {
    let mut clean_route = String::from("");
    for part in route.split("/").collect::<Vec<_>>() {
        if part == "" || part == "." || part == ".." {
            continue;
        }
        clean_route.push_str(format!("/{part}").as_str());
    }
    if clean_route == "" {
        clean_route = String::from("/");
    }
    clean_route
}
```

This strips `..` and `.` segments from the route. Combined with the hashmap-based routing (only known routes return 200), the server should reject traversal attempts with 404.

### Why These Tests Are Critical (Necessity: 9/10)

Path traversal is a top OWASP vulnerability. Even though the current architecture provides protection via the route hashmap, these tests:
1. **Document** the expected security behavior
2. **Guard against regressions** if the routing logic changes
3. **Validate defense-in-depth** before any canonicalization feature is added
4. **Cover encoded variants** that might bypass string-level filtering

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`

**Add:** Path traversal test functions.
**Modify:** `main()` function to register new tests.

---

## Step-by-Step Implementation

### Step 1: Add Test Functions

```rust
fn test_path_traversal_dotdot(addr: &str) -> Result<(), String> {
    // Basic ../ traversal attempt
    let resp = send_request(addr, "GET", "/../etc/passwd")?;
    if resp.status_code == 200 {
        return Err("Path traversal succeeded! Got 200 for /../etc/passwd".to_string());
    }
    // Accept 400, 403, or 404 — any non-200 is acceptable
    Ok(())
}

fn test_path_traversal_deep(addr: &str) -> Result<(), String> {
    // Multiple levels of traversal
    let resp = send_request(addr, "GET", "/../../../../../../etc/passwd")?;
    if resp.status_code == 200 {
        return Err("Deep path traversal succeeded!".to_string());
    }
    Ok(())
}

fn test_path_traversal_dotdot_in_middle(addr: &str) -> Result<(), String> {
    // Traversal embedded within a seemingly valid path
    let resp = send_request(addr, "GET", "/howdy/../../etc/passwd")?;
    if resp.status_code == 200 {
        return Err("Mid-path traversal succeeded!".to_string());
    }
    Ok(())
}

fn test_path_traversal_src_main(addr: &str) -> Result<(), String> {
    // Attempt to read the server's own source code
    let resp = send_request(addr, "GET", "/../src/main.rs")?;
    if resp.status_code == 200 {
        return Err("Source code leak via path traversal!".to_string());
    }
    // Also check the body doesn't contain Rust source markers
    if resp.body.contains("fn main()") || resp.body.contains("use std::") {
        return Err("Response body contains Rust source code!".to_string());
    }
    Ok(())
}

fn test_path_traversal_cargo_toml(addr: &str) -> Result<(), String> {
    // Attempt to read Cargo.toml
    let resp = send_request(addr, "GET", "/../Cargo.toml")?;
    if resp.status_code == 200 && resp.body.contains("[package]") {
        return Err("Cargo.toml leaked via path traversal!".to_string());
    }
    Ok(())
}

fn test_path_traversal_dot_segment(addr: &str) -> Result<(), String> {
    // Single dot segments should be harmless
    let resp = send_request(addr, "GET", "/./././")?;
    // Should resolve to / and return 200 (or 404 if handling differs)
    // The important thing is it doesn't crash
    if resp.status_code != 200 && resp.status_code != 404 {
        return Err(format!("Unexpected status {} for dot segments", resp.status_code));
    }
    Ok(())
}

fn test_path_traversal_mixed_slashes(addr: &str) -> Result<(), String> {
    // Mixed forward slashes and traversal
    let resp = send_request(addr, "GET", "/..//..//..//etc/passwd")?;
    if resp.status_code == 200 {
        return Err("Mixed-slash traversal succeeded!".to_string());
    }
    Ok(())
}

fn test_valid_route_still_works_after_traversal_attempts(addr: &str) -> Result<(), String> {
    // Send several traversal attempts, then verify normal service
    let _ = send_request(addr, "GET", "/../etc/passwd");
    let _ = send_request(addr, "GET", "/../../etc/shadow");
    let _ = send_request(addr, "GET", "/../src/main.rs");
    let resp = send_request(addr, "GET", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "status after traversal attempts")?;
    assert_contains_or_err(&resp.body, "Hello!", "body after traversal attempts")?;
    Ok(())
}
```

### Step 2: Register Tests in `main()`

Add to the `results` vector in `main()`:

```rust
run_test("path_traversal_dotdot", || test_path_traversal_dotdot(&addr)),
run_test("path_traversal_deep", || test_path_traversal_deep(&addr)),
run_test("path_traversal_dotdot_in_middle", || test_path_traversal_dotdot_in_middle(&addr)),
run_test("path_traversal_src_main", || test_path_traversal_src_main(&addr)),
run_test("path_traversal_cargo_toml", || test_path_traversal_cargo_toml(&addr)),
run_test("path_traversal_dot_segment", || test_path_traversal_dot_segment(&addr)),
run_test("path_traversal_mixed_slashes", || test_path_traversal_mixed_slashes(&addr)),
run_test("valid_route_after_traversal", || test_valid_route_still_works_after_traversal_attempts(&addr)),
```

---

## Edge Cases & Considerations

### 1. URL Encoding

**Scenario:** `%2e%2e` is the URL-encoded form of `..`. If the server decodes URIs before routing, encoded traversals could bypass `clean_route()`.

**Current behavior:** The server does NOT percent-decode URIs (that feature doesn't exist yet). So `%2e%2e` is treated literally and won't match any route → 404.

**Note:** When percent-decoding is implemented, these tests should be expanded with encoded variants. A separate test file or updated tests should cover `/%2e%2e/etc/passwd`.

### 2. Null Bytes

**Scenario:** `GET /pages%00/../etc/passwd` — null bytes can truncate paths in some systems.

**Current behavior:** The server doesn't process null bytes specially. They'd be part of the route string, which won't match any route → 404.

**Note:** Null byte rejection is a separate feature in the Security category.

### 3. Windows Backslashes

**Scenario:** `GET /..\..\..\etc\passwd` — backslash traversal.

**Current behavior:** The server only splits on `/`, so `..\..\` is treated as a single segment and won't match any route → 404.

### 4. Symlinks

**Scenario:** A symlink inside `pages/` pointing to `../src/main.rs`.

**Behavior:** The route hashmap would contain the symlink as a valid route, and `fs::read_to_string()` would follow it. This is outside the scope of these tests (handled by the path canonicalization feature).

---

## Testing Strategy

### Running the Tests

```bash
cargo build && cargo run --bin integration_test
```

### Expected Results

All traversal tests should show `[PASS]`. Every traversal attempt should receive a non-200 status (typically 404). The server should remain functional after all attempts.

### Manual Verification

```bash
cargo run &

curl -i http://127.0.0.1:7878/../etc/passwd
# Expected: HTTP/1.1 404 Not Found

curl -i http://127.0.0.1:7878/../../../../../../etc/passwd
# Expected: HTTP/1.1 404 Not Found

curl -i http://127.0.0.1:7878/../src/main.rs
# Expected: HTTP/1.1 404 Not Found

# Verify server still works
curl -i http://127.0.0.1:7878/
# Expected: HTTP/1.1 200 OK
```

---

## Implementation Checklist

- [ ] Add `test_path_traversal_dotdot()` test
- [ ] Add `test_path_traversal_deep()` test
- [ ] Add `test_path_traversal_dotdot_in_middle()` test
- [ ] Add `test_path_traversal_src_main()` test
- [ ] Add `test_path_traversal_cargo_toml()` test
- [ ] Add `test_path_traversal_dot_segment()` test
- [ ] Add `test_path_traversal_mixed_slashes()` test
- [ ] Add `test_valid_route_still_works_after_traversal_attempts()` test
- [ ] Register all tests in `main()`
- [ ] Run `cargo build` — no compiler errors
- [ ] Run `cargo run --bin integration_test` — all tests pass

---

## Related Features

- **Security > Path Traversal Prevention**: The canonicalization-based defense-in-depth feature. These tests validate the current string-based protection and will also serve as regression tests for the canonicalization implementation.
- **HTTP Protocol > Percent-Decode URI**: When implemented, additional encoded-traversal tests should be added.
- **Security > Null Byte Rejection**: Complementary security tests.

---

## References

- [OWASP Path Traversal](https://owasp.org/www-community/attacks/Path_Traversal)
- [CWE-22: Improper Limitation of a Pathname](https://cwe.mitre.org/data/definitions/22.html)
- [HackTricks: File Inclusion / Path Traversal](https://book.hacktricks.xyz/pentesting-web/file-inclusion)
