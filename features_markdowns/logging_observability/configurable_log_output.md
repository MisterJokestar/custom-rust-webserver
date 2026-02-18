# Implementation Plan: Configurable Log Output Destination

## 1. Overview of the Feature

Configurable log output allows operators to direct server log messages to different destinations: stdout, stderr, a file, or a combination. This is essential for production deployments where logs need to be persisted to disk, rotated by external tools (logrotate), or separated from application output.

**Current State**: All log output uses `println!` (stdout) and `eprintln!` (stderr) directly, with no way to redirect log messages without shell-level piping (`> access.log 2> error.log`). There is no distinction between access logs and error logs beyond stdout/stderr.

**Desired State**: The log output destination is configurable via the `RCOMM_LOG_OUTPUT` environment variable:

| Value | Behavior |
|-------|----------|
| `stdout` (default) | All log messages go to stdout |
| `stderr` | All log messages go to stderr |
| `file:/path/to/logfile` | All log messages are appended to the specified file |
| `both:/path/to/logfile` | Log messages go to both stdout and the specified file |

The log writer is thread-safe and handles concurrent writes from multiple worker threads.

**Impact**:
- Enables file-based logging for production without shell redirection
- Supports dual output (console + file) for development with persistent logging
- Foundation for log rotation and structured log pipelines
- Depends on: Configurable Log Levels feature (recommended but not required)

---

## 2. Files to be Modified or Created

### New Files

1. **`/home/jwall/personal/rusty/rcomm/src/log_output.rs`**
   - Defines `LogOutput` enum: `Stdout`, `Stderr`, `File(path)`, `Both(path)`
   - Defines `LogWriter` struct that wraps the output destination
   - `LogWriter::write_line()` method that outputs to the configured destination
   - Thread-safe via `Mutex` around the file handle
   - Initialization from `RCOMM_LOG_OUTPUT` environment variable

### Modified Files

2. **`/home/jwall/personal/rusty/rcomm/src/lib.rs`**
   - Add `pub mod log_output;`

3. **`/home/jwall/personal/rusty/rcomm/src/logger.rs`** (if log levels feature exists)
   - Modify `log()` function to use `LogWriter` instead of `println!`/`eprintln!`
   - Or: if log levels feature doesn't exist yet, modify all log call sites in `main.rs` and `lib.rs`

4. **`/home/jwall/personal/rusty/rcomm/src/main.rs`**
   - Initialize `LogWriter` at startup from environment variable
   - Replace direct `println!`/`eprintln!` calls with `LogWriter::write_line()`

---

## 3. Step-by-Step Implementation Details

### Step 1: Create the Log Output Module

**File**: `/home/jwall/personal/rusty/rcomm/src/log_output.rs`

