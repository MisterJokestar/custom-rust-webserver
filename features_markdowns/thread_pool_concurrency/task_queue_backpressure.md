# Task Queue Depth Monitoring and Backpressure

**Feature**: Add task queue depth monitoring and backpressure (reject connections when queue is full with `503 Service Unavailable`)
**Category**: Thread Pool & Concurrency
**Complexity**: 5/10
**Necessity**: 5/10

---

## Overview

The current thread pool uses an unbounded `mpsc::channel()` for job dispatch. If connections arrive faster than workers can process them, the channel queue grows without limit, consuming memory. Eventually the server either runs out of memory or becomes so backlogged that responses arrive long after clients have given up.

Backpressure means setting a maximum queue depth. When the queue is full, new connections are rejected with `503 Service Unavailable` rather than being queued indefinitely.

### Current State

**`src/lib.rs` line 24**:
```rust
let (sender, receiver) = mpsc::channel(); // unbounded channel
```

**`src/lib.rs` line 43** (`execute()`):
```rust
self.sender.as_ref().unwrap().send(job).unwrap(); // never fails (unbounded)
```

**`src/main.rs` lines 36-43** (listener loop):
```rust
for stream in listener.incoming() {
    let routes_clone = routes.clone();
    let stream = stream.unwrap();

    pool.execute(move || {
        handle_connection(stream, routes_clone);
    });
}
```

