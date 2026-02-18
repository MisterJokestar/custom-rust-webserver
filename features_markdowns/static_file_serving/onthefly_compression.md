# On-the-fly Compression Implementation Plan

## Overview

On-the-fly compression enables rcomm to compress response bodies in real time using gzip or deflate encoding when the client indicates support via the `Accept-Encoding` request header. Unlike pre-compressed file serving (which serves pre-built `.gz` files from disk), this feature compresses content dynamically at request time.

**Complexity**: 7 (high — implementing DEFLATE/gzip from scratch without external dependencies is a significant undertaking)
**Necessity**: 4

**Key Challenge**: rcomm has a strict no-external-dependencies policy. The `flate2` crate (or similar) cannot be used. This means the DEFLATE compression algorithm (RFC 1951) and the gzip wrapper format (RFC 1952) must be implemented from scratch using only `std`. This is the dominant source of complexity for this feature.

**Key Changes**:
- Implement DEFLATE compression algorithm (RFC 1951) from scratch
- Implement gzip framing (RFC 1952) around DEFLATE output
- Parse `Accept-Encoding` request header
- Compress eligible response bodies and set `Content-Encoding` header
- Add `Vary: Accept-Encoding` to responses
- Only compress text-based content types (HTML, CSS, JS, JSON, XML, SVG)

---

## Files to Modify or Create

### 1. `/home/jwall/personal/rusty/rcomm/src/compression/mod.rs` (new)

**Purpose**: New module containing the DEFLATE and gzip implementations.

**Submodules**:
- `deflate.rs` — DEFLATE compression (RFC 1951)
- `gzip.rs` — Gzip framing (RFC 1952) wrapping DEFLATE output

### 2. `/home/jwall/personal/rusty/rcomm/src/lib.rs`

**Current State** (exports `ThreadPool` and `models` module):

**Changes Required**:
- Add `pub mod compression;` to export the new compression module

### 3. `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Current State**:
- `handle_connection()` (line 46) reads file content at line 70 and sends it uncompressed
- No `Accept-Encoding` parsing or `Content-Encoding` response header

**Changes Required**:
- Parse `Accept-Encoding` from request headers
- After reading file content, compress if client supports gzip and content is compressible
- Set `Content-Encoding: gzip` and `Vary: Accept-Encoding` headers
- Skip compression for small responses (< ~150 bytes) and binary content types

### 4. `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`

**Changes Required**:
- Add tests that send `Accept-Encoding: gzip` and verify compressed response
- Add tests for decompressing and verifying content integrity

---

## Step-by-Step Implementation

### Step 1: Implement DEFLATE Compression (Simplified)

**Location**: `src/compression/deflate.rs`

Implementing a full optimal DEFLATE compressor is extremely complex (LZ77 + Huffman coding). A practical approach for a no-dependency server is to use DEFLATE's "stored blocks" (non-compressed) mode, which is valid DEFLATE but provides no actual compression. Alternatively, implement a simplified LZ77 + fixed Huffman encoding.

**Option A: Stored Blocks Only (Simplest, valid but no size reduction)**

```rust
/// Encode data using DEFLATE stored blocks (type 00).
/// This produces valid DEFLATE output but does not actually compress.
/// Each block: 1-byte header + 2-byte len + 2-byte nlen + data
pub fn deflate_stored(data: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();
    let max_block = 65535; // Max stored block size

    let chunks: Vec<&[u8]> = data.chunks(max_block).collect();
    let total = chunks.len();

    for (i, chunk) in chunks.iter().enumerate() {
        let is_final = i == total - 1;
        // BFINAL (1 if last block) | BTYPE=00 (stored)
        output.push(if is_final { 0x01 } else { 0x00 });

        let len = chunk.len() as u16;
        let nlen = !len;
        output.extend_from_slice(&len.to_le_bytes());
        output.extend_from_slice(&nlen.to_le_bytes());
        output.extend_from_slice(chunk);
    }

    output
}
```

**Option B: Fixed Huffman Encoding (Better compression, more complex)**

