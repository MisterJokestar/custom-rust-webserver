# Unix Domain Socket Listening Implementation Plan

## Overview

Unix domain sockets (UDS) provide inter-process communication on the same machine without the overhead of TCP/IP networking (no loopback interface, no TCP handshake, no checksums). They are commonly used for:
- Reverse proxy setups (nginx/Caddy → application server via UDS)
- Local-only services that don't need network exposure
- Higher throughput for local connections vs TCP loopback

This feature adds support for listening on a Unix domain socket path (e.g., `/tmp/rcomm.sock`) in addition to or instead of TCP. The same request handling logic is reused — only the listener and stream types differ.

**Complexity**: 4
**Necessity**: 2

**Key Changes**:
- Add `RCOMM_SOCKET` environment variable for the UDS path
- Create a `UnixListener` when `RCOMM_SOCKET` is set
- Abstract over `TcpStream` and `UnixStream` so `handle_connection()` works with both
- Handle socket file cleanup on startup and shutdown

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Current State**:
- Imports: `net::{TcpListener, TcpStream}` — TCP only
- Line 26: `TcpListener::bind()` — TCP only
- Line 46: `handle_connection(mut stream: TcpStream, ...)` — typed to `TcpStream`
- Lines 36-43: `listener.incoming()` — `TcpListener`-specific

**Changes Required**:
- Add `std::os::unix::net::{UnixListener, UnixStream}` imports
- Abstract `handle_connection()` to accept any `Read + Write` stream
- Add UDS listener creation and accept loop
- Handle socket file lifecycle (remove stale socket, clean up on exit)

### 2. `/home/jwall/personal/rusty/rcomm/src/models/http_request.rs`

**Current State**:
- `build_from_stream()` takes `&TcpStream` specifically
- Needs to accept any readable stream

**Changes Required**:
- Change `build_from_stream()` to accept `&impl Read` or a trait object `&mut dyn Read`

---

## Step-by-Step Implementation

### Step 1: Generalize `handle_connection()` Over Stream Type

The core challenge is that `TcpStream` and `UnixStream` are different types but both implement `Read + Write`. The simplest approach is to make `handle_connection()` generic or use trait objects.

**Option A: Generic Function**

```rust
fn handle_connection<S: Read + Write>(mut stream: S, routes: Arc<HashMap<String, PathBuf>>) {
    // ... existing body unchanged, since it only uses Read + Write traits ...
}
```

**Option B: Trait Object**

```rust
fn handle_connection(stream: &mut (dyn Read + Write), routes: Arc<HashMap<String, PathBuf>>) {
    // ...
}
```

**Recommended: Option A** — zero runtime cost, no `dyn` dispatch overhead, and the compiler monomorphizes for each stream type.

### Step 2: Generalize `build_from_stream()`

**Location**: `src/models/http_request.rs`

**Current Signature**:
```rust
pub fn build_from_stream(stream: &TcpStream) -> Result<HttpRequest, HttpParseError>
```

**New Signature**:
```rust
pub fn build_from_stream(stream: &(impl Read + std::io::BufRead)) -> Result<HttpRequest, HttpParseError>
```

Or wrap the stream in a `BufReader` internally:
```rust
pub fn build_from_stream(stream: impl Read) -> Result<HttpRequest, HttpParseError> {
    let reader = BufReader::new(stream);
    // ... parse from reader ...
}
```

**Note**: Need to check what the current implementation does with the stream. If it already wraps in `BufReader`, just change the parameter type. If it calls `TcpStream`-specific methods, those need to be replaced with trait methods.

### Step 3: Add Configuration

**Location**: `src/main.rs`

```rust
fn get_socket_path() -> Option<String> {
    std::env::var("RCOMM_SOCKET").ok()
}
```

### Step 4: Add Unix Listener Setup

**Location**: `src/main.rs`, in `main()`

```rust
use std::os::unix::net::UnixListener;

if let Some(socket_path) = get_socket_path() {
    // Remove stale socket file if it exists
    if Path::new(&socket_path).exists() {
        fs::remove_file(&socket_path).unwrap();
    }

    let unix_listener = UnixListener::bind(&socket_path).unwrap();
    println!("Listening on unix:{socket_path}");

    for stream in unix_listener.incoming() {
        let routes_clone = Arc::clone(&routes);
        let stream = stream.unwrap();

        pool.execute(move || {
            handle_connection(stream, routes_clone);
        });
    }
} else {
    // Existing TCP listener path
    let port = get_port();
    let address = get_address();
    let full_address = format!("{address}:{port}");
    let listener = TcpListener::bind(&full_address).unwrap();

    for stream in listener.incoming() {
        let routes_clone = Arc::clone(&routes);
        let stream = stream.unwrap();

        pool.execute(move || {
            handle_connection(stream, routes_clone);
        });
    }
}
```

