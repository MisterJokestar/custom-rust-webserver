# Verbose Flag for Debug Output

**Feature**: Add a `--verbose` flag for debug-level output
**Category**: Developer Experience
**Complexity**: 2/10
**Necessity**: 5/10

---

## Overview

The server currently prints all request/response information unconditionally via `println!()`. There's no way to control verbosity — the output is either everything or nothing (redirect stdout to `/dev/null`). A `--verbose` (or `-v`) flag would enable detailed debug output while keeping default output minimal (just startup info and errors).

### Current State

**`src/main.rs` lines 33-34, 60, 73**:
```rust
println!("Routes:\n{routes:#?}\n\n");   // Always prints full route map
println!("Listening on {full_address}"); // Always prints
// ...
println!("Request: {http_request}");     // Every request
println!("Response: {response}");        // Every response
```

All output is unconditional. Under load, the request/response logging floods the terminal and obscures important information like startup messages and errors.

### Desired Behavior

**Default (no flag)**:
```
Listening on http://127.0.0.1:7878
```

**With `--verbose` or `-v`**:
```
Routes:
{
    "/": "pages/index.html",
    "/howdy": "pages/howdy/page.html",
    ...
}

Listening on http://127.0.0.1:7878
Request: GET / HTTP/1.1
Response: HTTP/1.1 200 OK
Request: GET /howdy HTTP/1.1
Response: HTTP/1.1 200 OK
```

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Changes Required**:
- Add `--verbose` / `-v` argument detection
- Pass verbosity flag to `handle_connection()` (or use a global/shared flag)
- Gate route dump and per-request logging behind the verbose flag
- Keep startup message and error output always visible

---

## Step-by-Step Implementation

### Step 1: Parse the Verbose Flag

**Location**: `src/main.rs`, `main()` function

```rust
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let verbose = args.iter().any(|a| a == "--verbose" || a == "-v");

    let port = get_port();
    let address = get_address();
    let full_address = format!("{address}:{port}");
    let listener = TcpListener::bind(&full_address).unwrap();
    // ...
```

### Step 2: Gate Route Dump Behind Verbose Flag

**Location**: `src/main.rs`, line 33

**Current**:
```rust
println!("Routes:\n{routes:#?}\n\n");
```

**Updated**:
```rust
if verbose {
    println!("Routes:\n{routes:#?}\n\n");
}
```

### Step 3: Pass Verbose Flag to Connection Handler

Since `handle_connection()` runs in worker threads, the verbose flag needs to be shared. Use an `Arc<bool>` or, more simply, just pass it as a `bool` (which is `Copy`):

**Location**: `src/main.rs`, listener loop

**Current**:
```rust
for stream in listener.incoming() {
    let routes_clone = routes.clone();
    let stream = stream.unwrap();

    pool.execute(move || {
        handle_connection(stream, routes_clone);
    });
}
```

**Updated**:
```rust
for stream in listener.incoming() {
    let routes_clone = routes.clone();
    let stream = stream.unwrap();

    pool.execute(move || {
        handle_connection(stream, routes_clone, verbose);
    });
}
```

### Step 4: Gate Request/Response Logging

**Location**: `src/main.rs`, `handle_connection()` function

**Updated signature**:
```rust
fn handle_connection(mut stream: TcpStream, routes: HashMap<String, PathBuf>, verbose: bool) {
```

**Updated logging**:
```rust
if verbose {
    println!("Request: {http_request}");
}
// ... build response ...
if verbose {
    println!("Response: {response}");
}
```

Error output (`eprintln!` for bad requests) should remain unconditional — errors are always important.

### Step 5: Environment Variable Alternative

For cases where the binary is run without direct CLI access, also support `RCOMM_VERBOSE`:

```rust
let verbose = args.iter().any(|a| a == "--verbose" || a == "-v")
    || std::env::var("RCOMM_VERBOSE").is_ok();
```

---

## Edge Cases & Handling

### 1. Verbose with High Traffic
**Scenario**: `--verbose` is enabled and the server receives hundreds of requests per second.
**Handling**: `println!()` acquires a lock on stdout for each call. Under extreme load, this can become a bottleneck. This is acceptable for a development tool — verbose mode is for debugging, not production.

### 2. Integration Tests
**Scenario**: Integration tests spawn the server binary. The verbose flag shouldn't interfere.
**Handling**: Integration tests don't pass `--verbose`, so the server runs in quiet mode. Tests communicate over TCP and don't parse stdout. No changes needed to tests.

### 3. Future Logging System
**Scenario**: The structured logging feature (Logging & Observability) adds proper log levels.
**Handling**: The `--verbose` flag maps naturally to a log level: default = `error`/`warn`, verbose = `info`/`debug`. When the logging system is added, `--verbose` becomes sugar for `--log-level=debug`.

---

## Implementation Checklist

- [ ] Add `--verbose` / `-v` argument parsing in `main()`
- [ ] Add `RCOMM_VERBOSE` environment variable support
- [ ] Gate route dump `println!` behind verbose flag
- [ ] Update `handle_connection()` signature to accept `verbose: bool`
- [ ] Gate request `println!` behind verbose flag
- [ ] Gate response `println!` behind verbose flag
- [ ] Keep `eprintln!` error output unconditional
- [ ] Run `cargo build` to verify compilation
- [ ] Run `cargo test` to verify unit tests pass
- [ ] Run `cargo run --bin integration_test` to verify integration tests pass
- [ ] Manual test: start without `--verbose`, verify only startup message appears
- [ ] Manual test: start with `--verbose`, verify request/response logging appears

---

## Backward Compatibility

**Breaking change**: Default behavior changes from verbose to quiet. Previously, all requests/responses were logged. After this change, the default is quiet (startup message + errors only). This is intentional — the current behavior is a development artifact, not a designed feature. Users who want the old behavior use `--verbose`.

If this breaking change is undesirable, the default could be inverted: verbose by default, with a `--quiet` / `-q` flag to suppress. However, quiet-by-default is the convention for production servers.

---

## Related Features

- **Logging & Observability > Configurable Log Levels**: A full log level system supersedes this flag but can use it as the initial toggle
- **Developer Experience > Colored Terminal Output**: Colors apply to verbose output; the two features compose naturally
- **Configuration > Command-Line Argument Parsing**: This is the second CLI flag (after `--watch`); a proper argument parser should unify them
