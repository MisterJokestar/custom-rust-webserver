# Feature: Make Thread Pool Size Configurable via `RCOMM_THREADS`

**Category:** Configuration
**Complexity:** 1/10
**Necessity:** 7/10

---

## Overview

The thread pool size is currently hardcoded to 4 in `src/main.rs` line 28:

```rust
let pool = ThreadPool::new(4);
```

This feature adds an environment variable `RCOMM_THREADS` to allow operators to tune the thread pool size at runtime, following the same pattern already used for `RCOMM_PORT` and `RCOMM_ADDRESS`.

---

## Files to Modify

1. **`src/main.rs`** — Add `get_threads()` helper and use it in `main()`

---

## Step-by-Step Implementation

### Step 1: Add `get_threads()` Helper Function

**File:** `src/main.rs`, near `get_port()` and `get_address()`

```rust
fn get_threads() -> usize {
    std::env::var("RCOMM_THREADS")
        .ok()
        .and_then(|val| val.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(4)
}
```

The `.filter(|&n| n > 0)` guard prevents passing 0 to `ThreadPool::new()`, which would panic due to the `assert!(size > 0)` in `src/lib.rs` line 22.

### Step 2: Use `get_threads()` in `main()`

**File:** `src/main.rs`, line 28

**Current:**
```rust
let pool = ThreadPool::new(4);
```

**New:**
```rust
let threads = get_threads();
let pool = ThreadPool::new(threads);
```

### Step 3: Print Thread Count on Startup

**File:** `src/main.rs`, after the existing `println!` statements

```rust
println!("Listening on {full_address}");
println!("Worker threads: {threads}");
```

---

## Edge Cases & Handling

### 1. Invalid Values
Non-numeric strings (e.g., `RCOMM_THREADS=abc`) fall through `parse::<usize>().ok()` and use the default of 4. No error is raised.

### 2. Zero
`RCOMM_THREADS=0` is filtered out by `.filter(|&n| n > 0)` and falls back to 4, avoiding the panic in `ThreadPool::new()`.

### 3. Negative Numbers
Negative values cannot parse into `usize`, so they silently fall back to the default.

### 4. Very Large Values
`RCOMM_THREADS=10000` is technically valid. The OS will limit actual thread creation. A reasonable upper bound guard could be added later but is out of scope for this minimal feature.

---

## Testing Strategy

### Manual Testing

```bash
# Default (4 threads)
cargo run
# Observe: "Worker threads: 4"

# Custom thread count
RCOMM_THREADS=8 cargo run
# Observe: "Worker threads: 8"

# Invalid value falls back to default
RCOMM_THREADS=abc cargo run
# Observe: "Worker threads: 4"

# Zero falls back to default
RCOMM_THREADS=0 cargo run
# Observe: "Worker threads: 4"
```

### Integration Tests

No new integration tests required — this is a startup configuration change. Existing integration tests (which set `RCOMM_PORT`) already prove the env-var pattern works. The integration test binary spawns the server with default threads, which still works.

---

## Implementation Checklist

- [ ] Add `get_threads()` function to `src/main.rs`
- [ ] Replace `ThreadPool::new(4)` with `ThreadPool::new(get_threads())`
- [ ] Print thread count on startup
- [ ] Run `cargo build` to verify compilation
- [ ] Run `cargo test` to verify unit tests pass
- [ ] Run `cargo run --bin integration_test` to verify integration tests pass
- [ ] Manual test with various `RCOMM_THREADS` values

---

## Backward Compatibility

No behavioral change when `RCOMM_THREADS` is unset — the default remains 4. Existing deployments are unaffected.
