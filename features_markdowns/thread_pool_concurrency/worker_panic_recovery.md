# Worker Thread Panic Recovery

**Feature**: Add worker thread panic recovery — restart crashed workers instead of losing pool capacity
**Category**: Thread Pool & Concurrency
**Complexity**: 5/10
**Necessity**: 8/10

---

## Overview

When a worker thread panics (due to an `unwrap()` on a failed file read, a write error, or any other unhandled error), the thread dies permanently. The thread pool loses capacity with each panic, eventually degrading to zero workers and a completely unresponsive server.

### Current State

**`src/lib.rs` lines 59-79** (Worker):
```rust
impl Worker {
    fn new(id: usize, reciever: Arc<Mutex<mpsc::Receiver<Job>>>) -> Worker {
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

        Worker { id, thread }
    }
}
```

### Panic Sources in Current Code

The worker runs the `job()` closure, which is `handle_connection()` from `main.rs`. Current panic points inside that closure:

1. **Line 64** (`main.rs`): `routes.get(&clean_target).unwrap().to_str().unwrap()` — panics if route value isn't valid UTF-8
2. **Line 70** (`main.rs`): `fs::read_to_string(filename).unwrap()` — panics if file read fails (permissions, deleted file, binary file)
3. **Line 74** (`main.rs`): `stream.write_all(&response.as_bytes()).unwrap()` — panics if client disconnected
4. **Line 63** (`lib.rs`): `reciever.lock().unwrap()` — panics if the mutex is poisoned (which happens when another worker panics while holding the lock)

**The mutex poisoning cascade is particularly dangerous**: Worker A panics while holding the mutex → mutex becomes poisoned → Worker B calls `lock().unwrap()` → panic → Worker C panics → all workers dead.

### Impact

- Each panic permanently reduces the thread pool by one worker
- Mutex poisoning can cascade and kill all workers in rapid succession
- The server becomes unresponsive with no indication of why
- The main listener loop keeps accepting connections and enqueueing jobs, but no workers process them

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/lib.rs`

- Wrap `job()` in `std::panic::catch_unwind()` inside the worker loop
- Handle poisoned mutex gracefully
- Optionally: restructure `Worker` to support thread respawning

---

## Step-by-Step Implementation

### Approach: `catch_unwind` Inside Worker Loop

Rather than respawning threads (which requires restructuring the `Worker` to hold an `Option<JoinHandle>`), the simpler and more robust approach is to catch panics inside the worker loop so the thread never dies.

### Step 1: Wrap Job Execution in `catch_unwind`

**Current** (`src/lib.rs` worker loop):
```rust
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
```

**New**:
```rust
match message {
    Ok(job) => {
        println!("Worker {id} got a job; executing.");
        if let Err(panic_info) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            job();
        })) {
            let msg = if let Some(s) = panic_info.downcast_ref::<&str>() {
                s.to_string()
            } else if let Some(s) = panic_info.downcast_ref::<String>() {
                s.clone()
            } else {
                "unknown panic payload".to_string()
            };
            eprintln!("Worker {id} recovered from panic: {msg}");
        }
    }
    Err(_) => {
        println!("Worker {id} disconnected; shutting down.");
        break;
    }
}
```

**Key points**:
- `catch_unwind` catches panics from `unwrap()`, `panic!()`, array out-of-bounds, etc.
- `AssertUnwindSafe` is needed because `Job` (`Box<dyn FnOnce()>`) doesn't implement `UnwindSafe`. This is safe here because we don't access any shared mutable state after the panic — we just loop back and wait for the next job.
- The panic payload is extracted as a string for logging. Panics carry `Box<dyn Any + Send>`, which is usually a `&str` (from `panic!("msg")`) or `String` (from `unwrap()` errors).
- After catching the panic, the worker continues looping — it's immediately available for the next job.

### Step 2: Handle Poisoned Mutex

When a panic occurs while the mutex is held, the mutex becomes "poisoned." Subsequent `lock()` calls return `Err(PoisonError)`. The current code panics on this with `.unwrap()`.

**Current**:
```rust
let message = reciever.lock().unwrap().recv();
```

**New**:
```rust
let lock = match reciever.lock() {
    Ok(guard) => guard,
    Err(poisoned) => {
        eprintln!("Worker {id}: mutex was poisoned, recovering");
        poisoned.into_inner()
    }
};
let message = lock.recv();
```

`PoisonError::into_inner()` gives access to the underlying `MutexGuard` despite the poisoning. This is safe because:
- The receiver itself is not corrupted — `mpsc::Receiver` is a well-defined state machine
- The panic happened in a job closure, not in the channel operations
- We just need to call `recv()` on the receiver, which is independent of whatever state the panicking job left

### Step 3: Complete Updated Worker

```rust
impl Worker {
    fn new(id: usize, reciever: Arc<Mutex<mpsc::Receiver<Job>>>) -> Worker {
        let thread = thread::spawn(move || {
            loop {
                let lock = match reciever.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => {
                        eprintln!("Worker {id}: mutex was poisoned, recovering");
                        poisoned.into_inner()
                    }
                };
                let message = lock.recv();
                drop(lock); // Release the mutex before running the job

                match message {
                    Ok(job) => {
                        println!("Worker {id} got a job; executing.");
                        if let Err(panic_info) = std::panic::catch_unwind(
                            std::panic::AssertUnwindSafe(|| {
                                job();
                            })
                        ) {
                            let msg = if let Some(s) = panic_info.downcast_ref::<&str>() {
                                s.to_string()
                            } else if let Some(s) = panic_info.downcast_ref::<String>() {
                                s.clone()
                            } else {
                                "unknown panic payload".to_string()
                            };
                            eprintln!("Worker {id} recovered from panic: {msg}");
                        }
                    }
                    Err(_) => {
                        println!("Worker {id} disconnected; shutting down.");
                        break;
                    }
                }
            }
        });

