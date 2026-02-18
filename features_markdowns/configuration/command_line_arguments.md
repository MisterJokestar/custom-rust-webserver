# Feature: Add Command-Line Argument Parsing

**Category:** Configuration
**Complexity:** 4/10
**Necessity:** 6/10

---

## Overview

Currently all server configuration is done via environment variables. This feature adds command-line argument parsing so operators can configure the server directly from the shell:

```bash
rcomm --port 8080 --address 0.0.0.0 --document-root ./public --threads 8
```

Since rcomm has zero external dependencies, the argument parser must be hand-written. The parser needs to handle `--key value` and `--key=value` forms for a small, fixed set of known flags.

**Priority order** (highest wins):
1. Command-line arguments
2. Environment variables
3. Configuration file values (if config file feature is implemented)
4. Built-in defaults

---

## Files to Modify

1. **`src/main.rs`** — Parse `std::env::args()` and integrate with existing config helpers
2. **`src/config.rs`** (new or existing) — Argument parser and unified config resolution

---

## Supported Arguments

| Flag | Short | Value | Default | Description |
|------|-------|-------|---------|-------------|
| `--port` | `-p` | `<u16>` | `7878` | TCP port to listen on |
| `--address` | `-a` | `<ip>` | `127.0.0.1` | Bind address |
| `--threads` | `-t` | `<usize>` | `4` | Worker thread count |
| `--document-root` | `-d` | `<path>` | `./pages` | Static file directory |
| `--help` | `-h` | — | — | Print usage and exit (see separate feature) |
| `--version` | `-V` | — | — | Print version and exit (see separate feature) |

---

## Step-by-Step Implementation

### Step 1: Define Parsed Arguments Struct

**File:** `src/main.rs` (or `src/config.rs` if that module exists)

```rust
struct CliArgs {
    port: Option<String>,
    address: Option<String>,
    threads: Option<String>,
    document_root: Option<String>,
    help: bool,
    version: bool,
}
```

### Step 2: Implement Argument Parser

```rust
fn parse_args() -> CliArgs {
    let args: Vec<String> = std::env::args().collect();
    let mut cli = CliArgs {
        port: None,
        address: None,
        threads: None,
        document_root: None,
        help: false,
        version: false,
    };

    let mut i = 1; // skip binary name
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => cli.help = true,
            "--version" | "-V" => cli.version = true,
            "--port" | "-p" => {
                i += 1;
                if i < args.len() {
                    cli.port = Some(args[i].clone());
                }
            }
            "--address" | "-a" => {
                i += 1;
                if i < args.len() {
                    cli.address = Some(args[i].clone());
                }
            }
            "--threads" | "-t" => {
                i += 1;
                if i < args.len() {
                    cli.threads = Some(args[i].clone());
                }
            }
            "--document-root" | "-d" => {
                i += 1;
                if i < args.len() {
                    cli.document_root = Some(args[i].clone());
                }
            }
            other => {
                // Handle --key=value form
                if let Some((key, value)) = other.split_once('=') {
                    match key {
                        "--port" | "-p" => cli.port = Some(value.to_string()),
                        "--address" | "-a" => cli.address = Some(value.to_string()),
                        "--threads" | "-t" => cli.threads = Some(value.to_string()),
                        "--document-root" | "-d" => cli.document_root = Some(value.to_string()),
                        _ => eprintln!("Warning: unknown argument '{other}'"),
                    }
                } else {
                    eprintln!("Warning: unknown argument '{other}'");
                }
            }
        }
        i += 1;
    }

    cli
}
```

### Step 3: Integrate with Config Resolution

Update the `get_*()` helpers to check CLI args first:

