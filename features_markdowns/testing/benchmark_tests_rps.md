# Benchmark Tests for Requests-Per-Second Throughput

**Category:** Testing
**Complexity:** 5/10
**Necessity:** 3/10
**Status:** Planning

---

## Overview

Add benchmark tests that measure the server's requests-per-second (RPS) throughput under various conditions. These benchmarks establish a performance baseline and detect regressions as features are added. Unlike functional tests, benchmarks measure speed, not correctness.

**Goal:** Measure server performance across single-threaded, multi-threaded, and mixed-workload scenarios. Provide reproducible benchmark numbers for comparison across code changes.

**Note:** Higher complexity (5/10) due to the need for statistical rigor (multiple runs, warm-up, variance measurement), avoiding test flakiness on different hardware, and designing meaningful workloads.

---

## Current State

### No Existing Benchmarks

The project has no performance tests. The existing integration test `test_concurrent_requests` sends 10 requests but only checks correctness, not speed.

### Server Architecture

- 4 worker threads in the thread pool
- Single `mpsc` channel for work distribution (contention point)
- Full file read per request (no caching)
- Full response serialization per request

### Performance Characteristics (Expected)

For a static file server with no caching:
- Small file serving: Bottleneck is thread pool dispatch and TCP overhead
- Large file serving: Bottleneck is file I/O and memory allocation
- Concurrency: Limited by 4 worker threads and mutex contention on the channel

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`

**Option A:** Add benchmark functions to the existing integration test binary.

**Option B (Recommended):** Create a separate benchmark binary `src/bin/benchmark.rs` to keep benchmarks isolated from functional tests.

---

## Step-by-Step Implementation

### Step 1: Create Benchmark Binary

Create `src/bin/benchmark.rs`:

```rust
use std::{
    env,
    io::{Read, Write},
    net::TcpStream,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{Arc, atomic::{AtomicUsize, Ordering}},
    thread,
    time::{Duration, Instant},
};

