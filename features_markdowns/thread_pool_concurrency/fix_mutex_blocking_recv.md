# Fix Mutex-Blocking-Recv Pattern

**Feature**: Fix the mutex-blocking-recv pattern — only one worker can wait for a job at a time; consider per-worker channels or a work-stealing scheduler
**Category**: Thread Pool & Concurrency
**Complexity**: 7/10
**Necessity**: 6/10

---

## Overview

The current thread pool uses a single `mpsc::Receiver` wrapped in `Arc<Mutex<...>>` to distribute jobs to workers. This means only **one worker thread at a time** can hold the mutex lock and call `recv()`. All other workers block on the mutex even though the channel could have jobs waiting.

### Current State

**`src/lib.rs` lines 60-76** (Worker):
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

### The Problem

1. Worker calls `reciever.lock().unwrap()` — acquires the mutex
2. Then calls `.recv()` — blocks waiting for a job **while still holding the lock**
3. All other workers are blocked on `lock()` — they can't even check for jobs
4. When a job arrives, only the one worker holding the lock receives it
5. The lock is released when the `MutexGuard` is dropped (after `recv()` returns)
6. Then the next worker acquires the lock and waits

In practice, this means job distribution is **serialized through the mutex**. When the channel is empty, exactly one worker blocks on `recv()` while the others block on `lock()`. When a burst of jobs arrives, they are dispatched one-at-a-time as each worker serially acquires the lock, takes a job, and releases it.

### Why It Matters

- Under high concurrency, the mutex becomes a bottleneck for job distribution
- Job dispatch latency increases linearly with worker count
- CPU cores sit idle waiting for the mutex instead of processing jobs
- The pattern is a well-known anti-pattern in Rust concurrent programming

---

## Approach Options

### Option A: Per-Worker Channels (Recommended)

Give each worker its own `mpsc::Receiver`. The dispatcher (main thread via `execute()`) selects which worker to send to using round-robin or a smarter strategy.

**Pros**: No shared mutex, each worker blocks only on its own channel, simple to implement
**Cons**: Uneven load distribution if using round-robin (one worker might be busy while its queue fills up)

### Option B: Crossbeam Channel

Replace `std::sync::mpsc` + `Arc<Mutex<...>>` with `crossbeam::channel::unbounded()`, which supports multiple receivers natively without a mutex.

**Pros**: Drop-in replacement, designed for multi-consumer, high performance
**Cons**: Adds an external dependency (rcomm currently has zero dependencies)

### Option C: Work-Stealing Scheduler

Each worker has its own local deque. When a worker's deque is empty, it "steals" from another worker's deque. This is how `rayon` and `tokio` work internally.

**Pros**: Optimal load balancing, cache-friendly
**Cons**: Significantly more complex to implement, overkill for a static file server

### Recommendation: Option A (Per-Worker Channels)