```rust
fn get_port(cli: &CliArgs) -> String {
    cli.port.clone()
        .or_else(|| std::env::var("RCOMM_PORT").ok())
        .unwrap_or_else(|| String::from("7878"))
}

fn get_address(cli: &CliArgs) -> String {
    cli.address.clone()
        .or_else(|| std::env::var("RCOMM_ADDRESS").ok())
        .unwrap_or_else(|| String::from("127.0.0.1"))
}

fn get_threads(cli: &CliArgs) -> usize {
    cli.threads.clone()
        .or_else(|| std::env::var("RCOMM_THREADS").ok())
        .and_then(|val| val.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(4)
}

fn get_document_root(cli: &CliArgs) -> String {
    cli.document_root.clone()
        .or_else(|| std::env::var("RCOMM_DOCUMENT_ROOT").ok())
        .unwrap_or_else(|| String::from("./pages"))
}
```

### Step 4: Update `main()`

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

    let port = get_port(&cli);
    let address = get_address(&cli);
    let threads = get_threads(&cli);
    let document_root = get_document_root(&cli);

    let full_address = format!("{address}:{port}");
    let listener = TcpListener::bind(&full_address).unwrap();

    let pool = ThreadPool::new(threads);

    let path = Path::new(&document_root);
    let routes = build_routes(String::from(""), path);

    println!("Routes:\n{routes:#?}\n\n");
    println!("Listening on {full_address}");

    for stream in listener.incoming() {
        let routes_clone = routes.clone();
        let stream = stream.unwrap();

        pool.execute(move || {
            handle_connection(stream, routes_clone);
        });
    }
}
```

---

## Edge Cases & Handling

### 1. Missing Value After Flag
`rcomm --port` (no value following) — the `i += 1` check `if i < args.len()` prevents out-of-bounds. The option remains `None` and falls through to env var or default.

### 2. Invalid Values
`rcomm --port abc` — the port string `"abc"` is passed to `TcpListener::bind()`, which will fail with an error. This matches the current behavior for invalid `RCOMM_PORT` values.

### 3. Unknown Arguments
Unknown flags print a warning to stderr but don't cause the server to exit. This is forgiving behavior that allows forward compatibility.

### 4. Combined Short Flags
Combined forms like `-pt 8080 8` are not supported. Each flag must be specified separately. This keeps the parser simple.

### 5. `--key=value` Form
Supported via `split_once('=')` fallback in the match arm. Both `--port 8080` and `--port=8080` work.

---

## Testing Strategy

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_args_empty() {
        // With only binary name, all fields should be None/false
    }

    #[test]
    fn parse_args_long_form() {
        // --port 8080 --address 0.0.0.0
    }

    #[test]
    fn parse_args_short_form() {
        // -p 8080 -a 0.0.0.0
    }

    #[test]
    fn parse_args_equals_form() {
        // --port=8080
    }
}
```

### Manual Testing

```bash
# Long form
cargo run -- --port 9090 --address 0.0.0.0 --threads 8

# Short form
cargo run -- -p 9090 -a 0.0.0.0 -t 8

# Equals form
cargo run -- --port=9090

# CLI overrides env var
RCOMM_PORT=3000 cargo run -- --port 9090
# Observe: Listening on 127.0.0.1:9090 (CLI wins)

# Unknown args warn but don't crash
cargo run -- --unknown-flag
# Observe: Warning printed, server starts normally
```

---

## Dependencies

This feature is independent but should be coordinated with:
- **Configurable thread pool size** — adds `--threads` flag
- **Configuration file format** — determines priority order (CLI > env > file > default)
- **`--help` and `--version` flags** — handled in the same parser

---

## Implementation Checklist

- [ ] Define `CliArgs` struct
- [ ] Implement `parse_args()` function
- [ ] Refactor `get_port()`, `get_address()` to accept `&CliArgs`
- [ ] Add `get_threads()`, `get_document_root()` helpers
- [ ] Update `main()` to parse args and resolve config
- [ ] Handle `--help` and `--version` early exits
- [ ] Add unit tests for argument parsing
- [ ] Run `cargo build` and `cargo test`
- [ ] Run `cargo run --bin integration_test`
- [ ] Manual test with various argument combinations

---

## Backward Compatibility

Fully backward compatible. When no arguments are passed, the server behaves identically to today. Environment variables continue to work and serve as the fallback when no CLI argument is provided.
