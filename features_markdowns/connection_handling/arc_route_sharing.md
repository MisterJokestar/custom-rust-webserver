# Arc Route Map Sharing Implementation Plan

## Overview

Currently, the route map (`HashMap<String, PathBuf>`) is **cloned for every incoming connection** in `main()` at line 37 of `src/main.rs`:

```rust
for stream in listener.incoming() {
    let routes_clone = routes.clone();
    let stream = stream.unwrap();

    pool.execute(move || {
        handle_connection(stream, routes_clone);
    });
}
```

Since routes are built once at startup and never mutated, this per-connection clone is wasteful. Wrapping the map in `Arc<HashMap<String, PathBuf>>` allows all worker threads to share a single read-only reference, eliminating the clone overhead entirely.

**Complexity**: 2
**Necessity**: 8

**Key Changes**:
- Wrap the route map in `Arc` after building it
- Clone the `Arc` (cheap pointer increment) instead of the `HashMap` per connection
- Update `handle_connection()` signature to accept `Arc<HashMap<String, PathBuf>>`

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Current State**:
- Line 1-7: Imports — `Arc` is not imported (only used in `lib.rs`)
- Line 31: `routes` is a plain `HashMap<String, PathBuf>`
- Line 37: `routes.clone()` deep-copies the entire HashMap per connection
- Line 46: `handle_connection()` takes owned `HashMap<String, PathBuf>`

**Changes Required**:
- Add `sync::Arc` to imports
- Wrap `routes` in `Arc` after `build_routes()` returns
- Replace `routes.clone()` with `Arc::clone(&routes)` in the listener loop
- Update `handle_connection()` to accept `Arc<HashMap<String, PathBuf>>`

---

## Step-by-Step Implementation

### Step 1: Add `Arc` Import

**Location**: `src/main.rs`, line 1

**Current**:
```rust
use std::{
    collections::HashMap,
    fs,
    io::prelude::*,
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
};
```

**New**:
```rust
use std::{
    collections::HashMap,
    fs,
    io::prelude::*,
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::Arc,
};
```

### Step 2: Wrap Routes in `Arc`

**Location**: `src/main.rs`, after line 31

**Current**:
```rust
let routes = build_routes(String::from(""), path);
```

**New**:
```rust
let routes = Arc::new(build_routes(String::from(""), path));
```

### Step 3: Replace `HashMap::clone()` with `Arc::clone()`

**Location**: `src/main.rs`, line 37

**Current**:
```rust
let routes_clone = routes.clone();
```

**New**:
```rust
let routes_clone = Arc::clone(&routes);
```

The `.clone()` call would also work (Rust dispatches to `Arc::clone`), but `Arc::clone(&routes)` is the idiomatic convention since it makes it explicit that only the reference count is being incremented, not the inner data.

### Step 4: Update `handle_connection()` Signature

**Location**: `src/main.rs`, line 46

**Current**:
```rust
fn handle_connection(mut stream: TcpStream, routes: HashMap<String, PathBuf>) {
```

**New**:
```rust
fn handle_connection(mut stream: TcpStream, routes: Arc<HashMap<String, PathBuf>>) {
```

No changes needed inside `handle_connection()` — `Arc<HashMap>` dereferences to `HashMap` automatically via `Deref`, so `routes.contains_key()`, `routes.get()`, etc. all work unchanged.

---

## Edge Cases & Handling

### 1. Thread Safety
- `Arc<HashMap<String, PathBuf>>` is `Send + Sync` because `HashMap<String, PathBuf>` is `Send + Sync`. No `Mutex` needed since the map is never mutated after creation.

### 2. Future Route Hot-Reload
- If route hot-reloading is added later (FEATURES.md: "Hot-reload routes when files in `pages/` are added, modified, or deleted"), this `Arc` becomes `Arc<RwLock<HashMap<String, PathBuf>>>`. The current change moves in the right direction.

### 3. Performance Impact
- `Arc::clone()` increments an atomic counter (~1 CPU cycle). `HashMap::clone()` allocates a new hash table, copies all keys and values. For a route map with N entries, this saves O(N) allocation and copy work per connection.

---

## Implementation Checklist

- [ ] Add `sync::Arc` to `use std::{...}` imports
- [ ] Wrap `build_routes()` result in `Arc::new()`
- [ ] Replace `routes.clone()` with `Arc::clone(&routes)` in listener loop
- [ ] Update `handle_connection()` parameter type to `Arc<HashMap<String, PathBuf>>`
- [ ] Run `cargo build` to verify compilation
- [ ] Run `cargo test` to verify unit tests pass
- [ ] Run `cargo run --bin integration_test` to verify integration tests pass

---

## Backward Compatibility

No behavioral changes. Routes are still read-only after startup. All existing tests pass unchanged. The only difference is reduced memory allocation under load.
