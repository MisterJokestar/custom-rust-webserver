# Feature: Support `--help` and `--version` Command-Line Flags

**Category:** Configuration
**Complexity:** 2/10
**Necessity:** 5/10

---

## Overview

Add standard `--help` / `-h` and `--version` / `-V` command-line flags. These are expected by users of any CLI tool and provide basic discoverability without requiring external documentation.

This feature can be implemented standalone (checking `std::env::args()` directly) or as part of the broader command-line argument parsing feature. The plan below covers both approaches.

---

## Files to Modify

1. **`src/main.rs`** — Add flag detection and output functions

---

## Step-by-Step Implementation

### Step 1: Add `print_usage()` Function

**File:** `src/main.rs`

```rust
fn print_usage() {
    println!("rcomm {}", env!("CARGO_PKG_VERSION"));
    println!();
    println!("A multi-threaded HTTP web server written in Rust.");
    println!();
    println!("USAGE:");
    println!("    rcomm [OPTIONS]");
    println!();
    println!("OPTIONS:");
    println!("    -p, --port <PORT>            Port to listen on [default: 7878]");
    println!("    -a, --address <ADDRESS>      Bind address [default: 127.0.0.1]");
    println!("    -t, --threads <COUNT>        Worker thread count [default: 4]");
    println!("    -d, --document-root <PATH>   Static file directory [default: ./pages]");
    println!("    -h, --help                   Print this help message and exit");
    println!("    -V, --version                Print version and exit");
    println!();
    println!("ENVIRONMENT VARIABLES:");
    println!("    RCOMM_PORT             Override port (lower priority than --port)");
    println!("    RCOMM_ADDRESS          Override bind address");
    println!("    RCOMM_THREADS          Override worker thread count");
    println!("    RCOMM_DOCUMENT_ROOT    Override document root path");
}
```

### Step 2: Add Flag Detection in `main()`

**Standalone approach** (without full argument parser):

```rust
fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
        return;
    }

    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("rcomm {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    // ... rest of main unchanged
}
```

**Integrated approach** (with CLI argument parser):

If the command-line argument parsing feature is already implemented, these flags are already parsed into `CliArgs.help` and `CliArgs.version`. Just add the early-exit logic:

```rust
fn main() {
    let cli = parse_args();

    if cli.help {
        print_usage();
        return;
    }
    if cli.version {
        println!("rcomm {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    // ... rest of main
}
```

### Step 3: Ensure Version is Sourced from `Cargo.toml`

The `env!("CARGO_PKG_VERSION")` macro reads the version from `Cargo.toml` at compile time, so the version string (`0.1.0`) stays in sync automatically. No manual version constant needed.

---

## Expected Output

### `--help`

```
rcomm 0.1.0

A multi-threaded HTTP web server written in Rust.

USAGE:
    rcomm [OPTIONS]

OPTIONS:
    -p, --port <PORT>            Port to listen on [default: 7878]
    -a, --address <ADDRESS>      Bind address [default: 127.0.0.1]
    -t, --threads <COUNT>        Worker thread count [default: 4]
    -d, --document-root <PATH>   Static file directory [default: ./pages]
    -h, --help                   Print this help message and exit
    -V, --version                Print version and exit

ENVIRONMENT VARIABLES:
    RCOMM_PORT             Override port (lower priority than --port)
    RCOMM_ADDRESS          Override bind address
    RCOMM_THREADS          Override worker thread count
    RCOMM_DOCUMENT_ROOT    Override document root path
```

### `--version`

```
rcomm 0.1.0
```

---

## Edge Cases & Handling

### 1. `--help` Combined with Other Flags
`rcomm --port 8080 --help` should print help and exit without starting the server. Help takes priority.

### 2. `--version` Combined with Other Flags
Same behavior — print version and exit.

### 3. Both `--help` and `--version`
`--help` takes priority (checked first).

### 4. After `--` Separator
Standard convention is that `--` stops flag parsing. Since rcomm takes no positional arguments, this is irrelevant. `rcomm -- --help` would print a warning about unknown argument `--help` and start normally. This is acceptable edge behavior.

---

## Testing Strategy

### Manual Testing

```bash
cargo run -- --help
cargo run -- -h
cargo run -- --version
cargo run -- -V
cargo run -- --help --port 8080   # Should still print help, not start server
```

### Unit Tests

If `print_usage()` and version output are simple `println!` calls, unit testing the output is low value. The flag detection logic can be tested if extracted into a testable function (e.g., `parse_args()`).

---

## Dependencies

- **Command-line argument parsing** — This feature can be implemented standalone, but if the full argument parser is built first, these flags integrate directly.
- **`Cargo.toml` version** — Currently `0.1.0`. The `env!("CARGO_PKG_VERSION")` macro ensures the version stays in sync.

---

## Implementation Checklist

- [ ] Add `print_usage()` function to `src/main.rs`
- [ ] Add `--help` / `-h` flag detection with early exit
- [ ] Add `--version` / `-V` flag detection with early exit
- [ ] Verify `env!("CARGO_PKG_VERSION")` resolves correctly
- [ ] Run `cargo build`
- [ ] Manual test both flags
- [ ] Update help text if other CLI flags are added later

---

## Backward Compatibility

No impact. The server currently ignores all command-line arguments. Adding flag detection doesn't change behavior when no flags are passed.