        Worker { id, thread }
    }
}
```

**Critical detail**: `drop(lock)` releases the mutex **before** running the job. This ensures that if `job()` panics, the mutex is NOT held, and therefore is NOT poisoned. This eliminates the cascade problem entirely.

Wait — looking at the current code more carefully:

```rust
let message = reciever.lock().unwrap().recv();
```

The `MutexGuard` is a temporary here. It's dropped at the end of the statement (after `.recv()` returns), so the mutex is released before `job()` runs. This means the current code does NOT hold the mutex during job execution, and panics in jobs do NOT poison the mutex.

However, there's still a subtle issue: if `recv()` itself panics (it shouldn't, but hypothetically), or if the `MutexGuard` drop panics, the mutex would be poisoned. The explicit `lock()`/`drop()` pattern makes the release point unambiguous.

**Revised understanding**: The poisoned mutex handling is a defensive measure. The primary value of this feature is `catch_unwind` around `job()`.

### Step 4: Add Imports

Add to the imports in `src/lib.rs`:

```rust
use std::panic::{catch_unwind, AssertUnwindSafe};
```

---

## Edge Cases & Handling

### 1. Panic in File Read (`unwrap()` on `read_to_string`)
**Scenario**: A routed file is deleted between route building and request serving.
**Before**: Worker panics, thread dies, pool loses capacity.
**After**: Panic is caught, error logged, worker continues. Client connection is dropped (no response sent), but the server remains healthy.

### 2. Panic in Response Write (`unwrap()` on `write_all`)
**Scenario**: Client disconnects before the response is sent.
**Before**: Worker panics, thread dies.
**After**: Panic caught, logged. Worker survives.

### 3. Double Panic
If the `catch_unwind` handler itself panics (e.g., the `eprintln!` macro fails), the thread will abort. This is extremely unlikely since `eprintln!` only fails if stderr is closed.

### 4. Panic While Holding External Locks
If a job acquires a lock (e.g., `Mutex`) and panics while holding it, that mutex becomes poisoned. `catch_unwind` doesn't prevent this — it just keeps the worker thread alive. The poisoned external lock is a separate concern addressed by proper error handling in the job closures.

### 5. Stack Overflow
`catch_unwind` does NOT catch stack overflows. On Linux, stack overflow triggers `SIGSEGV`, which kills the thread regardless. This is rare and would require extremely deep recursion.

### 6. Abort-on-Panic Configuration
If the binary is compiled with `panic = "abort"` in `Cargo.toml`, `catch_unwind` is a no-op — the process aborts on any panic. The current `Cargo.toml` uses the default `panic = "unwind"`, so `catch_unwind` works correctly.

---

## Testing Strategy

### Unit Tests

```rust
#[test]
fn worker_survives_panic_in_job() {
    let counter = Arc::new(AtomicUsize::new(0));

    let pool = ThreadPool::new(2);

    // Submit a job that panics
    pool.execute(|| {
        panic!("intentional test panic");
    });

    // Brief pause to let the panic be caught
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Submit a normal job — should still work
    let counter_clone = Arc::clone(&counter);
    pool.execute(move || {
        counter_clone.fetch_add(1, Ordering::SeqCst);
    });

    drop(pool);
    assert_eq!(counter.load(Ordering::SeqCst), 1, "Worker should recover and process next job");
}

