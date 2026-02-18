# Watch Mode (Auto-Restart on Source Change)

**Feature**: Add a `--watch` mode that auto-restarts the server when source files change
**Category**: Developer Experience
**Complexity**: 6/10
**Necessity**: 3/10

---

## Overview

When developing locally, having to manually stop and restart the server after every source code change is tedious. A `--watch` flag would monitor the project's source files and the `pages/` directory for changes, then automatically rebuild and restart the server. Since rcomm has zero external dependencies, this must be implemented using OS-native file watching facilities.

### Current State

The server starts once and runs until killed:

**`src/main.rs` line 22-43**:
```rust
fn main() {
    let port = get_port();
    let address = get_address();
    let full_address = format!("{address}:{port}");
    let listener = TcpListener::bind(&full_address).unwrap();
    // ... runs until process killed
}
```

There is no mechanism for detecting file changes or restarting the server process.

### Desired Behavior

```bash
cargo run -- --watch
# Server starts, watches for changes, auto-restarts on file modification
```

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Changes Required**:
- Add command-line argument parsing to detect `--watch` flag
- In watch mode: fork the server as a child process, monitor files, kill and respawn on change
- In normal mode: current behavior unchanged

### 2. (New) `/home/jwall/personal/rusty/rcomm/src/watcher.rs`

**Changes Required**:
- Implement file system polling (since we have no external dependencies)
- Track modification timestamps for files in `src/` and `pages/`
- Provide a `watch()` function that blocks until a change is detected

---

## Step-by-Step Implementation

### Step 1: Add Basic Argument Parsing

**Location**: `src/main.rs`, `main()` function

```rust
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let watch_mode = args.iter().any(|a| a == "--watch");

    if watch_mode {
        run_with_watch();
    } else {
        run_server();
    }
}
```

Extract the current `main()` body into a `run_server()` function.

### Step 2: Implement File System Polling

**Location**: New file `src/watcher.rs`

Since rcomm has no external dependencies, use `std::fs::metadata()` to poll file modification times:

```rust
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub struct FileWatcher {
    watch_dirs: Vec<PathBuf>,
    snapshots: HashMap<PathBuf, SystemTime>,
    poll_interval: std::time::Duration,
}

impl FileWatcher {
    pub fn new(dirs: Vec<PathBuf>, poll_interval_ms: u64) -> Self {
        let mut watcher = FileWatcher {
            watch_dirs: dirs,
            snapshots: HashMap::new(),
            poll_interval: std::time::Duration::from_millis(poll_interval_ms),
        };
        watcher.snapshot();
        watcher
    }

    /// Take a snapshot of all file modification times
    fn snapshot(&mut self) {
        self.snapshots.clear();
        for dir in &self.watch_dirs {
            self.scan_dir(dir);
        }
    }

    fn scan_dir(&mut self, dir: &Path) {
        let Ok(entries) = fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                self.scan_dir(&path);
            } else if let Ok(meta) = fs::metadata(&path) {
                if let Ok(modified) = meta.modified() {
                    self.snapshots.insert(path, modified);
                }
            }
        }
    }

    /// Block until a file change is detected. Returns the changed path.
    pub fn wait_for_change(&mut self) -> PathBuf {
        loop {
            std::thread::sleep(self.poll_interval);
            let old = self.snapshots.clone();
            self.snapshot();

            // Check for new or modified files
            for (path, mtime) in &self.snapshots {
                match old.get(path) {
                    None => return path.clone(),             // New file
                    Some(old_mtime) if mtime != old_mtime => return path.clone(), // Modified
                    _ => {}
                }
            }

            // Check for deleted files
            for path in old.keys() {
                if !self.snapshots.contains_key(path) {
                    return path.clone(); // Deleted file
                }
            }
        }
    }
}
```

### Step 3: Implement Watch Runner

**Location**: `src/main.rs`

The watch runner spawns the server as a child process using `cargo run` (without `--watch`), then kills and respawns it when changes are detected:

