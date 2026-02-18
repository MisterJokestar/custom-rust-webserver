# Replace `unwrap()` in `build_routes()` Extension Check with Skip-and-Log

**Feature**: Replace `unwrap()` in `build_routes()` extension check with a skip-and-log for extensionless files
**Category**: Error Handling
**Complexity**: 1/10
**Necessity**: 7/10

---

## Overview

The `build_routes()` function in `src/main.rs` panics when it encounters a file without an extension or a file with non-UTF-8 characters in its name or extension. This happens at startup during route building and crashes the entire server before it can serve any requests.

### Current State

**`src/main.rs` lines 94-103**:
```rust
for entry in fs::read_dir(directory).unwrap() {
    let entry = entry.unwrap();
    let path = entry.path();
    let name = path.file_name().unwrap().to_str().unwrap();
    if path.is_dir() {
        routes.extend(
            build_routes(format!("{route}/{name}"), &path)
        );
    } else if path.is_file() {
        match path.extension().unwrap().to_str().unwrap() {
```

There are **6 unwrap() calls** in this code path:
1. `fs::read_dir(directory).unwrap()` — panics if directory is unreadable
2. `entry.unwrap()` — panics on individual entry read errors
3. `path.file_name().unwrap()` — panics if path has no file name component
4. `.to_str().unwrap()` — panics if file name contains non-UTF-8
5. `path.extension().unwrap()` — panics if file has no extension (e.g., `Makefile`, `LICENSE`, `.gitignore`)
6. `.to_str().unwrap()` — panics if extension contains non-UTF-8

### Impact
- Placing any extensionless file (e.g., `Makefile`, `LICENSE`, `README`, `Dockerfile`) in the `pages/` directory crashes the server at startup
- A permission error on a subdirectory of `pages/` crashes the server at startup
- Files with non-UTF-8 names crash the server at startup

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Lines 91-123**: The entire `build_routes()` function needs its unwraps replaced with graceful error handling.

---

## Step-by-Step Implementation

### Step 1: Replace `fs::read_dir()` Unwrap

**Current code** (line 94):
```rust
for entry in fs::read_dir(directory).unwrap() {
```

**Updated code**:
```rust
let entries = match fs::read_dir(directory) {
    Ok(e) => e,
    Err(e) => {
        eprintln!("Error reading directory {}: {e}", directory.display());
        return routes;
    }
};

for entry_result in entries {
```

**Rationale**: If a subdirectory is unreadable, return whatever routes have been built so far. The server starts with a reduced route set rather than crashing.

### Step 2: Replace Entry Unwrap

**Current code** (line 95):
```rust
let entry = entry.unwrap();
```

**Updated code**:
```rust
let entry = match entry_result {
    Ok(e) => e,
    Err(e) => {
        eprintln!("Error reading directory entry in {}: {e}", directory.display());
        continue;
    }
};
```

**Rationale**: Skip individual unreadable entries; continue processing the rest.

### Step 3: Replace File Name Unwraps

**Current code** (line 97):
```rust
let name = path.file_name().unwrap().to_str().unwrap();
```

**Updated code**:
```rust
let name = match path.file_name().and_then(|n| n.to_str()) {
    Some(n) => n,
    None => {
        eprintln!("Skipping file with invalid name: {}", path.display());
        continue;
    }
};
```

**Rationale**: Files with no name component or non-UTF-8 names are skipped with a log message.

### Step 4: Replace Extension Unwraps

**Current code** (line 103):
```rust
match path.extension().unwrap().to_str().unwrap() {
```

**Updated code**:
```rust
let ext = match path.extension().and_then(|e| e.to_str()) {
    Some(e) => e,
    None => {
        continue; // No extension — skip silently (common for LICENSE, Makefile, etc.)
    }
};
match ext {
```

**Rationale**: Extensionless files are expected (README, LICENSE, Makefile, etc.) and should be silently skipped since the server only routes `.html`, `.css`, and `.js` files. No log message needed for this common case.

### Complete Updated Function

