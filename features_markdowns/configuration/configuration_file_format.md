# Feature: Add Configuration File Format (TOML or JSON)

**Category:** Configuration
**Complexity:** 5/10
**Necessity:** 4/10

---

## Overview

Currently all configuration is done via environment variables (`RCOMM_PORT`, `RCOMM_ADDRESS`). This feature adds support for a configuration file as an alternative, allowing operators to persist settings in a file rather than setting environment variables on every run.

Since rcomm has a zero-dependency philosophy, the configuration file format must be parsed by hand. TOML's full spec is complex (nested tables, arrays of tables, multiline strings, etc.), so a **minimal INI-style key=value format** is the most practical approach. The file would be named `rcomm.conf` and placed alongside the binary or at a path specified by `RCOMM_CONFIG`.

**Priority order** (highest wins):
1. Environment variables (`RCOMM_PORT`, etc.)
2. Configuration file values
3. Built-in defaults

---

## Files to Modify

1. **`src/main.rs`** — Add config file loading, integrate with existing `get_*()` helpers
2. **`src/config.rs`** (new) — Config file parser and `ServerConfig` struct
3. **`src/lib.rs`** — Export the new `config` module

---

## Configuration File Format

### `rcomm.conf`

```ini
# rcomm server configuration
port = 7878
address = 127.0.0.1
threads = 4
document_root = ./pages
```

Rules:
- Lines starting with `#` are comments
- Empty lines are ignored
- Keys and values are separated by `=`
- Leading/trailing whitespace is trimmed from both keys and values
- Unknown keys are ignored (forward compatibility)
- All values are strings; parsing to the correct type happens in the consumer

---

## Step-by-Step Implementation

### Step 1: Create `ServerConfig` Struct

**File:** `src/config.rs` (new)

```rust
use std::collections::HashMap;
use std::fs;
use std::path::Path;

pub struct ServerConfig {
    values: HashMap<String, String>,
}

impl ServerConfig {
    /// Load config from file, returning empty config if file doesn't exist
    pub fn load(path: &Path) -> ServerConfig {
        let values = if path.exists() {
            Self::parse_file(path)
        } else {
            HashMap::new()
        };
        ServerConfig { values }
    }

    /// Load from default locations: RCOMM_CONFIG env var, then ./rcomm.conf
    pub fn load_default() -> ServerConfig {
        let path = std::env::var("RCOMM_CONFIG")
            .unwrap_or_else(|_| String::from("rcomm.conf"));
        Self::load(Path::new(&path))
    }

    pub fn get(&self, key: &str) -> Option<&String> {
        self.values.get(key)
    }

    fn parse_file(path: &Path) -> HashMap<String, String> {
        let mut map = HashMap::new();
        let contents = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Warning: could not read config file {}: {}", path.display(), e);
                return map;
            }
        };

        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                map.insert(key.trim().to_string(), value.trim().to_string());
            }
        }

        map
    }
}
```

### Step 2: Export Config Module

**File:** `src/lib.rs`

Add at the top:
```rust
pub mod config;
pub mod models;
```

### Step 3: Integrate Config with Existing Helpers

**File:** `src/main.rs`

Refactor the `get_*()` helpers to check the config file as a fallback between env vars and defaults:

```rust
use rcomm::config::ServerConfig;

fn get_port(config: &ServerConfig) -> String {
    std::env::var("RCOMM_PORT")
        .ok()
        .or_else(|| config.get("port").cloned())
        .unwrap_or_else(|| String::from("7878"))
}

fn get_address(config: &ServerConfig) -> String {
    std::env::var("RCOMM_ADDRESS")
        .ok()
        .or_else(|| config.get("address").cloned())
        .unwrap_or_else(|| String::from("127.0.0.1"))
}

fn get_threads(config: &ServerConfig) -> usize {
    std::env::var("RCOMM_THREADS")
        .ok()
        .or_else(|| config.get("threads").cloned())
        .and_then(|val| val.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(4)
}

fn get_document_root(config: &ServerConfig) -> String {
    std::env::var("RCOMM_DOCUMENT_ROOT")
        .ok()
        .or_else(|| config.get("document_root").cloned())
        .unwrap_or_else(|| String::from("./pages"))
}
```

### Step 4: Update `main()` to Load Config

**File:** `src/main.rs`

```rust
fn main() {
    let config = ServerConfig::load_default();
    let port = get_port(&config);
    let address = get_address(&config);
    let threads = get_threads(&config);
    let document_root = get_document_root(&config);
    // ... rest of main
}
```

---

## Edge Cases & Handling

### 1. Missing Config File
`ServerConfig::load()` returns an empty config when the file doesn't exist. All values fall through to env vars or defaults. No error is printed.

### 2. Unreadable Config File
A warning is printed to stderr, and an empty config is returned. The server continues with env vars/defaults.

### 3. Malformed Lines
Lines without `=` are silently skipped. This is intentional for forward compatibility and simplicity.

### 4. Duplicate Keys
Later values overwrite earlier ones (HashMap behavior). This is standard for config files.

### 5. Priority Conflicts
Environment variables always take precedence over the config file. This follows the 12-factor app convention and allows per-invocation overrides.

### 6. Empty Values
`port =` (empty value) results in an empty string, which will fail to parse as a port number and fall through to the default. This is acceptable.

---

## Testing Strategy

### Unit Tests (in `src/config.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic_config() {
        // Write a temp file, load it, verify values
    }

    #[test]
    fn skip_comments_and_blank_lines() {
        // Config with comments and blanks should parse only key=value lines
    }

    #[test]
    fn missing_file_returns_empty_config() {
        let config = ServerConfig::load(Path::new("/nonexistent/rcomm.conf"));
        assert!(config.get("port").is_none());
    }

    #[test]
    fn whitespace_trimmed() {
        // "  port  =  8080  " should yield key="port", value="8080"
    }
}
```

### Manual Testing

```bash
# Create config file
cat > rcomm.conf << 'EOF'
# Test configuration
port = 9090
address = 0.0.0.0
threads = 8
EOF

# Run with config file
cargo run
# Observe: Listening on 0.0.0.0:9090, Worker threads: 8

# Env var overrides config file
RCOMM_PORT=3000 cargo run
# Observe: Listening on 0.0.0.0:3000

# Custom config path
RCOMM_CONFIG=/etc/rcomm/server.conf cargo run

# No config file — defaults used
rm rcomm.conf && cargo run
# Observe: Listening on 127.0.0.1:7878
```

---

## Dependencies

This feature depends on:
- **Configurable thread pool size** (`RCOMM_THREADS`) — should be implemented first so the config file can include `threads`
- **Configurable document root** — can be bundled with this feature or implemented separately

---

## Implementation Checklist

- [ ] Create `src/config.rs` with `ServerConfig` struct and parser
- [ ] Export `config` module from `src/lib.rs`
- [ ] Refactor `get_port()`, `get_address()` to accept `&ServerConfig`
- [ ] Add `get_threads()`, `get_document_root()` helpers
- [ ] Load config in `main()` before other initialization
- [ ] Add unit tests for config parsing
- [ ] Run `cargo build` and `cargo test`
- [ ] Run `cargo run --bin integration_test`
- [ ] Manual test with config file, env overrides, and missing file

---

## Backward Compatibility

Fully backward compatible. When no `rcomm.conf` exists and `RCOMM_CONFIG` is unset, the server behaves identically to today. Environment variables continue to work as before and take priority over the config file.
