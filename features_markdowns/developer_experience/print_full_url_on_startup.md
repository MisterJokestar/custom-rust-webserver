# Print Full URL on Startup

**Feature**: Print the full URL (including address and port) on startup for easy click-to-open
**Category**: Developer Experience
**Complexity**: 1/10
**Necessity**: 5/10

---

## Overview

When the server starts, it prints `Listening on 127.0.0.1:7878`, which is an address:port pair but not a clickable URL. Most terminals support click-to-open for `http://` URLs, so printing `http://127.0.0.1:7878` would let developers Ctrl+click (or Cmd+click) to open the server directly in their browser.

### Current State

**`src/main.rs` line 34**:
```rust
println!("Listening on {full_address}");
```

Output:
```
Listening on 127.0.0.1:7878
```

### Desired Output

```
Listening on http://127.0.0.1:7878
```

Or, for improved clarity:

```
Server running at http://127.0.0.1:7878
```

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Line 34**: Update the `println!` format string to include the `http://` protocol prefix.

---

## Step-by-Step Implementation

### Step 1: Update the Startup Message

**Location**: `src/main.rs`, line 34

**Current**:
```rust
println!("Listening on {full_address}");
```

**Updated**:
```rust
println!("Listening on http://{full_address}");
```

That's it. The `full_address` variable already contains `127.0.0.1:7878` (or whatever is configured via `RCOMM_ADDRESS` and `RCOMM_PORT`). Prepending `http://` makes it a valid URL that terminals will recognize as clickable.

### Step 2 (Optional): Improve Startup Banner

For a slightly more informative startup message:

```rust
println!("\nServer running at http://{full_address}\n");
```

The surrounding newlines make the URL stand out from the route debug dump that precedes it (line 33: `println!("Routes:\n{routes:#?}\n\n")`).

---

## Edge Cases & Handling

### 1. Custom Address `0.0.0.0`
**Scenario**: User sets `RCOMM_ADDRESS=0.0.0.0` to bind to all interfaces.
**Handling**: The URL `http://0.0.0.0:7878` is technically valid but not useful in a browser. However, this is standard behavior — tools like `python -m http.server` and `npm start` also print `0.0.0.0`. The user knows to use `localhost` or their actual IP. A future enhancement could print both the bind address and a `localhost` hint when binding to `0.0.0.0`.

### 2. Non-Standard Port
**Scenario**: User sets `RCOMM_PORT=443` or another well-known port.
**Handling**: The URL is still correct: `http://127.0.0.1:443`. When TLS support is added, the protocol prefix should change to `https://`.

### 3. IPv6 Address
**Scenario**: User sets `RCOMM_ADDRESS=::1`.
**Handling**: The URL `http://::1:7878` is ambiguous. IPv6 URLs require brackets: `http://[::1]:7878`. This is a minor edge case that can be addressed when IPv6 support is explicitly added. The current default is `127.0.0.1` (IPv4).

---

## Implementation Checklist

- [ ] Update `println!` on line 34 to prepend `http://` to the address
- [ ] Run `cargo build` to verify compilation
- [ ] Run `cargo test` to verify unit tests pass
- [ ] Run `cargo run --bin integration_test` to verify integration tests pass
- [ ] Manual test: start server, verify URL is clickable in terminal

---

## Backward Compatibility

The only change is the format of the startup log message. No functional behavior changes. Integration tests don't parse the startup message (they use TCP connections directly), so no test changes needed.

---

## Related Features

- **Security > TLS Support**: When TLS is added, the URL prefix should change to `https://`
- **Developer Experience > Watch Mode**: The watch runner should also print this URL after each restart
- **Configuration > Command-Line Arguments**: The `--help` output should show the default URL
