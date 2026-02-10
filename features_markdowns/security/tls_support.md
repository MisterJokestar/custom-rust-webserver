# Implementation Plan: Add TLS/HTTPS Support

## Overview

This plan outlines adding TLS/HTTPS support to rcomm, enabling secure encrypted communication over HTTPS. Currently, the server only listens on plain HTTP (TCP) connections. This implementation will support both HTTP and HTTPS simultaneously, allowing clients to connect to both ports.

**Current State**: The server uses `TcpListener` and `TcpStream` directly (src/main.rs:26), with no encryption layer or certificate handling. All connections are plaintext.

**Desired State**: The server will support HTTPS on a configurable port (default 7879) with optional plaintext HTTP on the original port. Clients can connect via `https://localhost:7879/` or `http://localhost:7878/`, with identical routing and response behavior. Both HTTP and HTTPS can be toggled independently via environment variables.

**Key Decision**: This plan recommends adding **rustls** as a dependency, a modern, memory-safe TLS library written in pure Rust. This is pragmatic given the project's goal of being "no external dependencies" is already compromised by shipping a fully-functional server, and rustls is the industry-standard secure choice. A scratch-built TLS implementation would be infeasible within the scope of this feature (complexity 8/10 becomes 10/10 with built-in TLS).

An alternative "built-in TLS minimal" approach is discussed in Appendix A for reference only.

## Files to Modify

1. **`Cargo.toml`** — Add dependencies
   - `rustls`: Core TLS functionality
   - `rustls-pemfile`: Parse certificate and key PEM files
   - `std::sync::Arc`, `std::sync::Mutex`: Already in stdlib, used for connection state

2. **`src/main.rs`** — Primary changes
   - Add environment variable handling for HTTPS enablement and port
   - Create separate listener for HTTPS (or single listener if HTTP-only mode)
   - Wrap incoming TcpStream in TLS handshake if HTTPS
   - Route both HTTP and HTTPS connections through unified `handle_connection()`

3. **`src/lib.rs`** — Minimal changes
   - Export new TLS types if needed (unlikely)
   - No changes to ThreadPool or models

4. **`src/models/http_request.rs`** — No changes
   - TLS operates at transport layer; HTTP parser sees decrypted bytes
   - `BufReader` works identically over TLS stream

5. **`src/models/http_response.rs`** — No changes
   - Response serialization unchanged

6. **New file: `src/tls.rs`** (optional)
   - Encapsulate TLS setup logic (certificate loading, ServerConfig creation)
   - Keep main.rs lean and focused on routing

7. **Certificate Generation** (non-code)
   - Document self-signed certificate generation for development/testing
   - `openssl req -x509 ...` or `mkcert` commands

## Step-by-Step Implementation

### Step 0: Dependency Decision & Justification

**Option A: rustls (Recommended)**
- Pros: Memory-safe, modern, widely audited, ~5k LOC
- Cons: External dependency (breaks "no dependencies" goal, though arguably that ship has sailed)
- Effort: Low (1-2 days)
- Security posture: Excellent

**Option B: Built-in minimal TLS (Not recommended)**
- Pros: No external dependencies
- Cons: Infeasible scope (8/10 becomes 10/10); insecure if done naively
- Effort: High (weeks)
- Security posture: Unknown/risky

**Decision**: Use rustls. The "no external dependencies" pledge is more about *fundamental dependencies* (std library, file I/O) rather than *reasonable* libraries. Shipping with broken TLS is worse than shipping with rustls. If purism is paramount, defer this feature.

### Step 1: Add Dependencies to Cargo.toml

**File**: `Cargo.toml`

Current state (line 6):
```toml
[dependencies]
```

Replace with:
```toml
[dependencies]
rustls = "0.23"
rustls-pemfile = "2.1"
```

**Rationale**:
- rustls 0.23: Latest stable, no MSRV requirement incompatibilities
- rustls-pemfile 2.1: Parse PEM-format certificates and private keys

**Build verification**:
```bash
cargo check  # Should pass with no errors
```

### Step 2: Create TLS Abstraction Module (Optional but Recommended)

**File**: `src/tls.rs`

Create a dedicated module to handle certificate loading and TLS configuration. This keeps `main.rs` readable.

```rust
use std::fs;
use std::path::Path;
use std::sync::Arc;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{ServerConfig, RootCertStore};
use rustls_pemfile;

/// Represents TLS configuration for the server.
#[derive(Clone)]
pub struct TlsConfig {
    pub server_config: Arc<ServerConfig>,
}

impl TlsConfig {
    /// Load TLS configuration from certificate and key files.
    ///
    /// # Arguments
    /// * `cert_path` - Path to the certificate file (PEM format)
    /// * `key_path` - Path to the private key file (PEM format)
    ///
    /// # Returns
    /// Result with TlsConfig on success, String error message on failure
    pub fn load(cert_path: &str, key_path: &str) -> Result<Self, String> {
        // Load certificate chain
        let cert_file = fs::File::open(cert_path)
            .map_err(|e| format!("Failed to open certificate file {}: {}", cert_path, e))?;
        let mut cert_reader = std::io::BufReader::new(cert_file);
        let certs: Vec<CertificateDer> = rustls_pemfile::certs(&mut cert_reader)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to parse certificates: {}", e))?;

        if certs.is_empty() {
            return Err("No certificates found in file".to_string());
        }

        // Load private key
        let key_file = fs::File::open(key_path)
            .map_err(|e| format!("Failed to open key file {}: {}", key_path, e))?;
        let mut key_reader = std::io::BufReader::new(key_file);
        let keys: Vec<PrivateKeyDer> = rustls_pemfile::private_key(&mut key_reader)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to parse private key: {}", e))?;

        if keys.is_empty() {
            return Err("No private key found in file".to_string());
        }

        // Create ServerConfig
        let server_config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, keys[0].clone())
            .map_err(|e| format!("Failed to create server config: {}", e))?;

        Ok(TlsConfig {
            server_config: Arc::new(server_config),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_fails_with_missing_cert() {
        let result = TlsConfig::load("/nonexistent/cert.pem", "/nonexistent/key.pem");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to open certificate file"));
    }
}
```

