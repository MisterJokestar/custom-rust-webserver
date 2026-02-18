# Thread Pool Size Auto-Tuning Based on CPU Cores

**Feature**: Add thread pool size auto-tuning based on available CPU cores
**Category**: Thread Pool & Concurrency
**Complexity**: 2/10
**Necessity**: 4/10

---

## Overview

The thread pool is currently hardcoded to 4 workers in `src/main.rs` line 28:

```rust
let pool = ThreadPool::new(4);
```

On machines with more cores, this underutilizes available parallelism. On machines with fewer cores (e.g., a 2-core VPS), 4 threads causes unnecessary context switching. The pool size should default to the number of available CPU cores, providing a sensible baseline without configuration.

### Current State

- `ThreadPool::new(4)` is hardcoded in `main()`
- No environment variable or auto-detection for thread count
- The `FEATURES.md` also lists `RCOMM_THREADS` as a separate configuration feature (Complexity 1, Necessity 7) — this auto-tuning serves as the fallback default when that variable is not set

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Line 28**: Replace hardcoded `4` with auto-detected core count

---

## Step-by-Step Implementation

### Step 1: Detect Available CPU Cores

Rust's standard library provides `std::thread::available_parallelism()` (stabilized in Rust 1.59). This returns the number of CPUs available to the process, respecting cgroups, CPU affinity, and other OS-level limits.

**Location**: `src/main.rs`, new helper function

```rust
fn get_thread_count() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}
```

**Rationale**:
- `available_parallelism()` returns `Result<NonZeroUsize>` — it can fail on exotic platforms
- `unwrap_or(4)` falls back to the current hardcoded default
- `.get()` converts `NonZeroUsize` to `usize`

### Step 2: Use Auto-Detected Count in `main()`

**Current** (`src/main.rs` line 28):
```rust
let pool = ThreadPool::new(4);
```

**New**:
```rust
let thread_count = get_thread_count();
let pool = ThreadPool::new(thread_count);
```

### Step 3: Log the Thread Count on Startup

Add the thread count to the existing startup message so operators can verify:

**Current** (`src/main.rs` line 34):
```rust
println!("Listening on {full_address}");
```

**New**:
```rust
println!("Listening on {full_address} with {thread_count} worker threads");
```

---

## Future Integration with `RCOMM_THREADS`

When the `RCOMM_THREADS` environment variable feature is implemented, `get_thread_count()` becomes:

```rust
fn get_thread_count() -> usize {
    std::env::var("RCOMM_THREADS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4)
        })
}
```

For now, this feature implements just the auto-detection. The env var override is a separate feature.

---

## Edge Cases & Handling

### 1. Single-Core Machine
`available_parallelism()` returns `NonZeroUsize(1)`. The pool has 1 worker. This is correct — no benefit to multiple threads on a single core for CPU-bound work, and I/O-bound work (file reads) is minimal for static serving.

### 2. High Core Count (e.g., 64 Cores)
Each worker thread consumes a stack (default 8MB on Linux). 64 workers = 512MB of stack space. For a static file server, this is acceptable. However, having more threads than typical concurrent connections is wasteful. A reasonable upper bound could be added (e.g., `min(cores, 32)`), but this is a refinement for later.

### 3. Containerized Environments (Docker/cgroups)
`available_parallelism()` respects cgroup CPU limits. A container with `--cpus=2` on a 64-core host will correctly return `2`.

### 4. `available_parallelism()` Fails
Falls back to `4`, matching the current behavior exactly.

---

## Testing Strategy

### Unit Test

The auto-detection logic is trivial and relies on a standard library function. A basic smoke test:

```rust
#[test]
fn get_thread_count_returns_positive() {
    let count = get_thread_count();
    assert!(count > 0);
}
```

### Manual Verification

```bash
cargo run
# Output should include: "Listening on 127.0.0.1:7878 with N worker threads"
# where N matches the output of: nproc
```

---

## Implementation Checklist

- [ ] Add `get_thread_count()` function to `src/main.rs`
- [ ] Replace `ThreadPool::new(4)` with `ThreadPool::new(get_thread_count())`
- [ ] Add thread count to startup log message
- [ ] Run `cargo build` — verify compilation
- [ ] Run `cargo test` — all tests pass
- [ ] Run `cargo run --bin integration_test` — all integration tests pass
- [ ] Verify startup output shows correct core count

---

## Backward Compatibility

No behavioral change on a 4-core machine. On other machines, the server automatically uses a more appropriate thread count. No public API changes. All existing tests pass unchanged since they create `ThreadPool` with explicit sizes.

---

## Related Features

- **Configuration > `RCOMM_THREADS` Environment Variable**: Will use this auto-detection as the fallback default
- **Configuration > Command-Line Argument Parsing**: `--threads N` would be another override layer
- **Thread Pool > Name Worker Threads**: Naturally paired — when auto-detecting count, naming threads helps verify the right number started