```rust
use std::process::{Command, Child};

fn run_with_watch() {
    let watch_dirs = vec![
        PathBuf::from("./src"),
        PathBuf::from("./pages"),
    ];
    let mut watcher = watcher::FileWatcher::new(watch_dirs, 500);
    let mut child: Option<Child> = None;

    loop {
        // Kill previous server if running
        if let Some(ref mut c) = child {
            let _ = c.kill();
            let _ = c.wait();
        }

        // Rebuild
        println!("[watch] Building...");
        let build = Command::new("cargo")
            .args(["build"])
            .status();

        match build {
            Ok(status) if status.success() => {
                println!("[watch] Starting server...");
                child = Command::new("cargo")
                    .args(["run"])
                    .spawn()
                    .ok();
            }
            Ok(status) => {
                eprintln!("[watch] Build failed with {status}, waiting for changes...");
            }
            Err(e) => {
                eprintln!("[watch] Failed to run cargo: {e}");
            }
        }

        let changed = watcher.wait_for_change();
        println!("[watch] Change detected: {}", changed.display());
    }
}
```

### Step 4: Wire Up the Module

**Location**: `src/main.rs`

```rust
mod watcher;
```

---

## Edge Cases & Handling

### 1. Rapid File Saves (Debouncing)
**Scenario**: IDE saves multiple files in quick succession (e.g., format-on-save touches several files within milliseconds).
**Handling**: After detecting the first change, sleep an additional 200ms before rebuilding. This coalesces rapid saves into a single restart.

### 2. Build Failure
**Scenario**: A source file change introduces a compilation error.
**Handling**: Log the build failure, keep the previous server running (if any), and wait for the next change. Do not crash the watcher.

### 3. Server Fails to Start
**Scenario**: The compiled server panics on startup (e.g., port already in use).
**Handling**: The child process exits immediately. The watcher continues waiting for changes and will attempt to restart after the next modification.

### 4. Pages-Only Changes
**Scenario**: An HTML/CSS file in `pages/` changes but no Rust source changed.
**Handling**: Still triggers a restart. For pages-only changes, a rebuild isn't strictly necessary (the binary is unchanged), but restarting ensures the route map is refreshed. A future optimization could skip the `cargo build` step when only `pages/` files changed.

### 5. Signal Forwarding
**Scenario**: User presses Ctrl+C while the watcher is running.
**Handling**: The watcher process receives SIGINT. The child server process should also be killed. Rust's default SIGINT handler will terminate the watcher, and the OS cleans up child processes. For cleaner behavior, install a signal handler that kills the child before exiting.

---

## Implementation Checklist

- [ ] Create `src/watcher.rs` with `FileWatcher` struct and polling logic
- [ ] Extract current `main()` body into `run_server()` function
- [ ] Add `--watch` argument detection in `main()`
- [ ] Implement `run_with_watch()` using child process spawn/kill loop
- [ ] Add debounce delay (200ms after first detected change)
- [ ] Add `[watch]` prefixed log messages for visibility
- [ ] Handle build failures gracefully (don't crash watcher)
- [ ] Run `cargo build` to verify compilation
- [ ] Test: modify a `.rs` file and verify server restarts
- [ ] Test: modify a `pages/*.html` file and verify server restarts
- [ ] Test: introduce a compilation error and verify watcher survives
- [ ] Test: Ctrl+C cleanly kills both watcher and server

---

## Backward Compatibility

No behavioral changes when `--watch` is not passed. The default server startup path is unchanged. This feature is development-only and adds no runtime overhead to the normal server.

---

## Related Features

- **Configuration > Command-Line Argument Parsing**: The `--watch` flag is the first CLI argument; a future argument parsing system should incorporate it
- **Routing > Hot-Reload Routes**: Hot-reload rebuilds the route map without restarting the process; watch mode restarts the entire process. They solve different problems — hot-reload is for production, watch is for development
- **Developer Experience > Print Full URL on Startup**: Watch mode should also print the URL after each restart