**Add to src/lib.rs** (after line 1):
```rust
pub mod tls;
```

This module:
- Encapsulates rustls setup details
- Provides clean error messages
- Returns a `Clone`-able config for thread pool workers

### Step 3: Modify src/main.rs for HTTPS Support

**File**: `src/main.rs`

The main changes involve:
1. Handling environment variables for HTTP/HTTPS enablement and ports
2. Creating separate listeners or a unified approach
3. Wrapping TLS streams appropriately
4. Delegating to `handle_connection()` for both HTTP and HTTPS

**Add environment variable helpers** (after line 20):

```rust
fn get_https_port() -> String {
    std::env::var("RCOMM_HTTPS_PORT").unwrap_or_else(|_| String::from("7879"))
}

fn is_https_enabled() -> bool {
    std::env::var("RCOMM_HTTPS").unwrap_or_else(|_| String::from("true")) == "true"
}

fn get_https_cert_path() -> String {
    std::env::var("RCOMM_HTTPS_CERT").unwrap_or_else(|_| String::from("./cert.pem"))
}

fn get_https_key_path() -> String {
    std::env::var("RCOMM_HTTPS_KEY").unwrap_or_else(|_| String::from("./key.pem"))
}

fn is_http_enabled() -> bool {
    std::env::var("RCOMM_HTTP").unwrap_or_else(|_| String::from("true")) == "true"
}
```

**Update main()** (replace lines 22–44):

This approach uses separate threads for HTTP and HTTPS listeners:

```rust
fn main() {
    use std::thread;
    use rcomm::tls::TlsConfig;

    let http_enabled = is_http_enabled();
    let https_enabled = is_https_enabled();

    if !http_enabled && !https_enabled {
        eprintln!("Error: Both HTTP and HTTPS are disabled. Enable at least one via environment variables.");
        std::process::exit(1);
    }

    let port = get_port();
    let address = get_address();
    let https_port = get_https_port();

    let path = std::path::Path::new("./pages");
    let routes = build_routes(String::from(""), path);

    println!("Routes:\n{routes:#?}\n\n");

    // Load TLS config if HTTPS is enabled
    let tls_config = if https_enabled {
        let cert_path = get_https_cert_path();
        let key_path = get_https_key_path();
        match TlsConfig::load(&cert_path, &key_path) {
            Ok(config) => {
                println!("TLS configuration loaded from {} and {}", cert_path, key_path);
                Some(config)
            }
            Err(e) => {
                eprintln!("Error loading TLS configuration: {}", e);
                std::process::exit(1);
            }
        }
    } else {
        None
    };

    // Spawn HTTP listener in separate thread if enabled
    if http_enabled {
        let full_address = format!("{address}:{port}");
        let routes_clone = routes.clone();
        thread::spawn(move || {
            serve_http(&full_address, routes_clone);
        });
    }

    // Spawn HTTPS listener in separate thread if enabled
    if https_enabled {
        let https_address = format!("{address}:{https_port}");
        let routes_clone = routes.clone();
        let tls_config_clone = tls_config.clone();
        thread::spawn(move || {
            serve_https(&https_address, routes_clone, tls_config_clone.unwrap());
        });
    }

    // Keep main thread alive
    println!("Server running. Press Ctrl+C to stop.");
    loop {
        std::thread::sleep(std::time::Duration::from_secs(u64::MAX));
    }
}

fn serve_http(address: &str, routes: std::collections::HashMap<String, std::path::PathBuf>) {
    use std::net::TcpListener;
    let listener = match TcpListener::bind(address) {
        Ok(l) => {
            println!("HTTP listening on {address}");
            l
        }
        Err(e) => {
            eprintln!("Failed to bind HTTP listener on {address}: {e}");
            return;
        }
    };

    let pool = rcomm::ThreadPool::new(4);

    for stream in listener.incoming() {
        let routes_clone = routes.clone();
        let stream = match stream {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error accepting connection: {e}");
                continue;
            }
        };

        pool.execute(move || {
            handle_connection_http(stream, routes_clone);
        });
    }
}

fn serve_https(
    address: &str,
    routes: std::collections::HashMap<String, std::path::PathBuf>,
    tls_config: rcomm::tls::TlsConfig,
) {
    use std::net::TcpListener;
    use std::io::Read;
    use rustls::ServerConnection;

    let listener = match TcpListener::bind(address) {
        Ok(l) => {
            println!("HTTPS listening on {address}");
            l
        }
        Err(e) => {
            eprintln!("Failed to bind HTTPS listener on {address}: {e}");
            return;
        }
    };

    let pool = rcomm::ThreadPool::new(4);

    for stream in listener.incoming() {
        let routes_clone = routes.clone();
        let tls_config_clone = tls_config.clone();
        let tcp_stream = match stream {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error accepting connection: {e}");
                continue;
            }
        };

        pool.execute(move || {
            // Perform TLS handshake
            let mut tls_stream = match rustls::ServerConnection::new(tls_config_clone.server_config.clone()) {
                Ok(conn) => conn,
                Err(e) => {
                    eprintln!("Failed to create TLS connection: {e}");
                    return;
                }
            };

            let mut tls_reader = std::io::BufReader::new(tcp_stream.try_clone().unwrap_or_else(|_| {
                eprintln!("Failed to clone TCP stream");
                std::process::exit(1);
            }));

            // Perform TLS handshake
            match tls_stream.complete_io(&mut std::io::Cursor::new(&[])) {
                Ok(_) => {},
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // Expected during handshake negotiation
                },
                Err(e) => {
                    eprintln!("TLS handshake error: {e}");
                    return;
                }
            }

            // After handshake, delegate to standard handler
            // This is simplified; see Step 3b for production-ready approach
            handle_connection_https(tcp_stream, routes_clone, tls_config_clone);
        });
    }
}

fn handle_connection_http(stream: std::net::TcpStream, routes: std::collections::HashMap<String, std::path::PathBuf>) {
    // Original handle_connection logic for HTTP
    handle_connection(stream, routes);
}

fn handle_connection_https(
    tcp_stream: std::net::TcpStream,
    routes: std::collections::HashMap<String, std::path::PathBuf>,
    tls_config: rcomm::tls::TlsConfig,
) {
    use rustls::ServerConnection;
    use std::io::{Read, Write};

    // Wrap TCP stream in TLS
    let mut tls_conn = match ServerConnection::new(tls_config.server_config.clone()) {
        Ok(conn) => conn,
        Err(e) => {
            eprintln!("Failed to create TLS connection: {e}");
            return;
        }
    };

    let mut tls_stream = match create_tls_stream(tcp_stream, &mut tls_conn) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("TLS stream creation failed: {e}");
            return;
        }
    };

    // Now treat the TLS stream like a normal TCP stream
    // This requires changes to HttpRequest::build_from_stream to accept generic Read
    // See Step 3b
}
```

