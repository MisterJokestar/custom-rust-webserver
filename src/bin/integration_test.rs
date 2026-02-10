use std::{
    collections::HashMap,
    env,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

// ---------------------------------------------------------------------------
// Server lifecycle
// ---------------------------------------------------------------------------

fn pick_free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("failed to bind to port 0");
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}

fn find_server_binary() -> PathBuf {
    let current_exe = env::current_exe().expect("failed to get current exe path");
    // The integration_test binary lives next to the rcomm binary in the same target dir
    let dir = current_exe.parent().expect("no parent directory");
    let candidate = dir.join("rcomm");
    if candidate.exists() {
        return candidate;
    }
    panic!(
        "Could not find rcomm binary. Looked in: {}",
        dir.display()
    );
}

fn find_project_root() -> PathBuf {
    let mut dir = env::current_exe()
        .expect("failed to get current exe path")
        .parent()
        .unwrap()
        .to_path_buf();
    // Walk up from target/debug (or target/release) until we find Cargo.toml
    loop {
        if dir.join("Cargo.toml").exists() && dir.join("pages").exists() {
            return dir;
        }
        if !dir.pop() {
            panic!("Could not locate project root (Cargo.toml + pages/)");
        }
    }
}

fn start_server(port: u16) -> Child {
    let binary = find_server_binary();
    let project_root = find_project_root();
    Command::new(binary)
        .env("RCOMM_PORT", port.to_string())
        .env("RCOMM_ADDRESS", "127.0.0.1")
        .current_dir(project_root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to start rcomm server")
}

fn wait_for_server(addr: &str, timeout: Duration) -> Result<(), String> {
    let start = Instant::now();
    loop {
        if TcpStream::connect(addr).is_ok() {
            return Ok(());
        }
        if start.elapsed() > timeout {
            return Err(format!(
                "Server did not become ready within {}s",
                timeout.as_secs()
            ));
        }
        thread::sleep(Duration::from_millis(50));
    }
}

// ---------------------------------------------------------------------------
// HTTP client helpers
// ---------------------------------------------------------------------------

#[allow(dead_code)]
struct TestResponse {
    status_code: u16,
    status_phrase: String,
    headers: HashMap<String, String>,
    body: String,
}

fn read_response(stream: &mut TcpStream) -> Result<TestResponse, String> {
    let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);

    // Status line
    let mut status_line = String::new();
    reader
        .read_line(&mut status_line)
        .map_err(|e| format!("reading status line: {e}"))?;
    let parts: Vec<&str> = status_line.trim().splitn(3, ' ').collect();
    if parts.len() < 2 {
        return Err(format!("malformed status line: {status_line}"));
    }
    let status_code: u16 = parts[1]
        .parse()
        .map_err(|_| format!("bad status code: {}", parts[1]))?;
    let status_phrase = if parts.len() == 3 {
        parts[2].to_string()
    } else {
        String::new()
    };

    // Headers
    let mut headers = HashMap::new();
    loop {
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|e| format!("reading header: {e}"))?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some((key, val)) = trimmed.split_once(':') {
            headers.insert(
                key.trim().to_lowercase(),
                val.trim().to_string(),
            );
        }
    }

    // Body via Content-Length
    let body = if let Some(cl) = headers.get("content-length") {
        let len: usize = cl
            .parse()
            .map_err(|_| format!("bad content-length: {cl}"))?;
        let mut buf = vec![0u8; len];
        reader
            .read_exact(&mut buf)
            .map_err(|e| format!("reading body: {e}"))?;
        String::from_utf8_lossy(&buf).to_string()
    } else {
        String::new()
    };

    Ok(TestResponse {
        status_code,
        status_phrase,
        headers,
        body,
    })
}

