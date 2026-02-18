# Fix TOCTOU Race in `pick_free_port()`

**Category:** Testing
**Complexity:** 3/10
**Necessity:** 4/10
**Status:** Planning

---

## Overview

The `pick_free_port()` function in `src/bin/integration_test.rs` has a Time-of-Check-Time-of-Use (TOCTOU) race condition. It binds a `TcpListener` to port 0 to get a free port, then immediately drops the listener, and passes the port number to the server. Between dropping the listener and the server binding to that port, another process could claim the port, causing the server to fail to start.

**Goal:** Eliminate the race condition by binding the listener once and passing the bound listener (or its port) directly to the server, without releasing the port in between.

---

## Current State

### pick_free_port() (integration_test.rs, lines 17-22)

```rust
fn pick_free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("failed to bind to port 0");
    let port = listener.local_addr().unwrap().port();
    drop(listener);  // <-- Port is released here
    port              // <-- Port might be taken by another process
}
```

### Server Start (integration_test.rs, lines 55-66)

```rust
fn start_server(port: u16) -> Child {
    let binary = find_server_binary();
    let project_root = find_project_root();
    Command::new(binary)
        .env("RCOMM_PORT", port.to_string())
        .env("RCOMM_ADDRESS", "127.0.0.1")
        .current_dir(project_root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to start rcomm server")
}
```

The server reads `RCOMM_PORT` from the environment and binds to that port (src/main.rs, lines 14-26).

### Race Window

```
Time →
[Test: bind port 0] → [Test: get port 54321] → [Test: drop listener] → ... → [Server: bind 54321]
                                                      ↑                    ↑
                                                 Port released     Another process could
                                                                   bind to 54321 here
```

### Practical Impact

In practice, this race is rare on a typical development machine. The OS usually doesn't reassign the same ephemeral port immediately. However:
- CI environments with parallel test suites are more susceptible
- Repeatedly running tests increases the probability
- Intermittent test failures are difficult to diagnose

---

## Solution Design

### Approach A: Pass `TcpListener` to the Server (Recommended)

Modify the server to accept a pre-bound `TcpListener` instead of binding its own. The test creates the listener, and the server inherits it.

**Problem:** The server is started as a separate process via `Command::new()`. You can't pass a `TcpListener` object across process boundaries directly.

**Workaround:** Use file descriptor inheritance:
1. Bind the listener in the test process
2. Pass the raw file descriptor to the child process via an environment variable
3. The server reconstructs the `TcpListener` from the file descriptor

### Approach B: Reserve Port via SO_REUSEADDR (Simpler)

Keep the current architecture but use `SO_REUSEADDR` to allow the server to bind to the port while the test still holds it:

1. Test binds to port 0 with `SO_REUSEADDR`
2. Server starts with `SO_REUSEADDR` and binds to the same port
3. Test drops its listener after server is confirmed ready

**Problem:** `SO_REUSEADDR` semantics vary across platforms, and `TcpListener::bind()` in Rust doesn't expose `SO_REUSEADDR` without using platform-specific APIs.

### Approach C: Retry Loop (Pragmatic)

Keep the current `pick_free_port()` but add a retry loop in `start_server()`:

1. Pick a free port
2. Start the server
3. If `wait_for_server()` fails, pick a new port and retry
4. Retry up to N times

### Approach D: File Descriptor Passing via Env (Unix-specific)

Use `std::os::unix::io::FromRawFd` to pass the socket:

1. Test creates a `TcpListener` on port 0
2. Set the fd to not close on exec (remove `FD_CLOEXEC`)
3. Pass the fd number to the server via env var
4. Server reconstructs the listener from the fd

---

## Recommended Implementation: Approach C (Retry Loop)

