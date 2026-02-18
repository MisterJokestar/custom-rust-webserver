# Log and Recover from TCP Write Errors

**Feature**: Log and recover from TCP write errors (client disconnect mid-response) instead of panicking
**Category**: Error Handling
**Complexity**: 2/10
**Necessity**: 8/10

---

## Overview

The server currently panics when writing a response to a TCP stream fails. This happens at `src/main.rs` line 74, where `stream.write_all(&response.as_bytes()).unwrap()` kills the worker thread if the client disconnects before or during response transmission. In production, client disconnects are routine — browsers cancel requests, network connections drop, load balancers close idle connections. A server must handle these gracefully.

### Current State

**`src/main.rs` line 74**:
```rust
stream.write_all(&response.as_bytes()).unwrap();
```

This is the only point where the full response is sent to the client. When `write_all()` fails:
- The worker thread panics and dies
- The thread pool permanently loses one worker
- No error is logged
- Under sustained client disconnects, the server degrades to zero workers

### Common Causes of Write Failures
- **Broken pipe (`EPIPE`)**: Client closed the connection before the response was sent
- **Connection reset (`ECONNRESET`)**: Client sent a TCP RST packet
- **Connection timed out**: Network path failed during transmission
- **Connection refused**: Rare, can happen with proxied connections

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Line 74**: Replace `stream.write_all().unwrap()` with error handling

Also ensure the error response writes in the 400 handler (line 54) are handled consistently — they already use `let _ =` which is correct but doesn't log.

---

## Step-by-Step Implementation

### Step 1: Replace Response Write Unwrap

**Current code** (`src/main.rs` line 74):
```rust
stream.write_all(&response.as_bytes()).unwrap();
```

**Updated code**:
```rust
if let Err(e) = stream.write_all(&response.as_bytes()) {
    eprintln!("Error writing response: {e}");
}
```

**Rationale**:
- The response write is the last action in `handle_connection()` — there's nothing else to do after it fails
- Logging the error provides visibility into client disconnect patterns
- The function returns normally, and the worker thread survives

### Step 2: Add Client IP Context to Write Error Logs

For better diagnostics, include the client's address in the error log:

```rust
fn handle_connection(mut stream: TcpStream, routes: HashMap<String, PathBuf>) {
    let peer = stream.peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    // ... existing request handling ...

    if let Err(e) = stream.write_all(&response.as_bytes()) {
        eprintln!("Error writing response to {peer}: {e}");
    }
}
```

**Rationale**: Knowing which client disconnected helps identify problematic clients, network issues, or load balancer misconfigurations.

### Step 3: Consistently Handle Error Response Writes

The 400 Bad Request handler already uses `let _ =` for the write (line 54). Update it to log on failure:

**Current code** (line 54):
```rust
let _ = stream.write_all(&response.as_bytes());
```

**Updated code**:
```rust
if let Err(e) = stream.write_all(&response.as_bytes()) {
    eprintln!("Error writing 400 response to {peer}: {e}");
}
```

Apply the same pattern to any future error response writes (500 responses from the file read fix, etc.).

### Complete Updated `handle_connection()`

```rust
fn handle_connection(mut stream: TcpStream, routes: HashMap<String, PathBuf>) {
    let peer = stream.peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    let http_request = match HttpRequest::build_from_stream(&stream) {
        Ok(req) => req,
        Err(e) => {
            eprintln!("Bad request from {peer}: {e}");
            let mut response = HttpResponse::build(String::from("HTTP/1.1"), 400);
            let body = format!("Bad Request: {e}");
            response.add_body(body.into());
            if let Err(e) = stream.write_all(&response.as_bytes()) {
                eprintln!("Error writing 400 response to {peer}: {e}");
            }
            return;
        }
    };
    let clean_target = clean_route(&http_request.target);

    println!("Request from {peer}: {http_request}");

    let (mut response, filename) = if routes.contains_key(&clean_target) {
        (HttpResponse::build(String::from("HTTP/1.1"), 200),
            routes.get(&clean_target).unwrap().to_str().unwrap())
    } else {
        (HttpResponse::build(String::from("HTTP/1.1"), 404),
            "pages/not_found.html")
    };

    let contents = fs::read_to_string(filename).unwrap();
    response.add_body(contents.into());

    println!("Response to {peer}: {response}");
    if let Err(e) = stream.write_all(&response.as_bytes()) {
        eprintln!("Error writing response to {peer}: {e}");
    }
}
```

Note: The `unwrap()` calls on lines for route lookup and file read are addressed by separate features. This feature focuses specifically on the write error.

