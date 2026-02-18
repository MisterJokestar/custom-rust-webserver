# Implementation Plan: Configurable Log Levels

## 1. Overview of the Feature

Configurable log levels allow operators to control the verbosity of server output, filtering messages by severity. This is essential for production deployments (where only errors should be logged) and development/debugging (where trace-level detail is needed).

**Current State**: The server uses raw `println!` and `eprintln!` calls scattered across `src/main.rs` and `src/lib.rs`. All output is unconditionally emitted regardless of importance. Worker lifecycle messages ("Worker {id} got a job; executing.") are always printed alongside request/response data, creating noisy output in production.

**Desired State**: A lightweight logging system with five severity levels:

| Level | Value | Usage |
|-------|-------|-------|
| `Error` | 0 | Unrecoverable failures (failed to bind port, panics) |
| `Warn` | 1 | Recoverable issues (bad requests, file not found) |
| `Info` | 2 | Normal operations (access logs, startup messages) |
| `Debug` | 3 | Detailed operational info (route building, worker assignment) |
| `Trace` | 4 | Full request/response dumps, very verbose |

The active log level is configured via `RCOMM_LOG_LEVEL` environment variable (default: `info`). Messages at or below the configured level are emitted; messages above are suppressed.

**Impact**:
- Production deployments can set `RCOMM_LOG_LEVEL=error` for minimal output
- Development can use `RCOMM_LOG_LEVEL=trace` for maximum visibility
- Foundation for log output destination routing (future feature)
- Replaces ad-hoc println/eprintln with structured severity-aware logging

---

## 2. Files to be Modified or Created

### New Files

1. **`/home/jwall/personal/rusty/rcomm/src/logger.rs`**
   - Defines `LogLevel` enum with `Error`, `Warn`, `Info`, `Debug`, `Trace` variants
   - Defines `Logger` struct that holds the current level threshold
   - Provides `log()` method that checks level before emitting
   - Provides convenience macros or methods: `error()`, `warn()`, `info()`, `debug()`, `trace()`
   - Thread-safe via a global `static` with `std::sync::OnceLock`

### Modified Files

2. **`/home/jwall/personal/rusty/rcomm/src/lib.rs`**
   - Add `pub mod logger;` to exports
   - Replace `println!("Worker {id} got a job; executing.")` with `logger.debug()`
   - Replace `println!("Worker {id} disconnected; shutting down.")` with `logger.info()`
   - Replace `println!("Shutting down worker {}", worker.id)` with `logger.info()`

3. **`/home/jwall/personal/rusty/rcomm/src/main.rs`**
   - Initialize the global logger at startup from `RCOMM_LOG_LEVEL` env var
   - Replace `println!("Routes:\n{routes:#?}\n\n")` with `logger.debug()`
   - Replace `println!("Listening on {full_address}")` with `logger.info()`
   - Replace `eprintln!("Bad request: {e}")` with `logger.warn()`
   - Replace access log `println!` with `logger.info()`

4. **`/home/jwall/personal/rusty/rcomm/src/models.rs`**
   - No changes needed (logger is a sibling module to models, not inside models)

---

## 3. Step-by-Step Implementation Details

### Step 1: Create the Logger Module

**File**: `/home/jwall/personal/rusty/rcomm/src/logger.rs`