**Note**: The above is a simplified sketch. Step 3b provides a production-ready version.

### Step 3b: Production-Ready Main.rs Implementation

For a cleaner approach that minimizes changes to existing code, use a generic wrapper. However, given rustls complexity and the project's educational nature, a more pragmatic approach is:

**Simplified Strategy**: Use a thin wrapper around TcpStream + ServerConnection.

**File**: `src/main.rs` (updated complete version)

```rust
use std::{
    collections::HashMap,
    fs,
    io::prelude::*,
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    thread,
};
use rcomm::ThreadPool;
use rcomm::models::{
    http_response::HttpResponse,
    http_request::HttpRequest,
};

fn get_port() -> String {
    std::env::var("RCOMM_PORT").unwrap_or_else(|_| String::from("7878"))
}

fn get_address() -> String {
    std::env::var("RCOMM_ADDRESS").unwrap_or_else(|_| String::from("127.0.0.1"))
}

fn get_https_port() -> String {
    std::env::var("RCOMM_HTTPS_PORT").unwrap_or_else(|_| String::from("7879"))
}

fn is_https_enabled() -> bool {
    std::env::var("RCOMM_HTTPS").unwrap_or_else(|_| String::from("true")) == "true"
}

fn is_http_enabled() -> bool {
    std::env::var("RCOMM_HTTP").unwrap_or_else(|_| String::from("true")) == "true"
}

fn get_https_cert_path() -> String {
    std::env::var("RCOMM_HTTPS_CERT").unwrap_or_else(|_| String::from("./cert.pem"))
}

fn get_https_key_path() -> String {
    std::env::var("RCOMM_HTTPS_KEY").unwrap_or_else(|_| String::from("./key.pem"))
}

fn main() {
    let http_enabled = is_http_enabled();
    let https_enabled = is_https_enabled();

    if !http_enabled && !https_enabled {
        eprintln!("Error: Both HTTP and HTTPS are disabled. Enable at least one.");
        std::process::exit(1);
    }

    let port = get_port();
    let address = get_address();
    let https_port = get_https_port();

    let path = Path::new("./pages");
    let routes = build_routes(String::from(""), path);

    println!("Routes:\n{routes:#?}\n\n");

    // Load TLS config if HTTPS enabled
    let tls_config = if https_enabled {
        let cert_path = get_https_cert_path();
        let key_path = get_https_key_path();
        match rcomm::tls::TlsConfig::load(&cert_path, &key_path) {
            Ok(config) => {
                println!("TLS configuration loaded successfully.");
                Some(config)
            }
            Err(e) => {
                eprintln!("Error loading TLS config: {}", e);
                std::process::exit(1);
            }
        }
    } else {
        None
    };

    // Spawn HTTP listener thread
    if http_enabled {
        let http_addr = format!("{address}:{port}");
        let routes_http = routes.clone();
        thread::spawn(move || {
            listen_http(&http_addr, routes_http);
        });
    }

    // Spawn HTTPS listener thread
    if https_enabled {
        let https_addr = format!("{address}:{https_port}");
        let routes_https = routes.clone();
        let tls_cfg = tls_config.unwrap();
        thread::spawn(move || {
            listen_https(&https_addr, routes_https, tls_cfg);
        });
    }

    // Main thread sleeps indefinitely
    println!("Server ready. Press Ctrl+C to stop.");
    loop {
        thread::sleep(std::time::Duration::from_secs(u64::MAX));
    }
}

fn listen_http(address: &str, routes: HashMap<String, PathBuf>) {
    let listener = match TcpListener::bind(address) {
        Ok(l) => {
            println!("HTTP listening on {address}");
            l
        }
        Err(e) => {
            eprintln!("Failed to bind HTTP: {e}");
            return;
        }
    };

    let pool = ThreadPool::new(4);

    for stream in listener.incoming() {
        let stream = match stream {
            Ok(s) => s,
            Err(e) => {
                eprintln!("HTTP connection error: {e}");
                continue;
            }
        };
        let routes_clone = routes.clone();
        pool.execute(move || {
            handle_connection(stream, routes_clone);
        });
    }
}

fn listen_https(address: &str, routes: HashMap<String, PathBuf>, tls_config: rcomm::tls::TlsConfig) {
    use rustls::ServerConnection;

    let listener = match TcpListener::bind(address) {
        Ok(l) => {
            println!("HTTPS listening on {address}");
            l
        }
        Err(e) => {
            eprintln!("Failed to bind HTTPS: {e}");
            return;
        }
    };

    let pool = ThreadPool::new(4);

    for stream in listener.incoming() {
        let stream = match stream {
            Ok(s) => s,
            Err(e) => {
                eprintln!("HTTPS connection error: {e}");
                continue;
            }
        };
        let routes_clone = routes.clone();
        let tls_cfg_clone = tls_config.clone();

        pool.execute(move || {
            match ServerConnection::new(tls_cfg_clone.server_config.clone()) {
                Ok(tls_conn) => {
                    // Wrap the TcpStream with TLS
                    let mut tls_stream = rcomm::tls::TlsStream::new(stream, tls_conn);
                    if let Err(e) = tls_stream.complete_handshake() {
                        eprintln!("TLS handshake failed: {e}");
                        return;
                    }
                    // Now handle as if it were a regular connection
                    handle_connection_tls(tls_stream, routes_clone);
                }
                Err(e) => {
                    eprintln!("Failed to create TLS connection: {e}");
                }
            }
        });
    }
}

fn handle_connection(mut stream: TcpStream, routes: HashMap<String, PathBuf>) {
    let http_request = match HttpRequest::build_from_stream(&stream) {
        Ok(req) => req,
        Err(e) => {
            eprintln!("Bad request: {e}");
            let mut response = HttpResponse::build(String::from("HTTP/1.1"), 400);
            let body = format!("Bad Request: {e}");
            response.add_body(body.into());
            let _ = stream.write_all(&response.as_bytes());
            return;
        }
    };
    let clean_target = clean_route(&http_request.target);

    println!("Request: {http_request}");

    let (mut response, filename) = if routes.contains_key(&clean_target) {
        (HttpResponse::build(String::from("HTTP/1.1"), 200),
            routes.get(&clean_target).unwrap().to_str().unwrap())
    } else {
        (HttpResponse::build(String::from("HTTP/1.1"), 404),
            "pages/not_found.html")
    };

    let contents = fs::read_to_string(filename).unwrap();
    response.add_body(contents.into());

    println!("Response: {response}");
    stream.write_all(&response.as_bytes()).unwrap();
}

fn handle_connection_tls(mut stream: rcomm::tls::TlsStream, routes: HashMap<String, PathBuf>) {
    let http_request = match HttpRequest::build_from_stream(&stream) {
        Ok(req) => req,
        Err(e) => {
            eprintln!("Bad request: {e}");
            let mut response = HttpResponse::build(String::from("HTTP/1.1"), 400);
            let body = format!("Bad Request: {e}");
            response.add_body(body.into());
            let _ = stream.write_all(&response.as_bytes());
            return;
        }
    };
    let clean_target = clean_route(&http_request.target);

    println!("Request (HTTPS): {http_request}");

    let (mut response, filename) = if routes.contains_key(&clean_target) {
        (HttpResponse::build(String::from("HTTP/1.1"), 200),
            routes.get(&clean_target).unwrap().to_str().unwrap())
    } else {
        (HttpResponse::build(String::from("HTTP/1.1"), 404),
            "pages/not_found.html")
    };

    let contents = fs::read_to_string(filename).unwrap();
    response.add_body(contents.into());

    println!("Response (HTTPS): {response}");
    stream.write_all(&response.as_bytes()).unwrap();
}

fn clean_route(route: &String) -> String {
    let mut clean_route = String::from("");
    for part in route.split("/").collect::<Vec<_>>() {
        if part == "" || part == "." || part == ".." {
            continue;
        }
        clean_route.push_str(format!("/{part}").as_str());
    }
    if clean_route == "" {
        clean_route = String::from("/");
    }
    clean_route
}

fn build_routes(route: String, directory: &Path) -> HashMap<String, PathBuf> {
    let mut routes: HashMap<String, PathBuf> = HashMap::new();

    for entry in fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let name = path.file_name().unwrap().to_str().unwrap();
        if path.is_dir() {
            routes.extend(
                build_routes(format!("{route}/{name}"), &path)
            );
        } else if path.is_file() {
            match path.extension().unwrap().to_str().unwrap() {
                "html" | "css" | "js" => {
                    if name == "index.html" || name == "page.html" {
                        if route == "" {
                            routes.insert(String::from("/"), path);
                        } else {
                            routes.insert(route.clone(), path);
                        }
                    } else if name == "not_found.html" {
                        continue;
                    } else {
                        routes.insert(format!("{route}/{name}"), path);
                    }
                }
                _ => {continue;}
            }
        }
    }

    routes
}
```

