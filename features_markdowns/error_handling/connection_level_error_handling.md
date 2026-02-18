# Connection-Level Error Handling

**Feature**: Add connection-level error handling so a single bad request doesn't affect other connections
**Category**: Error Handling
**Complexity**: 3/10
**Necessity**: 8/10

---

## Overview

The rcomm server currently has several failure modes where a problem with one connection can impact the server's ability to handle other connections. The most critical issue is in the main accept loop (`src/main.rs` line 38), where `stream.unwrap()` panics on a failed connection accept, crashing the main thread and killing the entire server. Additionally, panics inside `handle_connection()` (from unwraps on file reads, writes, etc.) kill worker threads permanently, reducing the thread pool's capacity.

### Current State

**Accept loop** (`src/main.rs` lines 36-43):
```rust
for stream in listener.incoming() {
    let routes_clone = routes.clone();
    let stream = stream.unwrap();  // Panics on accept error — kills the server

    pool.execute(move || {
        handle_connection(stream, routes_clone);
    });
}
```

**Worker thread** (`src/lib.rs` lines 61-77):
```rust
let thread = thread::spawn(move || {
    loop {
        let message = reciever.lock().unwrap().recv();  // Panics on poisoned mutex

        match message {
            Ok(job) => {
                println!("Worker {id} got a job; executing.");
                job();  // If job panics, worker thread dies permanently
            }
            Err(_) => {
                println!("Worker {id} disconnected; shutting down.");
                break;
            }
        }
    }
});
```

### Failure Modes

