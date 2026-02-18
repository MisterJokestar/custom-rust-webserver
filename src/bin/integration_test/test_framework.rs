use std::{
    collections::HashMap,
    env,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

// ---------------------------------------------------------------------------
// Server lifecycle
// ---------------------------------------------------------------------------

pub fn pick_free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("failed to bind to port 0");
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}

pub fn find_server_binary() -> PathBuf {
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

pub fn find_project_root() -> PathBuf {
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

pub fn start_server(port: u16) -> Child {
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

pub fn wait_for_server(addr: &str, timeout: Duration) -> Result<(), String> {
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
pub struct TestResponse {
    pub status_code: u16,
    pub status_phrase: String,
    pub headers: HashMap<String, String>,
    pub body: String,
}

pub fn read_response(stream: &mut TcpStream) -> Result<TestResponse, String> {
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

pub fn send_request(addr: &str, method: &str, path: &str) -> Result<TestResponse, String> {
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

pub struct TestResult {
    pub name: String,
    pub passed: bool,
    pub detail: String,
}

pub fn run_test<F>(name: &str, f: F) -> TestResult
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

pub fn assert_eq_or_err<T: PartialEq + std::fmt::Debug>(
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

pub fn assert_contains_or_err(haystack: &str, needle: &str, label: &str) -> Result<(), String> {
    if haystack.contains(needle) {
        Ok(())
    } else {
        Err(format!(
            "{label}: expected body to contain {needle:?}, body was {} bytes",
            haystack.len()
        ))
    }
}