```rust
fn build_routes(route: String, directory: &Path) -> HashMap<String, PathBuf> {
    let mut routes: HashMap<String, PathBuf> = HashMap::new();

    let entries = match fs::read_dir(directory) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Error reading directory {}: {e}", directory.display());
            return routes;
        }
    };

    for entry_result in entries {
        let entry = match entry_result {
            Ok(e) => e,
            Err(e) => {
                eprintln!("Error reading directory entry in {}: {e}", directory.display());
                continue;
            }
        };

        let path = entry.path();
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => {
                eprintln!("Skipping file with invalid name: {}", path.display());
                continue;
            }
        };

        if path.is_dir() {
            routes.extend(
                build_routes(format!("{route}/{name}"), &path)
            );
        } else if path.is_file() {
            let ext = match path.extension().and_then(|e| e.to_str()) {
                Some(e) => e,
                None => continue,
            };
            match ext {
                "html" | "css" | "js" => {
                    if name == "index.html" || name == "page.html" {
                        if route == "" {
                            routes.insert(String::from("/"), path);
                        } else {
                            routes.insert(route.clone(), path);
                        }
                    } else if name == "not_found.html" {
                        continue;
                    } else {
                        routes.insert(format!("{route}/{name}"), path);
                    }
                }
                _ => { continue; }
            }
        }
    }

    routes
}
```

---

## Edge Cases

### 1. Extensionless Files in `pages/`
**Scenario**: `pages/LICENSE` or `pages/.gitignore` exists.
**Handling**: `path.extension()` returns `None`. File is silently skipped. This is expected behavior.

### 2. Unreadable Subdirectory
**Scenario**: `pages/secret/` has 000 permissions.
**Handling**: `fs::read_dir()` returns `Err(PermissionDenied)`. Error is logged, routes from other directories are still built, and the server starts.

### 3. Non-UTF-8 File Names
**Scenario**: A file is named with bytes that aren't valid UTF-8 (rare on modern systems, possible on Linux).
**Handling**: `file_name().to_str()` returns `None`. File is skipped with a warning log.

### 4. Dotfiles
**Scenario**: Files like `.DS_Store`, `.gitkeep` in `pages/`.
**Handling**: These have extensions (`.DS_Store` has extension `DS_Store`, `.gitkeep` has extension `gitkeep`). They fall through to the `_ => continue` arm in the match. No issue.

### 5. Empty `pages/` Directory
**Scenario**: `pages/` exists but is empty.
**Handling**: `fs::read_dir()` succeeds but the iterator is empty. Returns an empty HashMap. Server starts with no routes — all requests get 404.

---

## Testing Strategy

### Unit Tests

```rust
#[test]
fn build_routes_handles_extensionless_files() {
    // Create a temp directory with an extensionless file
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("LICENSE"), "MIT").unwrap();
    std::fs::write(tmp.path().join("index.html"), "<h1>Hi</h1>").unwrap();

    let routes = build_routes(String::from(""), tmp.path());
    assert!(routes.contains_key("/"));
    assert_eq!(routes.len(), 1); // LICENSE was skipped
}
```

### Manual Testing

```bash
# Add an extensionless file to pages/
touch pages/README

# Start server — should NOT panic
cargo run
# Expected: Server starts normally, README is silently skipped

# Clean up
rm pages/README
```

---

## Implementation Checklist

- [ ] Replace `fs::read_dir(directory).unwrap()` with `match` returning empty routes on error
- [ ] Replace `entry.unwrap()` with `match` and `continue` on error
- [ ] Replace `path.file_name().unwrap().to_str().unwrap()` with `and_then()` chain
- [ ] Replace `path.extension().unwrap().to_str().unwrap()` with `and_then()` chain
- [ ] Log errors for directory/entry read failures and invalid file names
- [ ] Silently skip extensionless files (no log needed)
- [ ] Run `cargo test` — no regressions
- [ ] Run `cargo run --bin integration_test` — all tests pass
- [ ] Manual test: add extensionless file to `pages/`, verify server starts

---

## Backward Compatibility

No behavioral changes for valid `pages/` directories with properly named files. The only difference is that previously-crashing scenarios now degrade gracefully.

---

## Related Features

- **Security > Replace All unwrap() Calls**: This feature addresses the `build_routes()` subset
- **Routing > Route Matching for More File Types**: When more extensions are supported, the skip logic remains the same
- **Logging & Observability > Error Detail Logging**: The `eprintln!()` calls here are a precursor to structured logging
