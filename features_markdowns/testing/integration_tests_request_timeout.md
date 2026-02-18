# Integration Tests for Request Timeout Behavior

**Category:** Testing
**Complexity:** 4/10
**Necessity:** 5/10
**Status:** Planning

---

## Overview

Add integration tests to `src/bin/integration_test.rs` that verify the server's request timeout behavior. Currently, the server has **no request timeouts** — a client that connects but never sends data (or sends data very slowly) will hold a worker thread indefinitely. These tests document the current behavior and serve as acceptance tests for when timeouts are implemented.

**Goal:** Validate that the server handles slow/stalled clients correctly, and that timeout behavior (once implemented) reclaims worker threads.

**Note:** The higher complexity (4/10) reflects the difficulty of testing timeout behavior — tests must coordinate timing between client and server, handle platform-specific networking behavior, and avoid test flakiness.

---

## Current State

### No Timeout Configuration

The server's `TcpStream` connections have no read/write timeout set:

```rust
// src/main.rs, lines 36-43
for stream in listener.incoming() {
    let routes_clone = routes.clone();
    let stream = stream.unwrap();

    pool.execute(move || {
        handle_connection(stream, routes_clone);
    });
}
```

No calls to `stream.set_read_timeout()` or `stream.set_write_timeout()`.

### Impact

A single stalled client can permanently consume one of the 4 worker threads. Four stalled connections would make the server completely unresponsive.

### Request Parsing (HttpRequest::build_from_stream)

