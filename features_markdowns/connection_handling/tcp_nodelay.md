# TCP_NODELAY Implementation Plan

## Overview

By default, TCP uses Nagle's algorithm, which buffers small outgoing packets and coalesces them to reduce the number of TCP segments sent. While this is efficient for bulk data transfers, it adds latency for request-response protocols like HTTP where the server wants to send a response as soon as it's ready.

Setting `TCP_NODELAY` on the socket disables Nagle's algorithm, causing each `write()` call to be sent immediately. This reduces response latency, especially for small responses.

Currently, `src/main.rs` accepts `TcpStream` connections without setting any socket options. This feature adds a single `set_nodelay(true)` call to each accepted stream.

**Complexity**: 1
**Necessity**: 5

**Key Changes**:
- Call `stream.set_nodelay(true)` on each accepted `TcpStream`

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Current State**:
- Line 38: `let stream = stream.unwrap();` — no socket options configured
- Response is written via `stream.write_all(&response.as_bytes())` (line 74) — a single large write, but Nagle's algorithm could still buffer it if the kernel decides to

**Changes Required**:
- Add `stream.set_nodelay(true)` after accepting the connection

---

## Step-by-Step Implementation

### Step 1: Set `TCP_NODELAY` After Accept

**Location**: `src/main.rs`, in the listener loop after line 38

**Current**:
```rust
for stream in listener.incoming() {
    let routes_clone = routes.clone();
    let stream = stream.unwrap();

    pool.execute(move || {
        handle_connection(stream, routes_clone);
    });
}
```

**New**:
```rust
for stream in listener.incoming() {
    let routes_clone = routes.clone();
    let stream = stream.unwrap();

    if let Err(e) = stream.set_nodelay(true) {
        eprintln!("Warning: failed to set TCP_NODELAY: {e}");
    }

    pool.execute(move || {
        handle_connection(stream, routes_clone);
    });
}
```

This uses Rust's built-in `TcpStream::set_nodelay()` — no external dependencies or unsafe code needed.

### Step 2 (Optional): Combine with `configure_stream()`

If implementing TCP keepalive at the same time (see `tcp_keepalive.md`), fold this into the shared `configure_stream()` helper:

```rust
fn configure_stream(stream: &TcpStream) {
    if let Err(e) = stream.set_nodelay(true) {
        eprintln!("Warning: failed to set TCP_NODELAY: {e}");
    }

    // ... keepalive options ...
}
```

---

## Edge Cases & Handling

### 1. `set_nodelay()` Fails
- Returns `io::Error` on failure (rare — only if the fd is invalid or the OS rejects the option)
- **Behavior**: Log a warning and continue. The connection still works; responses are just potentially slightly delayed
- Do not panic or reject the connection

### 2. Impact on Large Responses
- For large file responses, `TCP_NODELAY` causes more small TCP segments instead of fewer large ones
- In practice, `write_all()` writes the entire response buffer at once, so the kernel typically sends it in MSS-sized segments regardless of Nagle's algorithm
- Net effect: negligible for large responses, beneficial for small responses

### 3. Interaction with HTTP Pipelining
- With pipelining (multiple requests per connection), `TCP_NODELAY` ensures each response is sent immediately rather than waiting for the next kernel buffer flush
- This is especially important for pipelining correctness — clients expect responses promptly

### 4. Localhost vs. Network
- On localhost connections (common during development), Nagle's algorithm has minimal impact since RTT is ~0
- The benefit is more noticeable for remote clients with non-trivial RTT

---

## Implementation Checklist

- [ ] Add `stream.set_nodelay(true)` after accepting each connection
- [ ] Handle the error case with a warning (don't unwrap)
- [ ] Run `cargo build` to verify compilation
- [ ] Run `cargo test` and `cargo run --bin integration_test` to verify no regressions

---

## Backward Compatibility

No behavioral changes visible to clients. Responses arrive with the same content, just potentially with lower latency. All existing tests pass unchanged.
