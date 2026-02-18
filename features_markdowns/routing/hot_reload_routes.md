# Hot-Reload Routes Implementation Plan

## Overview

Currently, rcomm builds its route table once at startup by scanning the `pages/` directory (`build_routes()` at line 31 of `src/main.rs`). Any files added, modified, or deleted while the server is running require a full restart for changes to take effect.

This feature adds file system watching so that route tables are automatically rebuilt when the `pages/` directory changes, enabling a live-reload development workflow without server restarts.

**Complexity**: 7 (high — requires file system polling or OS-specific APIs, thread-safe route table updates, and careful concurrency design)
**Necessity**: 3

**Key Challenge**: Without external dependencies (no `notify` crate), file system watching must be implemented using periodic polling (`fs::read_dir()` + `metadata().modified()`) rather than OS-level file system events (inotify on Linux, FSEvents on macOS).

**Key Changes**:
- Replace the shared `HashMap<String, PathBuf>` routes with an `Arc<RwLock<HashMap<String, PathBuf>>>` for thread-safe reads and writes
- Add a background watcher thread that periodically re-scans `pages/` and detects changes
- Rebuild routes atomically when changes are detected
- Log route changes for operator visibility

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Current State**:
- Line 31: `let routes = build_routes(String::from(""), path);` — built once
- Line 37: `let routes_clone = routes.clone();` — full HashMap cloned per connection
- Line 41: `handle_connection(stream, routes_clone);` — each connection gets an owned copy

**Changes Required**:
- Wrap routes in `Arc<RwLock<HashMap<String, PathBuf>>>`
- Replace per-connection `clone()` with `Arc::clone()` (cheap reference count increment)
- Update `handle_connection()` to acquire a read lock on routes
- Spawn a background watcher thread that periodically re-scans and updates routes

### 2. `/home/jwall/personal/rusty/rcomm/src/lib.rs`

**No changes required**: Thread pool already supports arbitrary closures.

---

## Step-by-Step Implementation

### Step 1: Change Route Storage to `Arc<RwLock<>>`

**Location**: `src/main.rs`, in `main()`

**Current Code** (lines 30-31):
```rust
    let path = Path::new("./pages");
    let routes = build_routes(String::from(""), path);
```

**New Code**:
```rust
    use std::sync::{Arc, RwLock};
    use std::thread;
    use std::time::Duration;

    let path = Path::new("./pages");
    let initial_routes = build_routes(String::from(""), path);
    let routes = Arc::new(RwLock::new(initial_routes));
```

### Step 2: Update Connection Loop

**Current Code** (lines 36-43):
```rust
    for stream in listener.incoming() {
        let routes_clone = routes.clone();
        let stream = stream.unwrap();

        pool.execute(move || {
            handle_connection(stream, routes_clone);
        });
    }
```

**New Code**:
```rust
    for stream in listener.incoming() {
        let routes_ref = Arc::clone(&routes);
        let stream = stream.unwrap();

        pool.execute(move || {
            let routes_snapshot = routes_ref.read().unwrap().clone();
            handle_connection(stream, routes_snapshot);
        });
    }
```

**Design Decision**: Clone the routes under a read lock at the start of each connection, then release the lock. This means:
- The read lock is held very briefly (just for the clone)
- The watcher thread can acquire a write lock between connections
- Each connection works with a consistent snapshot

### Step 3: Implement File System Watcher

**Location**: `src/main.rs`, add a new function

```rust
/// Track file metadata for change detection.
#[derive(Clone, PartialEq)]
struct FileSnapshot {
    path: PathBuf,
    modified: std::time::SystemTime,
    size: u64,
}

/// Scan a directory recursively and collect file metadata snapshots.
fn scan_directory(directory: &Path) -> Vec<FileSnapshot> {
    let mut snapshots = Vec::new();

    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(_) => return snapshots,
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();

        if path.is_dir() {
            snapshots.extend(scan_directory(&path));
        } else if path.is_file() {
            if let Ok(metadata) = path.metadata() {
                let modified = metadata.modified().unwrap_or(std::time::UNIX_EPOCH);
                let size = metadata.len();
                snapshots.push(FileSnapshot { path, modified, size });
            }
        }
    }

    snapshots.sort_by(|a, b| a.path.cmp(&b.path));
    snapshots
}

/// Start a background thread that watches for file changes and rebuilds routes.
fn start_route_watcher(
    pages_dir: PathBuf,
    routes: Arc<RwLock<HashMap<String, PathBuf>>>,
    poll_interval: Duration,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut last_snapshot = scan_directory(&pages_dir);

        loop {
            thread::sleep(poll_interval);

            let current_snapshot = scan_directory(&pages_dir);

            if current_snapshot != last_snapshot {
                println!("File changes detected, rebuilding routes...");
                let new_routes = build_routes(String::from(""), &pages_dir);
                let route_count = new_routes.len();

                let mut routes_guard = routes.write().unwrap();
                *routes_guard = new_routes;
                drop(routes_guard);

                println!("Routes rebuilt: {route_count} route(s) registered");
                last_snapshot = current_snapshot;
            }
        }
    })
}
```

### Step 4: Start Watcher in `main()`

**Location**: After route building, before the listener loop

