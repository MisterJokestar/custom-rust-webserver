# Implementation Plan: Log Error Details for File Reads and Connection Handling

## 1. Overview of the Feature

Detailed error logging is essential for diagnosing operational issues: missing files, permission errors, broken pipes, and unexpected failures. Without specific error information, operators must guess at root causes from generic error messages or silent failures.

**Current State**: The server has several error-handling gaps:

1. **File read errors** (`src/main.rs:70`): `fs::read_to_string(filename).unwrap()` panics on any file read failure (permission denied, file deleted between routing and serving, etc.), killing the worker thread.

2. **Response write errors** (`src/main.rs:74`): `stream.write_all(&response.as_bytes()).unwrap()` panics if the client disconnects mid-response, again killing the worker thread.

3. **Bad request errors** (`src/main.rs:50`): `eprintln!("Bad request: {e}")` logs the error but doesn't include the client IP or any connection context.

4. **Route building errors** (`src/main.rs:94-97`): `entry.unwrap()` and `path.extension().unwrap()` panic on filesystem errors or extensionless files during startup.

**Desired State**: All error conditions are logged with:
- Error category (file read, connection, parse, etc.)
- Specific error message from the OS or parser
- Context (filename, client IP, route path)
- No panics — errors are logged and the server continues operating

Example error log lines:
```
[ERROR] File read failed: pages/howdy/page.html — Permission denied (os error 13)
[ERROR] Response write failed to 127.0.0.1:54321: Broken pipe (os error 32)
[WARN] Bad request from 127.0.0.1:54321: Malformed request line
[WARN] Skipping file during route build: pages/.hidden — no extension
```

**Impact**:
- Eliminates worker thread panics from file/connection errors
- Provides actionable diagnostic information for operators
- Improves server reliability (one bad file or client doesn't affect others)
- Overlaps with the "Replace unwrap() calls" security feature

---

## 2. Files to be Modified or Created

### Modified Files

1. **`/home/jwall/personal/rusty/rcomm/src/main.rs`**
   - Replace `fs::read_to_string(filename).unwrap()` with error handling that logs and returns 500
   - Replace `stream.write_all(&response.as_bytes()).unwrap()` with error handling that logs
   - Add client IP context to bad request error logging
   - Replace `unwrap()` calls in `build_routes()` with logged fallbacks

2. **`/home/jwall/personal/rusty/rcomm/src/models/http_status_codes.rs`**
   - Add `500` status code mapping if not already present (verify)

### No New Files Required

This feature modifies existing error handling in place. If the logger feature exists, error messages use `logger::error()` and `logger::warn()`. Otherwise, `eprintln!()` is used directly.

---

## 3. Step-by-Step Implementation Details

### Step 1: Handle File Read Errors in handle_connection()

**File**: `/home/jwall/personal/rusty/rcomm/src/main.rs`

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
        eprintln!("[ERROR] File read failed: {filename} — {e}");
        let mut error_response = HttpResponse::build(String::from("HTTP/1.1"), 500);
        let body = String::from("Internal Server Error");
        error_response.add_body(body.into());
        let _ = stream.write_all(&error_response.as_bytes());
        return;
    }
};
response.add_body(contents.into());
```

**Rationale**:
- Logs the specific filename and OS error (e.g., "Permission denied")
- Returns a 500 Internal Server Error to the client instead of panicking
- Uses `let _ =` for the error response write since the connection may already be broken

### Step 2: Handle Response Write Errors

**File**: `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Current code** (line 74):
```rust
stream.write_all(&response.as_bytes()).unwrap();
```

**Updated code**:
```rust
if let Err(e) = stream.write_all(&response.as_bytes()) {
    eprintln!("[ERROR] Response write failed: {e}");
}
```

**Rationale**:
- Client may disconnect mid-response (broken pipe, connection reset)
- Logging the error helps identify network issues
- No response can be sent at this point — just log and return

### Step 3: Add Context to Bad Request Logging

**File**: `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Current code** (line 50):
```rust
eprintln!("Bad request: {e}");
```

**Updated code** (assumes `peer` variable from client IP logging feature):
```rust
eprintln!("[WARN] Bad request from {peer}: {e}");
```

If client IP logging is not yet implemented:
```rust
let peer = stream.peer_addr()
    .map(|a| a.to_string())
    .unwrap_or_else(|_| "unknown".to_string());