This is the most practical approach — it doesn't require modifying the server binary and works cross-platform.

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`

**Modify:** `pick_free_port()` and `start_server()` functions, or combine them into a single reliable function.

---

## Step-by-Step Implementation

### Step 1: Create a Combined Start Function with Retry

Replace the separate `pick_free_port()` + `start_server()` + `wait_for_server()` pattern with a single function:

```rust
fn start_server_with_retry(max_attempts: usize) -> (Child, String) {
    for attempt in 1..=max_attempts {
        let port = pick_free_port();
        let addr = format!("127.0.0.1:{port}");

        let mut server = start_server(port);

        match wait_for_server(&addr, Duration::from_secs(3)) {
            Ok(()) => {
                if attempt > 1 {
                    println!("Server started on attempt {attempt} (port {port})");
                }
                return (server, addr);
            }
            Err(e) => {
                eprintln!(
                    "Attempt {attempt}/{max_attempts}: failed to start on port {port}: {e}"
                );
                let _ = server.kill();
                let _ = server.wait();
            }
        }
    }

    panic!(
        "Failed to start server after {max_attempts} attempts (TOCTOU race in port selection?)"
    );
}
```

### Step 2: Update `main()` to Use the New Function

**Current:**
```rust
fn main() {
    let port = pick_free_port();
    let addr = format!("127.0.0.1:{port}");

    println!("Starting server on {addr}...");
    let mut server = start_server(port);

    if let Err(e) = wait_for_server(&addr, Duration::from_secs(5)) {
        eprintln!("ERROR: {e}");
        let _ = server.kill();
        std::process::exit(1);
    }
    println!("Server is ready.\n");
    // ...
}
```

**Updated:**
```rust
fn main() {
    println!("Starting server...");
    let (mut server, addr) = start_server_with_retry(5);
    println!("Server is ready on {addr}.\n");
    // ...
}
```

### Step 3: Keep `pick_free_port()` as Internal Helper

The function still works the same way, but it's now called inside the retry loop:

```rust
fn pick_free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("failed to bind to port 0");
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}
```

---

## Alternative: File Descriptor Passing (Approach D)

For a complete fix (no race at all), modify both the test and the server:

### Server Changes (src/main.rs)

```rust
fn main() {
    let listener = if let Ok(fd_str) = std::env::var("RCOMM_LISTENER_FD") {
        // Reconstruct listener from inherited file descriptor
        #[cfg(unix)]
        {
            use std::os::unix::io::FromRawFd;
            let fd: i32 = fd_str.parse().expect("bad RCOMM_LISTENER_FD");
            unsafe { TcpListener::from_raw_fd(fd) }
        }
        #[cfg(not(unix))]
        {
            panic!("RCOMM_LISTENER_FD not supported on this platform");
        }
    } else {
        let port = get_port();
        let address = get_address();
        TcpListener::bind(format!("{address}:{port}")).unwrap()
    };

    // ... rest of main unchanged
}
```

### Test Changes (integration_test.rs)

```rust
#[cfg(unix)]
fn start_server_with_listener() -> (Child, String) {
    use std::os::unix::io::AsRawFd;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind failed");
    let addr = listener.local_addr().unwrap().to_string();
    let fd = listener.as_raw_fd();

    // Clear FD_CLOEXEC so the child inherits the fd
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFD);
        libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC);
    }

    let binary = find_server_binary();
    let project_root = find_project_root();
    let server = Command::new(binary)
        .env("RCOMM_LISTENER_FD", fd.to_string())
        .current_dir(project_root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to start server");

    // Keep listener alive until server is ready, then drop
    wait_for_server(&addr, Duration::from_secs(5)).expect("server didn't start");
    drop(listener);

    (server, addr)
}
```

**Note:** This approach requires the `libc` crate or `nix` crate for `fcntl`, which conflicts with the project's "no external dependencies" philosophy. It could be done with raw `unsafe` system calls.

---

## Edge Cases & Considerations

### 1. Race Window Duration

**Analysis:** The race window is from `drop(listener)` until the server calls `TcpListener::bind()`. With `Command::spawn()`, this includes:
- Process creation (~1-5ms)
- Server initialization (~1ms)
- `TcpListener::bind()` call (~0.1ms)

Total window: ~2-6ms. Probability of collision is extremely low.

### 2. CI/Parallel Tests

**Risk:** Higher in CI environments running multiple test suites simultaneously.

**Mitigation:** The retry loop (Approach C) handles this transparently.

### 3. Port Reuse Timing

**Behavior:** On Linux, the OS's `TIME_WAIT` state typically prevents immediate port reuse for 60 seconds. However, since we're binding to `127.0.0.1` (loopback) and the kernel usually handles this efficiently, the risk is lower.

### 4. Cross-Platform

**Retry approach (C):** Works on all platforms.
**FD passing (D):** Unix-only (Linux, macOS). Windows would need a different mechanism (named pipes or similar).

---

## Testing Strategy

### Validating the Fix

```bash
# Run integration tests repeatedly to check for flakiness
for i in $(seq 1 20); do
    cargo run --bin integration_test || echo "FAILED on run $i"
done
```

### Expected Results

With the retry loop, all 20 runs should succeed even if the port race occurs occasionally.

---

## Implementation Checklist

- [ ] Add `start_server_with_retry()` function
- [ ] Update `main()` to use the new function
- [ ] Keep `pick_free_port()` and `start_server()` as internal helpers
- [ ] Test with repeated runs (20+ times)
- [ ] Run `cargo build` — no compiler errors
- [ ] Run `cargo run --bin integration_test` — all tests pass

---

## Related Features

- **No direct feature dependency** — this is an infrastructure improvement for the test suite

---

## References

- [TOCTOU Race Condition](https://en.wikipedia.org/wiki/Time-of-check_to_time-of-use)
- [Rust TcpListener::bind](https://doc.rust-lang.org/std/net/struct.TcpListener.html#method.bind)
- [Linux SO_REUSEADDR](https://man7.org/linux/man-pages/man7/socket.7.html)