#[test]
fn worker_survives_multiple_panics() {
    let counter = Arc::new(AtomicUsize::new(0));

    let pool = ThreadPool::new(2);

    // Submit several panicking jobs
    for _ in 0..10 {
        pool.execute(|| {
            panic!("intentional test panic");
        });
    }

    std::thread::sleep(std::time::Duration::from_millis(200));

    // All workers should still be alive
    for _ in 0..4 {
        let c = Arc::clone(&counter);
        pool.execute(move || {
            c.fetch_add(1, Ordering::SeqCst);
        });
    }

    drop(pool);
    assert_eq!(counter.load(Ordering::SeqCst), 4, "All workers should survive panics");
}
```

### Integration Tests

```rust
fn test_server_survives_panic_inducing_requests(addr: &str) -> Result<(), String> {
    // This tests the end-to-end behavior. If the server has routes that
    // trigger unwrap() panics (e.g., deleted files), the server should
    // continue serving other requests.

    // Send many valid requests after potential panic triggers
    for _ in 0..20 {
        let resp = send_request(addr, "GET", "/")?;
        assert_eq_or_err(&resp.status_code, &200, "server should keep responding")?;
    }
    Ok(())
}
```

---

## Implementation Checklist

- [ ] Add `std::panic::{catch_unwind, AssertUnwindSafe}` imports
- [ ] Wrap `job()` call in `catch_unwind(AssertUnwindSafe(|| { job() }))`
- [ ] Extract and log panic message from `Box<dyn Any + Send>`
- [ ] Add poisoned mutex recovery with `poisoned.into_inner()`
- [ ] Ensure mutex is released before job execution (explicit `drop(lock)`)
- [ ] Add unit test: worker survives single panic
- [ ] Add unit test: worker survives multiple panics
- [ ] Run `cargo build` — verify compilation
- [ ] Run `cargo test` — all tests pass (including new panic tests)
- [ ] Run `cargo run --bin integration_test` — all integration tests pass

---

## Backward Compatibility

No public API changes. The only behavioral change is that worker panics are caught and logged instead of killing the thread. Previously, a panicking job would silently reduce pool capacity. Now it logs a clear error and continues. All existing tests pass unchanged.

---

## Related Features

- **Error Handling > Replace `unwrap()` Calls**: Addresses the root cause of panics. This feature is the safety net for any remaining or future `unwrap()` calls.
- **Error Handling > `handle_connection()` File Read 500**: Replacing `unwrap()` with proper error handling eliminates the most common panic source.
- **Error Handling > TCP Write Error Recovery**: Replacing write `unwrap()` eliminates another panic source.
- **Thread Pool > Name Worker Threads**: Named threads make panic recovery logs more informative (which worker panicked).
- **Thread Pool > Fix Mutex-Blocking-Recv**: If per-worker channels are used, mutex poisoning is no longer a concern, but `catch_unwind` is still needed for job panics.
- **Logging & Observability > Error Detail Logging**: Panic recovery events should be included in structured logs.
