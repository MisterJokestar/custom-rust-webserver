# TCP Keepalive Socket Options Implementation Plan

## Overview

TCP keepalive is an operating system–level mechanism that periodically sends probe packets on idle TCP connections to detect dead peers. Without it, a connection where the client disappears (network failure, crash, etc.) remains open on the server side indefinitely, consuming a worker thread and file descriptor.

Currently, `src/main.rs` creates a `TcpListener` and accepts `TcpStream` connections without configuring any socket options. This feature adds `SO_KEEPALIVE` (and related parameters) to accepted connections so the OS can detect and clean up dead connections automatically.

**Complexity**: 2
**Necessity**: 4

**Key Changes**:
- Enable `SO_KEEPALIVE` on each accepted `TcpStream`
- Optionally configure keepalive interval and probe count via the `socket2` crate or raw libc calls (or use Rust std's limited API)
- Consider making keepalive parameters configurable via environment variables

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Current State**:
- Line 38: `let stream = stream.unwrap();` — raw `TcpStream` with no socket configuration
- No socket options set anywhere

**Changes Required**:
- After accepting a stream, call `stream.set_keepalive()` or equivalent
- Rust's `std::net::TcpStream` does not expose `set_keepalive()` directly in stable Rust — alternatives are needed

---

## Approach Options

### Option A: Use `socket2` Crate
The `socket2` crate provides cross-platform socket option APIs including keepalive with idle time, interval, and retry count. However, rcomm has **no external dependencies**, and adding one for a single socket option may not be justified.

### Option B: Use Rust's `std` API (Limited)
As of Rust edition 2024, `TcpStream` does not expose keepalive directly in std. However, `TcpStream` implements `AsRawFd` (Unix) / `AsRawSocket` (Windows), allowing raw syscalls.

### Option C: Raw `libc` Calls (Recommended)
Use `libc`-style raw syscalls via `std::os::unix::io::AsRawFd` and `unsafe` setsockopt. This keeps the zero-dependency guarantee while providing full control. Since rcomm targets Linux, this is straightforward.

**Recommended: Option C** — maintains zero dependencies, full control, acceptable complexity for a learning project.

---

## Step-by-Step Implementation

### Step 1: Add a `configure_stream()` Helper

**Location**: `src/main.rs`, before `handle_connection()`

```rust
#[cfg(unix)]
fn configure_stream(stream: &TcpStream) {
    use std::os::unix::io::AsRawFd;

    let fd = stream.as_raw_fd();

    unsafe {
        // Enable SO_KEEPALIVE
        let enable: libc::c_int = 1;
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_KEEPALIVE,
            &enable as *const _ as *const libc::c_void,
            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
        );

        // Set keepalive idle time (seconds before first probe)
        let idle: libc::c_int = 60;
        libc::setsockopt(
            fd,
            libc::IPPROTO_TCP,
            libc::TCP_KEEPIDLE,
            &idle as *const _ as *const libc::c_void,
            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
        );

        // Set keepalive probe interval (seconds between probes)
        let interval: libc::c_int = 10;
        libc::setsockopt(
            fd,
            libc::IPPROTO_TCP,
            libc::TCP_KEEPINTVL,
            &interval as *const _ as *const libc::c_void,
            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
        );

        // Set keepalive probe count (number of failed probes before closing)
        let count: libc::c_int = 5;
        libc::setsockopt(
            fd,
            libc::IPPROTO_TCP,
            libc::TCP_KEEPCNT,
            &count as *const _ as *const libc::c_void,
            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
        );
    }
}
```

**Note on `libc` dependency**: The constants `SOL_SOCKET`, `SO_KEEPALIVE`, `TCP_KEEPIDLE`, etc. are from the `libc` crate. Since rcomm has no external dependencies, there are two sub-options:

**Sub-option C1**: Add `libc` as a dependency (it's a thin FFI binding, universally used, arguably not a "real" dependency).

**Sub-option C2**: Define the constants manually:
```rust
#[cfg(target_os = "linux")]
mod socket_constants {
    pub const SOL_SOCKET: i32 = 1;
    pub const SO_KEEPALIVE: i32 = 9;
    pub const IPPROTO_TCP: i32 = 6;
    pub const TCP_KEEPIDLE: i32 = 4;
    pub const TCP_KEEPINTVL: i32 = 5;
    pub const TCP_KEEPCNT: i32 = 6;
}
```

And use raw `unsafe` FFI:
```rust
extern "C" {
    fn setsockopt(
        socket: i32,
        level: i32,
        optname: i32,
        optval: *const std::ffi::c_void,
        optlen: u32,
    ) -> i32;
}
```

**Recommendation**: Use `libc` crate (Sub-option C1). It's the standard way to do syscalls in Rust, adds minimal overhead, and is maintained by the Rust project. If the strict zero-dependency policy is firm, use Sub-option C2.

### Step 2: Call `configure_stream()` After Accept

**Location**: `src/main.rs`, in the listener loop (after line 38)

**Current**:
```rust
let stream = stream.unwrap();

pool.execute(move || {
    handle_connection(stream, routes_clone);
});
```

**New**:
```rust
let stream = stream.unwrap();
configure_stream(&stream);

pool.execute(move || {
    handle_connection(stream, routes_clone);
});
```

### Step 3: Optional — Make Parameters Configurable

```rust
fn get_keepalive_idle() -> i32 {
    std::env::var("RCOMM_KEEPALIVE_IDLE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60)
}

fn get_keepalive_interval() -> i32 {
    std::env::var("RCOMM_KEEPALIVE_INTERVAL")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10)
}

fn get_keepalive_count() -> i32 {
    std::env::var("RCOMM_KEEPALIVE_COUNT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5)
}
```

---

## Edge Cases & Handling

### 1. `setsockopt` Fails
- Returns -1 on error (e.g., invalid fd, unsupported option)
- **Behavior**: Silently ignore. Keepalive is a best-effort optimization; failure is non-fatal
- Optionally log a warning: `eprintln!("Warning: failed to set SO_KEEPALIVE")`

### 2. Platform Portability
- `TCP_KEEPIDLE`, `TCP_KEEPINTVL`, `TCP_KEEPCNT` are Linux-specific
- macOS uses `TCP_KEEPALIVE` instead of `TCP_KEEPIDLE`
- Windows uses `SIO_KEEPALIVE_VALS` ioctl
- **Status**: Use `#[cfg(target_os = "linux")]` for Linux-specific options; fall back to just `SO_KEEPALIVE` on other platforms

### 3. Interaction with HTTP Pipelining
- Keepalive probes only fire on idle connections. During active request/response exchange, TCP traffic acts as implicit keepalive
- After pipelining is implemented, keepalive becomes more important for detecting dead peers during idle periods between request bursts

### 4. Very Short Keepalive Values
- Setting idle time too low (e.g., 1 second) generates unnecessary network traffic
- Default of 60 seconds idle, 10 seconds interval, 5 retries = dead peer detected within ~110 seconds

---

## Implementation Checklist

- [ ] Decide on dependency approach: add `libc` crate vs. manual constants
- [ ] Implement `configure_stream()` with `SO_KEEPALIVE` and keepalive parameters
- [ ] Call `configure_stream()` after accepting each connection in the listener loop
- [ ] Add `#[cfg]` guards for platform-specific constants
- [ ] Optionally add environment variable configuration for keepalive parameters
- [ ] Run `cargo build` to verify compilation
- [ ] Run `cargo test` and `cargo run --bin integration_test` to verify no regressions
- [ ] Manual test: verify keepalive is set with `ss -tno` or `netstat -to` on a live connection

---

## Backward Compatibility

No behavioral changes for normal operation. Connections that were previously left open indefinitely after a client crash will now be cleaned up by the OS after the keepalive timeout expires. All existing tests pass unchanged.
