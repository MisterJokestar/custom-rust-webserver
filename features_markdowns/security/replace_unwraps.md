# Replace All `unwrap()` Calls with Proper Error Handling

**Feature**: Replace all `unwrap()` calls with proper error handling to prevent worker thread panics
**Category**: Security
**Complexity**: 4/10
**Necessity**: 10/10

---

## Overview

The rcomm server currently uses 67 `unwrap()` calls throughout the codebase, creating critical vulnerabilities where any I/O operation failure, file system error, or parsing issue will cause worker threads to panic. This defeats the purpose of the multi-threaded architecture, as a single malformed request or missing file can crash the entire server.

### Current State
- **Total unwrap() calls**: 67
- **In src/main.rs**: 10 (critical path operations)
- **In src/lib.rs**: 3 (thread pool execution)
- **In src/models/http_request.rs**: 27+ (mostly in tests)
- **In src/models/http_response.rs**: 3 (mostly in tests)

### Security Impact
When a worker thread panics due to an unwrap(), it exits silently without:
1. Logging the cause to the client
2. Sending an error response
3. Gracefully handling the connection
4. Preserving server stability

This could be exploited to create denial-of-service (DoS) conditions.

---

## Files to Modify

### Production Code (Priority 1)
1. **src/main.rs** (10 critical unwraps)
   - Line 26: `TcpListener::bind()` unwrap
   - Line 38: `listener.incoming()` stream unwrap
   - Line 64: Route lookup and path conversion unwraps
   - Line 70: `fs::read_to_string()` unwrap
   - Line 74: `stream.write_all()` unwrap
   - Lines 94, 95, 97, 103: Directory traversal unwraps in `build_routes()`

2. **src/lib.rs** (3 unwraps in thread pool)
   - Line 43: `sender.send()` unwrap (execute method)
   - Line 54: `worker.thread.join()` unwrap (Drop trait)
   - Line 63: `receiver.lock()` unwrap (worker loop)

### Test Code (Priority 2)
3. **src/models/http_request.rs** (27+ unwraps in tests)
   - Lines 238, 251: `String::from_utf8()` unwraps
   - Lines 261-278, 286-307, etc.: Test setup unwraps

4. **src/models/http_response.rs** (3 unwraps in tests)
   - Line 143: `String::from_utf8()` unwrap

---

## Step-by-Step Implementation

### Phase 1: Define Error Types and Error Context

#### 1.1 Extend Error Types in http_request.rs
The `HttpParseError` enum already exists but needs expansion:

```rust
#[derive(Debug)]
pub enum HttpParseError {
    HeaderTooLong,
    MissingHostHeader,
    MalformedRequestLine,
    IoError(std::io::Error),
    // Add new variants:
    InvalidUtf8(std::string::FromUtf8Error),
}

impl fmt::Display for HttpParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HttpParseError::HeaderTooLong => write!(f, "Header line exceeds maximum length"),
            HttpParseError::MissingHostHeader => write!(f, "Missing required Host header"),
            HttpParseError::MalformedRequestLine => write!(f, "Malformed request line"),
            HttpParseError::IoError(e) => write!(f, "IO error: {e}"),
            HttpParseError::InvalidUtf8(_) => write!(f, "Invalid UTF-8 in request"),
        }
    }
}
```

#### 1.2 Create New Error Type for Server Operations
Add to src/main.rs or create src/errors.rs:

```rust
#[derive(Debug)]
pub enum ServerError {
    BindError(std::io::Error),
    IoError(std::io::Error),
    PathError(String),
    Utf8Error(std::path::PathBuf),
}

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServerError::BindError(e) => write!(f, "Failed to bind listener: {e}"),
            ServerError::IoError(e) => write!(f, "IO operation failed: {e}"),
            ServerError::PathError(msg) => write!(f, "Path error: {msg}"),
            ServerError::Utf8Error(path) => write!(f, "Invalid UTF-8 in path: {:?}", path),
        }
    }
}
```

---

### Phase 2: Refactor src/main.rs

#### 2.1 Main Function - Listener Binding
**Line 26**: Replace socket binding unwrap

**Before**:
```rust
let listener = TcpListener::bind(&full_address).unwrap();
```

**After**:
```rust
let listener = match TcpListener::bind(&full_address) {
    Ok(l) => {
        println!("Successfully bound to {full_address}");
        l
    }
    Err(e) => {
        eprintln!("Error: Failed to bind to {full_address}: {e}");
        std::process::exit(1);
    }
};
```

**Rationale**: Binding failure is fatal to server startup. Graceful exit with error message is appropriate.