### Step 5: Socket File Cleanup on Shutdown

Add cleanup when the server exits to remove the socket file:

```rust
// In main(), after the listener loop (or via a Drop guard)
struct SocketCleanup {
    path: Option<String>,
}

impl Drop for SocketCleanup {
    fn drop(&mut self) {
        if let Some(ref path) = self.path {
            let _ = fs::remove_file(path);
            println!("Removed socket file: {path}");
        }
    }
}

// Usage:
let _cleanup = SocketCleanup { path: get_socket_path() };
```

### Step 6: Support Both TCP and UDS Simultaneously (Optional Enhancement)

If both `RCOMM_LISTEN` (TCP) and `RCOMM_SOCKET` (UDS) are set, the server could listen on both. This would require the multi-listener approach from `multi_address_binding.md`:

```rust
// Spawn a thread for TCP listener
let tcp_handle = std::thread::spawn(move || {
    for stream in tcp_listener.incoming() { /* ... */ }
});

// Spawn a thread for UDS listener
let uds_handle = std::thread::spawn(move || {
    for stream in unix_listener.incoming() { /* ... */ }
});
```

This is optional for the initial implementation — start with either/or.

---

## Edge Cases & Handling

### 1. Stale Socket File Exists
- A previous server instance crashed without cleaning up the socket file
- `UnixListener::bind()` fails with "Address already in use"
- **Behavior**: Remove the existing socket file before binding (Step 4 handles this)
- **Risk**: If another server instance is actually running and using that socket, removing it will break that instance. Could check with a connect attempt first.

### 2. Socket Path Directory Doesn't Exist
- `RCOMM_SOCKET=/nonexistent/dir/rcomm.sock`
- **Behavior**: `UnixListener::bind()` fails. Panic with a clear error message.

### 3. Permission Denied
- Socket path is in a directory the user doesn't have write access to
- **Behavior**: `UnixListener::bind()` fails. Panic with the OS error message.

### 4. Socket File Permissions
- By default, Unix domain sockets inherit umask permissions
- For security, the socket should be readable/writable only by the server user
- **Optional**: Set socket file permissions after binding with `fs::set_permissions()`

### 5. HTTP Host Header
- Clients connecting via UDS typically still send a `Host` header, but it may be empty or arbitrary
- **Behavior**: No impact on current routing (Host header is not checked)

### 6. Platform Portability
- `std::os::unix::net` is Unix-only (Linux, macOS, BSDs)
- **Behavior**: Use `#[cfg(unix)]` to conditionally compile UDS support. On Windows, this feature is unavailable.

### 7. Client IP Logging
- UDS connections have no remote IP address (they use `peer_cred` for process identity)
- **Impact**: Any future access logging that includes client IP will need to handle UDS connections differently

---

## Implementation Checklist

- [ ] Generalize `handle_connection()` to accept generic `Read + Write` streams
- [ ] Generalize `build_from_stream()` to accept generic `Read` streams
- [ ] Add `get_socket_path()` configuration function
- [ ] Add `UnixListener` creation with stale socket cleanup
- [ ] Add `SocketCleanup` drop guard for shutdown cleanup
- [ ] Use `#[cfg(unix)]` for UDS-specific code
- [ ] Run `cargo build` to verify compilation
- [ ] Run `cargo test` to verify unit tests pass (generic function should work for both types)
- [ ] Run `cargo run --bin integration_test` for TCP regression tests
- [ ] Manual test: `RCOMM_SOCKET=/tmp/rcomm.sock cargo run` then `curl --unix-socket /tmp/rcomm.sock http://localhost/`

---

## Backward Compatibility

When `RCOMM_SOCKET` is not set, behavior is identical to current — the server listens on TCP only. The generalization of `handle_connection()` from `TcpStream` to a generic `Read + Write` is a transparent refactor with no behavioral changes. All existing tests pass unchanged.
