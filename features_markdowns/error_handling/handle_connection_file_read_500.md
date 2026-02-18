# Replace `unwrap()` in `handle_connection()` File Read with Fallback to 500 Internal Server Error

**Feature**: Replace `unwrap()` in `handle_connection()` file read with fallback to `500 Internal Server Error`
**Category**: Error Handling
**Complexity**: 2/10
**Necessity**: 10/10

---

## Overview

The `handle_connection()` function in `src/main.rs` currently panics when a file read fails, killing the worker thread. This is the highest-necessity error handling fix in the project because it sits directly in the hot path — every single request that matches a route passes through `fs::read_to_string(filename).unwrap()` at line 70. If the file is missing (deleted after route building), has wrong permissions, or encounters any I/O error, the worker thread dies silently without sending any response to the client.

### Current State

**`src/main.rs` line 70**:
```rust
let contents = fs::read_to_string(filename).unwrap();
response.add_body(contents.into());
```

There are also two related unwraps on line 64 in the route lookup:
```rust
(HttpResponse::build(String::from("HTTP/1.1"), 200),
    routes.get(&clean_target).unwrap().to_str().unwrap())
```

### Impact of Current Behavior
- A single missing or unreadable file crashes a worker thread
- The client receives no response (connection reset)
- The thread pool permanently loses one worker (no recovery)
- Under repeated failures, the server becomes completely unresponsive

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Line 62-68**: Route lookup with double `unwrap()` — `routes.get()` and `.to_str()`
**Line 70**: `fs::read_to_string(filename).unwrap()` — file read panic

### 2. `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`

Add integration test verifying the server survives and responds after encountering errors.

---

## Step-by-Step Implementation

### Step 1: Replace Route Lookup Unwraps

**Current code** (lines 62-68):
```rust
let (mut response, filename) = if routes.contains_key(&clean_target) {
    (HttpResponse::build(String::from("HTTP/1.1"), 200),
        routes.get(&clean_target).unwrap().to_str().unwrap())
} else {
    (HttpResponse::build(String::from("HTTP/1.1"), 404),
        "pages/not_found.html")
};
```

**Updated code**:
```rust
let (mut response, filename) = if let Some(path_buf) = routes.get(&clean_target) {
    match path_buf.to_str() {
        Some(path_str) => (HttpResponse::build(String::from("HTTP/1.1"), 200), path_str),
        None => {
            eprintln!("Path contains invalid UTF-8: {:?}", path_buf);
            (HttpResponse::build(String::from("HTTP/1.1"), 500), "pages/not_found.html")
        }
    }
} else {
    (HttpResponse::build(String::from("HTTP/1.1"), 404),
        "pages/not_found.html")
};
```

**Rationale**: Uses `if let Some()` instead of `contains_key()` + `get().unwrap()`, eliminating both unwraps. Invalid UTF-8 paths are treated as a 500 error.

### Step 2: Replace File Read Unwrap

**Current code** (line 70):
```rust
let contents = fs::read_to_string(filename).unwrap();
response.add_body(contents.into());
```

**Updated code**:
```rust
let contents = match fs::read_to_string(filename) {
    Ok(c) => c,
    Err(e) => {
        eprintln!("Error reading file {filename}: {e}");
        let mut error_response = HttpResponse::build(String::from("HTTP/1.1"), 500);
        error_response.add_body("Internal Server Error".to_string().into());
        let _ = stream.write_all(&error_response.as_bytes());
        return;
    }
};
response.add_body(contents.into());
```

**Rationale**:
- Logs the specific filename and OS error (e.g., "Permission denied", "No such file or directory")
- Sends a 500 Internal Server Error to the client
- Uses `let _ =` on the error response write since the connection may already be broken
- Returns early so the function exits cleanly

---

## Edge Cases

### 1. File Deleted Between Routing and Serving
**Scenario**: `build_routes()` discovers `pages/howdy/page.html`, but it's deleted before a request arrives.
**Handling**: `fs::read_to_string()` returns `Err(NotFound)`. Server logs the error and returns 500 to the client. Worker thread survives.

### 2. Permission Change After Startup
**Scenario**: File permissions are changed to 000 after the server starts.
**Handling**: `fs::read_to_string()` returns `Err(PermissionDenied)`. Same graceful 500 response.

### 3. Symlink Target Missing
**Scenario**: A route points to a symlink whose target no longer exists.
**Handling**: Same as file deletion — `read_to_string()` fails, error is logged, 500 returned.

### 4. Path with Invalid UTF-8
**Scenario**: A `PathBuf` in the routes map contains non-UTF-8 bytes.
**Handling**: `to_str()` returns `None`. Logged and treated as 500. Falls back to `not_found.html` path (which is always valid UTF-8).

### 5. 500 Response Write Also Fails
**Scenario**: The file read fails AND the client already disconnected.
**Handling**: File read error is logged. The 500 response write uses `let _ =` to silently ignore the write error. Both errors are independent.

---

## Testing Strategy

### Integration Tests

```rust
fn test_server_survives_after_errors(addr: &str) -> Result<(), String> {
    // First, request a known-bad route to trigger 404 (not_found.html read)
    let resp = send_request(addr, "GET", "/does-not-exist")?;
    assert_eq_or_err(&resp.status_code, &404, "404 status")?;

    // Then verify server is still alive and serving
    let resp = send_request(addr, "GET", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "200 status after error")?;
    Ok(())
}
```

### Manual Testing

```bash
cargo run &

# Test file read error (temporarily change file permissions)
chmod 000 pages/howdy/page.html
curl -i http://127.0.0.1:7878/howdy
# Expected: HTTP/1.1 500 Internal Server Error
# Server logs: Error reading file pages/howdy/page.html — Permission denied
chmod 644 pages/howdy/page.html

# Verify server still works after error
curl -i http://127.0.0.1:7878/
# Expected: HTTP/1.1 200 OK
```

---

## Implementation Checklist

- [ ] Replace `routes.contains_key()` + `get().unwrap()` with `if let Some()` pattern
- [ ] Replace `path_buf.to_str().unwrap()` with `match` and 500 fallback
- [ ] Replace `fs::read_to_string(filename).unwrap()` with `match` returning 500
- [ ] Log error with filename and OS error message
- [ ] Verify 500 status code exists in `http_status_codes.rs` (it does — line 55)
- [ ] Add integration test for server resilience
- [ ] Run `cargo test` — no regressions
- [ ] Run `cargo run --bin integration_test` — all tests pass

---

## Backward Compatibility

No external API changes. The only behavioral change is that file read errors now return a 500 response instead of crashing the worker thread — a strictly better outcome for clients and server stability.

---

## Related Features

- **Security > Replace All unwrap() Calls**: This feature is a subset of that broader effort
- **Error Handling > Log and Recover from TCP Write Errors**: The 500 response write uses `let _ =` which overlaps
- **Error Handling > Structured Error Responses**: The 500 body is plain text; the structured responses feature would make it HTML/JSON