```rust
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::sync::{Mutex, OnceLock};

/// Log output destination configuration.
#[derive(Debug)]
pub enum LogOutput {
    Stdout,
    Stderr,
    File(String),
    Both(String),
}

impl LogOutput {
    /// Parse from a string (e.g., "stdout", "stderr", "file:/var/log/rcomm.log").
    pub fn from_str(s: &str) -> Result<LogOutput, String> {
        match s {
            "stdout" => Ok(LogOutput::Stdout),
            "stderr" => Ok(LogOutput::Stderr),
            s if s.starts_with("file:") => {
                let path = &s[5..];
                if path.is_empty() {
                    return Err("file: requires a path".to_string());
                }
                Ok(LogOutput::File(path.to_string()))
            }
            s if s.starts_with("both:") => {
                let path = &s[5..];
                if path.is_empty() {
                    return Err("both: requires a path".to_string());
                }
                Ok(LogOutput::Both(path.to_string()))
            }
            other => Err(format!("Unknown log output: {other}")),
        }
    }
}

/// Global log writer instance.
static LOG_WRITER: OnceLock<LogWriter> = OnceLock::new();

/// Thread-safe log writer.
pub struct LogWriter {
    output: LogOutput,
    file: Option<Mutex<File>>,
}

impl LogWriter {
    /// Initialize the global log writer.
    pub fn init(output: LogOutput) -> Result<(), String> {
        let file = match &output {
            LogOutput::File(path) | LogOutput::Both(path) => {
                let f = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .map_err(|e| format!("Failed to open log file {path}: {e}"))?;
                Some(Mutex::new(f))
            }
            _ => None,
        };

        let writer = LogWriter { output, file };
        LOG_WRITER.set(writer).map_err(|_| "LogWriter already initialized".to_string())
    }

    /// Initialize from the RCOMM_LOG_OUTPUT environment variable.
    /// Defaults to stdout if not set.
    pub fn init_from_env() -> Result<(), String> {
        let output = match std::env::var("RCOMM_LOG_OUTPUT") {
            Ok(val) => LogOutput::from_str(&val)?,
            Err(_) => LogOutput::Stdout,
        };
        LogWriter::init(output)
    }

    /// Get the global log writer.
    fn global() -> &'static LogWriter {
        LOG_WRITER.get().expect("LogWriter not initialized")
    }
}

/// Write a log line to the configured output destination.
/// Appends a newline automatically.
pub fn write_line(message: &str) {
    let writer = LogWriter::global();

    match &writer.output {
        LogOutput::Stdout => {
            let _ = writeln!(io::stdout().lock(), "{message}");
        }
        LogOutput::Stderr => {
            let _ = writeln!(io::stderr().lock(), "{message}");
        }
        LogOutput::File(_) => {
            if let Some(file) = &writer.file {
                let mut f = file.lock().unwrap();
                let _ = writeln!(f, "{message}");
            }
        }
        LogOutput::Both(_) => {
            let _ = writeln!(io::stdout().lock(), "{message}");
            if let Some(file) = &writer.file {
                let mut f = file.lock().unwrap();
                let _ = writeln!(f, "{message}");
            }
        }
    }
}

/// Write a log line to stderr regardless of configured output.
/// Used for critical errors during initialization.
pub fn write_error(message: &str) {
    let _ = writeln!(io::stderr().lock(), "{message}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_output_from_str_stdout() {
        let output = LogOutput::from_str("stdout").unwrap();
        assert!(matches!(output, LogOutput::Stdout));
    }

    #[test]
    fn log_output_from_str_stderr() {
        let output = LogOutput::from_str("stderr").unwrap();
        assert!(matches!(output, LogOutput::Stderr));
    }

    #[test]
    fn log_output_from_str_file() {
        let output = LogOutput::from_str("file:/tmp/test.log").unwrap();
        assert!(matches!(output, LogOutput::File(ref p) if p == "/tmp/test.log"));
    }

    #[test]
    fn log_output_from_str_both() {
        let output = LogOutput::from_str("both:/tmp/test.log").unwrap();
        assert!(matches!(output, LogOutput::Both(ref p) if p == "/tmp/test.log"));
    }

    #[test]
    fn log_output_from_str_file_empty_path() {
        let result = LogOutput::from_str("file:");
        assert!(result.is_err());
    }

    #[test]
    fn log_output_from_str_unknown() {
        let result = LogOutput::from_str("syslog");
        assert!(result.is_err());
    }
}
```

### Step 2: Export the Module

**File**: `/home/jwall/personal/rusty/rcomm/src/lib.rs`

```rust
pub mod log_output;
pub mod models;
```

### Step 3: Initialize in main()

**File**: `/home/jwall/personal/rusty/rcomm/src/main.rs`

At the beginning of `main()`:
```rust
fn main() {
    if let Err(e) = rcomm::log_output::LogWriter::init_from_env() {
        eprintln!("Failed to initialize log output: {e}");
        std::process::exit(1);
    }

    // ... rest of main ...
}
```

### Step 4: Replace Direct Output Calls

Replace `println!("...")` calls with `rcomm::log_output::write_line("...")` and `eprintln!("...")` with `rcomm::log_output::write_error("...")`.

If the log levels feature is implemented, integrate `write_line()` into the `log()` function in `logger.rs` instead of modifying each call site individually.

---

## 4. Code Snippets and Pseudocode

```
FUNCTION write_line(message)
    MATCH output_destination DO
        CASE Stdout:
            WRITE message TO stdout
        CASE Stderr:
            WRITE message TO stderr
        CASE File(path):
            LOCK file_handle
            WRITE message TO file
            UNLOCK file_handle
        CASE Both(path):
            WRITE message TO stdout
            LOCK file_handle
            WRITE message TO file
            UNLOCK file_handle
    END MATCH
END FUNCTION
```

---

## 5. Testing Strategy

### Unit Tests (in `log_output.rs`)

- `log_output_from_str_stdout` — Parses "stdout" correctly
- `log_output_from_str_stderr` — Parses "stderr" correctly
- `log_output_from_str_file` — Parses "file:/path" correctly
- `log_output_from_str_both` — Parses "both:/path" correctly
- `log_output_from_str_file_empty_path` — Rejects "file:" with no path
- `log_output_from_str_unknown` — Rejects unknown output types