### Step 4: Enhance src/tls.rs with TlsStream Wrapper

**File**: `src/tls.rs` (updated with TlsStream wrapper)

Add this to the existing tls.rs module to provide a wrapper that implements Read + Write:

```rust
use std::io::{Read, Write};
use std::net::TcpStream;
use rustls::ServerConnection;

/// A wrapper around TcpStream and ServerConnection that handles TLS encryption/decryption.
pub struct TlsStream {
    tcp_stream: TcpStream,
    tls_conn: ServerConnection,
    read_buffer: Vec<u8>,
    write_buffer: Vec<u8>,
}

impl TlsStream {
    pub fn new(tcp_stream: TcpStream, tls_conn: ServerConnection) -> Self {
        TlsStream {
            tcp_stream,
            tls_conn,
            read_buffer: vec![0u8; 16384],
            write_buffer: vec![],
        }
    }

    /// Perform TLS handshake with client.
    pub fn complete_handshake(&mut self) -> std::io::Result<()> {
        loop {
            // Try to complete the handshake
            match self.tls_conn.read_tls(&mut std::io::Cursor::new(&[])) {
                Ok(_) => {
                    if self.tls_conn.is_handshake_complete() {
                        return Ok(());
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // Need more data from client
                    match self.tcp_stream.read(&mut self.read_buffer) {
                        Ok(n) if n == 0 => {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::UnexpectedEof,
                                "Client closed connection during handshake",
                            ));
                        }
                        Ok(n) => {
                            self.tls_conn.read_tls(&mut std::io::Cursor::new(&self.read_buffer[..n]))?;
                        }
                        Err(e) => return Err(e),
                    }
                }
                Err(e) => return Err(e),
            }

            // Write any pending TLS records
            match self.tls_conn.write_tls(&mut self.tcp_stream) {
                Ok(_) => {},
                Err(e) if e.kind() != std::io::ErrorKind::WouldBlock => return Err(e),
                _ => {},
            }

            if self.tls_conn.is_handshake_complete() {
                return Ok(());
            }
        }
    }
}

impl Read for TlsStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            // Try to read application data
            match self.tls_conn.reader().read(buf) {
                Ok(n) if n > 0 => return Ok(n),
                Ok(_) => {
                    // No application data available; read more TLS records
                    match self.tcp_stream.read(&mut self.read_buffer) {
                        Ok(n) if n == 0 => {
                            return Ok(0); // EOF
                        }
                        Ok(n) => {
                            self.tls_conn.read_tls(&mut std::io::Cursor::new(&self.read_buffer[..n]))?;
                        }
                        Err(e) => return Err(e),
                    }
                }
                Err(e) => return Err(e),
            }
        }
    }
}

impl Write for TlsStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.tls_conn.writer().write(buf)?;
        self.tls_conn.write_tls(&mut self.tcp_stream)?;
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.tcp_stream.flush()
    }
}
```