eprintln!("[WARN] Bad request from {peer}: {e}");
```

### Step 4: Handle Errors in build_routes()

**File**: `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Current code** (lines 91–123):
```rust
fn build_routes(route: String, directory: &Path) -> HashMap<String, PathBuf> {
    let mut routes: HashMap<String, PathBuf> = HashMap::new();

    for entry in fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let name = path.file_name().unwrap().to_str().unwrap();
        if path.is_dir() {
            routes.extend(
                build_routes(format!("{route}/{name}"), &path)
            );
        } else if path.is_file() {
            match path.extension().unwrap().to_str().unwrap() {
                // ...
            }
        }
    }

    routes
}
```

**Updated code**:
```rust
fn build_routes(route: String, directory: &Path) -> HashMap<String, PathBuf> {
    let mut routes: HashMap<String, PathBuf> = HashMap::new();

    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(e) => {
            eprintln!("[ERROR] Failed to read directory {}: {e}", directory.display());
            return routes;
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                eprintln!("[WARN] Failed to read directory entry in {}: {e}", directory.display());
                continue;
            }
        };
        let path = entry.path();
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => {
                eprintln!("[WARN] Skipping file with invalid name: {}", path.display());
                continue;
            }
        };
        if path.is_dir() {
            routes.extend(
                build_routes(format!("{route}/{name}"), &path)
            );
        } else if path.is_file() {
            let ext = match path.extension().and_then(|e| e.to_str()) {
                Some(e) => e,
                None => {
                    eprintln!("[WARN] Skipping file without extension: {}", path.display());
                    continue;
                }
            };
            match ext {
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
                _ => { continue; }
            }
        }
    }

    routes
}
```

**Rationale**:
- Replaces 4 `unwrap()` calls with logged error handling
- Continues processing remaining files when one entry fails
- Logs specific file paths and error messages
- Server starts even if some files have issues (graceful degradation)

---

## 4. Code Snippets and Pseudocode

### Error-Resilient Connection Handler

```
FUNCTION handle_connection(stream, routes)
    LET peer = stream.peer_addr() OR "unknown"
    LET request = TRY parse_request(stream)
        CATCH error:
            LOG WARN "[{peer}] Bad request: {error}"
            SEND 400 response (ignore write errors)
            RETURN

    LET filename = route_lookup(request, routes)
    LET contents = TRY read_file(filename)
        CATCH error:
            LOG ERROR "File read failed: {filename} — {error}"
            SEND 500 response (ignore write errors)
            RETURN

    LET response = build_response(contents)
    TRY stream.write(response)
        CATCH error:
            LOG ERROR "[{peer}] Response write failed: {error}"
END FUNCTION
```

---

## 5. Testing Strategy

### Unit Tests

No new unit tests needed for the logging changes themselves. However, the error handling behavior (returning 500 instead of panicking) should be tested:

```rust
// In integration_test.rs
fn test_500_on_missing_file(addr: &str) -> Result<(), String> {
    // This requires a route that maps to a file that gets deleted after route building.
    // Difficult to test without modifying the server. Deferred to manual testing.
    Ok(())
}
```

### Integration Tests

Add a test that verifies the server doesn't crash on a bad write:

```rust
fn test_server_survives_client_disconnect(addr: &str) -> Result<(), String> {
    // Connect and immediately close without reading the response
    let mut stream = TcpStream::connect(addr)
        .map_err(|e| format!("connect: {e}"))?;
    let request = "GET / HTTP/1.1\r\nHost: localhost\r\n\r\n";
    stream.write_all(request.as_bytes())
        .map_err(|e| format!("write: {e}"))?;
    // Close immediately without reading response
    drop(stream);

    // Verify server is still alive by making another request
    std::thread::sleep(std::time::Duration::from_millis(100));
    let resp = send_request(addr, "GET", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "status after disconnect")?;
    Ok(())
}
```

### Manual Testing

