# Name Worker Threads for Debugging

**Feature**: Name worker threads for easier debugging (e.g. `rcomm-worker-0`)
**Category**: Thread Pool & Concurrency
**Complexity**: 1/10
**Necessity**: 4/10

---

## Overview

Worker threads are currently spawned with default names via `thread::spawn()`. In debuggers (`gdb`, `lldb`), profilers (`perf`, `flamegraph`), and system tools (`top -H`, `htop`, `ps -eLf`), these show up as unnamed threads, making it difficult to distinguish rcomm workers from each other or from threads in other processes.

Using `thread::Builder::new().name(...)` gives each worker a descriptive name visible in all these tools.

### Current State

**`src/lib.rs` lines 61-77** (Worker):
```rust
fn new(id: usize, reciever: Arc<Mutex<mpsc::Receiver<Job>>>) -> Worker {
    let thread = thread::spawn(move || {
        loop {
            let message = reciever.lock().unwrap().recv();
            // ...
        }
    });

    Worker { id, thread }
}
```

`thread::spawn()` creates a thread with no name. The OS sees it as an anonymous thread inheriting the process name.

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/lib.rs`

**Lines 61-77**: Replace `thread::spawn()` with `thread::Builder::new().name(...).spawn()`

---

## Step-by-Step Implementation

### Step 1: Replace `thread::spawn()` with Named Thread Builder

**Current** (`src/lib.rs` lines 61-77):
```rust
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
```

**New**:
```rust
fn new(id: usize, reciever: Arc<Mutex<mpsc::Receiver<Job>>>) -> Worker {
    let thread_name = format!("rcomm-worker-{id}");
    let thread = thread::Builder::new()
        .name(thread_name)
        .spawn(move || {
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
        })
        .expect("Failed to spawn worker thread");

    Worker { id, thread }
}
```

### Key Differences from `thread::spawn()`

- `thread::spawn()` returns `JoinHandle<T>` directly
- `thread::Builder::new().spawn()` returns `io::Result<JoinHandle<T>>` — it can fail if the OS refuses to create the thread (e.g., resource limits)
- `.expect("Failed to spawn worker thread")` panics with a clear message if thread creation fails, which is the correct behavior during pool initialization — if we can't create workers, the server can't function

### Thread Name Format

`rcomm-worker-{id}` where `id` is 0-indexed:
- `rcomm-worker-0`
- `rcomm-worker-1`
- `rcomm-worker-2`
- `rcomm-worker-3`

The `rcomm-` prefix distinguishes these from threads in other processes or libraries. Note: on Linux, `pthread_setname_np` truncates names to 15 characters. `rcomm-worker-0` is exactly 14 characters, and `rcomm-worker-99` is 15 — so this naming works for up to 99 workers (plus `rcomm-worker-100` at 16 chars would be truncated to `rcomm-worker-10` on Linux). For pools larger than 100, a shorter format like `rcomm-w-{id}` could be used, but this is unlikely to matter in practice.

---

## Edge Cases & Handling

### 1. Thread Spawn Failure
`Builder::spawn()` can fail with `io::Error` if the OS can't create the thread (out of memory, thread limit). The `.expect()` panics, which is correct — if we can't spawn workers, `ThreadPool::new()` should fail loudly during startup.

### 2. Name Truncation on Linux
Linux limits thread names to 15 bytes via `pthread_setname_np`. Names longer than 15 characters are silently truncated. Worker IDs 0-99 produce names of 14-15 characters, which fit. Worker IDs 100+ produce 16+ characters, resulting in truncated names visible in `top`/`htop`. This is cosmetic only and doesn't affect functionality.

### 3. Access from Within the Thread
Workers can access their own thread name via `thread::current().name()`, which can be used in log messages:
```rust
// Inside the worker thread:
let name = thread::current().name().unwrap_or("unknown").to_string();
println!("[{name}] got a job; executing.");
```

This is a natural follow-up but not part of this minimal feature.

---

## Testing Strategy

### Unit Test

```rust
#[test]
fn worker_threads_are_named() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let name_found = Arc::new(AtomicBool::new(false));
    let name_found_clone = Arc::clone(&name_found);

    let pool = ThreadPool::new(1);
    pool.execute(move || {
        let name = std::thread::current().name().unwrap_or("").to_string();
        if name == "rcomm-worker-0" {
            name_found_clone.store(true, Ordering::SeqCst);
        }
    });

    drop(pool);
    assert!(name_found.load(Ordering::SeqCst), "Worker thread should be named rcomm-worker-0");
}
```

### Manual Verification

```bash
cargo run &
# In another terminal:
ps -eLf | grep rcomm
# Or:
top -H -p $(pgrep rcomm)
# Worker threads should show as rcomm-worker-0, rcomm-worker-1, etc.
```

---

## Implementation Checklist

- [ ] Replace `thread::spawn()` with `thread::Builder::new().name(...).spawn().expect(...)` in `Worker::new()`
- [ ] Run `cargo build` — verify compilation
- [ ] Run `cargo test` — all existing tests pass
- [ ] Run `cargo run --bin integration_test` — all integration tests pass
- [ ] Manually verify thread names in `top -H` or `ps -eLf`

---

## Backward Compatibility

No API or behavioral changes. The only difference is that worker threads have OS-visible names. All existing tests pass unchanged.

---

## Related Features

- **Thread Pool > Fix Mutex-Blocking-Recv**: If per-worker channels are implemented, thread naming should be preserved
- **Thread Pool > Worker Thread Panic Recovery**: Named threads make panic logs more useful — you can see which worker crashed
- **Logging & Observability > Configurable Log Levels**: Thread names can be included in structured log output