// Reuse server lifecycle helpers (or copy them)
fn pick_free_port() -> u16 { /* same as integration_test.rs */ }
fn find_server_binary() -> PathBuf { /* same */ }
fn find_project_root() -> PathBuf { /* same */ }
fn start_server(port: u16) -> Child { /* same */ }
fn wait_for_server(addr: &str, timeout: Duration) -> Result<(), String> { /* same */ }
```

### Step 2: Add Single-Thread Sequential Benchmark

```rust
fn bench_sequential_requests(addr: &str, num_requests: usize) -> Duration {
    let start = Instant::now();

    for _ in 0..num_requests {
        let mut stream = TcpStream::connect(addr).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let request = format!("GET / HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
        stream.write_all(request.as_bytes()).unwrap();

        // Read full response
        let mut buf = Vec::new();
        let _ = stream.read_to_end(&mut buf);
    }

    start.elapsed()
}
```

### Step 3: Add Multi-Thread Concurrent Benchmark

```rust
fn bench_concurrent_requests(
    addr: &str,
    num_threads: usize,
    requests_per_thread: usize,
) -> Duration {
    let addr = addr.to_string();
    let total_completed = Arc::new(AtomicUsize::new(0));

    let start = Instant::now();
    let mut handles = Vec::new();

    for _ in 0..num_threads {
        let addr = addr.clone();
        let completed = Arc::clone(&total_completed);
        handles.push(thread::spawn(move || {
            for _ in 0..requests_per_thread {
                if let Ok(mut stream) = TcpStream::connect(&addr) {
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                    let request = format!(
                        "GET / HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
                    );
                    if stream.write_all(request.as_bytes()).is_ok() {
                        let mut buf = Vec::new();
                        let _ = stream.read_to_end(&mut buf);
                        if !buf.is_empty() {
                            completed.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            }
        }));
    }

    for h in handles {
        let _ = h.join();
    }

    let elapsed = start.elapsed();
    let total = total_completed.load(Ordering::Relaxed);
    println!(
        "  Completed: {total}/{} requests",
        num_threads * requests_per_thread
    );

    elapsed
}
```

### Step 4: Add Benchmark Runner

```rust
struct BenchResult {
    name: String,
    total_requests: usize,
    duration: Duration,
    rps: f64,
}

fn run_benchmark<F>(name: &str, total_requests: usize, f: F) -> BenchResult
where
    F: FnOnce() -> Duration,
{
    // Warm-up run (not measured)
    println!("  [warm-up] {name}...");

    let duration = f();
    let rps = total_requests as f64 / duration.as_secs_f64();

    BenchResult {
        name: name.to_string(),
        total_requests,
        duration,
        rps,
    }
}

fn main() {
    let port = pick_free_port();
    let addr = format!("127.0.0.1:{port}");

    println!("Starting server on {addr}...");
    let mut server = start_server(port);

    if let Err(e) = wait_for_server(&addr, Duration::from_secs(5)) {
        eprintln!("ERROR: {e}");
        let _ = server.kill();
        std::process::exit(1);
    }
    println!("Server ready.\n");

    // Warm up
    for _ in 0..10 {
        let _ = TcpStream::connect(&addr).and_then(|mut s| {
            s.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;
            let mut buf = Vec::new();
            s.read_to_end(&mut buf)?;
            Ok(())
        });
    }

    let results = vec![
        run_benchmark("sequential (100 requests)", 100, || {
            bench_sequential_requests(&addr, 100)
        }),
        run_benchmark("concurrent 4 threads x 25 requests", 100, || {
            bench_concurrent_requests(&addr, 4, 25)
        }),
        run_benchmark("concurrent 8 threads x 25 requests", 200, || {
            bench_concurrent_requests(&addr, 8, 25)
        }),
        run_benchmark("concurrent 16 threads x 25 requests", 400, || {
            bench_concurrent_requests(&addr, 16, 25)
        }),
    ];

    // Print results table
    println!("\n{:-<70}", "");
    println!(
        "{:<45} {:>8} {:>8} {:>8}",
        "Benchmark", "Reqs", "Time", "RPS"
    );
    println!("{:-<70}", "");
    for r in &results {
        println!(
            "{:<45} {:>8} {:>7.2}s {:>8.0}",
            r.name,
            r.total_requests,
            r.duration.as_secs_f64(),
            r.rps
        );
    }
    println!("{:-<70}", "");

    let _ = server.kill();
    let _ = server.wait();
}
```

### Step 5: Run the Benchmark

```bash
cargo run --bin benchmark
```

---

## Edge Cases & Considerations

### 1. Hardware Variance

**Scenario:** Benchmark numbers vary significantly across machines.

**Mitigation:** Benchmarks are relative, not absolute. Compare runs on the same machine. Print system info if available.

### 2. OS Scheduling

**Scenario:** Background processes or OS scheduling can affect results.

**Mitigation:** Run benchmarks multiple times and take the best/median result. Add a `--runs N` flag for repeated measurement.

### 3. TCP Port Exhaustion

**Scenario:** Many rapid connections can exhaust ephemeral ports on some systems.

**Mitigation:** Use `Connection: close` and keep request counts reasonable (100-400 per benchmark). Add delays between benchmarks if needed.

### 4. Server Warm-Up

**Scenario:** The first few requests may be slower due to cold caches, JIT (not applicable to Rust), and OS buffer allocation.

**Mitigation:** Run a warm-up phase before measuring.

### 5. Meaningful Comparisons

**Scenario:** Benchmark numbers without context are meaningless.

**Recommendation:** Save results to a file with a timestamp and git commit hash for historical comparison:

```rust
println!("Git commit: {}", env!("GIT_HASH", "unknown"));
```

---

## Testing Strategy

### Running the Benchmark

```bash
cargo build --release && cargo run --release --bin benchmark
```

**Important:** Always benchmark with `--release` for meaningful numbers.

### Sample Expected Output

```
----------------------------------------------------------------------
Benchmark                                         Reqs     Time      RPS
----------------------------------------------------------------------
sequential (100 requests)                          100    0.15s      667
concurrent 4 threads x 25 requests                 100    0.08s     1250
concurrent 8 threads x 25 requests                 200    0.12s     1667
concurrent 16 threads x 25 requests                400    0.35s     1143
----------------------------------------------------------------------
```

---

## Implementation Checklist

- [ ] Create `src/bin/benchmark.rs`
- [ ] Add server lifecycle helpers (or extract shared module)
- [ ] Implement `bench_sequential_requests()` function
- [ ] Implement `bench_concurrent_requests()` function
- [ ] Implement `run_benchmark()` wrapper with timing
- [ ] Add warm-up phase
- [ ] Add results table printing
- [ ] Run `cargo build` — no compiler errors
- [ ] Run `cargo run --release --bin benchmark` — benchmarks complete
- [ ] Document baseline numbers in a comment or separate file

---

## Future Enhancements

1. **JSON Output**: Write results to a JSON file for automated tracking
2. **Comparison Mode**: Compare current results against a saved baseline
3. **Latency Percentiles**: Measure p50, p95, p99 latency in addition to throughput
4. **Different File Sizes**: Benchmark with varying file sizes (small HTML vs large assets)
5. **Keep-Alive Benchmark**: Measure throughput with persistent connections (once implemented)

---

## Dependencies

- **No new external dependencies**
- Reuses server lifecycle helpers from `integration_test.rs`

---

## References

- [Rust Benchmark Testing](https://doc.rust-lang.org/cargo/commands/cargo-bench.html)
- [wrk - HTTP Benchmarking Tool](https://github.com/wg/wrk) (external tool for comparison)
- [Apache Bench (ab)](https://httpd.apache.org/docs/2.4/programs/ab.html) (external tool for comparison)