```rust
use std::fmt;
use std::sync::OnceLock;

/// Log severity levels, ordered from most severe to least.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Error = 0,
    Warn = 1,
    Info = 2,
    Debug = 3,
    Trace = 4,
}

impl LogLevel {
    /// Parse a log level from a string (case-insensitive).
    pub fn from_str(s: &str) -> Option<LogLevel> {
        match s.to_lowercase().as_str() {
            "error" => Some(LogLevel::Error),
            "warn" | "warning" => Some(LogLevel::Warn),
            "info" => Some(LogLevel::Info),
            "debug" => Some(LogLevel::Debug),
            "trace" => Some(LogLevel::Trace),
            _ => None,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            LogLevel::Error => "ERROR",
            LogLevel::Warn => "WARN",
            LogLevel::Info => "INFO",
            LogLevel::Debug => "DEBUG",
            LogLevel::Trace => "TRACE",
        }
    }
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label())
    }
}

/// Global logger instance.
static LOGGER: OnceLock<Logger> = OnceLock::new();

pub struct Logger {
    level: LogLevel,
}

impl Logger {
    /// Initialize the global logger with the given level.
    /// Can only be called once; subsequent calls are ignored.
    pub fn init(level: LogLevel) {
        let _ = LOGGER.set(Logger { level });
    }

    /// Initialize the global logger from the RCOMM_LOG_LEVEL environment variable.
    /// Defaults to Info if not set or unrecognized.
    pub fn init_from_env() {
        let level = std::env::var("RCOMM_LOG_LEVEL")
            .ok()
            .and_then(|s| LogLevel::from_str(&s))
            .unwrap_or(LogLevel::Info);
        Logger::init(level);
    }

    /// Get the global logger. Panics if not initialized.
    fn global() -> &'static Logger {
        LOGGER.get().expect("Logger not initialized. Call Logger::init() first.")
    }

    /// Check if a message at the given level would be emitted.
    pub fn is_enabled(level: LogLevel) -> bool {
        Logger::global().level >= level
    }
}

/// Log a message at the given level.
/// Messages above the configured level are silently discarded.
pub fn log(level: LogLevel, message: &str) {
    let logger = LOGGER.get().expect("Logger not initialized");
    if logger.level >= level {
        let label = level.label();
        if level <= LogLevel::Warn {
            eprintln!("[{label}] {message}");
        } else {
            println!("[{label}] {message}");
        }
    }
}

/// Log at each severity level.
pub fn error(message: &str) { log(LogLevel::Error, message); }
pub fn warn(message: &str) { log(LogLevel::Warn, message); }
pub fn info(message: &str) { log(LogLevel::Info, message); }
pub fn debug(message: &str) { log(LogLevel::Debug, message); }
pub fn trace(message: &str) { log(LogLevel::Trace, message); }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_level_ordering() {
        assert!(LogLevel::Error < LogLevel::Warn);
        assert!(LogLevel::Warn < LogLevel::Info);
        assert!(LogLevel::Info < LogLevel::Debug);
        assert!(LogLevel::Debug < LogLevel::Trace);
    }

    #[test]
    fn log_level_from_str_valid() {
        assert_eq!(LogLevel::from_str("error"), Some(LogLevel::Error));
        assert_eq!(LogLevel::from_str("WARN"), Some(LogLevel::Warn));
        assert_eq!(LogLevel::from_str("Info"), Some(LogLevel::Info));
        assert_eq!(LogLevel::from_str("debug"), Some(LogLevel::Debug));
        assert_eq!(LogLevel::from_str("trace"), Some(LogLevel::Trace));
        assert_eq!(LogLevel::from_str("warning"), Some(LogLevel::Warn));
    }

    #[test]
    fn log_level_from_str_invalid() {
        assert_eq!(LogLevel::from_str("verbose"), None);
        assert_eq!(LogLevel::from_str(""), None);
        assert_eq!(LogLevel::from_str("42"), None);
    }

    #[test]
    fn log_level_display() {
        assert_eq!(format!("{}", LogLevel::Error), "ERROR");
        assert_eq!(format!("{}", LogLevel::Info), "INFO");
        assert_eq!(format!("{}", LogLevel::Trace), "TRACE");
    }
}
```

### Step 2: Export the Logger Module

**File**: `/home/jwall/personal/rusty/rcomm/src/lib.rs`

Add at the top:
```rust
pub mod logger;
pub mod models;
```

### Step 3: Initialize Logger in main()

**File**: `/home/jwall/personal/rusty/rcomm/src/main.rs`

At the beginning of `main()`:
```rust
fn main() {
    rcomm::logger::Logger::init_from_env();

    let port = get_port();
    let address = get_address();
    // ...
```

### Step 4: Replace println/eprintln Calls

**File**: `/home/jwall/personal/rusty/rcomm/src/main.rs`

Replace:
```rust
println!("Routes:\n{routes:#?}\n\n");
println!("Listening on {full_address}");
```
With:
```rust
rcomm::logger::debug(&format!("Routes:\n{routes:#?}"));
rcomm::logger::info(&format!("Listening on {full_address}"));
```

Replace:
```rust
eprintln!("Bad request: {e}");
```
With:
```rust
rcomm::logger::warn(&format!("Bad request: {e}"));
```

**File**: `/home/jwall/personal/rusty/rcomm/src/lib.rs`

Replace:
```rust
println!("Worker {id} got a job; executing.");
```
With:
```rust
crate::logger::debug(&format!("Worker {id} got a job; executing."));
```

Replace:
```rust
println!("Worker {id} disconnected; shutting down.");
```
With:
```rust
crate::logger::info(&format!("Worker {id} disconnected; shutting down."));
```

Replace:
```rust
println!("Shutting down worker {}", worker.id);
```
With:
```rust
crate::logger::info(&format!("Shutting down worker {}", worker.id));
```

---

## 4. Code Snippets and Pseudocode

### Logger Decision Flow

```
FUNCTION log(level, message)
    IF global_logger.level >= level THEN
        LET label = level.to_label()
        IF level <= WARN THEN
            PRINT TO STDERR "[{label}] {message}"
        ELSE
            PRINT TO STDOUT "[{label}] {message}"
        END IF
    END IF
END FUNCTION
```

### Initialization Flow

```
FUNCTION init_from_env()
    LET level_str = ENV("RCOMM_LOG_LEVEL") OR "info"
    LET level = parse_log_level(level_str) OR LogLevel::Info
    SET global_logger.level = level
END FUNCTION
```