#### 2.2 Connection Accept Loop
**Line 38**: Replace incoming stream unwrap

**Before**:
```rust
for stream in listener.incoming() {
    let routes_clone = routes.clone();
    let stream = stream.unwrap();
    pool.execute(move || {
        handle_connection(stream, routes_clone);
    });
}
```

**After**:
```rust
for stream in listener.incoming() {
    let routes_clone = routes.clone();
    let stream = match stream {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error accepting connection: {e}");
            continue;  // Skip this connection, continue accepting others
        }
    };
    pool.execute(move || {
        handle_connection(stream, routes_clone);
    });
}
```

**Rationale**: Accept errors should not crash the server; skip and continue accepting new connections.

#### 2.3 Handle Connection - File Reading
**Line 70**: Replace fs::read_to_string unwrap

**Before**:
```rust
let contents = fs::read_to_string(filename).unwrap();
response.add_body(contents.into());
```

**After**:
```rust
let contents = match fs::read_to_string(filename) {
    Ok(c) => c,
    Err(e) => {
        eprintln!("Error reading file {filename}: {e}");
        let mut error_response = HttpResponse::build(String::from("HTTP/1.1"), 500);
        error_response.add_body(format!("Internal Server Error: Failed to read file").into());
        let _ = stream.write_all(&error_response.as_bytes());
        return;
    }
};
response.add_body(contents.into());
```

**Rationale**: Send 500 error to client instead of panicking. Log error for debugging.

#### 2.4 Handle Connection - Write All
**Line 74**: Replace stream write_all unwrap

**Before**:
```rust
stream.write_all(&response.as_bytes()).unwrap();
```

**After**:
```rust
if let Err(e) = stream.write_all(&response.as_bytes()) {
    eprintln!("Error writing response: {e}");
    // Connection is broken; handler exits gracefully
}
```

**Rationale**: Write failures mean the client disconnected; log but don't panic.

#### 2.5 Handle Connection - Route Lookup
**Lines 64**: Replace get + to_str unwraps

**Before**:
```rust
let (mut response, filename) = if routes.contains_key(&clean_target) {
    (HttpResponse::build(String::from("HTTP/1.1"), 200),
        routes.get(&clean_target).unwrap().to_str().unwrap())
} else {
    (HttpResponse::build(String::from("HTTP/1.1"), 404),
        "pages/not_found.html")
};
```

**After**:
```rust
let (mut response, filename) = if routes.contains_key(&clean_target) {
    match routes.get(&clean_target) {
        Some(path_buf) => {
            match path_buf.to_str() {
                Some(path_str) => (HttpResponse::build(String::from("HTTP/1.1"), 200), path_str),
                None => {
                    eprintln!("Path contains invalid UTF-8: {:?}", path_buf);
                    (HttpResponse::build(String::from("HTTP/1.1"), 500), "pages/not_found.html")
                }
            }
        }
        None => {
            eprintln!("Route found in map but get() failed (synchronization issue)");
            (HttpResponse::build(String::from("HTTP/1.1"), 500), "pages/not_found.html")
        }
    }
} else {
    (HttpResponse::build(String::from("HTTP/1.1"), 404), "pages/not_found.html")
};
```

**Rationale**: Defensive check for invalid UTF-8 in paths; treat as server error, not panic.

#### 2.6 Build Routes - Directory Reading
**Lines 94-95**: Replace read_dir and entry unwraps

**Before**:
```rust
for entry in fs::read_dir(directory).unwrap() {
    let entry = entry.unwrap();
    let path = entry.path();
```

**After**:
```rust
let entries = match fs::read_dir(directory) {
    Ok(e) => e,
    Err(err) => {
        eprintln!("Error reading directory {:?}: {err}", directory);
        return routes;  // Return partial routes, continue serving
    }
};

for entry_result in entries {
    let entry = match entry_result {
        Ok(e) => e,
        Err(err) => {
            eprintln!("Error reading directory entry: {err}");
            continue;  // Skip this entry
        }
    };
    let path = entry.path();
```

**Rationale**: Directory read errors are non-fatal; return partial routes and continue.

#### 2.7 Build Routes - Path Metadata
**Lines 97, 103**: Replace file_name and extension unwraps

**Before**:
```rust
let name = path.file_name().unwrap().to_str().unwrap();
// ...
match path.extension().unwrap().to_str().unwrap() {
```

