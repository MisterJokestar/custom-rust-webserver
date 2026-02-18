# Integration Tests for Concurrent Connection Limits

**Category:** Testing
**Complexity:** 3/10
**Necessity:** 5/10
**Status:** Planning

---

## Overview

Add integration tests to `src/bin/integration_test.rs` that verify the server's behavior under concurrent connection pressure. The existing `test_concurrent_requests` test sends 10 concurrent requests, but it only validates that all return 200. This feature adds more thorough concurrency tests including:

1. Higher concurrency levels (50+ simultaneous connections)
2. Mixed valid/invalid requests concurrently
3. Verifying no response corruption under load
4. Testing behavior when connections exceed the thread pool size (4 threads)

**Goal:** Validate server stability and correctness under concurrent load, especially when connections exceed the thread pool capacity.

---

## Current State

### Thread Pool (src/lib.rs)

The server uses a fixed thread pool of 4 workers. Incoming connections are queued via an `mpsc` channel. When all workers are busy, new connections wait in the channel queue.

```rust
let pool = ThreadPool::new(4);
```

### Existing Concurrency Test (integration_test.rs, lines 296-322)

```rust
fn test_concurrent_requests(addr: &str) -> Result<(), String> {
    let addr = addr.to_string();
    let results: Arc<Mutex<Vec<Result<u16, String>>>> = Arc::new(Mutex::new(Vec::new()));
    let mut handles = Vec::new();

    for _ in 0..10 {
        let addr = addr.clone();
        let results = Arc::clone(&results);
        handles.push(thread::spawn(move || {
            let result = send_request(&addr, "GET", "/").map(|r| r.status_code);
            results.lock().unwrap().push(result);
        }));
    }
    // ... join and check results
}
```