```rust
    let watch_enabled = std::env::var("RCOMM_WATCH")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);

    if watch_enabled {
        let poll_interval = Duration::from_secs(
            std::env::var("RCOMM_WATCH_INTERVAL")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(2)
        );
        let watcher_routes = Arc::clone(&routes);
        let pages_path = PathBuf::from("./pages");
        start_route_watcher(pages_path, watcher_routes, poll_interval);
        println!("File watcher started (polling every {}s)", poll_interval.as_secs());
    }
```

### Step 5: Configuration

| Environment Variable | Default | Description |
|---------------------|---------|-------------|
| `RCOMM_WATCH` | `false` | Enable file system watching |
| `RCOMM_WATCH_INTERVAL` | `2` | Poll interval in seconds |

---

## Testing Strategy

### Unit Tests

```rust
#[cfg(test)]
mod watcher_tests {
    use super::*;

    #[test]
    fn scan_directory_finds_files() {
        let snapshots = scan_directory(Path::new("./pages"));
        assert!(!snapshots.is_empty());
    }

    #[test]
    fn scan_directory_empty_dir() {
        // Create temp dir, scan it, verify empty
        let dir = std::env::temp_dir().join("rcomm_test_empty");
        let _ = fs::create_dir(&dir);
        let snapshots = scan_directory(&dir);
        assert!(snapshots.is_empty());
        let _ = fs::remove_dir(&dir);
    }
}
```

### Integration Tests

Hot-reload is difficult to test in integration tests because it requires:
1. Starting the server with `RCOMM_WATCH=true`
2. Adding a file to `pages/`
3. Waiting for the poll interval
4. Requesting the new route

```rust
fn test_hot_reload_new_file(addr: &str) -> Result<(), String> {
    // Create a new file
    fs::write("pages/hot_reload_test.html", "<p>hot reload</p>")
        .map_err(|e| format!("write: {e}"))?;

    // Wait for watcher to detect
    thread::sleep(Duration::from_secs(3));

    // Request the new route
    let resp = send_request(addr, "GET", "/hot_reload_test.html")?;

    // Cleanup
    let _ = fs::remove_file("pages/hot_reload_test.html");

    assert_eq_or_err(&resp.status_code, &200, "status")?;
    Ok(())
}
```

### Manual Testing

```bash
RCOMM_WATCH=true cargo run &

# Initial routes
curl -i http://127.0.0.1:7878/           # 200

# Add a new page
echo '<p>new</p>' > pages/dynamic.html
sleep 3
curl -i http://127.0.0.1:7878/dynamic.html   # 200

# Delete it
rm pages/dynamic.html
sleep 3
curl -i http://127.0.0.1:7878/dynamic.html   # 404
```

---

## Edge Cases & Handling

### 1. File Changed During Route Rebuild
- **Behavior**: Route rebuild reads the filesystem at a point in time; if a file changes mid-scan, the next poll will catch it
- **Status**: Acceptable for polling-based approach

### 2. Very Frequent Changes (Rapid Saves)
- **Behavior**: Each poll interval triggers at most one rebuild
- **Status**: Debouncing built-in via poll interval

### 3. `pages/` Directory Deleted
- **Behavior**: `scan_directory()` returns empty; `build_routes()` returns empty map; all routes return 404
- **Status**: Graceful degradation

### 4. Permission Errors on Files
- **Behavior**: `scan_directory()` skips unreadable files; routes built from readable files only
- **Status**: Handled by `.ok()` / `Err(_) => continue`

### 5. Write Lock Contention
- **Behavior**: Route rebuild acquires a write lock briefly; concurrent connections wait for read lock
- **Mitigation**: Write lock held only for HashMap swap (microseconds)
- **Status**: Minimal impact

### 6. Large Directory Trees
- **Behavior**: Polling scans entire tree; may be slow for thousands of files
- **Mitigation**: Configurable poll interval; only rebuild when changes detected
- **Status**: Acceptable for development use

### 7. Watcher Disabled by Default
- **Behavior**: No background thread, no polling, no `RwLock` overhead
- **Note**: When disabled, routes could remain a plain `HashMap` without `Arc<RwLock<>>` wrapping
- **Status**: For simplicity, always use `Arc<RwLock<>>` even when watcher is disabled

---

## Implementation Checklist

- [ ] Change routes storage to `Arc<RwLock<HashMap<String, PathBuf>>>`
- [ ] Update connection loop to clone routes under read lock
- [ ] Implement `FileSnapshot` struct for change detection
- [ ] Implement `scan_directory()` function
- [ ] Implement `start_route_watcher()` function
- [ ] Add `RCOMM_WATCH` and `RCOMM_WATCH_INTERVAL` environment variables
- [ ] Start watcher thread conditionally in `main()`
- [ ] Add unit tests for `scan_directory()`
- [ ] Add integration test for hot-reload (with file creation and polling wait)
- [ ] Run `cargo test` and `cargo run --bin integration_test`
- [ ] Manual test: add/remove files while server is running

---

## Backward Compatibility

- **Default**: Watcher is disabled; behavior identical to current
- **With `RCOMM_WATCH=true`**: Routes are rebuilt on file changes
- The `Arc<RwLock<>>` wrapping adds negligible overhead (one read lock + clone per connection vs. one clone per connection)
- All existing tests pass unchanged

---

## Future Enhancements

1. **OS-level file watching**: Use `inotify` (Linux) or `kqueue` (macOS) syscalls directly for instant change detection instead of polling
2. **Incremental rebuild**: Only add/remove changed routes instead of full rebuild
3. **WebSocket live reload**: Notify connected browsers to refresh when routes change