**After**:
```rust
let name = match path.file_name() {
    Some(fname) => {
        match fname.to_str() {
            Some(n) => n,
            None => {
                eprintln!("File name contains invalid UTF-8: {:?}", path);
                continue;
            }
        }
    }
    None => {
        eprintln!("Could not extract file name from path: {:?}", path);
        continue;
    }
};
// ...
let extension = match path.extension() {
    Some(ext) => {
        match ext.to_str() {
            Some(e) => e,
            None => {
                eprintln!("File extension contains invalid UTF-8: {:?}", path);
                continue;
            }
        }
    }
    None => {
        // No extension, skip
        continue;
    }
};
match extension {
```

**Rationale**: Invalid UTF-8 in file names should be logged and skipped, not crash route building.

---

### Phase 3: Refactor src/lib.rs

#### 3.1 ThreadPool Execute Method
**Line 43**: Replace sender.send unwrap

**Before**:
```rust
pub fn execute<F>(&self, f: F)
where
    F: FnOnce() + Send + 'static,
{
    let job = Box::new(f);
    self.sender.as_ref().unwrap().send(job).unwrap();
}
```

**After**:
```rust
pub fn execute<F>(&self, f: F) -> Result<(), Box<dyn std::any::Any + Send>>
where
    F: FnOnce() + Send + 'static,
{
    let job = Box::new(f);
    match self.sender.as_ref() {
        Some(sender) => {
            match sender.send(job) {
                Ok(()) => Ok(()),
                Err(mpsc::SendError(_)) => {
                    Err(Box::new("Worker threads have disconnected".to_string()))
                }
            }
        }
        None => {
            Err(Box::new("ThreadPool has been shut down".to_string()))
        }
    }
}
```

**Alternative (non-panicking)**: Return Result but handle in main.rs

**Rationale**: Sender failures indicate worker shutdown; propagate error up so main loop can decide.

#### 3.2 ThreadPool Drop - Join
**Line 54**: Replace worker.thread.join() unwrap

**Before**:
```rust
impl Drop for ThreadPool {
    fn drop(&mut self) {
        drop(self.sender.take());
        for worker in self.workers.drain(..) {
            println!("Shutting down worker {}", worker.id);
            worker.thread.join().unwrap();
        }
    }
}
```

**After**:
```rust
impl Drop for ThreadPool {
    fn drop(&mut self) {
        drop(self.sender.take());
        for worker in self.workers.drain(..) {
            println!("Shutting down worker {}", worker.id);
            match worker.thread.join() {
                Ok(()) => println!("Worker {} shut down successfully", worker.id),
                Err(e) => {
                    eprintln!("Worker {} panicked during shutdown: {:?}", worker.id, e);
                }
            }
        }
    }
}
```

**Rationale**: Worker panics during shutdown should be logged, not propagate to Drop. Print detailed error info.

#### 3.3 Worker Loop - Mutex Lock
**Line 63**: Replace receiver.lock() unwrap

**Before**:
```rust
let thread = thread::spawn(move || {
    loop {
        let message = reciever.lock().unwrap().recv();
        // ...
    }
});
```

**After**:
```rust
let thread = thread::spawn(move || {
    loop {
        let message = match reciever.lock() {
            Ok(guard) => guard.recv(),
            Err(poisoned) => {
                eprintln!("Worker {id}: Mutex poisoned, attempting to recover");
                match poisoned.into_inner().recv() {
                    Ok(job) => Ok(job),
                    Err(e) => Err(e),
                }
            }
        };
        match message {
            Ok(job) => {
                println!("Worker {id} got a job; executing.");
                job();
            }
            Err(_) => {
                println!("Worker {id} disconnected; shutting down.");
                break;
            }
        }
    }
});
```

**Rationale**: Mutex poisoning is recoverable; attempt recovery instead of panicking.

---

### Phase 4: Update Test Code

#### 4.1 http_request.rs Tests
Tests use unwrap() for setup code. Replace with expect() or Result assertions:

**Before**:
```rust
let text = String::from_utf8(bytes).unwrap();
```

**After**:
```rust
let text = String::from_utf8(bytes).expect("Response contains valid UTF-8");
```

Or in integration tests:

```rust
let text = String::from_utf8(bytes)?;
assert!(text.starts_with("GET / HTTP/1.1\r\n"));
```

#### 4.2 Test Network Setup
Replace test listener binding unwraps:

**Before**:
```rust
let listener = TcpListener::bind("127.0.0.1:0").unwrap();
let addr = listener.local_addr().unwrap();
```

**After**:
```rust
let listener = TcpListener::bind("127.0.0.1:0")
    .expect("Failed to bind test listener");
let addr = listener.local_addr()
    .expect("Failed to get local address");
```

