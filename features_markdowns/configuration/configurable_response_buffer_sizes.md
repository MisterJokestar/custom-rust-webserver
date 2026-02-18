# Feature: Add Configurable Response Buffer Sizes

**Category:** Configuration
**Complexity:** 3/10
**Necessity:** 3/10

---

## Overview

Currently, responses are serialized into a single `Vec<u8>` via `HttpResponse::as_bytes()` and written to the TCP stream in one `write_all()` call. This means the entire response (headers + body) is held in memory as a contiguous buffer before any data is sent to the client.

This feature adds:
1. **Configurable write buffer size** — Controls how much data is written to the socket per `write()` call
2. **Buffered writing** — Uses `BufWriter` with a configurable capacity to batch small writes and control memory usage for large responses

This is primarily a performance tuning knob for operators serving large files, and a foundation for future streaming response support.

---

## Files to Modify

1. **`src/main.rs`** — Add buffer size configuration, use `BufWriter` for response writing
2. **`src/models/http_response.rs`** — Optionally add a method to write response in chunks rather than serializing to a single `Vec<u8>`

---

## Step-by-Step Implementation

### Step 1: Add Buffer Size Configuration

**File:** `src/main.rs`

```rust
const DEFAULT_WRITE_BUFFER_SIZE: usize = 8 * 1024; // 8 KB

fn get_write_buffer_size() -> usize {
    std::env::var("RCOMM_WRITE_BUFFER_SIZE")
        .ok()
        .and_then(|val| val.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(DEFAULT_WRITE_BUFFER_SIZE)
}
```

### Step 2: Use `BufWriter` in `handle_connection()`

**File:** `src/main.rs`

**Current (line 74):**
```rust
stream.write_all(&response.as_bytes()).unwrap();
```

**New:**
```rust
use std::io::BufWriter;

fn handle_connection(mut stream: TcpStream, routes: HashMap<String, PathBuf>, write_buffer_size: usize) {
    // ... request parsing unchanged ...

    let response_bytes = response.as_bytes();
    let mut writer = BufWriter::with_capacity(write_buffer_size, &mut stream);
    writer.write_all(&response_bytes).unwrap();
    writer.flush().unwrap();
}
```

The `BufWriter` collects writes up to `write_buffer_size` before flushing to the underlying stream. For a single `write_all()` call, this primarily controls the kernel-level buffer interaction. The real benefit comes when response writing is split into multiple calls (headers, then body chunks).

### Step 3: Add Chunked Response Writing Method (Optional Enhancement)

**File:** `src/models/http_response.rs`

Add a method that writes the response in parts rather than serializing everything first:

```rust
use std::io::{self, Write};

impl HttpResponse {
    /// Write the response directly to a writer, avoiding a single large allocation
    pub fn write_to<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        // Write status line
        write!(writer, "{} {} {}\r\n",
            self.version,
            self.status_code,
            get_status_phrase(self.status_code)
        )?;

        // Write headers
        for (key, value) in &self.headers {
            write!(writer, "{}: {}\r\n", key, value)?;
        }
        writer.write_all(b"\r\n")?;

        // Write body
        if let Some(body) = &self.body {
            writer.write_all(body)?;
        }

        writer.flush()
    }
}
```

Then in `handle_connection()`:

```rust
let mut writer = BufWriter::with_capacity(write_buffer_size, &mut stream);
response.write_to(&mut writer).unwrap();
```

This avoids the intermediate `Vec<u8>` allocation from `as_bytes()`, reducing peak memory usage for large responses.

### Step 4: Pass Buffer Size Through the Connection Handler

**File:** `src/main.rs`

```rust
fn main() {
    // ...
    let write_buffer_size = get_write_buffer_size();

    for stream in listener.incoming() {
        let routes_clone = routes.clone();
        let stream = stream.unwrap();
        let buf_size = write_buffer_size;

        pool.execute(move || {
            handle_connection(stream, routes_clone, buf_size);
        });
    }
}
```

### Step 5: Print Configuration on Startup

```rust
println!("Write buffer size: {} bytes", write_buffer_size);
```

---

## Environment Variables

| Variable | Type | Default | Description |
|----------|------|---------|-------------|
| `RCOMM_WRITE_BUFFER_SIZE` | `usize` (bytes) | `8192` | Size of the `BufWriter` buffer for response output |

---

## Edge Cases & Handling

### 1. Buffer Size of 0
Filtered out by `.filter(|&n| n > 0)`. A zero-size buffer would cause `BufWriter` to flush on every write, which degrades performance.

### 2. Very Small Buffer
`RCOMM_WRITE_BUFFER_SIZE=1` is valid but forces a flush per byte. Not practically useful, but harmless.

### 3. Very Large Buffer
`RCOMM_WRITE_BUFFER_SIZE=104857600` (100 MB) allocates 100 MB per connection per worker. With 4 workers, that's 400 MB just for write buffers. No upper bound is enforced, but the startup message makes the configured value visible.

### 4. Response Smaller Than Buffer
If the full response fits within the buffer, `BufWriter` accumulates everything and flushes once on `flush()`. Behavior is identical to the current single `write_all()` approach.

### 5. Error During Flush
If `flush()` fails (client disconnected), the error propagates. Currently this would hit an `unwrap()` and panic — a pre-existing issue tracked in the Error Handling features.

---

## Performance Considerations

- **Default 8 KB buffer** matches common TCP MSS (Maximum Segment Size) and is a reasonable starting point
- For large files (images, downloads), increasing to 64 KB or 128 KB can reduce syscall overhead
- For small HTML pages, the default is more than sufficient since most responses fit in a single buffer
- The `write_to()` method eliminates one allocation (the `Vec<u8>` from `as_bytes()`), which matters for large responses

---

## Testing Strategy

### Unit Tests

```rust
#[test]
fn write_to_produces_valid_http() {
    let mut response = HttpResponse::build(String::from("HTTP/1.1"), 200);
    response.add_header("Content-Type".to_string(), "text/plain".to_string());
    response.add_body(b"Hello".to_vec());

    let mut output = Vec::new();
    response.write_to(&mut output).unwrap();

    let output_str = String::from_utf8(output).unwrap();
    assert!(output_str.starts_with("HTTP/1.1 200"));
    assert!(output_str.contains("content-type: text/plain"));
    assert!(output_str.ends_with("Hello"));
}
```

### Integration Tests

Existing integration tests verify correct response content. Since `BufWriter` is transparent to the client, all existing tests should pass without modification.

### Manual Testing

```bash
# Default buffer size
cargo run

# Large buffer for file serving
RCOMM_WRITE_BUFFER_SIZE=65536 cargo run

# Verify responses are identical
curl -s http://localhost:7878/ | md5sum
```

---

## Implementation Checklist

- [ ] Add `DEFAULT_WRITE_BUFFER_SIZE` constant and `get_write_buffer_size()` helper
- [ ] Add `BufWriter` wrapping in `handle_connection()`
- [ ] Optionally add `write_to()` method on `HttpResponse`
- [ ] Pass buffer size through `main()` to `handle_connection()`
- [ ] Print buffer size on startup
- [ ] Run `cargo build` and `cargo test`
- [ ] Run `cargo run --bin integration_test`
- [ ] Manual test with different buffer sizes

---

## Dependencies

- None. This feature is independent of other configuration features.
- The `write_to()` method lays groundwork for future **chunked transfer encoding** and **streaming responses**.

---

## Backward Compatibility

No behavioral change from the client's perspective. The response bytes are identical; only the internal write strategy changes. The default 8 KB buffer is suitable for the current workload of small HTML/CSS/JS files.