```rust
/// Encode data using DEFLATE with fixed Huffman codes (type 01).
/// Implements literal-only encoding (no LZ77 back-references).
/// Provides modest compression for text content.
pub fn deflate_fixed_huffman(data: &[u8]) -> Vec<u8> {
    let mut bits = BitWriter::new();

    // BFINAL=1, BTYPE=01 (fixed Huffman)
    bits.write_bits(1, 1); // BFINAL
    bits.write_bits(0b01, 2); // BTYPE = fixed Huffman

    for &byte in data {
        encode_literal_fixed(&mut bits, byte as u16);
    }
    // End of block symbol (256)
    encode_literal_fixed(&mut bits, 256);

    bits.finish()
}

fn encode_literal_fixed(bits: &mut BitWriter, value: u16) {
    match value {
        0..=143 => bits.write_bits_reversed(0x30 + value as u32, 8),
        144..=255 => bits.write_bits_reversed(0x190 + (value - 144) as u32, 9),
        256..=279 => bits.write_bits_reversed((value - 256) as u32, 7),
        280..=287 => bits.write_bits_reversed(0xC0 + (value - 280) as u32, 8),
        _ => {}
    }
}

struct BitWriter {
    bytes: Vec<u8>,
    current_byte: u8,
    bit_pos: u8,
}

impl BitWriter {
    fn new() -> Self {
        BitWriter { bytes: Vec::new(), current_byte: 0, bit_pos: 0 }
    }

    fn write_bits(&mut self, value: u32, count: u8) {
        for i in 0..count {
            if (value >> i) & 1 == 1 {
                self.current_byte |= 1 << self.bit_pos;
            }
            self.bit_pos += 1;
            if self.bit_pos == 8 {
                self.bytes.push(self.current_byte);
                self.current_byte = 0;
                self.bit_pos = 0;
            }
        }
    }

    fn write_bits_reversed(&mut self, value: u32, count: u8) {
        for i in (0..count).rev() {
            if (value >> i) & 1 == 1 {
                self.current_byte |= 1 << self.bit_pos;
            }
            self.bit_pos += 1;
            if self.bit_pos == 8 {
                self.bytes.push(self.current_byte);
                self.current_byte = 0;
                self.bit_pos = 0;
            }
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.bit_pos > 0 {
            self.bytes.push(self.current_byte);
        }
        self.bytes
    }
}
```

**Recommendation**: Start with Option A (stored blocks) to get the feature working end-to-end with valid gzip output, then upgrade to Option B for actual compression.

---

### Step 2: Implement Gzip Wrapper

**Location**: `src/compression/gzip.rs`

```rust
use super::deflate;

/// CRC-32 lookup table (precomputed for IEEE polynomial 0xEDB88320)
const CRC32_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u32;
        let mut j = 0;
        while j < 8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
            j += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
};

fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFFFFFF;
    for &byte in data {
        let index = ((crc ^ byte as u32) & 0xFF) as usize;
        crc = (crc >> 8) ^ CRC32_TABLE[index];
    }
    crc ^ 0xFFFFFFFF
}

/// Compress data using gzip format (RFC 1952).
/// Returns a valid gzip byte stream.
pub fn gzip_compress(data: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();

    // Gzip header (10 bytes, minimal)
    output.push(0x1F); // ID1
    output.push(0x8B); // ID2
    output.push(0x08); // CM = deflate
    output.push(0x00); // FLG = no flags
    output.extend_from_slice(&[0, 0, 0, 0]); // MTIME = 0
    output.push(0x00); // XFL
    output.push(0xFF); // OS = unknown

    // Compressed data (DEFLATE)
    let compressed = deflate::deflate_stored(data);
    output.extend_from_slice(&compressed);

    // Gzip trailer
    let checksum = crc32(data);
    output.extend_from_slice(&checksum.to_le_bytes()); // CRC32
    let size = data.len() as u32;
    output.extend_from_slice(&size.to_le_bytes()); // ISIZE (original size mod 2^32)

    output
}
```