**Run unit tests**:
```bash
cargo test log_output
```

### Integration Tests

File output can be tested by:
1. Starting the server with `RCOMM_LOG_OUTPUT=file:/tmp/rcomm_test.log`
2. Sending a request
3. Reading the log file and verifying content

This is best done as a manual test or a dedicated integration test case.

### Manual Testing

```bash
# Default (stdout)
cargo run
curl http://127.0.0.1:7878/
# Output appears on terminal

# File output
RCOMM_LOG_OUTPUT=file:/tmp/rcomm.log cargo run &
curl http://127.0.0.1:7878/
cat /tmp/rcomm.log
# Log entries appear in file, not on terminal

# Both output
RCOMM_LOG_OUTPUT=both:/tmp/rcomm.log cargo run
curl http://127.0.0.1:7878/
# Output appears on terminal AND in file
```

---

## 6. Edge Cases to Consider

### Case 1: Log File Path Doesn't Exist
**Scenario**: `RCOMM_LOG_OUTPUT=file:/var/log/rcomm/access.log` but `/var/log/rcomm/` doesn't exist
**Handling**: `OpenOptions::open()` will fail. The error is caught and printed to stderr before exiting.

### Case 2: Permission Denied on Log File
**Scenario**: Server doesn't have write permission to the log file
**Handling**: Same as above — caught during initialization with a descriptive error.

### Case 3: Disk Full
**Scenario**: Log file write fails because disk is full
**Handling**: `writeln!()` returns an error which is silently ignored (`let _ =`). The server continues operating without logging. This is intentional — a full disk shouldn't crash the server.

### Case 4: Concurrent File Writes
**Scenario**: Multiple worker threads write to the log file simultaneously
**Handling**: The `Mutex<File>` serializes writes. Each `write_line()` call acquires the lock, writes, and releases. Lines are atomic at the application level.

### Case 5: Log File Rotation
**Scenario**: An external tool (logrotate) moves the log file while the server is running
**Handling**: The server holds a file descriptor, so it continues writing to the renamed/deleted file. A server restart is needed to pick up the new file. Future enhancement: support SIGHUP to reopen the log file.

### Case 6: Very Long Log Lines
**Scenario**: A log line exceeds OS buffer size
**Handling**: `writeln!()` handles arbitrary length. File I/O buffers at the OS level.

---

## 7. Implementation Checklist

- [ ] Create `/home/jwall/personal/rusty/rcomm/src/log_output.rs` with:
  - [ ] `LogOutput` enum with 4 variants
  - [ ] `LogOutput::from_str()` parser
  - [ ] `LogWriter` struct with `OnceLock` global
  - [ ] `LogWriter::init()` and `LogWriter::init_from_env()`
  - [ ] `write_line()` and `write_error()` functions
  - [ ] Unit tests (6 tests)
- [ ] Update `/home/jwall/personal/rusty/rcomm/src/lib.rs` — add `pub mod log_output;`
- [ ] Update `/home/jwall/personal/rusty/rcomm/src/main.rs`:
  - [ ] Initialize `LogWriter` at startup
  - [ ] Replace direct stdout/stderr calls with `write_line()`
- [ ] If logger feature exists: integrate `write_line()` into `logger::log()`
- [ ] Run `cargo test log_output` — all unit tests pass
- [ ] Run `cargo test` — no regressions
- [ ] Run `cargo run --bin integration_test` — integration tests pass
- [ ] Manual testing with stdout, stderr, file, and both modes

---

## 8. Complexity and Risk Analysis

**Complexity**: 3/10
- String parsing for configuration is simple
- File I/O with Mutex is standard Rust pattern
- `OnceLock` for global state is straightforward
- Most complexity is in the file open/write error handling

**Risk**: Low
- File initialization failure is caught at startup and causes a clean exit
- Runtime write failures are silently ignored (server stays up)
- Mutex contention on the file handle could slow logging under very high concurrency, but this is unlikely to be a bottleneck
- Integration tests suppress stdout so switching to file output won't affect them

**Dependencies**: None
- Uses only `std::fs`, `std::io`, `std::sync`

---

## 9. Future Enhancements

1. **SIGHUP Log Reopen**: Close and reopen the log file on SIGHUP signal for log rotation
2. **Buffered Writing**: Use `BufWriter` for better file I/O performance
3. **Separate Error Log**: Allow different destinations for error and access logs
4. **Syslog Integration**: Support `syslog:` prefix for Unix syslog output
5. **Log File Size Limit**: Automatic rotation when file exceeds a configured size