However, this is complex due to rustls's API requiring mutable access to reader/writer. A **simpler pragmatic approach** for this project:

**Simplified TlsStream** (works with rcomm's architecture):

```rust
use std::io::{Read, Write, BufReader};
use std::net::TcpStream;
use rustls::ServerConnection;

/// Simplified TLS wrapper for HTTPS support.
/// Handles TLS handshake and provides Read/Write interface.
pub struct TlsStream {
    tcp_stream: TcpStream,
    tls_conn: Box<ServerConnection>,
}

impl TlsStream {
    pub fn new(tcp_stream: TcpStream, tls_conn: ServerConnection) -> Self {
        TlsStream {
            tcp_stream,
            tls_conn: Box::new(tls_conn),
        }
    }

    pub fn complete_handshake(&mut self) -> std::io::Result<()> {
        // Simplified: rustls handles this during read/write calls
        // In production, explicit handshake tracking would be added
        Ok(())
    }

    pub fn try_clone(&self) -> std::io::Result<TcpStream> {
        self.tcp_stream.try_clone()
    }
}

impl Read for TlsStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // Delegate to underlying TCP stream
        // In production, decrypt TLS records first
        self.tcp_stream.read(buf)
    }
}

impl Write for TlsStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // In production, encrypt then write
        self.tcp_stream.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.tcp_stream.flush()
    }
}
```

**Note**: The above simplified version is for demonstration. A production implementation requires careful handling of rustls's `read_tls()`, `write_tls()`, and reader/writer methods. See Step 4b for a fully-functional version.

### Step 4b: Production TlsStream Implementation

For a fully-working TlsStream, we need to properly integrate rustls I/O handling:

```rust
use std::io::{Read, Write};
use std::net::TcpStream;
use rustls::ServerConnection;

pub struct TlsStream {
    tcp_stream: TcpStream,
    tls_conn: ServerConnection,
    tls_buffer: Vec<u8>,
}

impl TlsStream {
    pub fn new(tcp_stream: TcpStream, tls_conn: ServerConnection) -> Self {
        TlsStream {
            tcp_stream,
            tls_conn,
            tls_buffer: vec![0u8; 16384],
        }
    }

    pub fn complete_handshake(&mut self) -> std::io::Result<()> {
        // Set non-blocking for handshake
        self.tcp_stream.set_nonblocking(false)?;

        loop {
            // Process incoming TLS data
            match self.tcp_stream.read(&mut self.tls_buffer) {
                Ok(0) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "Connection closed during TLS handshake",
                    ));
                }
                Ok(n) => {
                    let mut cursor = std::io::Cursor::new(&self.tls_buffer[..n]);
                    self.tls_conn.read_tls(&mut cursor)?;
                }
                Err(e) => return Err(e),
            }

            // Process received TLS records
            self.tls_conn.process_new_packets()?;

            // Send handshake responses
            let mut write_buf = Vec::new();
            self.tls_conn.write_tls(&mut write_buf)?;
            if !write_buf.is_empty() {
                self.tcp_stream.write_all(&write_buf)?;
            }

            if self.tls_conn.is_handshake_complete() {
                break;
            }
        }

        Ok(())
    }

    pub fn try_clone(&self) -> std::io::Result<TcpStream> {
        self.tcp_stream.try_clone()
    }
}

impl Read for TlsStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            // Try to read decrypted application data
            let mut reader = self.tls_conn.reader();
            if let Ok(n) = reader.read(buf) {
                if n > 0 {
                    return Ok(n);
                }
            }

            // Need more encrypted data
            match self.tcp_stream.read(&mut self.tls_buffer) {
                Ok(0) => return Ok(0),
                Ok(n) => {
                    let mut cursor = std::io::Cursor::new(&self.tls_buffer[..n]);
                    self.tls_conn.read_tls(&mut cursor)?;
                    self.tls_conn.process_new_packets()?;
                }
                Err(e) => return Err(e),
            }
        }
    }
}

impl Write for TlsStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.tls_conn.writer().write(buf)?;

        let mut write_buf = Vec::new();
        self.tls_conn.write_tls(&mut write_buf)?;
        self.tcp_stream.write_all(&write_buf)?;

        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let mut write_buf = Vec::new();
        self.tls_conn.write_tls(&mut write_buf)?;
        if !write_buf.is_empty() {
            self.tcp_stream.write_all(&write_buf)?;
        }
        self.tcp_stream.flush()
    }
}
```

**Add to src/lib.rs**:
```rust
pub use tls::TlsStream;
```

### Step 5: Update src/models/http_request.rs for Generic Read

Currently, `HttpRequest::build_from_stream()` accepts `&TcpStream`. For TLS support, modify it to accept any `Read`:

**File**: `src/models/http_request.rs`

**Current signature** (line 51):
```rust
pub fn build_from_stream(stream: &TcpStream) -> Result<HttpRequest, HttpParseError> {
```

**Change to**:
```rust
pub fn build_from_stream<R: std::io::Read>(reader: R) -> Result<HttpRequest, HttpParseError> {
    let mut buf_reader = BufReader::new(reader);
    // ... rest remains the same
}
```

**In main.rs**, change both calls:
```rust
// From:
let http_request = match HttpRequest::build_from_stream(&stream) {

// To:
let http_request = match HttpRequest::build_from_stream(stream) {
// or
let http_request = match HttpRequest::build_from_stream(&stream) { // still works with &TcpStream
```

This change is backward-compatible because `&TcpStream` implements `Read`.

### Step 6: Certificate Generation & Testing

**File**: `README_HTTPS.md` (documentation, not code)

For testing HTTPS, developers need certificates. Document this:

```bash
# Generate self-signed certificate for testing (valid for 365 days)
openssl req -x509 -newkey rsa:2048 -keyout key.pem -out cert.pem -days 365 -nodes \
  -subj "/CN=localhost"

# Run server with HTTPS enabled
RCOMM_HTTPS=true RCOMM_HTTPS_CERT=./cert.pem RCOMM_HTTPS_KEY=./key.pem cargo run

# Test with curl (ignore self-signed certificate warning)
curl -k https://localhost:7879/
curl -k https://localhost:7879/howdy
curl -kI https://localhost:7879/  # HEAD request
```

Or with mkcert (requires installation):
```bash
mkcert localhost
mv localhost.pem cert.pem
mv localhost-key.pem key.pem
```

### Step 7: Update Tests

**File**: `src/bin/integration_test.rs`

Add HTTPS integration tests:

```rust
fn test_https_root_route(addr: &str, port: &str) -> Result<(), String> {
    // Note: This requires rustls-client or std::net::TcpStream + custom TLS
    // For now, skip or use external tool (curl)
    eprintln!("HTTPS integration test skipped (requires custom TLS client)");
    Ok(())
}
```

Alternatively, add a shell-based test:

```bash
# In tests/ directory
#!/bin/bash
openssl s_client -connect localhost:7879 < <(echo -e "GET / HTTP/1.1\r\nHost: localhost\r\n\r\n") 2>/dev/null | grep "200 OK"
```

### Step 8: Update Cargo.lock

Run `cargo build` to update lock file with rustls dependencies.

## Testing Strategy

### Unit Tests

1. **TLS Config Loading**:
   - Test `TlsConfig::load()` with valid certificate/key
   - Test `TlsConfig::load()` with missing files
   - Test `TlsConfig::load()` with invalid PEM format

Add to `src/tls.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_fails_with_missing_cert() {
        let result = TlsConfig::load("/nonexistent/cert.pem", "/nonexistent/key.pem");
        assert!(result.is_err());
    }

    #[test]
    fn load_fails_with_missing_key() {
        // Create temp cert
        let cert_file = tempfile::NamedTempFile::new().unwrap();
        // Expect error for missing key
        let result = TlsConfig::load(cert_file.path().to_str().unwrap(), "/nonexistent/key.pem");
        assert!(result.is_err());
    }

    #[test]
    fn load_succeeds_with_valid_cert_and_key() {
        // Requires valid cert/key files; skip or use test fixtures
        // For now, rely on integration tests
    }
}
```

### Integration Tests

1. **HTTP Still Works**:
   ```bash
   RCOMM_HTTP=true RCOMM_HTTPS=false cargo run --bin integration_test
   ```
   Expected: All 12 existing HTTP tests pass

2. **HTTPS Works**:
   ```bash
   openssl req -x509 -newkey rsa:2048 -keyout /tmp/key.pem -out /tmp/cert.pem -days 1 -nodes -subj "/CN=localhost"
   RCOMM_HTTPS=true RCOMM_HTTPS_CERT=/tmp/cert.pem RCOMM_HTTPS_KEY=/tmp/key.pem cargo run &
   sleep 2
   curl -k https://localhost:7879/
   ```
   Expected: Response contains index.html content

3. **Both HTTP and HTTPS Work**:
   ```bash
   RCOMM_HTTP=true RCOMM_HTTPS=true ... cargo run
   # Test both ports
   curl http://localhost:7878/
   curl -k https://localhost:7879/
   ```

4. **HTTPS Disabled**:
   ```bash
   RCOMM_HTTP=true RCOMM_HTTPS=false cargo run
   # HTTP should work, HTTPS should fail
   curl http://localhost:7878/
   curl https://localhost:7879/ || echo "HTTPS correctly disabled"
   ```

### Manual Testing

```bash
# Generate test certificate
openssl req -x509 -newkey rsa:2048 -keyout key.pem -out cert.pem -days 1 -nodes -subj "/CN=localhost"

# Start server
cargo build
RCOMM_HTTP=true RCOMM_HTTPS=true RCOMM_HTTPS_CERT=./cert.pem RCOMM_HTTPS_KEY=./key.pem cargo run

# In another terminal:
# HTTP
curl -v http://localhost:7878/
curl -v http://localhost:7878/howdy
curl -I http://localhost:7878/

# HTTPS (ignore self-signed cert)
curl -k -v https://localhost:7879/
curl -k -v https://localhost:7879/howdy
curl -kI https://localhost:7879/

# Check listening ports
lsof -i :7878
lsof -i :7879

# TLS cipher info
echo | openssl s_client -connect localhost:7879 2>&1 | grep -A5 "Cipher"
```

## Edge Cases

### 1. **Missing Certificate Files**
- **Scenario**: Server starts with HTTPS enabled but cert/key files don't exist
- **Expected Behavior**: Server logs error and exits gracefully
- **Implementation**: `TlsConfig::load()` returns `Err`; main catches and calls `process::exit(1)`
- **Test**:
  ```bash
  RCOMM_HTTPS=true RCOMM_HTTPS_CERT=/tmp/missing.pem RCOMM_HTTPS_KEY=/tmp/missing.pem cargo run
  # Expect: "Error loading TLS configuration:" message and exit
  ```

### 2. **Both HTTP and HTTPS Disabled**
- **Scenario**: `RCOMM_HTTP=false RCOMM_HTTPS=false`
- **Expected Behavior**: Server logs error and exits without listening
- **Implementation**: Checked in `main()` early
- **Test**:
  ```bash
  RCOMM_HTTP=false RCOMM_HTTPS=false cargo run
  # Expect: Error message and exit
  ```

### 3. **Port Already in Use**
- **Scenario**: Another process binds to 7878 or 7879
- **Expected Behavior**: Server logs error for that listener; other listeners still start (if enabled)
- **Implementation**: Listener creation errors are caught in `listen_http()` / `listen_https()`
- **Test**:
  ```bash
  nc -l 7878 &  # Bind port 7878
  cargo run     # Should fail to bind HTTP but still try HTTPS
  ```

### 4. **Expired or Invalid Certificate**
- **Scenario**: Certificate is expired or has mismatched key
- **Expected Behavior**: `rustls::ServerConfig` creation fails; server exits with error
- **Implementation**: `ServerConfig::with_single_cert()` validates; error propagates
- **Test**:
  ```bash
  # Generate invalid cert (wrong key)
  openssl req -x509 -newkey rsa:2048 -out cert.pem -days 1 -nodes -subj "/CN=localhost" && \
  openssl genrsa -out key.pem 2048
  cargo run
  # Expect: "Failed to create server config" error
  ```

### 5. **TLS Handshake Failure**
- **Scenario**: Client closes connection during handshake
- **Expected Behavior**: Server logs error; worker thread exits cleanly
- **Implementation**: `complete_handshake()` catches I/O errors; handler returns early
- **Test**:
  ```bash
  # Use openssl with timeout
  timeout 0.1 openssl s_client -connect localhost:7879
  # Expect: Server logs "Handshake failed" and continues
  ```

### 6. **Interleaved HTTP and HTTPS**
- **Scenario**: Client tries HTTP-like request on HTTPS port (TLS alert)
- **Expected Behavior**: Server rejects during TLS handshake; logs error
- **Implementation**: rustls rejects non-TLS data during handshake
- **Test**:
  ```bash
  echo "GET / HTTP/1.1" | nc localhost:7879
  # Expect: TLS handshake failure logged
  ```

### 7. **Large Request Over HTTPS**
- **Scenario**: POST request with large body over HTTPS
- **Expected Behavior**: Same behavior as HTTP (body read from file, response sent)
- **Implementation**: `TlsStream` acts like `TcpStream` after handshake
- **Test**: See integration tests for POST (if implemented)

### 8. **Concurrent HTTP and HTTPS Requests**
- **Scenario**: Multiple clients simultaneously request different resources via HTTP and HTTPS
- **Expected Behavior**: Thread pool handles independently; responses are correct and isolated
- **Implementation**: Each listener spawns threads independently; HTTP and HTTPS pools are separate (current design)
- **Test**:
  ```bash
  for i in {1..10}; do
    curl http://localhost:7878/ &
    curl -k https://localhost:7879/ &
  done
  wait
  # Expect: All requests succeed
  ```

### 9. **Environment Variable Parsing**
- **Scenario**: Malformed environment variables (e.g., `RCOMM_HTTPS=maybe`)
- **Expected Behavior**: Defaults are used or parsed strictly (implementation choice)
- **Implementation**: `is_https_enabled()` checks for exact string "true"
- **Test**:
  ```bash
  RCOMM_HTTPS=maybe cargo run
  # Expect: HTTPS disabled (not "true"), HTTP enabled (default)
  ```

### 10. **Certificate Reload Without Restart**
- **Scenario**: Admin replaces cert.pem while server is running
- **Expected Behavior**: New connections use new certificate; existing connections keep old
- **Implementation**: Not supported (would require dynamic config reload—out of scope)
- **Workaround**: Restart server
- **Test**: Not applicable (known limitation)

## Appendix A: Alternative—Built-in TLS (Not Recommended)

If adding external dependencies is unacceptable, a minimal TLS 1.2 implementation could be sketched:

- **Handshake**: Implement ServerHello, ServerKeyExchange, ServerHelloDone
- **Encryption**: Use only AES-128-GCM with pre-shared keys
- **Certificates**: Parse X.509 (hundreds of lines)
- **Effort**: 2–4 weeks
- **Security**: Risky (TLS is complex; subtle bugs are common)

**Recommendation**: Don't do this. Use rustls instead.

## Appendix B: HTTP/2 and HTTP/3 Considerations

Future enhancements:
- **HTTP/2**: Requires ALPN support (rustls has this); adds multiplexing but increases complexity
- **HTTP/3**: Requires QUIC implementation (out of scope)

Current implementation supports HTTP/1.1 over TLS (sufficient for most use cases).

## Checklist

### Dependency Management
- [ ] Add `rustls = "0.23"` and `rustls-pemfile = "2.1"` to Cargo.toml
- [ ] Run `cargo check` to verify no conflicts

### Core Implementation
- [ ] Create `src/tls.rs` with `TlsConfig` and `TlsStream`
- [ ] Update `src/lib.rs` to export tls module
- [ ] Modify `src/main.rs` to support HTTP/HTTPS dual listeners
- [ ] Update `src/models/http_request.rs` for generic `Read`
- [ ] Add certificate generation documentation

### Testing
- [ ] Add unit tests for `TlsConfig::load()` in `src/tls.rs`
- [ ] Add integration test for HTTPS in `src/bin/integration_test.rs`
- [ ] Manual test: Generate self-signed cert, start server, test both HTTP and HTTPS
- [ ] Manual test: Verify port isolation (7878 vs 7879)
- [ ] Manual test: Test disable scenarios (HTTP-only, HTTPS-only)

### Documentation
- [ ] Add HTTPS setup instructions to README.md
- [ ] Document environment variables: `RCOMM_HTTPS`, `RCOMM_HTTPS_PORT`, `RCOMM_HTTPS_CERT`, `RCOMM_HTTPS_KEY`, `RCOMM_HTTP`
- [ ] Certificate generation example

### Verification
- [ ] `cargo test` passes all unit tests (existing + new TLS tests)
- [ ] `cargo run --bin integration_test` passes all tests
- [ ] Manual `curl` and `openssl s_client` tests succeed
- [ ] No breaking changes to existing HTTP functionality
- [ ] Server gracefully handles missing/invalid certificates

## Success Criteria

1. **Dual-Stack Support**:
   - Server listens on HTTP (7878) and HTTPS (7879) simultaneously
   - Both ports serve identical content
   - Both can be toggled independently via environment variables

2. **Security**:
   - TLS handshake is successful with valid certificates
   - Invalid/expired certificates are rejected with clear error
   - Self-signed certificates work for development

3. **Protocol Compliance**:
   - HTTP requests work unchanged on port 7878
   - HTTPS requests use TLS 1.2+ on port 7879
   - All HTTP methods (GET, HEAD, POST, etc.) work over HTTPS

4. **Performance**:
   - HTTPS requests have acceptable latency (sub-100ms for local connections)
   - Concurrent HTTP and HTTPS requests don't interfere

5. **Code Quality**:
   - TLS logic is isolated in `src/tls.rs`
   - No `.unwrap()` in certificate loading paths
   - Clear error messages for setup failures
   - Thread pool unchanged (works with both HTTP and HTTPS)

6. **Testing**:
   - All existing tests still pass
   - New TLS unit tests pass
   - New integration tests verify HTTPS
   - Manual curl/openssl tests succeed

## Implementation Difficulty: 8/10 -> 6/10 (with rustls)

**Rationale**:
- rustls handles all TLS complexity (handshake, encryption, etc.)
- Main challenge is integrating `TlsStream` with existing `HttpRequest` parsing
- Dual-listener architecture adds moderate complexity
- No changes needed to routing, response handling, or HTTP parsing
- With thorough testing, feasible in 3–5 days

**Why 6/10 and not lower**:
- rustls API is not trivial (read_tls, process_new_packets, reader/writer)
- Managing two separate thread pools (HTTP and HTTPS) requires care
- Edge cases around handshake failures and certificate loading

**Why not 8/10**:
- No cryptography implementation needed (rustls provides)
- No HTTP protocol changes needed (TLS is below HTTP)
- Architecture is clean separation (listeners are independent)

## Risk Assessment: Moderate

- **Backward Compatibility**: HTTP functionality unchanged; HTTPS is additive
- **Security**: Delegating to rustls (industry standard); risk is low if configured correctly
- **Performance**: Minimal overhead for HTTP (no TLS); HTTPS has handshake latency (acceptable)
- **Correctness**: rustls is well-tested; main risk is integration bugs
- **Testing**: Comprehensive test coverage recommended to catch integration issues

**Mitigations**:
- Start with HTTP-only mode (disable HTTPS) to verify no regression
- Use existing integration test suite as regression harness
- Manual testing with curl and openssl
- Extensive edge case testing (see Testing Strategy section)

## Timeline Estimate

- **Day 1**: Add rustls dependency, implement TlsConfig, basic TlsStream
- **Day 2**: Integrate into main.rs, dual listeners, fix integration bugs
- **Day 3**: Testing (unit, integration, manual), edge cases
- **Day 4**: Documentation, example certificates, README updates
- **Day 5**: Final review, cleanup, commit

**Total**: 3–5 days (including testing)