**Rationale**: In tests, setup failures should fail the test clearly with `.expect()`, not silently panic with `.unwrap()`.

---

## Code Snippets - Complete Examples

### Example 1: Robust File Serving Handler
```rust
fn handle_connection(mut stream: TcpStream, routes: HashMap<String, PathBuf>) {
    // Parse request
    let http_request = match HttpRequest::build_from_stream(&stream) {
        Ok(req) => req,
        Err(e) => {
            eprintln!("Bad request: {e}");
            let mut response = HttpResponse::build(String::from("HTTP/1.1"), 400);
            response.add_body(format!("Bad Request: {e}").into());
            let _ = stream.write_all(&response.as_bytes());
            return;
        }
    };

    let clean_target = clean_route(&http_request.target);
    println!("Request: {http_request}");

    // Get file path with error handling
    let (mut response, filename) = if let Some(path_buf) = routes.get(&clean_target) {
        match path_buf.to_str() {
            Some(path_str) => (HttpResponse::build(String::from("HTTP/1.1"), 200), path_str),
            None => {
                eprintln!("Path contains invalid UTF-8: {:?}", path_buf);
                let mut err_resp = HttpResponse::build(String::from("HTTP/1.1"), 500);
                err_resp.add_body("Internal Server Error".into());
                let _ = stream.write_all(&err_resp.as_bytes());
                return;
            }
        }
    } else {
        (HttpResponse::build(String::from("HTTP/1.1"), 404), "pages/not_found.html")
    };

    // Read file with error handling
    match fs::read_to_string(filename) {
        Ok(contents) => {
            response.add_body(contents.into());
            println!("Response: {response}");
            if let Err(e) = stream.write_all(&response.as_bytes()) {
                eprintln!("Error writing response: {e}");
            }
        }
        Err(e) => {
            eprintln!("Error reading file {filename}: {e}");
            let mut error_response = HttpResponse::build(String::from("HTTP/1.1"), 500);
            error_response.add_body(format!("Internal Server Error").into());
            let _ = stream.write_all(&error_response.as_bytes());
        }
    }
}
```

### Example 2: Resilient Route Building
```rust
fn build_routes(route: String, directory: &Path) -> HashMap<String, PathBuf> {
    let mut routes: HashMap<String, PathBuf> = HashMap::new();

    let entries = match fs::read_dir(directory) {
        Ok(e) => e,
        Err(err) => {
            eprintln!("Error reading directory {:?}: {err}", directory);
            return routes;
        }
    };

    for entry_result in entries {
        let entry = match entry_result {
            Ok(e) => e,
            Err(err) => {
                eprintln!("Error reading directory entry: {err}");
                continue;
            }
        };

        let path = entry.path();
        let name = match path.file_name() {
            Some(fname) => match fname.to_str() {
                Some(n) => n,
                None => {
                    eprintln!("File name contains invalid UTF-8: {:?}", path);
                    continue;
                }
            },
            None => {
                eprintln!("Could not extract file name from path: {:?}", path);
                continue;
            }
        };

        if path.is_dir() {
            routes.extend(build_routes(format!("{route}/{name}"), &path));
        } else if path.is_file() {
            let extension = match path.extension() {
                Some(ext) => match ext.to_str() {
                    Some(e) => e,
                    None => {
                        eprintln!("File extension contains invalid UTF-8: {:?}", path);
                        continue;
                    }
                },
                None => continue,
            };

            match extension {
                "html" | "css" | "js" => {
                    if name == "index.html" || name == "page.html" {
                        if route == "" {
                            routes.insert(String::from("/"), path);
                        } else {
                            routes.insert(route.clone(), path);
                        }
                    } else if name != "not_found.html" {
                        routes.insert(format!("{route}/{name}"), path);
                    }
                }
                _ => continue,
            }
        }
    }

    routes
}
```

---

## Testing Strategy

### Unit Tests
1. **File Not Found Handling**
   - Test reading non-existent files returns 500 error, not panic
   - Verify error is logged

2. **Invalid UTF-8 Paths**
   - Create mock filesystem entries with invalid UTF-8
   - Verify route building skips them with error logs

3. **Directory Read Errors**
   - Mock permission-denied scenarios
   - Verify partial routes returned

4. **Connection Errors**
   - Simulate broken connections during write_all()
   - Verify error is logged, server continues

### Integration Tests
1. **Server Robustness**
   - Send requests for non-existent routes (404)
   - Verify server responds and continues accepting

2. **Malformed Requests**
   - Send oversized headers, invalid HTTP
   - Verify 400 response, no panic

