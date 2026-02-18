# Implementation Plan: Log Client IP Address and Port

## 1. Overview of the Feature

Logging the client IP address and port on each request is fundamental for access logging, security auditing, abuse detection, and debugging. Every production web server records client connection information.

**Current State**: The server's `handle_connection()` function in `src/main.rs` receives a `TcpStream` that contains the client's socket address, but this information is never extracted or logged. The current `println!("Request: {http_request}")` output only shows HTTP headers and the request line.

**Desired State**: Each request log entry includes the client's IP address and port. The `TcpStream::peer_addr()` method provides a `SocketAddr` containing both the IP and port.

Example output:
```
[127.0.0.1:54321] GET /howdy HTTP/1.1 -> 200
```

Or integrated with CLF (if structured access logging feature exists):
```
127.0.0.1 - - [12/Feb/2026:14:30:00 +0000] "GET / HTTP/1.1" 200 1234
```
(The CLF format already includes the host IP; this feature ensures it's available.)

**Impact**:
- Enables client identification for security and debugging
- Foundation for rate limiting per IP (separate feature)
- Required for any standard access log format
- Trivial implementation with high value

---

## 2. Files to be Modified or Created

### Modified Files

1. **`/home/jwall/personal/rusty/rcomm/src/main.rs`**
   - Call `stream.peer_addr()` at the start of `handle_connection()`
   - Include the IP:port in the log output

### No New Files Required

This is a minimal change — a single method call and a format string update.

---

## 3. Step-by-Step Implementation Details

### Step 1: Extract and Log Peer Address

**File**: `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Current code** (lines 46–75):
```rust
fn handle_connection(mut stream: TcpStream, routes: HashMap<String, PathBuf>) {
    let http_request = match HttpRequest::build_from_stream(&stream) {
        Ok(req) => req,
        Err(e) => {
            eprintln!("Bad request: {e}");
            // ...
            return;
        }
    };
    let clean_target = clean_route(&http_request.target);

    println!("Request: {http_request}");
    // ...
    println!("Response: {response}");
    stream.write_all(&response.as_bytes()).unwrap();
}
```

**Updated code**:
```rust
fn handle_connection(mut stream: TcpStream, routes: HashMap<String, PathBuf>) {
    let peer = stream.peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    let http_request = match HttpRequest::build_from_stream(&stream) {
        Ok(req) => req,
        Err(e) => {
            eprintln!("[{peer}] Bad request: {e}");
            let mut response = HttpResponse::build(String::from("HTTP/1.1"), 400);
            let body = format!("Bad Request: {e}");
            response.add_body(body.into());
            let _ = stream.write_all(&response.as_bytes());
            return;
        }
    };
    let clean_target = clean_route(&http_request.target);

    println!("[{peer}] Request: {http_request}");

    let (mut response, filename) = if routes.contains_key(&clean_target) {
        (HttpResponse::build(String::from("HTTP/1.1"), 200),
            routes.get(&clean_target).unwrap().to_str().unwrap())
    } else {
        (HttpResponse::build(String::from("HTTP/1.1"), 404),
            "pages/not_found.html")
    };

    let contents = fs::read_to_string(filename).unwrap();
    response.add_body(contents.into());

    println!("[{peer}] Response: {response}");
    stream.write_all(&response.as_bytes()).unwrap();
}
```

**Key changes**:
- Extract `peer_addr()` at the very start of the function (before any reads that might close the connection)
- Use `map/unwrap_or_else` to gracefully handle the unlikely case where `peer_addr()` fails
- Prefix all log lines with `[IP:port]`

---

## 4. Code Snippets and Pseudocode

```
FUNCTION handle_connection(stream, routes)
    LET peer = stream.peer_addr().to_string() OR "unknown"

    LET request = parse_request(stream)
    IF error THEN
        LOG "[{peer}] Bad request: {error}"
        RETURN
    END IF

    LOG "[{peer}] {method} {target} -> {status}"
    // ... response handling ...
END FUNCTION
```

---

## 5. Testing Strategy

### Unit Tests

No new unit tests needed. `TcpStream::peer_addr()` is a standard library method. The change is purely a logging format update.

### Integration Tests

The integration tests in `src/bin/integration_test.rs` connect to the server over TCP, so the server will see `127.0.0.1:<ephemeral_port>` as the peer address. Since integration tests suppress server stdout (`Stdio::null()`), the IP logging doesn't affect test behavior.

No new integration tests needed.

### Manual Testing

```bash
cargo run
# In another terminal:
curl http://127.0.0.1:7878/
# Server output should show client IP and port:
# [127.0.0.1:54321] Request: GET / HTTP/1.1 ...
# [127.0.0.1:54321] Response: HTTP/1.1 200 OK ...
```

---

## 6. Edge Cases to Consider

### Case 1: peer_addr() Failure
**Scenario**: The TCP connection is reset between `accept()` and `peer_addr()` call
**Handling**: `peer_addr()` returns `Err`, which maps to `"unknown"` string. Logging continues with "unknown" as the client identifier.

### Case 2: IPv6 Client
**Scenario**: Client connects via IPv6 loopback (`::1`)
**Handling**: `SocketAddr::to_string()` produces `[::1]:54321` for IPv6 addresses, which is a valid and readable format.

### Case 3: Multiple Requests from Same Client
**Scenario**: Same client makes multiple requests (from different ephemeral ports)
**Handling**: Each connection gets its own ephemeral port, so log entries will show different ports. The IP remains the same for correlation.

### Case 4: Logging Before and After Request Parsing
**Scenario**: Need to log the peer address even if the request parsing fails
**Handling**: `peer_addr()` is called at the very start of `handle_connection()`, before `build_from_stream()`. Even if parsing fails, the peer address is available for the error log.

---

## 7. Implementation Checklist

- [ ] Add `stream.peer_addr()` extraction at start of `handle_connection()`
- [ ] Update all `println!`/`eprintln!` calls in `handle_connection()` to include `[{peer}]` prefix
- [ ] Run `cargo test` — no regressions
- [ ] Run `cargo run --bin integration_test` — integration tests pass
- [ ] Manual verification: IP:port appears in server output

---

## 8. Complexity and Risk Analysis

**Complexity**: 1/10
- Single method call (`peer_addr()`)
- String format change in existing log lines
- No new modules, structs, or functions

**Risk**: Very Low
- Pure additive change to logging format
- Does not affect request handling or response behavior
- `peer_addr()` is infallible in practice (connection is already established)
- Graceful fallback to "unknown" if it does fail

**Dependencies**: None
- Uses only `std::net::TcpStream::peer_addr()`

---

## 9. Future Enhancements

1. **X-Forwarded-For Support**: Read client IP from `X-Forwarded-For` header when behind a reverse proxy
2. **IP-Based Rate Limiting**: Use the extracted IP for per-client rate limiting
3. **IP Blocklist**: Reject connections from blocked IPs before reading the request
4. **GeoIP Lookup**: Map client IPs to geographic locations (requires external data)
5. **IP Anonymization**: Option to mask the last octet of IPv4 addresses for privacy compliance