fn send_request(addr: &str, method: &str, path: &str) -> Result<TestResponse, String> {
    let mut stream = TcpStream::connect(addr).map_err(|e| format!("connect: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| format!("set timeout: {e}"))?;
    let request = format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("write: {e}"))?;
    read_response(&mut stream)
}

// ---------------------------------------------------------------------------
// Mini test framework
// ---------------------------------------------------------------------------

struct TestResult {
    name: String,
    passed: bool,
    detail: String,
}

fn run_test<F>(name: &str, f: F) -> TestResult
where
    F: FnOnce() -> Result<(), String>,
{
    match f() {
        Ok(()) => TestResult {
            name: name.to_string(),
            passed: true,
            detail: "OK".to_string(),
        },
        Err(detail) => TestResult {
            name: name.to_string(),
            passed: false,
            detail,
        },
    }
}

fn assert_eq_or_err<T: PartialEq + std::fmt::Debug>(
    actual: &T,
    expected: &T,
    label: &str,
) -> Result<(), String> {
    if actual == expected {
        Ok(())
    } else {
        Err(format!("{label}: expected {expected:?}, got {actual:?}"))
    }
}

fn assert_contains_or_err(haystack: &str, needle: &str, label: &str) -> Result<(), String> {
    if haystack.contains(needle) {
        Ok(())
    } else {
        Err(format!(
            "{label}: expected body to contain {needle:?}, body was {} bytes",
            haystack.len()
        ))
    }
}

// ---------------------------------------------------------------------------
// Test cases
// ---------------------------------------------------------------------------

fn test_root_route(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    assert_contains_or_err(&resp.body, "Hello!", "body")?;
    assert_contains_or_err(&resp.body, "Hi from Rust!", "body")?;
    Ok(())
}

fn test_index_css(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/index.css")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    assert_contains_or_err(&resp.body, "background", "body")?;
    Ok(())
}

fn test_howdy_route(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/howdy")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    assert_contains_or_err(&resp.body, "Howdy!", "body")?;
    assert_contains_or_err(&resp.body, "like a cowboy", "body")?;
    Ok(())
}

fn test_howdy_page_css(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/howdy/page.css")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    assert_contains_or_err(&resp.body, "background", "body")?;
    Ok(())
}

fn test_404_does_not_exist(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/does-not-exist")?;
    assert_eq_or_err(&resp.status_code, &404, "status")?;
    assert_contains_or_err(&resp.body, "Uh oh!", "body")?;
    Ok(())
}

fn test_404_deep_path(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/a/b/c/d")?;
    assert_eq_or_err(&resp.status_code, &404, "status")?;
    Ok(())
}

fn test_content_length_matches(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    let cl: usize = resp
        .headers
        .get("content-length")
        .ok_or("missing Content-Length header")?
        .parse()
        .map_err(|_| "Content-Length not a number".to_string())?;
    let actual_len = resp.body.len();
    assert_eq_or_err(&actual_len, &cl, "content-length vs body")?;
    Ok(())
}

fn test_trailing_slash(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/howdy/")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    assert_contains_or_err(&resp.body, "Howdy!", "body")?;
    Ok(())
}

fn test_double_slash(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "//")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    assert_contains_or_err(&resp.body, "Hello!", "body")?;
    Ok(())
}

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

    for h in handles {
        h.join().map_err(|_| "thread panicked".to_string())?;
    }

    let results = results.lock().unwrap();
    for (i, r) in results.iter().enumerate() {
        match r {
            Ok(code) => assert_eq_or_err(code, &200, &format!("thread {i} status"))?,
            Err(e) => return Err(format!("thread {i} failed: {e}")),
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Main orchestration
// ---------------------------------------------------------------------------

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
    println!("Server is ready.\n");

    let results = vec![
        run_test("root_route", || test_root_route(&addr)),
        run_test("index_css", || test_index_css(&addr)),
        run_test("howdy_route", || test_howdy_route(&addr)),
        run_test("howdy_page_css", || test_howdy_page_css(&addr)),
        run_test("404_does_not_exist", || test_404_does_not_exist(&addr)),
        run_test("404_deep_path", || test_404_deep_path(&addr)),
        run_test("content_length_matches", || {
            test_content_length_matches(&addr)
        }),
        run_test("trailing_slash", || test_trailing_slash(&addr)),
        run_test("double_slash", || test_double_slash(&addr)),
        run_test("concurrent_requests", || test_concurrent_requests(&addr)),
    ];

    let mut passed = 0;
    let mut failed = 0;
    for r in &results {
        if r.passed {
            println!("[PASS] {} -- {}", r.name, r.detail);
            passed += 1;
        } else {
            println!("[FAIL] {} -- {}", r.name, r.detail);
            failed += 1;
        }
    }

    let total = passed + failed;
    println!("\n{passed} passed, {failed} failed, {total} total");

    let _ = server.kill();
    let _ = server.wait();

    if failed > 0 {
        std::process::exit(1);
    }
}