There is no limit on queued jobs and no mechanism to reject connections.

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/lib.rs`

- Replace `mpsc::channel()` with `mpsc::sync_channel(capacity)`
- Change `execute()` to return a `Result` indicating success or queue-full
- Add a method to query current queue depth (optional)

### 2. `/home/jwall/personal/rusty/rcomm/src/main.rs`

- Handle the `Err` case from `execute()` by sending a `503 Service Unavailable` response and closing the connection

---

## Step-by-Step Implementation

### Step 1: Add Bounded Channel with `sync_channel`

**Current** (`src/lib.rs` line 24):
```rust
let (sender, receiver) = mpsc::channel();
```

**New**:
```rust
let (sender, receiver) = mpsc::sync_channel(capacity);
```

`mpsc::sync_channel(n)` creates a channel that blocks (or returns error with `try_send`) when `n` messages are buffered. This provides natural backpressure.

### Step 2: Add `capacity` Parameter to `ThreadPool::new()`

**Current**:
```rust
pub fn new(size: usize) -> ThreadPool {
```

**New**:
```rust
pub fn new(size: usize, queue_capacity: usize) -> ThreadPool {
```

Or, to maintain backward compatibility, provide a default:

```rust
pub fn new(size: usize) -> ThreadPool {
    Self::with_capacity(size, size * 64)
}

pub fn with_capacity(size: usize, queue_capacity: usize) -> ThreadPool {
    assert!(size > 0);
    assert!(queue_capacity > 0);

    let (sender, receiver) = mpsc::sync_channel(queue_capacity);
    // ... rest unchanged
}
```

A default capacity of `size * 64` (e.g., 256 for 4 workers) provides a reasonable buffer — enough to absorb short bursts without unbounded growth.

### Step 3: Change `execute()` to Use `try_send()` and Return `Result`

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
pub fn execute<F>(&self, f: F) -> Result<(), F>
where
    F: FnOnce() + Send + 'static,
{
    let job = Box::new(f);
    match self.sender.as_ref().unwrap().try_send(job) {
        Ok(()) => Ok(()),
        Err(mpsc::TrySendError::Full(job)) => {
            // Extract the closure back from the Box
            Err(*job)
        }
        Err(mpsc::TrySendError::Disconnected(_)) => {
            panic!("Thread pool channel disconnected unexpectedly");
        }
    }
}
```

However, recovering the original `F` from `Box<dyn FnOnce()>` is not possible due to type erasure. A simpler approach:

```rust
pub fn execute<F>(&self, f: F) -> bool
where
    F: FnOnce() + Send + 'static,
{
    let job = Box::new(f);
    match self.sender.as_ref().unwrap().try_send(job) {
        Ok(()) => true,
        Err(mpsc::TrySendError::Full(_)) => false,
        Err(mpsc::TrySendError::Disconnected(_)) => {
            panic!("Thread pool channel disconnected");
        }
    }
}
```

Returns `true` if the job was enqueued, `false` if the queue is full.

### Step 4: Handle Queue-Full in `main()` with 503 Response

**Current** (`src/main.rs` lines 36-43):
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
    let mut stream = stream.unwrap();

    let enqueued = pool.execute(move || {
        handle_connection(stream, routes_clone);
    });

    if !enqueued {
        // Job was rejected — queue is full
        // Note: `stream` was moved into the closure, so we need a different approach
    }
}
```

**Problem**: The `stream` is moved into the closure before we know if enqueue succeeded. We need to restructure to check capacity first, or handle rejection differently.

**Revised Approach** — Check before moving:

```rust
for stream in listener.incoming() {
    let mut stream = stream.unwrap();

    if !pool.has_capacity() {
        // Reject immediately on the main thread
        let mut response = HttpResponse::build(String::from("HTTP/1.1"), 503);
        response.add_body("Service Unavailable: server is overloaded".into());
        let _ = stream.write_all(&response.as_bytes());
        continue;
    }

    let routes_clone = routes.clone();
    pool.execute(move || {
        handle_connection(stream, routes_clone);
    });
}
```

**Alternatively**, restructure `execute()` to return the closure on failure, but this is complex with `FnOnce`. The cleanest approach is to attempt the send and handle the failure by sending the 503 from within the closure's error path. However, since the stream is consumed by the closure, the simplest correct approach is:

```rust
for stream in listener.incoming() {
    let routes_clone = routes.clone();
    let mut stream = stream.unwrap();

    let accepted = pool.try_execute(move || {
        handle_connection(stream, routes_clone);
    });

    if !accepted {
        // The closure was NOT executed and NOT enqueued.
        // But stream was moved into it. We need to send 503 differently.
    }
}
```

### Revised Design: Return Stream on Failure

The most practical approach wraps the stream handling so that the 503 is sent inside the fallback:

```rust
for stream in listener.incoming() {
    let routes_clone = routes.clone();
    let stream = stream.unwrap();

    if !pool.execute(move || {
        handle_connection(stream, routes_clone);
    }) {
        // The closure was dropped (stream closed).
        // But the client gets a connection reset, not a 503.
        // To send 503, we need the stream back.
    }
}
```

Since `execute` consumes the closure on both success and failure (the `Full` variant drops the closure), the stream is dropped and closed. The client sees a connection reset.

**Best Practical Design**: Accept the connection reset behavior on overload, or separate the accept from the enqueue:

```rust
for stream in listener.incoming() {
    let mut stream = match stream {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Accept error: {e}");
            continue;
        }
    };

    let routes_clone = routes.clone();

    if pool.try_execute(move || {
        handle_connection(stream, routes_clone);
    }) {
        continue; // Successfully enqueued
    }

    // Enqueue failed — stream was consumed and dropped by the failed closure.
    // We need to re-accept or use a two-phase approach.
}
```

### Final Recommended Design: Two-Phase Accept

```rust
for stream in listener.incoming() {
    let mut stream = match stream {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Accept error: {e}");
            continue;
        }
    };

    if pool.is_full() {
        let mut response = HttpResponse::build(String::from("HTTP/1.1"), 503);
        let body = String::from("503 Service Unavailable\n");
        response.add_body(body.into());
        let _ = stream.write_all(&response.as_bytes());
        continue;
    }

    let routes_clone = routes.clone();
    pool.execute(move || {
        handle_connection(stream, routes_clone);
    });
}
```

This adds an `is_full()` method to `ThreadPool` that checks queue depth without consuming the stream.

### Step 5: Add `is_full()` to ThreadPool

```rust
impl ThreadPool {
    pub fn is_full(&self) -> bool {
        // sync_channel doesn't expose length, so we use try_send with a dummy
        // Alternative: track queue depth with an AtomicUsize
        false // see below
    }
}
```

`mpsc::SyncSender` does not expose queue length or remaining capacity. To track depth, use an `AtomicUsize` counter:

```rust
pub struct ThreadPool {
    workers: Vec<Worker>,
    sender: Option<mpsc::SyncSender<Job>>,
    pending: Arc<AtomicUsize>,
    capacity: usize,
}
```

- `execute()` increments `pending` before sending
- Each worker decrements `pending` after receiving
- `is_full()` checks `pending.load() >= capacity`

```rust
pub fn is_full(&self) -> bool {
    self.pending.load(Ordering::Relaxed) >= self.capacity
}

