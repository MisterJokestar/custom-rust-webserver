# Colored Terminal Output for Request Logs

**Feature**: Add colored terminal output for request log lines (green for 2xx, yellow for 3xx, red for 4xx/5xx)
**Category**: Developer Experience
**Complexity**: 3/10
**Necessity**: 2/10

---

## Overview

Currently, all request/response log lines are printed as plain text via `println!()`, making it hard to spot errors when scrolling through server output. Adding ANSI color codes to status-code-bearing log lines provides instant visual feedback: green for successes, yellow for redirects, red for errors.

Since rcomm has zero external dependencies, colors must be implemented using raw ANSI escape sequences rather than a crate like `colored` or `termcolor`.

### Current State

**`src/main.rs` lines 60, 73**:
```rust
println!("Request: {http_request}");
// ...
println!("Response: {response}");
```

All output is unformatted plain text regardless of status code.

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Changes Required**:
- Add a helper function that maps status codes to ANSI color codes
- Wrap response log lines with appropriate color escape sequences
- Reset color after each colored output

---

## Step-by-Step Implementation

### Step 1: Add ANSI Color Constants

**Location**: `src/main.rs` (top of file or in a helper module)

```rust
mod colors {
    pub const GREEN: &str = "\x1b[32m";
    pub const YELLOW: &str = "\x1b[33m";
    pub const RED: &str = "\x1b[31m";
    pub const RESET: &str = "\x1b[0m";
}
```

### Step 2: Add Status-to-Color Mapping Function

**Location**: `src/main.rs`

```rust
fn status_color(status_code: u16) -> &'static str {
    match status_code {
        200..=299 => colors::GREEN,
        300..=399 => colors::YELLOW,
        400..=599 => colors::RED,
        _ => colors::RESET,
    }
}
```

### Step 3: Apply Colors to Response Log Line

**Location**: `src/main.rs`, line 73

**Current**:
```rust
println!("Response: {response}");
```

**Updated**:
```rust
let color = status_color(response.status_code);
println!("{color}Response: {response}{RESET}", RESET = colors::RESET);
```

This requires access to `response.status_code`. The `HttpResponse` struct stores the status code — verify it's accessible as a public field.

### Step 4: Color the Error Response Log in the 400 Handler

**Location**: `src/main.rs`, inside the `Err(e)` branch of `handle_connection()`

Currently there's no response log line for the 400 case. If one is added (or when the TCP write error recovery feature adds logging), apply red coloring:

```rust
eprintln!("{}Bad request: {e}{}", colors::RED, colors::RESET);
```

### Step 5: Verify `HttpResponse` Status Code Accessibility

**Location**: `src/models/http_response.rs`

Check that `status_code` is a public field on `HttpResponse`. If it's private, either:
- Make it `pub` (preferred — it's a data struct)
- Add a `pub fn status_code(&self) -> u16` getter

---

## Edge Cases & Handling

### 1. Non-TTY Output (Piped to File)
**Scenario**: Server output is redirected to a file (`cargo run > server.log 2>&1`).
**Handling**: ANSI codes will appear as literal escape sequences in the file, e.g. `\x1b[32mResponse: ...`. This is the standard behavior for most CLI tools. A future enhancement could detect `isatty()` and disable colors when output is not a terminal, but this requires platform-specific code (`libc::isatty` on Unix) which is out of scope for this feature.

### 2. Windows Terminal Compatibility
**Scenario**: Running on Windows where older terminals don't support ANSI codes.
**Handling**: Windows 10+ and Windows Terminal support ANSI escape sequences natively. Older `cmd.exe` may show garbled output. Since rcomm targets modern systems and is a learning project, this is acceptable.

### 3. Status Codes Outside 200-599
**Scenario**: Informational (1xx) responses or non-standard codes.
**Handling**: The `_ => colors::RESET` fallback leaves these uncolored. No current code paths produce 1xx responses.

### 4. Stderr vs Stdout
**Scenario**: Error messages go to stderr via `eprintln!`, request logs go to stdout via `println!`.
**Handling**: Both stdout and stderr support ANSI colors when connected to a terminal. Apply colors consistently to both streams.

---

## Implementation Checklist

- [ ] Add ANSI color constants (`GREEN`, `YELLOW`, `RED`, `RESET`)
- [ ] Add `status_color()` function mapping status code ranges to colors
- [ ] Verify `HttpResponse.status_code` is publicly accessible
- [ ] Update response `println!` to include color based on status code
- [ ] Update error `eprintln!` lines to use red coloring
- [ ] Run `cargo build` to verify compilation
- [ ] Run `cargo test` to verify unit tests pass
- [ ] Run `cargo run --bin integration_test` to verify integration tests pass
- [ ] Manual test: visit valid route and verify green output
- [ ] Manual test: visit invalid route and verify red output (404)
- [ ] Manual test: send malformed request and verify red output (400)

---

## Backward Compatibility

Log output will now contain ANSI escape sequences. This is a visual-only change that does not affect server behavior. Integration tests that parse stdout may need adjustment if they match exact output strings, but the current tests communicate over TCP and don't parse server log output.

---

## Related Features

- **Logging & Observability > Structured Access Logging**: Structured logging (CLF) may replace or supplement these `println!` lines; colors should be applied to the structured format as well
- **Logging & Observability > Configurable Log Output**: When logging to a file, colors should be disabled (future enhancement)
- **Developer Experience > Verbose Flag**: The `--verbose` flag could control whether colored output is used