This keeps zero external dependencies and eliminates the mutex contention. Round-robin distribution is acceptable for a static file server where request handling time is roughly uniform (all requests read a file and write a response).

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/lib.rs`

**Current State**:
- `ThreadPool` holds a single `mpsc::Sender<Job>` and a `Vec<Worker>`
- All workers share one `Arc<Mutex<mpsc::Receiver<Job>>>`
- `execute()` sends jobs to the single shared channel

**Changes Required**:
- Create one `mpsc::channel()` per worker
- Store `Vec<mpsc::Sender<Job>>` in `ThreadPool` instead of `Option<mpsc::Sender<Job>>`
- Add a round-robin index (or `AtomicUsize`) for dispatch
- Each `Worker` gets its own `mpsc::Receiver<Job>` (no `Arc<Mutex<...>>`)
- Update `Drop` to drop all senders

---

## Step-by-Step Implementation

### Step 1: Restructure `ThreadPool` Fields

**Current**:
```rust
pub struct ThreadPool {
    workers: Vec<Worker>,
    sender: Option<mpsc::Sender<Job>>,
}
```

**New**:
```rust
pub struct ThreadPool {
    workers: Vec<Worker>,
    senders: Vec<mpsc::Sender<Job>>,
    next_worker: AtomicUsize,
}
```

Add `use std::sync::atomic::{AtomicUsize, Ordering};` to imports.

### Step 2: Create Per-Worker Channels in `ThreadPool::new()`

**Current**:
```rust
pub fn new(size: usize) -> ThreadPool {
    assert!(size > 0);

    let (sender, receiver) = mpsc::channel();
    let receiver = Arc::new(Mutex::new(receiver));

    let mut workers = Vec::with_capacity(size);
    for id in 0..size {
        workers.push(Worker::new(id, Arc::clone(&receiver)));
    }

    ThreadPool { workers, sender: Some(sender) }
}
```

**New**:
```rust
pub fn new(size: usize) -> ThreadPool {
    assert!(size > 0);

    let mut workers = Vec::with_capacity(size);
    let mut senders = Vec::with_capacity(size);

    for id in 0..size {
        let (sender, receiver) = mpsc::channel();
        workers.push(Worker::new(id, receiver));
        senders.push(sender);
    }

    ThreadPool {
        workers,
        senders,
        next_worker: AtomicUsize::new(0),
    }
}
```

### Step 3: Update `execute()` with Round-Robin Dispatch

**Current**:
```rust
pub fn execute<F>(&self, f: F)
where
    F: FnOnce() + Send + 'static,
{
    let job = Box::new(f);
    self.sender.as_ref().unwrap().send(job).unwrap();
}
```

**New**:
```rust
pub fn execute<F>(&self, f: F)
where
    F: FnOnce() + Send + 'static,
{
    let job = Box::new(f);
    let index = self.next_worker.fetch_add(1, Ordering::Relaxed) % self.senders.len();
    self.senders[index].send(job).unwrap();
}
```

`Ordering::Relaxed` is sufficient here — we don't need strict ordering for round-robin, just atomicity. The modulo ensures wrapping.

### Step 4: Simplify `Worker::new()` — No More Mutex

**Current**:
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

**New**:
```rust
fn new(id: usize, receiver: mpsc::Receiver<Job>) -> Worker {
    let thread = thread::spawn(move || {
        loop {
            let message = receiver.recv();

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

Note: This also fixes the typo `reciever` → `receiver`.

### Step 5: Update `Drop` to Drop All Senders

**Current**:
```rust
impl Drop for ThreadPool {
    fn drop(&mut self) {
        drop(self.sender.take());

        for worker in self.workers.drain(..) {
            println!("Shutting down worker {}", worker.id);
            worker.thread.join().unwrap();
        }
    }
}
```

**New**:
```rust
impl Drop for ThreadPool {
    fn drop(&mut self) {
        self.senders.clear(); // drops all senders, causing workers to receive Err

        for worker in self.workers.drain(..) {
            println!("Shutting down worker {}", worker.id);
            worker.thread.join().unwrap();
        }
    }
}
```

### Step 6: Remove Unused Imports

Remove `Arc` and `Mutex` from the imports since they are no longer needed in this module:

**Current**:
```rust
use std::{
    sync::{Arc, Mutex, mpsc},
    thread,
};
```

**New**:
```rust
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
};
```

---

## Edge Cases & Handling

### 1. Worker Count Overflow in Round-Robin
`AtomicUsize::fetch_add` will wrap around at `usize::MAX`. After wrapping, `usize::MAX + 1 = 0` (wrapping arithmetic), so the modulo still works. With 4 workers, the sequence 0,1,2,3 repeats indefinitely.

### 2. Uneven Job Duration
If one worker gets a long-running job, its channel queue grows while other workers are idle. This is acceptable for static file serving (uniform job duration). If this becomes a problem, a work-stealing approach could be added later.

### 3. Shutdown Ordering
Each worker has its own sender. When all senders are dropped in `Drop`, each worker's `recv()` immediately returns `Err(RecvError)`, and they all shut down in parallel rather than serially waking up through the mutex.

### 4. Single Worker Pool
With `size = 1`, there's one sender and one worker. Round-robin always picks index 0. No behavioral change.

---

## Testing Strategy

### Existing Tests (Should Pass Unchanged)
All 5 existing tests in `src/lib.rs` exercise the same public API (`ThreadPool::new()`, `execute()`, `Drop`). The internal dispatch mechanism changes, but the external contract remains identical.

### New Tests

```rust
#[test]
fn thread_pool_distributes_across_workers() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::collections::HashSet;

    let worker_ids = Arc::new(Mutex::new(Vec::new()));
    let pool = ThreadPool::new(4);

    for _ in 0..8 {
        let ids = Arc::clone(&worker_ids);
        pool.execute(move || {
            let id = std::thread::current().id();
            ids.lock().unwrap().push(id);
        });
    }

    drop(pool);
    let ids = worker_ids.lock().unwrap();
    let unique: HashSet<_> = ids.iter().collect();
    // With round-robin and 8 jobs across 4 workers, all 4 should get work
    assert!(unique.len() > 1, "Expected multiple workers to receive jobs");
}
```

---

## Implementation Checklist

- [ ] Replace `Option<mpsc::Sender<Job>>` with `Vec<mpsc::Sender<Job>>` in `ThreadPool`
- [ ] Add `next_worker: AtomicUsize` field to `ThreadPool`
- [ ] Create per-worker channels in `ThreadPool::new()`
- [ ] Update `execute()` to use round-robin dispatch via `AtomicUsize`
- [ ] Change `Worker::new()` to accept `mpsc::Receiver<Job>` (no `Arc<Mutex<...>>`)
- [ ] Fix `reciever` typo to `receiver` in `Worker::new()`
- [ ] Update `Drop` to clear all senders
- [ ] Update imports: remove `Arc`/`Mutex`, add `AtomicUsize`/`Ordering`
- [ ] Run `cargo build` — verify compilation
- [ ] Run `cargo test` — all existing tests pass
- [ ] Run `cargo run --bin integration_test` — all integration tests pass

---

## Backward Compatibility

No public API changes. `ThreadPool::new(size)` and `execute(closure)` signatures are unchanged. The only difference is that job dispatch no longer serializes through a shared mutex. All existing tests pass without modification.

---

## Related Features

- **Thread Pool > Name Worker Threads**: When adding per-worker channels, it's natural to also name the threads at the same time
- **Thread Pool > Worker Thread Panic Recovery**: Per-worker channels make panic recovery simpler — a crashed worker's channel can be replaced without affecting others
- **Thread Pool > Task Queue Depth Monitoring**: Per-worker channels make it possible to inspect individual queue depths
- **Connection Handling > Arc Route Sharing**: Independent change, no interaction