pub fn execute<F>(&self, f: F)
where
    F: FnOnce() + Send + 'static,
{
    let pending = Arc::clone(&self.pending);
    let job = Box::new(move || {
        f();
        pending.fetch_sub(1, Ordering::Relaxed);
    });

    self.pending.fetch_add(1, Ordering::Relaxed);
    self.sender.as_ref().unwrap().send(job).unwrap();
}
```

---

## Edge Cases & Handling

### 1. Burst Traffic
During a burst, the queue fills to capacity. Subsequent connections receive 503. As workers drain the queue, new connections are accepted again. This self-healing behavior prevents memory exhaustion.

### 2. Slow Clients
If workers are blocked on slow `write_all()` calls, the queue fills up even at low request rates. The 503 response protects the server. The request timeout feature (separate) would address the root cause.

### 3. Race Between `is_full()` and `execute()`
Between checking `is_full()` and calling `execute()`, the queue state might change. This is benign:
- Queue went from full to not-full: `execute()` succeeds (correct)
- Queue went from not-full to full: `sync_channel::send()` blocks briefly until a slot opens (acceptable)

Using `try_send()` inside `execute()` instead of `send()` makes this fully non-blocking, returning false if the race caused the queue to fill.

### 4. Queue Capacity of 0
`mpsc::sync_channel(0)` creates a rendezvous channel (sender blocks until receiver is ready). This effectively means no queuing — only direct handoff. Valid but unusual. The `assert!(queue_capacity > 0)` prevents this.

### 5. Monitoring
The `pending` counter can be logged periodically or exposed via a health endpoint to monitor server load.

---

## Testing Strategy

### Unit Tests

```rust
#[test]
fn thread_pool_rejects_when_full() {
    // Create pool with tiny capacity
    let pool = ThreadPool::with_capacity(1, 1);

    // Fill the single worker with a blocking job
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let b = Arc::clone(&barrier);
    pool.execute(move || { b.wait(); });

    // Queue should be empty now (job is executing), fill the 1-capacity queue
    let b2 = Arc::clone(&barrier);
    pool.execute(move || { b2.wait(); }); // This fills the queue

    // Now the queue should be full
    assert!(pool.is_full());

    // Unblock workers
    barrier.wait();
}
```

### Integration Tests

```rust
fn test_server_returns_503_under_overload(addr: &str) -> Result<(), String> {
    // This is hard to test reliably in integration tests because
    // the queue capacity needs to be very small. Best tested via unit tests
    // and manual testing with load generators.
    Ok(())
}
```

### Manual Testing

```bash
# Use a load testing tool to saturate the server
# Install 'hey' or 'wrk' and blast the server
wrk -t12 -c400 -d10s http://127.0.0.1:7878/
# Some requests should return 503 if the queue fills up
```

---

## Implementation Checklist

- [ ] Replace `mpsc::channel()` with `mpsc::sync_channel(capacity)` in `ThreadPool`
- [ ] Add `pending: Arc<AtomicUsize>` counter to `ThreadPool`
- [ ] Add `capacity` field to `ThreadPool`
- [ ] Add `with_capacity(size, queue_capacity)` constructor
- [ ] Update default `new(size)` to use `size * 64` capacity
- [ ] Add `is_full()` method to `ThreadPool`
- [ ] Wrap job closures to decrement `pending` counter after execution
- [ ] Update `main()` listener loop to check `is_full()` and send 503
- [ ] Add 503 status code to `get_status_phrase()` if not already present
- [ ] Run `cargo build` — verify compilation
- [ ] Run `cargo test` — all existing tests pass
- [ ] Run `cargo run --bin integration_test` — all integration tests pass
- [ ] Add unit test for queue-full rejection

---

## Backward Compatibility

The `new(size)` constructor maintains the same signature with a sensible default capacity. Existing code calling `ThreadPool::new(4)` continues to work. The only behavioral change is that under extreme load, the server returns 503 instead of queuing indefinitely. This is strictly an improvement.

---

## Related Features

- **Security > Max Concurrent Connection Limit**: Complementary — limits connections at the accept level, while this limits at the queue level
- **Security > Request Timeout**: Reduces time workers spend on stalled connections, keeping the queue drained
- **Thread Pool > Fix Mutex-Blocking-Recv**: If per-worker channels are used, backpressure needs per-worker capacity tracking
- **Logging & Observability > Error Detail Logging**: 503 responses should be logged
- **HTTP Protocol > `Connection: close`**: Under backpressure, the server should send `Connection: close` to discourage persistent connections