---

## Edge Cases

### 1. Client Disconnects Before Response
**Scenario**: Client sends a request, then closes the connection immediately (e.g., browser navigated away).
**Handling**: `write_all()` returns `Err(BrokenPipe)`. Error is logged, function returns normally.

### 2. Client Disconnects Mid-Response
**Scenario**: For large responses, the client disconnects after receiving partial data.
**Handling**: `write_all()` returns `Err(BrokenPipe)` or `Err(ConnectionReset)` after writing some bytes. The partial data is lost. Error is logged.

### 3. Network Timeout
**Scenario**: The TCP connection stalls due to network issues.
**Handling**: `write_all()` blocks until the OS TCP timeout expires, then returns an error. This can be slow (minutes). The request timeout feature (separate) would address this by setting `SO_SNDTIMEO`.

### 4. Error Writing Error Response
**Scenario**: A 400 Bad Request error response write also fails.
**Handling**: Both the parse error and the write error are logged. The function returns normally.

### 5. `peer_addr()` Fails
**Scenario**: The socket is in an invalid state when `peer_addr()` is called.
**Handling**: Falls back to `"unknown"` string. This is extremely rare and only happens with unusual socket states.

---

## Testing Strategy

### Integration Tests

```rust
fn test_server_survives_client_disconnect(addr: &str) -> Result<(), String> {
    // Connect, send request, disconnect immediately without reading response
    let mut stream = TcpStream::connect(addr)
        .map_err(|e| format!("connect: {e}"))?;
    let request = "GET / HTTP/1.1\r\nHost: localhost\r\n\r\n";
    stream.write_all(request.as_bytes())
        .map_err(|e| format!("write: {e}"))?;
    drop(stream); // Disconnect before reading response

    // Brief pause for server to process
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Verify server is still alive
    let resp = send_request(addr, "GET", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "status after disconnect")?;
    Ok(())
}

fn test_server_survives_many_disconnects(addr: &str) -> Result<(), String> {
    // Disconnect 20 times rapidly
    for _ in 0..20 {
        let mut stream = TcpStream::connect(addr)
            .map_err(|e| format!("connect: {e}"))?;
        stream.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .map_err(|e| format!("write: {e}"))?;
        drop(stream);
    }

    std::thread::sleep(std::time::Duration::from_millis(200));

    // Server should still be fully functional
    let resp = send_request(addr, "GET", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "status after many disconnects")?;
    Ok(())
}
```

### Manual Testing

```bash
cargo run &

# Test broken pipe: send request but close before response
echo -ne "GET / HTTP/1.1\r\nHost: localhost\r\n\r\n" | nc -q 0 127.0.0.1 7878
# Server stderr should show: Error writing response to 127.0.0.1:XXXXX: Broken pipe

# Verify server still works
curl -i http://127.0.0.1:7878/
# Expected: HTTP/1.1 200 OK

# Stress test disconnects
for i in $(seq 1 50); do
    echo -ne "GET / HTTP/1.1\r\nHost: localhost\r\n\r\n" | nc -q 0 127.0.0.1 7878 &
done
wait
curl -i http://127.0.0.1:7878/
# Expected: HTTP/1.1 200 OK (server survived all disconnects)
```

---

## Implementation Checklist

- [ ] Replace `stream.write_all().unwrap()` at line 74 with `if let Err` and `eprintln!`
- [ ] Add `peer_addr()` extraction at top of `handle_connection()`
- [ ] Update 400 error response write (line 54) to log on failure
- [ ] Include client address in all write error log messages
- [ ] Add integration test: server survives single client disconnect
- [ ] Add integration test: server survives many rapid disconnects
- [ ] Run `cargo test` — no regressions
- [ ] Run `cargo run --bin integration_test` — all tests pass

---

## Backward Compatibility

No external API changes. The only behavioral change is that write failures log an error and return instead of panicking. Clients that disconnect will no longer cause worker thread death. All existing tests pass unchanged (they read responses fully before disconnecting).

---

## Related Features

- **Error Handling > handle_connection File Read 500**: The 500 error response path also needs write error handling
- **Error Handling > Connection-Level Error Handling**: `catch_unwind` in workers provides a safety net, but this feature prevents the panic from occurring in the first place
- **Logging & Observability > Client IP Logging**: The `peer_addr()` extraction here is shared with client IP logging
- **Logging & Observability > Error Detail Logging**: Write errors are a key category of operational errors to log
- **Security > Request Timeout**: Write timeouts (`SO_SNDTIMEO`) would prevent `write_all()` from blocking indefinitely on stalled connections