---

### Step 3: Create Module Structure

**Location**: `src/compression/mod.rs`

```rust
pub mod deflate;
pub mod gzip;
```

**Update `src/lib.rs`**:
```rust
pub mod compression;
```

---

### Step 4: Modify `handle_connection()` for Compression

**Location**: `src/main.rs`

Add a helper to determine if a content type is compressible:

```rust
fn is_compressible(filename: &str) -> bool {
    let compressible_extensions = ["html", "css", "js", "json", "xml", "svg", "txt"];
    Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .map(|ext| compressible_extensions.contains(&ext))
        .unwrap_or(false)
}
```

Modify `handle_connection()` after reading the file content (after line 71):

```rust
    let contents = fs::read_to_string(filename).unwrap();
    let body_bytes: Vec<u8> = contents.into();

    // Check if client supports gzip and content is compressible
    let accept_encoding = http_request
        .headers
        .get("accept-encoding")
        .map(|v| v.to_lowercase())
        .unwrap_or_default();

    let final_body = if accept_encoding.contains("gzip")
        && is_compressible(filename)
        && body_bytes.len() > 150
    {
        response.add_header("Content-Encoding".to_string(), "gzip".to_string());
        rcomm::compression::gzip::gzip_compress(&body_bytes)
    } else {
        body_bytes
    };

    response.add_header("Vary".to_string(), "Accept-Encoding".to_string());
    response.add_body(final_body);
```

---

### Step 5: Add Integration Tests

**Location**: `src/bin/integration_test.rs`

```rust
fn test_gzip_response_when_accepted(addr: &str) -> Result<(), String> {
    let mut stream = TcpStream::connect(addr)
        .map_err(|e| format!("connect: {e}"))?;
    let request = format!(
        "GET / HTTP/1.1\r\nHost: {}\r\nAccept-Encoding: gzip\r\nConnection: close\r\n\r\n",
        addr
    );
    stream.write_all(request.as_bytes())
        .map_err(|e| format!("write: {e}"))?;
    let resp = read_response(&mut stream)?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;

    // Check Content-Encoding header
    let encoding = resp.headers.get("content-encoding")
        .ok_or("missing Content-Encoding header")?;
    assert_eq_or_err(encoding, &"gzip".to_string(), "content-encoding")?;

    // Verify gzip magic bytes in body
    if resp.body.len() >= 2 {
        let bytes = resp.body.as_bytes();
        if bytes[0] != 0x1F || bytes[1] != 0x8B {
            return Err("Response body does not start with gzip magic bytes".to_string());
        }
    }
    Ok(())
}

fn test_no_compression_without_accept_encoding(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    if resp.headers.contains_key("content-encoding") {
        return Err("Should not have Content-Encoding without Accept-Encoding".to_string());
    }
    Ok(())
}
```

---

## Discussion: Implementing Compression Without External Crates

### The DEFLATE Challenge

DEFLATE (RFC 1951) is the compression algorithm underlying both gzip and zlib. A full implementation involves:

1. **LZ77 sliding window compression**: Finding repeated byte sequences and replacing them with back-references (distance, length pairs)
2. **Huffman coding**: Encoding the LZ77 output with variable-length bit codes
3. **Dynamic Huffman trees**: Optionally computing optimal Huffman trees per block

A production-quality DEFLATE compressor (like zlib) is ~15,000 lines of C. However, for rcomm's use case:

### Practical Approaches

| Approach | Compression | Complexity | Lines of Code |
|----------|-------------|------------|---------------|
| Stored blocks only | 0% (actually slightly larger) | Very low | ~20 |
| Fixed Huffman, literals only | ~30-40% on text | Medium | ~150 |
| Fixed Huffman + LZ77 | ~50-60% on text | High | ~400-600 |
| Dynamic Huffman + LZ77 | ~60-70% on text | Very high | ~1000+ |

**Recommended approach**: Start with stored blocks (valid gzip, no compression benefit) to establish the full pipeline, then iterate to fixed Huffman with literal encoding for meaningful text compression. LZ77 is a significant additional effort but provides the best results.