```bash
cargo run

# Test file read error (temporarily change file permissions)
chmod 000 pages/howdy/page.html
curl http://127.0.0.1:7878/howdy
# Server should log: [ERROR] File read failed: pages/howdy/page.html — Permission denied
# Client should receive: 500 Internal Server Error
chmod 644 pages/howdy/page.html

# Test client disconnect (broken pipe)
curl --max-time 0.001 http://127.0.0.1:7878/
# Server should log: [ERROR] Response write failed: Broken pipe

# Verify server still works after errors
curl http://127.0.0.1:7878/
# Should return 200 OK
```

---

## 6. Edge Cases to Consider

### Case 1: File Deleted Between Routing and Serving
**Scenario**: `build_routes()` discovers `pages/howdy/page.html`, but it's deleted before a request arrives
**Handling**: `fs::read_to_string()` returns `Err(NotFound)`. Server logs the error and returns 500.

### Case 2: Symlink Target Missing
**Scenario**: A route points to a symlink whose target no longer exists
**Handling**: Same as file deletion — `read_to_string()` fails, error is logged, 500 returned.

### Case 3: Client TCP RST
**Scenario**: Client sends RST packet during response transmission
**Handling**: `write_all()` returns `Err(ConnectionReset)`. Error is logged, function returns normally.

### Case 4: Partial Write Success
**Scenario**: Part of the response is sent before the connection breaks
**Handling**: `write_all()` returns an error after sending partial data. Client receives truncated response. Error is logged server-side.

### Case 5: Directory Permission Error During Route Build
**Scenario**: A subdirectory of `pages/` is not readable
**Handling**: `fs::read_dir()` returns `Err(PermissionDenied)`. Error is logged, and routes for that directory are skipped. Server starts with reduced route set.

### Case 6: File with Non-UTF8 Name
**Scenario**: A file in `pages/` has a name containing non-UTF8 bytes
**Handling**: `file_name().to_str()` returns `None`. Warning is logged and file is skipped.

### Case 7: Cascading Errors
**Scenario**: File read fails, then 500 response write also fails
**Handling**: File read error is logged. 500 response write uses `let _ =` which silently ignores the write error. Both errors are independent.

---

## 7. Implementation Checklist

- [ ] Replace `fs::read_to_string(filename).unwrap()` with error handling:
  - [ ] Log error with filename and OS error message
  - [ ] Return 500 Internal Server Error to client
- [ ] Replace `stream.write_all(&response.as_bytes()).unwrap()` with error handling:
  - [ ] Log write errors with client IP context
  - [ ] Continue execution (don't panic)
- [ ] Add client IP context to bad request error log
- [ ] Replace `unwrap()` calls in `build_routes()`:
  - [ ] `fs::read_dir()` — log and return empty routes
  - [ ] `entry.unwrap()` — log and skip
  - [ ] `file_name().unwrap().to_str().unwrap()` — log and skip
  - [ ] `extension().unwrap()` — log and skip
- [ ] Verify 500 status code is in `http_status_codes.rs`
- [ ] Add integration test for server survival after client disconnect
- [ ] Run `cargo test` — no regressions
- [ ] Run `cargo run --bin integration_test` — integration tests pass
- [ ] Manual testing: file permission errors, client disconnects

---

## 8. Complexity and Risk Analysis

**Complexity**: 2/10
- Replacing `unwrap()` with `match` is mechanical
- Error logging is simple string formatting
- Returning 500 instead of panicking is straightforward

**Risk**: Low-Medium
- Changing `unwrap()` to error handling changes behavior from "panic" to "log and continue"
- This is a strictly better behavior but is technically a behavioral change
- Risk of missing an `unwrap()` — review all call sites carefully
- Risk of incorrect error handling logic (e.g., forgetting to `return` after sending 500)

**Dependencies**: None
- Uses only `std::fs`, `std::io`, `std::net`
- No new crates needed

---

## 9. Future Enhancements

1. **Error Rate Tracking**: Count errors per type per time window for alerting
2. **Custom Error Pages**: Serve a styled 500.html page instead of plain text
3. **Error Response Bodies**: Include more detail in 500 responses (in debug mode only)
4. **Retry Logic**: For transient file read errors, retry once before returning 500
5. **Circuit Breaker**: If a file repeatedly fails, temporarily remove it from routes