1. **Accept error**: A failed `accept()` (e.g., file descriptor limit reached, `EMFILE`) panics the main thread, crashing the entire server.
2. **Worker panic propagation**: If `handle_connection()` panics (from any remaining unwrap), the worker thread dies. The thread pool loses capacity permanently.
3. **Mutex poisoning**: If a worker panics while holding the mutex lock, the mutex becomes poisoned and all subsequent workers panic when trying to lock it, cascading into total failure.
4. **Connection accept flood**: Under high load, rapid accept errors (e.g., from `ENFILE`) could crash the server before it can recover.

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/main.rs`

- Replace `stream.unwrap()` in accept loop with `match` and `continue`

### 2. `/home/jwall/personal/rusty/rcomm/src/lib.rs`

- Wrap `job()` execution in `std::panic::catch_unwind()` to prevent worker death
- Handle poisoned mutex gracefully in the worker loop

---

## Step-by-Step Implementation

### Step 1: Fix the Accept Loop

**Current code** (`src/main.rs` lines 36-43):
```rust
for stream in listener.incoming() {
    let routes_clone = routes.clone();
    let stream = stream.unwrap();

    pool.execute(move || {
        handle_connection(stream, routes_clone);
    });
}
```

**Updated code**:
```rust
for stream in listener.incoming() {
    let stream = match stream {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error accepting connection: {e}");
            continue;
        }
    };

    let routes_clone = routes.clone();
    pool.execute(move || {
        handle_connection(stream, routes_clone);
    });
}
```

**Rationale**:
- `TcpListener::incoming()` can yield errors for individual connection accepts (e.g., `EMFILE`, `ECONNABORTED`)
- Logging and continuing means the server survives transient OS errors
- Moved `routes_clone` after the match so it's only cloned for successful accepts

### Step 2: Add Panic Catch in Worker Thread

**Current code** (`src/lib.rs` lines 61-77):
```rust
let thread = thread::spawn(move || {
    loop {
        let message = reciever.lock().unwrap().recv();

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

**Updated code**:
```rust
let thread = thread::spawn(move || {
    loop {
        let message = match reciever.lock() {
            Ok(guard) => guard.recv(),
            Err(poisoned) => {
                eprintln!("Worker {id}: mutex poisoned, recovering");
                poisoned.into_inner().recv()
            }
        };

        match message {
            Ok(job) => {
                println!("Worker {id} got a job; executing.");
                if let Err(panic_info) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    job();
                })) {
                    eprintln!("Worker {id}: job panicked: {:?}", panic_info);
                }
            }
            Err(_) => {
                println!("Worker {id} disconnected; shutting down.");
                break;
            }
        }
    }
});
```

**Key changes**:

1. **Mutex poisoning recovery**: When another thread panics while holding the mutex, `lock()` returns `Err(PoisonError)`. We call `into_inner()` to get the underlying `MutexGuard` and continue operating. This is safe because the shared state (the `mpsc::Receiver`) is still valid — it's just a channel receiver, and poisoning doesn't corrupt it.

2. **Panic catch with `catch_unwind`**: Wraps `job()` in `std::panic::catch_unwind()` so that if `handle_connection()` panics (from any remaining unwrap or other bug), the worker thread survives and loops back to accept the next job. `AssertUnwindSafe` is needed because `Job` is `FnOnce() + Send` but not `UnwindSafe` by default.

### Step 3: Handle ThreadPool Execute Errors

**Current code** (`src/lib.rs` line 43):
```rust
self.sender.as_ref().unwrap().send(job).unwrap();
```

**Updated code**:
```rust
if let Some(sender) = self.sender.as_ref() {
    if let Err(e) = sender.send(job) {
        eprintln!("Failed to send job to worker pool: {e}");
    }
} else {
    eprintln!("Thread pool has been shut down, cannot execute job");
}
```

**Rationale**: After `Drop` takes the sender, `as_ref()` returns `None`. During shutdown, new jobs should be silently dropped rather than panicking.

### Step 4: Handle Worker Join Errors in Drop

**Current code** (`src/lib.rs` line 54):
```rust
worker.thread.join().unwrap();
```

**Updated code**:
```rust
match worker.thread.join() {
    Ok(()) => println!("Worker {} shut down cleanly", worker.id),
    Err(e) => eprintln!("Worker {} panicked during shutdown: {:?}", worker.id, e),
}
```

**Rationale**: A panicked worker's join returns `Err`. Logging instead of unwrapping ensures the Drop completes and remaining workers are also shut down.

---

## Edge Cases

### 1. File Descriptor Exhaustion (`EMFILE` / `ENFILE`)
**Scenario**: The system runs out of file descriptors.
**Before**: `stream.unwrap()` panics, server crashes.
**After**: Error is logged, server continues. When file descriptors are freed (by closed connections), new accepts succeed again.

### 2. Worker Thread Panic from Remaining `unwrap()`
**Scenario**: An uncaught `unwrap()` in `handle_connection()` panics.
**Before**: Worker thread dies permanently, pool loses capacity.
**After**: `catch_unwind()` catches the panic, logs it, and the worker loops back to accept new jobs.

### 3. Cascading Mutex Poisoning
**Scenario**: A worker panics while holding the mutex (between `lock()` and `recv()` completing).
**Before**: All other workers panic on their next `lock()` call, cascading into total pool failure.
**After**: Workers recover from poisoned mutex using `into_inner()` and continue operating.

### 4. Rapid Accept Errors
**Scenario**: OS returns errors for every accept call (e.g., network interface down).
**Handling**: The loop continues logging errors and retrying. Consider adding a brief sleep on repeated errors to avoid a tight log-spamming loop. This is an optional enhancement:
```rust
Err(e) => {
    eprintln!("Error accepting connection: {e}");
    std::thread::sleep(std::time::Duration::from_millis(10));
    continue;
}
```

### 5. Panic in `catch_unwind` with Non-Unwind Panic
**Scenario**: Panic runtime is set to `abort` instead of `unwind` (via `Cargo.toml` `[profile.*.panic]`).
**Handling**: `catch_unwind` has no effect when panic = abort. The default Rust panic strategy is `unwind`, so this works out of the box. Not a concern unless the user explicitly changes it.

---

## Testing Strategy

### Unit Tests

```rust
#[test]
fn thread_pool_survives_panicking_job() {
    let counter = Arc::new(AtomicUsize::new(0));

    let pool = ThreadPool::new(2);

    // Submit a panicking job
    pool.execute(|| {
        panic!("intentional test panic");
    });

    // Give the panicking job time to execute
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Submit a normal job after the panic
    let counter_clone = Arc::clone(&counter);
    pool.execute(move || {
        counter_clone.fetch_add(1, Ordering::SeqCst);
    });

    drop(pool);
    assert_eq!(counter.load(Ordering::SeqCst), 1, "Job after panic should still execute");
}
```

### Integration Tests

```rust
fn test_server_survives_bad_request(addr: &str) -> Result<(), String> {
    // Send garbage data
    let mut stream = TcpStream::connect(addr).map_err(|e| format!("connect: {e}"))?;
    stream.write_all(b"\x00\x01\x02\x03\r\n\r\n")
        .map_err(|e| format!("write: {e}"))?;
    drop(stream);

    // Brief pause
    std::thread::sleep(Duration::from_millis(50));

    // Verify server still works
    let resp = send_request(addr, "GET", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "status after bad request")?;
    Ok(())
}

fn test_server_survives_abrupt_disconnect(addr: &str) -> Result<(), String> {
    // Connect, send partial request, then disconnect
    let stream = TcpStream::connect(addr).map_err(|e| format!("connect: {e}"))?;
    drop(stream); // Disconnect immediately

    std::thread::sleep(Duration::from_millis(50));

    // Verify server still works
    let resp = send_request(addr, "GET", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "status after disconnect")?;
    Ok(())
}
```

---

## Implementation Checklist

- [ ] Replace `stream.unwrap()` in accept loop with `match` and `continue`
- [ ] Wrap `job()` in `std::panic::catch_unwind()` in Worker
- [ ] Handle poisoned mutex with `into_inner()` recovery
- [ ] Replace `sender.as_ref().unwrap().send(job).unwrap()` with graceful error handling
- [ ] Replace `worker.thread.join().unwrap()` with `match` in Drop
- [ ] Add unit test: thread pool survives panicking job
- [ ] Add integration test: server survives garbage input
- [ ] Add integration test: server survives abrupt client disconnect
- [ ] Run `cargo test` — no regressions
- [ ] Run `cargo run --bin integration_test` — all tests pass

---

## Backward Compatibility

No external API changes. The `ThreadPool::execute()` method signature stays the same (it currently returns `()` — we keep it returning `()` and log errors internally rather than changing to `Result`). All existing tests pass unchanged. The only behavioral change is that failure modes that previously crashed now log and recover.

---

## Related Features

- **Security > Replace All unwrap() Calls**: This feature addresses the connection-handling and thread pool unwraps specifically
- **Error Handling > handle_connection File Read 500**: Reduces the number of panics that `catch_unwind` needs to catch
- **Error Handling > TCP Write Error Recovery**: Together these two features make `handle_connection()` fully panic-free
- **Thread Pool > Worker Panic Recovery**: This feature directly implements worker panic recovery
- **Thread Pool > Graceful Shutdown**: The Drop improvements here support graceful shutdown