The parser reads from the stream using `BufReader::read_line()` which blocks indefinitely waiting for data when no timeout is set.

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`

**Add:** Timeout behavior test functions.
**Modify:** `main()` function to register new tests.

---

## Step-by-Step Implementation

### Step 1: Add Stalled Connection Test

This test connects but never sends any data, then verifies the server still serves other requests:

```rust
fn test_stalled_connection_doesnt_block_server(addr: &str) -> Result<(), String> {
    // Open a connection but don't send anything
    let stalled = TcpStream::connect(addr)
        .map_err(|e| format!("stalled connect: {e}"))?;
    // Don't write anything — just hold the connection open

    // Give the server a moment to accept the stalled connection
    thread::sleep(Duration::from_millis(100));

    // The server has 4 worker threads. Even with one stalled,
    // it should still serve requests on the other 3.
    let resp = send_request(addr, "GET", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "status with stalled conn")?;

    // Clean up
    drop(stalled);
    Ok(())
}
```

### Step 2: Add Multiple Stalled Connections Test

```rust
fn test_multiple_stalled_connections(addr: &str) -> Result<(), String> {
    // Open 3 stalled connections (leaves 1 worker free in a 4-thread pool)
    let mut stalled_conns = Vec::new();
    for _ in 0..3 {
        let conn = TcpStream::connect(addr)
            .map_err(|e| format!("stalled connect: {e}"))?;
        stalled_conns.push(conn);
    }

    thread::sleep(Duration::from_millis(200));

    // Should still be able to serve at least one request
    let resp = send_request(addr, "GET", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "status with 3 stalled conns")?;

    // Clean up
    drop(stalled_conns);
    Ok(())
}
```

### Step 3: Add Slow Client Test

```rust
fn test_slow_client_partial_request(addr: &str) -> Result<(), String> {
    // Send request headers very slowly (one byte at a time)
    let mut stream = TcpStream::connect(addr)
        .map_err(|e| format!("connect: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| format!("set timeout: {e}"))?;

    let request = b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";

    // Send one byte at a time with small delays
    for byte in request.iter() {
        stream.write_all(&[*byte])
            .map_err(|e| format!("write: {e}"))?;
        thread::sleep(Duration::from_millis(10));
    }

    // Should still get a valid response
    let resp = read_response(&mut stream)
        .map_err(|e| format!("read response: {e}"))?;
    assert_eq_or_err(&resp.status_code, &200, "slow client status")?;
    Ok(())
}
```

### Step 4: Add Incomplete Request Test (After Timeout Feature)

This test is for when the timeout feature is implemented:

```rust
fn test_incomplete_request_eventually_times_out(addr: &str) -> Result<(), String> {
    // Send an incomplete request (missing final \r\n)
    let mut stream = TcpStream::connect(addr)
        .map_err(|e| format!("connect: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .map_err(|e| format!("set timeout: {e}"))?;

    // Send partial headers — no terminating \r\n\r\n
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n")
        .map_err(|e| format!("write: {e}"))?;

    // Wait for server timeout to kick in
    // The server should close the connection after its timeout period
    // NOTE: Without the timeout feature, this test will hang.
    // TODO: Enable this test once request timeouts are implemented.

    // Try reading — should get either a timeout response or connection close
    let mut buf = vec![0u8; 1024];
    match stream.read(&mut buf) {
        Ok(0) => {
            // Connection closed by server — this is acceptable timeout behavior
            Ok(())
        }
        Ok(n) => {
            // Server sent a response (maybe 408 Request Timeout)
            let response = String::from_utf8_lossy(&buf[..n]);
            if response.contains("408") || response.contains("Timeout") {
                Ok(())
            } else {
                Err(format!("unexpected response: {}", &response[..std::cmp::min(n, 100)]))
            }
        }
        Err(e) => {
            // Read timeout or connection reset — both acceptable
            if e.kind() == std::io::ErrorKind::TimedOut
                || e.kind() == std::io::ErrorKind::ConnectionReset
            {
                Ok(())
            } else {
                Err(format!("unexpected error: {e}"))
            }
        }
    }
}
```

### Step 5: Register Tests in `main()`

```rust
run_test("stalled_conn_doesnt_block", || test_stalled_connection_doesnt_block_server(&addr)),
run_test("multiple_stalled_conns", || test_multiple_stalled_connections(&addr)),
run_test("slow_client_partial_request", || test_slow_client_partial_request(&addr)),
// TODO: Enable when timeout feature is implemented:
// run_test("incomplete_request_timeout", || test_incomplete_request_eventually_times_out(&addr)),
```

---

## Edge Cases & Considerations

### 1. Worker Thread Starvation

**Scenario:** With 4 workers and 4 stalled connections, the server is completely blocked. Opening a 4th stalled connection in tests would make the test itself hang (no workers available to process the verification request).

**Mitigation:** Tests stall at most 3 connections, leaving 1 worker free for verification.

### 2. TCP Backlog

**Scenario:** Stalled connections don't immediately consume a worker thread. The OS TCP backlog queues the connection until `accept()` is called. The server calls `accept()` (via `listener.incoming()`) which hands the stream to a worker. The worker blocks reading from the stream.

**Behavior:** The stalled connection IS dispatched to a worker thread immediately. The worker blocks on `BufReader::read_line()`.

### 3. Test Timing

**Scenario:** Network latency, system load, and thread scheduling make timeout tests inherently timing-dependent.

**Mitigation:**
- Use generous timeouts (10-15 seconds) for client-side reads
- Add `thread::sleep()` delays between operations
- Accept a range of behaviors (timeout, connection reset, etc.)

### 4. Slow Client Legitimacy

**Scenario:** `test_slow_client_partial_request` sends valid data slowly. The server should still serve the request. This is distinct from a stalled client that sends nothing.

**Expected behavior:** The server should tolerate slow-but-progressing clients. Only truly stalled clients should be timed out.

### 5. Platform Differences

**Scenario:** TCP behavior varies across Linux, macOS, and Windows.

**Mitigation:** Tests use standard `TcpStream` operations that work cross-platform. Timeout errors may have different `ErrorKind` values.

---

## Testing Strategy

### Running the Tests

```bash
cargo build && cargo run --bin integration_test
```

### Expected Results (Before Timeout Feature)

- `test_stalled_connection_doesnt_block_server` — PASS (other workers handle requests)
- `test_multiple_stalled_connections` — PASS (1 worker still free)
- `test_slow_client_partial_request` — PASS (server processes slow but complete request)
- `test_incomplete_request_eventually_times_out` — DISABLED (would hang without timeouts)

### Expected Results (After Timeout Feature)

All tests pass, including the incomplete request test which verifies the server closes stalled connections after the configured timeout period.

---

## Implementation Checklist

- [ ] Add `test_stalled_connection_doesnt_block_server()` test
- [ ] Add `test_multiple_stalled_connections()` test
- [ ] Add `test_slow_client_partial_request()` test
- [ ] Add `test_incomplete_request_eventually_times_out()` test (disabled)
- [ ] Register active tests in `main()`
- [ ] Run `cargo build` — no compiler errors
- [ ] Run `cargo run --bin integration_test` — all active tests pass

---

## Related Features

- **Security > Request Timeout**: The feature these tests primarily support
- **Security > Configurable Maximum Concurrent Connection Limit**: Complements timeout by limiting total connections
- **Thread Pool > Worker Thread Panic Recovery**: Ensures workers survive timeout-related cleanup

---

## References

- [Rust TcpStream::set_read_timeout](https://doc.rust-lang.org/std/net/struct.TcpStream.html#method.set_read_timeout)
- [RFC 7230 Section 6.5 - Timeouts](https://tools.ietf.org/html/rfc7230#section-6.5)
- [HTTP 408 Request Timeout](https://developer.mozilla.org/en-US/docs/Web/HTTP/Status/408)
