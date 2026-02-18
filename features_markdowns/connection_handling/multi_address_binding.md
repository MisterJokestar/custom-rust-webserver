# Multi-Address/Port Binding Implementation Plan

## Overview

Currently, the server binds to a single address and port specified by `RCOMM_ADDRESS` and `RCOMM_PORT` environment variables. This feature adds support for binding to multiple address:port combinations simultaneously, allowing the server to listen on e.g. both `127.0.0.1:8080` and `0.0.0.0:80`, or on multiple ports at once.

This is useful for:
- Listening on both localhost (for local dev) and all interfaces (for network access)
- Running HTTP on port 80 and a secondary port (e.g., 8080) simultaneously
- Binding to specific network interfaces via their IP addresses

**Complexity**: 4
**Necessity**: 2

**Key Changes**:
- Add `RCOMM_LISTEN` environment variable accepting comma-separated `address:port` pairs
- Create multiple `TcpListener` instances
- Multiplex incoming connections from all listeners into the single thread pool
- Maintain backward compatibility with `RCOMM_ADDRESS`/`RCOMM_PORT` when `RCOMM_LISTEN` is not set

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Current State**:
- Lines 14-16: `get_port()` returns a single port string
- Lines 18-20: `get_address()` returns a single address string
- Line 26: Single `TcpListener::bind(&full_address)`
- Lines 36-43: Single `listener.incoming()` loop — blocks on one listener

**Changes Required**:
- Add `get_listen_addresses()` that parses `RCOMM_LISTEN` or falls back to `RCOMM_ADDRESS:RCOMM_PORT`
- Create a `TcpListener` for each address
- Set all listeners to non-blocking mode
- Use a polling loop (or `select`/`poll` syscall) to accept from all listeners
- Dispatch accepted connections to the thread pool as before

---

## Step-by-Step Implementation

### Step 1: Add Multi-Address Configuration

**Location**: `src/main.rs`, after `get_address()`

```rust
fn get_listen_addresses() -> Vec<String> {
    if let Ok(listen) = std::env::var("RCOMM_LISTEN") {
        listen.split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    } else {
        let port = get_port();
        let address = get_address();
        vec![format!("{address}:{port}")]
    }
}
```

### Step 2: Create Multiple Listeners

**Location**: `src/main.rs`, in `main()` replacing lines 23-26

```rust
let addresses = get_listen_addresses();
let mut listeners: Vec<TcpListener> = Vec::new();

for addr in &addresses {
    let listener = TcpListener::bind(addr).unwrap();
    listener.set_nonblocking(true).unwrap();
    listeners.push(listener);
    println!("Listening on {addr}");
}
```

### Step 3: Multiplex Incoming Connections

**Approach A: Simple Polling Loop (No Dependencies)**

```rust
loop {
    let mut accepted_any = false;

    for listener in &listeners {
        match listener.accept() {
            Ok((stream, _addr)) => {
                accepted_any = true;
                stream.set_nonblocking(false).unwrap(); // Restore blocking for handler
                let routes_clone = Arc::clone(&routes);

                pool.execute(move || {
                    handle_connection(stream, routes_clone);
                });
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // No pending connection on this listener, try next
                continue;
            }
            Err(e) => {
                eprintln!("Accept error: {e}");
            }
        }
    }

    if !accepted_any {
        // Sleep briefly to avoid busy-spinning when no connections are pending
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}
```

**Approach B: Per-Listener Accept Threads (Recommended)**

Spawns a dedicated accept thread per listener, each feeding into the shared thread pool. Avoids the busy-loop polling and sleep overhead.

```rust
let routes = Arc::new(build_routes(String::from(""), path));

// Keep listeners alive by holding handles
let mut accept_handles = Vec::new();

for listener in listeners {
    let pool_sender = pool.get_sender(); // Need to expose sender
    let routes = Arc::clone(&routes);

    let handle = std::thread::spawn(move || {
        for stream in listener.incoming() {
            let stream = match stream {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Accept error: {e}");
                    continue;
                }
            };

            let routes = Arc::clone(&routes);
            // Send job to pool...
        }
    });

    accept_handles.push(handle);
}

// Wait for all accept threads (runs forever until shutdown)
for handle in accept_handles {
    handle.join().unwrap();
}
```

**Approach C: Use `poll()`/`select()` Syscall (Most Efficient)**

Use the `poll()` syscall to wait on multiple listener file descriptors simultaneously, then accept from whichever is ready. Most efficient but requires unsafe code or the `mio`/`polling` crate.

**Recommended: Approach A** for initial implementation (simplest, no API changes needed). Can upgrade to Approach B or C later if the 1ms sleep proves to be a bottleneck.

### Step 4: Handle Single-Listener Fast Path

When only one address is configured, use the original blocking `listener.incoming()` loop to avoid the polling overhead:

```rust
if listeners.len() == 1 {
    let listener = listeners.into_iter().next().unwrap();
    // Original blocking loop
    for stream in listener.incoming() {
        let routes_clone = Arc::clone(&routes);
        let stream = stream.unwrap();

        pool.execute(move || {
            handle_connection(stream, routes_clone);
        });
    }
} else {
    // Multi-listener polling loop
    // ...
}
```

---

## Edge Cases & Handling

### 1. Duplicate Addresses
- `RCOMM_LISTEN=127.0.0.1:8080,127.0.0.1:8080` — bind fails on the second listener
- **Behavior**: Panic with a clear error message indicating which address failed
- **Improvement**: Deduplicate addresses before binding

### 2. Port Already in Use
- One address binds successfully but another fails
- **Behavior**: Currently would panic. Consider binding all first, then starting to accept — or fail fast if any bind fails.

### 3. Mixed IPv4/IPv6
- `RCOMM_LISTEN=0.0.0.0:80,[::]:80` — bind to both IPv4 and IPv6
- **Behavior**: Works if the OS allows dual-stack. May fail on some systems where `[::]:80` also binds IPv4.

### 4. Empty `RCOMM_LISTEN`
- **Behavior**: Falls back to `RCOMM_ADDRESS:RCOMM_PORT` defaults

### 5. Invalid Address in List
- `RCOMM_LISTEN=127.0.0.1:8080,not-an-address`
- **Behavior**: `TcpListener::bind()` panics on the invalid address
- **Improvement**: Validate and skip invalid addresses with a warning

---

## Implementation Checklist

- [ ] Add `get_listen_addresses()` configuration function
- [ ] Create `TcpListener` for each configured address
- [ ] Implement connection multiplexing (polling loop or per-listener threads)
- [ ] Add single-listener fast path to avoid polling overhead
- [ ] Print all listening addresses on startup
- [ ] Handle bind failures gracefully with clear error messages
- [ ] Add integration tests with default single-address configuration
- [ ] Run `cargo test` and `cargo run --bin integration_test`
- [ ] Manual test: set `RCOMM_LISTEN=127.0.0.1:8080,127.0.0.1:9090` and verify both ports serve content

---

## Backward Compatibility

When `RCOMM_LISTEN` is not set, behavior is identical to current — the server reads `RCOMM_ADDRESS` and `RCOMM_PORT` and binds to that single address. All existing tests and configurations work unchanged.