---

## 5. Testing Strategy

### Unit Tests (in `logger.rs`)

- `log_level_ordering` — Verifies Error < Warn < Info < Debug < Trace
- `log_level_from_str_valid` — Verifies all valid level strings parse correctly
- `log_level_from_str_invalid` — Verifies invalid strings return None
- `log_level_display` — Verifies Display impl produces uppercase labels

**Run unit tests**:
```bash
cargo test logger
```

### Integration Tests

Log level filtering is difficult to test via integration tests since output goes to stdout/stderr which is suppressed in `start_server()`. This feature is best validated through:
- Unit tests for level parsing and ordering
- Manual testing with different `RCOMM_LOG_LEVEL` values

### Manual Testing

```bash
# Minimal output — only errors
RCOMM_LOG_LEVEL=error cargo run
# curl http://127.0.0.1:7878/  -> no output for successful requests

# Normal output — startup info + access logs
RCOMM_LOG_LEVEL=info cargo run
# Shows listening message and access log lines

# Verbose — includes route table and worker assignments
RCOMM_LOG_LEVEL=debug cargo run
# Shows route map, worker messages, and access logs

# Maximum verbosity
RCOMM_LOG_LEVEL=trace cargo run
# Shows everything including full request/response dumps
```

---

## 6. Edge Cases to Consider

### Case 1: Logger Not Initialized
**Scenario**: Code calls `log()` before `Logger::init()` is called
**Handling**: `LOGGER.get()` returns `None`, causing a panic with a descriptive message
**Mitigation**: `Logger::init_from_env()` is called at the very start of `main()`

### Case 2: Invalid Log Level String
**Scenario**: `RCOMM_LOG_LEVEL=verbose` (not a valid level)
**Handling**: `from_str()` returns `None`, falls back to `LogLevel::Info`

### Case 3: Thread Safety
**Scenario**: Multiple worker threads call `log()` concurrently
**Handling**: `OnceLock` provides thread-safe read access. `println!`/`eprintln!` internally lock stdout/stderr, so lines won't interleave within a single call.

### Case 4: Empty Environment Variable
**Scenario**: `RCOMM_LOG_LEVEL=` (set but empty)
**Handling**: `from_str("")` returns `None`, falls back to `Info`

### Case 5: Logging During Shutdown
**Scenario**: Worker threads log during `ThreadPool::drop()`
**Handling**: Logger is a static and outlives all threads. No issue.

---

## 7. Implementation Checklist

- [ ] Create `/home/jwall/personal/rusty/rcomm/src/logger.rs` with:
  - [ ] `LogLevel` enum with 5 variants
  - [ ] `LogLevel::from_str()` parser
  - [ ] `Logger` struct with `OnceLock` global
  - [ ] `Logger::init()` and `Logger::init_from_env()`
  - [ ] `log()`, `error()`, `warn()`, `info()`, `debug()`, `trace()` functions
  - [ ] Unit tests (4 tests)
- [ ] Update `/home/jwall/personal/rusty/rcomm/src/lib.rs`:
  - [ ] Add `pub mod logger;`
  - [ ] Replace 3 `println!` calls with logger calls
- [ ] Update `/home/jwall/personal/rusty/rcomm/src/main.rs`:
  - [ ] Call `Logger::init_from_env()` at start of `main()`
  - [ ] Replace all `println!`/`eprintln!` calls with appropriate logger level calls
- [ ] Run `cargo test logger` — all unit tests pass
- [ ] Run `cargo test` — no regressions
- [ ] Run `cargo run --bin integration_test` — integration tests pass
- [ ] Manual testing with `RCOMM_LOG_LEVEL=error`, `info`, `debug`, `trace`

---

## 8. Complexity and Risk Analysis

**Complexity**: 4/10
- `OnceLock` for global state is straightforward
- Level comparison is simple integer ordering
- Replacing println calls is mechanical but touches multiple files
- No external dependencies needed

**Risk**: Low-Medium
- Changing all println/eprintln calls to logger calls is a behavioral change to stdout/stderr output
- Integration tests suppress stdout so this shouldn't break them
- Logger initialization must happen before any log calls (ordering dependency)
- `OnceLock` is only available since Rust 1.70 (edition 2024 should be fine)

**Dependencies**: None
- Uses only `std::sync::OnceLock` and `std::fmt`

---

## 9. Future Enhancements

1. **Macro-based Logging**: Replace function calls with `log!()`, `info!()` macros that include file/line info and avoid formatting when level is filtered
2. **Per-Module Levels**: Allow `RCOMM_LOG_LEVEL=info,models=debug` for per-module control
3. **Log Format Customization**: Allow configuring the log line prefix format
4. **Integration with Log Output Destination**: Route different levels to different outputs (e.g., errors to file, info to stdout)
5. **Structured Fields**: Add key-value metadata to log entries for JSON output