This test validates basic concurrency (10 threads, all GET /, all expect 200).

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`

**Add:** Advanced concurrency test functions.
**Modify:** `main()` function to register new tests.

---

## Step-by-Step Implementation

### Step 1: Add High-Concurrency Test

```rust
fn test_high_concurrency(addr: &str) -> Result<(), String> {
    // 50 concurrent requests — well above the 4-thread pool size
    let addr = addr.to_string();
    let results: Arc<Mutex<Vec<Result<u16, String>>>> = Arc::new(Mutex::new(Vec::new()));
    let mut handles = Vec::new();
    let num_requests = 50;

    for _ in 0..num_requests {
        let addr = addr.clone();
        let results = Arc::clone(&results);
        handles.push(thread::spawn(move || {
            let result = send_request(&addr, "GET", "/").map(|r| r.status_code);
            results.lock().unwrap().push(result);
        }));
    }

    for h in handles {
        h.join().map_err(|_| "thread panicked".to_string())?;
    }

    let results = results.lock().unwrap();
    let mut successes = 0;
    let mut failures = 0;
    for r in results.iter() {
        match r {
            Ok(200) => successes += 1,
            Ok(code) => {
                return Err(format!("unexpected status code: {code}"));
            }
            Err(e) => failures += 1,
        }
    }

    // Allow some connection failures under heavy load, but most should succeed
    if successes < num_requests / 2 {
        return Err(format!(
            "too many failures: {successes} succeeded, {failures} failed out of {num_requests}"
        ));
    }
    Ok(())
}
```

### Step 2: Add Response Integrity Under Concurrency Test

```rust
fn test_concurrent_response_integrity(addr: &str) -> Result<(), String> {
    // Send concurrent requests to different routes and verify correct responses
    let addr = addr.to_string();
    let routes = vec![
        ("/", "Hello!"),
        ("/howdy", "Howdy!"),
        ("/index.css", "background"),
    ];

    let results: Arc<Mutex<Vec<Result<(), String>>>> = Arc::new(Mutex::new(Vec::new()));
    let mut handles = Vec::new();

    // Send 5 requests per route concurrently
    for _ in 0..5 {
        for (path, expected) in &routes {
            let addr = addr.clone();
            let path = path.to_string();
            let expected = expected.to_string();
            let results = Arc::clone(&results);
            handles.push(thread::spawn(move || {
                let result = (|| {
                    let resp = send_request(&addr, "GET", &path)?;
                    assert_eq_or_err(&resp.status_code, &200, &format!("{path} status"))?;
                    assert_contains_or_err(&resp.body, &expected, &format!("{path} body"))?;
                    Ok(())
                })();
                results.lock().unwrap().push(result);
            }));
        }
    }

    for h in handles {
        h.join().map_err(|_| "thread panicked".to_string())?;
    }

    let results = results.lock().unwrap();
    for r in results.iter() {
        if let Err(e) = r {
            return Err(format!("response integrity failure: {e}"));
        }
    }
    Ok(())
}
```

### Step 3: Add Concurrent Mixed Request Test

```rust
fn test_concurrent_mixed_valid_invalid(addr: &str) -> Result<(), String> {
    // Mix of valid and invalid requests concurrently
    let addr = addr.to_string();
    let results: Arc<Mutex<Vec<Result<(String, u16), String>>>> = Arc::new(Mutex::new(Vec::new()));
    let mut handles = Vec::new();

    let requests = vec![
        ("GET", "/", 200u16),
        ("GET", "/does-not-exist", 404),
        ("GET", "/howdy", 200),
        ("GET", "/zzz-nope", 404),
        ("GET", "/index.css", 200),
    ];

    for _ in 0..5 {
        for (method, path, expected_status) in &requests {
            let addr = addr.clone();
            let method = method.to_string();
            let path = path.to_string();
            let expected = *expected_status;
            let results = Arc::clone(&results);
            handles.push(thread::spawn(move || {
                let result = send_request(&addr, &method, &path)
                    .map(|r| (path.clone(), r.status_code));
                results.lock().unwrap().push(result);
            }));
        }
    }

    for h in handles {
        h.join().map_err(|_| "thread panicked".to_string())?;
    }

    let results = results.lock().unwrap();
    for r in results.iter() {
        match r {
            Ok((path, code)) => {
                // Just verify we got valid HTTP responses (not connection errors)
                if *code != 200 && *code != 404 {
                    return Err(format!("{path}: unexpected status {code}"));
                }
            }
            Err(e) => return Err(format!("connection error: {e}")),
        }
    }
    Ok(())
}
```

### Step 4: Add Sequential-After-Concurrent Test

```rust
fn test_server_healthy_after_load(addr: &str) -> Result<(), String> {
    // Hammer the server with concurrent requests, then verify it's still responsive
    let addr_str = addr.to_string();
    let mut handles = Vec::new();

    for _ in 0..30 {
        let addr = addr_str.clone();
        handles.push(thread::spawn(move || {
            let _ = send_request(&addr, "GET", "/");
        }));
    }

    for h in handles {
        let _ = h.join();
    }

    // Brief pause to let the server settle
    thread::sleep(Duration::from_millis(100));

    // Server should still be responsive
    let resp = send_request(addr, "GET", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "status after load")?;
    assert_contains_or_err(&resp.body, "Hello!", "body after load")?;
    Ok(())
}
```

### Step 5: Register Tests in `main()`

```rust
run_test("high_concurrency", || test_high_concurrency(&addr)),
run_test("concurrent_response_integrity", || test_concurrent_response_integrity(&addr)),
run_test("concurrent_mixed_valid_invalid", || test_concurrent_mixed_valid_invalid(&addr)),
run_test("server_healthy_after_load", || test_server_healthy_after_load(&addr)),
```

---

## Edge Cases & Considerations

### 1. OS Connection Limits

**Scenario:** The OS may reject connections if the TCP backlog is full or the file descriptor limit is reached.

**Mitigation:** 50 concurrent connections is well within default OS limits (typically 128+ backlog, 1024+ fd limit). Tests allow some failures.

### 2. Race Conditions in Test Framework

**Scenario:** The `Arc<Mutex<Vec>>` pattern for collecting results could have contention.

**Mitigation:** Acceptable for test code; the mutex is held briefly per result push.

### 3. Thread Pool Exhaustion

**Scenario:** With 4 workers and 50 requests, 46 requests queue. If workers are slow, queued connections may time out.

**Expected behavior:** `mpsc` channel has unlimited capacity, so requests queue indefinitely. The 5-second read timeout on the test client side is the limit.

### 4. Flaky Tests

**Scenario:** Under CI or constrained environments, concurrent tests may be more flaky.

**Mitigation:** The `test_high_concurrency` test allows up to 50% failure rate. Adjust thresholds if needed.

---

## Testing Strategy

### Running the Tests

```bash
cargo build && cargo run --bin integration_test
```

### Expected Results

All concurrency tests pass. The high-concurrency test may show some connection failures but the majority succeed. The server remains healthy after all tests.

---

## Implementation Checklist

- [ ] Add `test_high_concurrency()` test
- [ ] Add `test_concurrent_response_integrity()` test
- [ ] Add `test_concurrent_mixed_valid_invalid()` test
- [ ] Add `test_server_healthy_after_load()` test
- [ ] Register all tests in `main()`
- [ ] Run `cargo build` — no compiler errors
- [ ] Run `cargo run --bin integration_test` — all tests pass

---

## Related Features

- **Security > Configurable Maximum Concurrent Connection Limit**: Once implemented, add tests that verify the limit is enforced
- **Thread Pool > Task Queue Depth Monitoring**: Tests for 503 Service Unavailable when queue is full
- **Thread Pool > Worker Thread Panic Recovery**: Tests that verify workers recover from panics

---

## References

- [Rust std::thread](https://doc.rust-lang.org/std/thread/)
- [Rust std::sync::Arc](https://doc.rust-lang.org/std/sync/struct.Arc.html)