3. **Concurrent Error Scenarios**
   - Multiple malformed requests simultaneously
   - Verify server doesn't crash, handles all gracefully

4. **ThreadPool Shutdown**
   - Send jobs during shutdown
   - Verify graceful error handling

### Test Example
```rust
#[test]
fn handle_missing_file_returns_500() {
    use std::io::Write;
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("local_addr");

    let handle = std::thread::spawn(move || {
        let mut client = std::net::TcpStream::connect(addr).expect("connect");
        client.write_all(b"GET /nonexistent HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .expect("write");
        client.shutdown(std::net::Shutdown::Write).expect("shutdown");
    });

    let (stream, _) = listener.accept().expect("accept");
    let req = HttpRequest::build_from_stream(&stream).expect("parse");

    // Verify request parsed, but file doesn't exist
    assert_eq!(req.target, "/nonexistent");
    handle.join().expect("join");
}
```

---

## Edge Cases and Error Scenarios

### 1. Filesystem Errors
- **Read Permission Denied**: Return 500 "Internal Server Error"
- **Disk I/O Error**: Log error, return 500
- **Path with Invalid UTF-8**: Log, skip in route building
- **Symbolic Link Loops**: Path resolution may fail; handle gracefully

### 2. Network Errors
- **Client Disconnect During Read**: `BufReader::read_line()` returns `Err`; handled in `build_from_stream()`
- **Client Disconnect During Write**: `write_all()` returns `Err`; log and return
- **Malformed HTTP in Request**: Already handled with `HttpParseError`

### 3. Thread Pool Errors
- **Worker Thread Panic**: Drop trait logs panic info
- **Sender Disconnection**: `execute()` returns error; main loop handles
- **Mutex Poisoning**: Recover from poisoned lock, don't panic

### 4. Request Parsing Edge Cases
- **Headers Exceeding Limit**: Returns `HttpParseError::HeaderTooLong` (already handled)
- **Missing Host Header in HTTP/1.1**: Returns `HttpParseError::MissingHostHeader` (already handled)
- **Invalid UTF-8 in Body**: Existing code clones bytes; UTF-8 validation optional

---

## Implementation Order

### Week 1 - High Priority (Production Code)
1. Replace TcpListener::bind() error (main.rs:26)
2. Replace incoming stream accept errors (main.rs:38)
3. Replace fs::read_to_string() errors (main.rs:70)
4. Replace stream.write_all() errors (main.rs:74)

### Week 2 - Medium Priority (Route Building)
5. Replace build_routes() directory errors (main.rs:94-95)
6. Replace path metadata errors (main.rs:97, 103)
7. Replace route lookup unwraps (main.rs:64)

### Week 3 - Low Priority (Thread Pool + Tests)
8. Update ThreadPool::execute() error handling (lib.rs:43)
9. Update Drop trait error handling (lib.rs:54)
10. Update Worker loop mutex handling (lib.rs:63)
11. Update test code to use expect() instead of unwrap()

---

## Backwards Compatibility

### API Changes
- **ThreadPool::execute()** signature changes to return `Result`
  - **Impact**: Minimal, only main.rs uses it
  - **Migration**: Add error handling in main accept loop

### Error Logging
- No changes to HTTP response format
- Only error logging output increases
- Server behavior improves (no more panics)

---

## Success Criteria

1. **Zero Unwraps in Production Code**: No `.unwrap()` or `.expect()` outside tests
2. **All Errors Logged**: Every fallible operation logs its error
3. **Server Resilience**: Malformed requests/missing files don't crash server
4. **Test Coverage**: Tests verify each error path
5. **Clean Shutdown**: Worker panics logged, don't prevent graceful shutdown
6. **Concurrent Stability**: Multiple simultaneous errors handled gracefully

---

## Performance Impact

- **Minimal**: Error paths are rare in normal operation
- **Logging Overhead**: Slight increase in logging (negligible)
- **Route Building**: Same complexity, just with better error reporting
- **Memory**: No additional allocations in happy path

---

## Related Features

- **Logging/Observability** (logs errors to stderr)
- **Error Handling** (general error recovery strategy)
- **Connection Handling** (graceful client disconnection)
- **Testing** (error path coverage)

---

## Summary

Replacing 67 unwrap() calls transforms rcomm from crash-prone to resilient. The implementation is straightforward:
1. Define clear error types
2. Replace unwraps with match/if-let in production paths
3. Log all errors with context
4. Send appropriate HTTP responses to clients
5. Test error paths thoroughly

The result: a production-grade web server that handles errors gracefully and continues serving requests even when components fail.