### Why Stored Blocks Are Useful

Even with no compression, the stored-blocks approach:
- Produces valid gzip output that all clients can decompress
- Allows the full `Content-Encoding: gzip` pipeline to be tested end-to-end
- Can be upgraded later without changing any code outside the `deflate` module
- Still satisfies the HTTP protocol contract

---

## Edge Cases & Handling

### 1. Small Response Bodies
- **Behavior**: Skip compression for bodies < 150 bytes (gzip overhead ~18 bytes makes small files larger)
- **Status**: Handled by size check in `handle_connection()`

### 2. Binary Files (Images, Fonts, PDFs)
- **Behavior**: Skip compression — already compressed, CPU waste
- **Status**: Handled by `is_compressible()` extension check

### 3. Client Doesn't Support Gzip
- **Behavior**: Serve uncompressed content as normal
- **Status**: Handled by `accept_encoding.contains("gzip")` check

### 4. Content-Length Header
- **Behavior**: `add_body()` auto-sets `Content-Length` to the compressed body size
- **Status**: Correct — Content-Length reflects the transfer size

### 5. Chunked Transfer Encoding Interaction
- **Behavior**: Not supported yet; compression works with Content-Length framing only
- **Status**: Future enhancement when chunked encoding is implemented

### 6. gzip CRC32 Integrity
- **Behavior**: CRC32 computed over original (uncompressed) data, stored in gzip trailer
- **Status**: Clients verify this; implementation must be correct

### 7. Data Larger Than 4GB
- **Behavior**: ISIZE field in gzip trailer is mod 2^32; technically valid but unusual for a web server
- **Status**: Not a practical concern for static file serving

---

## Implementation Checklist

- [ ] Create `src/compression/mod.rs` module file
- [ ] Implement `src/compression/deflate.rs` with stored blocks
- [ ] Implement `src/compression/gzip.rs` with gzip framing + CRC32
- [ ] Update `src/lib.rs` to export `compression` module
- [ ] Add `is_compressible()` helper to `src/main.rs`
- [ ] Modify `handle_connection()` to compress eligible responses
- [ ] Add `Content-Encoding: gzip` header for compressed responses
- [ ] Add `Vary: Accept-Encoding` header to all responses
- [ ] Add unit tests for CRC32 implementation
- [ ] Add unit tests for DEFLATE stored blocks output
- [ ] Add unit tests for gzip wrapper (verify magic bytes, trailer)
- [ ] Add integration test: gzip response when `Accept-Encoding: gzip` is sent
- [ ] Add integration test: no compression without `Accept-Encoding`
- [ ] Run `cargo test` to verify all unit tests pass
- [ ] Run `cargo run --bin integration_test` to verify all integration tests pass
- [ ] (Optional) Upgrade DEFLATE to fixed Huffman for real compression

---

## Backward Compatibility

### Existing Tests
All existing tests pass without modification. Compression only activates when the `Accept-Encoding` header is present; existing tests don't send this header.

### New Headers
- `Vary: Accept-Encoding` is added to all responses (harmless, standard HTTP practice)
- `Content-Encoding: gzip` only added when compression is applied

### Performance Impact
- **CPU cost**: Compression adds CPU overhead per request (mitigated by only compressing text content)
- **Bandwidth savings**: Significant for text-based files (HTML, CSS, JS)
- **Memory**: Compressed output buffer allocated per request; freed after response sent

---

## References

- RFC 1951 — DEFLATE Compressed Data Format: https://tools.ietf.org/html/rfc1951
- RFC 1952 — GZIP File Format: https://tools.ietf.org/html/rfc1952
- RFC 7231 — Content-Encoding: https://tools.ietf.org/html/rfc7231#section-3.1.2.2
- CRC-32 algorithm: https://en.wikipedia.org/wiki/Cyclic_redundancy_check
- HTTP Compression overview: https://developer.mozilla.org/en-US/docs/Web/HTTP/Compression
