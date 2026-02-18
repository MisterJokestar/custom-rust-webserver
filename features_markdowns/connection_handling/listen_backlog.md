# Configurable Listen Backlog Size Implementation Plan

## Overview

When a TCP server calls `listen()`, the OS maintains a backlog queue of connections that have completed the TCP handshake but haven't been `accept()`ed yet. The backlog parameter controls the maximum size of this queue. When the queue is full, the OS rejects new connections (typically with `ECONNREFUSED` or by silently dropping SYN packets).

Rust's `TcpListener::bind()` calls `listen()` with a default backlog of 128 (on most platforms). This is fine for low-traffic scenarios, but under burst load (many simultaneous connections), a small backlog can cause connection rejections before the server has a chance to accept them.

This feature adds a configurable listen backlog size via an environment variable, allowing operators to tune the queue depth for their workload.

**Complexity**: 2
**Necessity**: 3

**Key Changes**:
- Replace `TcpListener::bind()` with a manual `socket() → bind() → listen(backlog)` sequence
- Add `RCOMM_BACKLOG` environment variable (default: 128)

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Current State**:
- Line 26: `let listener = TcpListener::bind(&full_address).unwrap();`
- Uses `TcpListener::bind()` which internally calls `listen()` with a platform-default backlog
- No way to specify the backlog size

**Changes Required**:
- Replace `TcpListener::bind()` with manual socket creation
- Add `get_backlog()` configuration function
- Use `std::net::TcpListener::from` raw fd approach or `socket2` crate

---

## Approach Options

### Option A: Use `socket2` Crate
The `socket2` crate provides `Socket::listen(backlog)`. Clean API, but adds a dependency.

### Option B: Raw Syscalls via `std::os::unix`
Use `std::os::unix::io::FromRawFd` and raw libc calls. Zero dependencies but requires `unsafe`.

### Option C: Use `net2` / Nightly Features
Not recommended — `net2` is deprecated, nightly features are unstable.

**Recommended: Option A or B** depending on the dependency policy decision made in `tcp_keepalive.md`. If `libc` is already added, use it here too. Otherwise, the manual approach works.

---

## Step-by-Step Implementation

### Step 1: Add Configuration Function

**Location**: `src/main.rs`, after `get_address()`

```rust
fn get_backlog() -> i32 {
    std::env::var("RCOMM_BACKLOG")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(128)
}
```

### Step 2: Replace `TcpListener::bind()` with Manual Sequence

**Location**: `src/main.rs`, line 26

**Using `libc` (if added as dependency)**:

```rust
fn create_listener(address: &str, backlog: i32) -> TcpListener {
    use std::os::unix::io::FromRawFd;

    let addr: std::net::SocketAddr = address.parse().unwrap();

    let socket = unsafe {
        let fd = libc::socket(
            libc::AF_INET,
            libc::SOCK_STREAM,
            0,
        );
        if fd < 0 {
            panic!("Failed to create socket");
        }

        // Set SO_REUSEADDR to avoid "address already in use" on restart
        let enable: libc::c_int = 1;
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_REUSEADDR,
            &enable as *const _ as *const libc::c_void,
            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
        );

        fd
    };

    // Bind
    let sockaddr = match addr {
        std::net::SocketAddr::V4(v4) => {
            let ip_bytes = v4.ip().octets();
            let port = v4.port();
            libc::sockaddr_in {
                sin_family: libc::AF_INET as libc::sa_family_t,
                sin_port: port.to_be(),
                sin_addr: libc::in_addr {
                    s_addr: u32::from_ne_bytes(ip_bytes),
                },
                sin_zero: [0; 8],
            }
        }
        _ => panic!("IPv6 not supported yet"),
    };

    unsafe {
        let ret = libc::bind(
            socket,
            &sockaddr as *const libc::sockaddr_in as *const libc::sockaddr,
            std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
        );
        if ret < 0 {
            panic!("Failed to bind socket");
        }

        let ret = libc::listen(socket, backlog);
        if ret < 0 {
            panic!("Failed to listen on socket");
        }

        TcpListener::from_raw_fd(socket)
    }
}
```

**Simpler alternative (using `socket2` crate)**:

```rust
fn create_listener(address: &str, backlog: i32) -> TcpListener {
    use socket2::{Socket, Domain, Type, Protocol};

    let addr: std::net::SocketAddr = address.parse().unwrap();
    let socket = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP)).unwrap();
    socket.set_reuse_address(true).unwrap();
    socket.bind(&addr.into()).unwrap();
    socket.listen(backlog).unwrap();
    socket.into()
}
```

### Step 3: Update `main()` to Use New Function

**Current** (line 26):
```rust
let listener = TcpListener::bind(&full_address).unwrap();
```

**New**:
```rust
let backlog = get_backlog();
let listener = create_listener(&full_address, backlog);
println!("Listening on {full_address} (backlog: {backlog})");
```

---

## Edge Cases & Handling

### 1. Backlog Value Too Large
- The OS may silently cap the backlog to its maximum (e.g., `net.core.somaxconn` on Linux, default 4096)
- **Behavior**: No error; the OS uses its maximum. Document that the effective backlog may be lower than configured.

### 2. Backlog Value of 0
- Some OSes treat 0 as "use implementation default", others allow it
- **Behavior**: Accept it. If the user sets `RCOMM_BACKLOG=0`, the OS decides.

### 3. Negative Backlog Value
- `parse::<i32>()` would accept negative values
- **Behavior**: Validate and clamp to minimum of 1, or reject with a warning and use default

### 4. `SO_REUSEADDR` Bonus
- The manual socket creation is a good opportunity to also set `SO_REUSEADDR`, which prevents "address already in use" errors when restarting the server quickly
- `TcpListener::bind()` does NOT set this by default in Rust

### 5. IPv6 Support
- The raw approach shown above only handles IPv4
- For IPv6 support, add a `SocketAddr::V6` branch
- **Status**: Can be added later; current server only uses IPv4

---

## Implementation Checklist

- [ ] Add `get_backlog()` configuration function
- [ ] Implement `create_listener()` with configurable backlog
- [ ] Replace `TcpListener::bind()` with `create_listener()` in `main()`
- [ ] Add `SO_REUSEADDR` while at it
- [ ] Validate backlog value (reject negative, log effective value)
- [ ] Run `cargo build` to verify compilation
- [ ] Run `cargo test` and `cargo run --bin integration_test` to verify no regressions
- [ ] Manual test: verify backlog with `ss -tln` showing the listen queue

---

## Backward Compatibility

When `RCOMM_BACKLOG` is not set, the default of 128 matches the platform default used by `TcpListener::bind()`. Behavior is identical. The addition of `SO_REUSEADDR` is a bonus improvement that only affects server restart behavior (no more "address already in use" errors).
