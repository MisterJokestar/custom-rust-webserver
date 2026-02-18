# Virtual Host Routing Implementation Plan

## Overview

Virtual host routing allows a single rcomm server instance to serve different content based on the `Host` request header. This enables hosting multiple websites on the same IP address and port, with each site having its own `pages/` directory tree and route table.

For example:
- Requests with `Host: example.com` serve from `sites/example.com/pages/`
- Requests with `Host: blog.example.com` serve from `sites/blog.example.com/pages/`
- Requests without a recognized `Host` header fall back to the default `pages/` directory

**Complexity**: 6
**Necessity**: 2

**Key Changes**:
- Parse `Host` header from requests (already available in `HttpRequest.headers`)
- Define a virtual host directory convention: `sites/{hostname}/pages/`
- Build separate route tables per virtual host at startup
- Route requests to the appropriate route table based on `Host` header
- Fall back to default routes when `Host` doesn't match any virtual host

---

## Files to Modify

### 1. `/home/jwall/personal/rusty/rcomm/src/main.rs`

**Current State**:
- Line 30-31: Routes built from single `./pages` directory
- Line 37: Routes cloned per connection as `HashMap<String, PathBuf>`
- Line 46-75: `handle_connection()` does single route table lookup
- No `Host` header inspection

**Changes Required**:
- Add virtual host discovery (scan `sites/` directory for hostname directories)
- Build per-host route tables: `HashMap<String, HashMap<String, PathBuf>>` (hostname → routes)
- Update `handle_connection()` to extract `Host` header and select the right route table
- Keep default `pages/` routes as fallback

### 2. `/home/jwall/personal/rusty/rcomm/src/bin/integration_test.rs`

**Changes Required**:
- Add tests that send `Host` header and verify virtual host routing
- Add tests for fallback to default routes

---

## Step-by-Step Implementation

### Step 1: Define Directory Convention

```
project_root/
  pages/                          ← Default site (no Host match)
    index.html
  sites/                          ← Virtual hosts directory
    example.com/
      pages/
        index.html               ← Served for Host: example.com
    blog.example.com/
      pages/
        index.html               ← Served for Host: blog.example.com
```

### Step 2: Add Virtual Host Discovery and Route Building

**Location**: `src/main.rs`, before `main()`

```rust
/// A collection of route tables keyed by hostname.
/// The empty string key "" represents the default (fallback) routes.
struct VirtualHosts {
    hosts: HashMap<String, HashMap<String, PathBuf>>,
    default_routes: HashMap<String, PathBuf>,
}

impl VirtualHosts {
    /// Look up routes for a given Host header value.
    /// Falls back to default routes if no virtual host matches.
    fn get_routes(&self, host: &str) -> &HashMap<String, PathBuf> {
        // Strip port from Host header (e.g., "example.com:7878" → "example.com")
        let hostname = host.split(':').next().unwrap_or(host);

        self.hosts
            .get(hostname)
            .unwrap_or(&self.default_routes)
    }
}

impl Clone for VirtualHosts {
    fn clone(&self) -> Self {
        VirtualHosts {
            hosts: self.hosts.clone(),
            default_routes: self.default_routes.clone(),
        }
    }
}

/// Discover virtual hosts from the sites/ directory and build route tables.
fn build_virtual_hosts(sites_dir: &Path, default_pages_dir: &Path) -> VirtualHosts {
    let default_routes = build_routes(String::from(""), default_pages_dir);
    let mut hosts: HashMap<String, HashMap<String, PathBuf>> = HashMap::new();

    if sites_dir.exists() && sites_dir.is_dir() {
        for entry in fs::read_dir(sites_dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();

            if path.is_dir() {
                let hostname = path.file_name().unwrap().to_str().unwrap().to_string();
                let host_pages = path.join("pages");

                if host_pages.exists() && host_pages.is_dir() {
                    let host_routes = build_routes(String::from(""), &host_pages);
                    println!("Virtual host '{hostname}': {} route(s)", host_routes.len());
                    hosts.insert(hostname, host_routes);
                }
            }
        }
    }

    VirtualHosts { hosts, default_routes }
}
```

### Step 3: Update `main()`

**Current Code** (lines 30-31):
```rust
    let path = Path::new("./pages");
    let routes = build_routes(String::from(""), path);
```

**New Code**:
```rust
    let default_pages = Path::new("./pages");
    let sites_dir = Path::new("./sites");
    let vhosts = build_virtual_hosts(sites_dir, default_pages);

    println!("Default routes: {} route(s)", vhosts.default_routes.len());
    if !vhosts.hosts.is_empty() {
        println!("Virtual hosts: {}", vhosts.hosts.keys()
            .collect::<Vec<_>>().join(", "));
    }
```

**Update connection loop** (lines 36-43):
```rust
    for stream in listener.incoming() {
        let vhosts_clone = vhosts.clone();
        let stream = stream.unwrap();

        pool.execute(move || {
            handle_connection(stream, vhosts_clone);
        });
    }
```

### Step 4: Update `handle_connection()`

**New Signature**:
```rust
fn handle_connection(mut stream: TcpStream, vhosts: VirtualHosts) {
```

**Add Host header extraction** (after `clean_target`, line 58):
```rust
    let clean_target = clean_route(&http_request.target);

    // Select route table based on Host header
    let host = http_request.headers
        .get("host")
        .map(|h| h.as_str())
        .unwrap_or("");
    let routes = vhosts.get_routes(host);
```

**Update route lookup** (line 62):
```rust
    let (mut response, filename) = if routes.contains_key(&clean_target) {
        (HttpResponse::build(String::from("HTTP/1.1"), 200),
            routes.get(&clean_target).unwrap().to_str().unwrap())
    } else {
        (HttpResponse::build(String::from("HTTP/1.1"), 404),
            "pages/not_found.html")
    };
```

**Note**: The 404 page should also be per-virtual-host. Look for `not_found.html` in the virtual host's pages directory first, then fall back to the default:

```rust
    } else {
        // Try virtual host's not_found.html, then default
        let not_found = if let Some(host_routes) = vhosts.hosts.get(
            host.split(':').next().unwrap_or(host)
        ) {
            // Virtual host might have its own not_found page
            let host_dir = format!("sites/{}/pages/not_found.html",
                host.split(':').next().unwrap_or(host));
            if Path::new(&host_dir).exists() {
                host_dir
            } else {
                "pages/not_found.html".to_string()
            }
        } else {
            "pages/not_found.html".to_string()
        };
        (HttpResponse::build(String::from("HTTP/1.1"), 404), not_found)
    };
```

---

## Testing Strategy

### Unit Tests

```rust
#[cfg(test)]
mod vhost_tests {
    use super::*;

    #[test]
    fn get_routes_returns_default_for_unknown_host() {
        let vhosts = VirtualHosts {
            hosts: HashMap::new(),
            default_routes: {
                let mut m = HashMap::new();
                m.insert("/".to_string(), PathBuf::from("pages/index.html"));
                m
            },
        };
        let routes = vhosts.get_routes("unknown.com");
        assert!(routes.contains_key("/"));
    }

    #[test]
    fn get_routes_returns_host_routes_when_matched() {
        let mut hosts = HashMap::new();
        let mut host_routes = HashMap::new();
        host_routes.insert("/".to_string(), PathBuf::from("sites/example.com/pages/index.html"));
        hosts.insert("example.com".to_string(), host_routes);

        let vhosts = VirtualHosts {
            hosts,
            default_routes: HashMap::new(),
        };
        let routes = vhosts.get_routes("example.com");
        assert!(routes.contains_key("/"));
    }

    #[test]
    fn get_routes_strips_port_from_host() {
        let mut hosts = HashMap::new();
        let mut host_routes = HashMap::new();
        host_routes.insert("/".to_string(), PathBuf::from("test"));
        hosts.insert("example.com".to_string(), host_routes);

        let vhosts = VirtualHosts {
            hosts,
            default_routes: HashMap::new(),
        };
        let routes = vhosts.get_routes("example.com:7878");
        assert!(routes.contains_key("/"));
    }
}
```

### Integration Tests

```rust
fn test_default_host_serves_default_pages(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    Ok(())
}

fn test_virtual_host_routing(addr: &str) -> Result<(), String> {
    // Requires sites/testhost.local/pages/index.html to exist
    let mut stream = TcpStream::connect(addr)
        .map_err(|e| format!("connect: {e}"))?;
    let request = format!(
        "GET / HTTP/1.1\r\nHost: testhost.local\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(request.as_bytes())
        .map_err(|e| format!("write: {e}"))?;
    let resp = read_response(&mut stream)?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    Ok(())
}
```

### Manual Testing

```bash
# Setup virtual host
mkdir -p sites/example.local/pages
echo '<h1>Example.local</h1>' > sites/example.local/pages/index.html

cargo run &

# Default host
curl -i http://127.0.0.1:7878/
# Returns pages/index.html content

# Virtual host
curl -i -H "Host: example.local" http://127.0.0.1:7878/
# Returns sites/example.local/pages/index.html content
```

---

## Edge Cases & Handling

### 1. No `sites/` Directory
- **Behavior**: `build_virtual_hosts()` returns empty hosts map; all requests use default routes
- **Status**: Handled; identical to current behavior

### 2. Missing `Host` Header
- **Behavior**: Falls back to default routes
- **Status**: Handled by `.unwrap_or("")`

### 3. `Host` Header with Port
- **Example**: `Host: example.com:7878`
- **Behavior**: Port stripped before lookup
- **Status**: Handled by `.split(':').next()`

### 4. Case Sensitivity of Hostname
- **Behavior**: Hostnames are case-insensitive per RFC, but filesystem directories are case-sensitive on Linux
- **Mitigation**: Could lowercase both the `Host` header and directory names
- **Status**: Document as limitation; convention is lowercase hostnames

### 5. Virtual Host Has No `pages/` Subdirectory
- **Behavior**: Skipped during discovery
- **Status**: Handled by `host_pages.exists()` check

### 6. Cloning Overhead
- **Behavior**: `VirtualHosts` is cloned per connection (all route tables)
- **Mitigation**: Use `Arc<VirtualHosts>` to share without cloning
- **Status**: Should use `Arc` for production use; clone works for simplicity

### 7. Subdomain Wildcard Matching
- **Example**: `*.example.com` matching `blog.example.com`
- **Behavior**: Not supported in initial implementation
- **Future**: Could add wildcard matching with `starts_with()` logic
- **Status**: Exact match only

---

## Implementation Checklist

- [ ] Define `sites/{hostname}/pages/` directory convention
- [ ] Add `VirtualHosts` struct with `get_routes()` method
- [ ] Add `build_virtual_hosts()` function
- [ ] Update `main()` to build virtual hosts
- [ ] Update `handle_connection()` to select routes based on `Host` header
- [ ] Handle per-virtual-host 404 pages
- [ ] Add unit tests for `VirtualHosts::get_routes()` (including port stripping)
- [ ] Add integration tests with `Host` header
- [ ] Run `cargo test` and `cargo run --bin integration_test`
- [ ] Manual test with `curl -H "Host: ..."`

---

## Backward Compatibility

When no `sites/` directory exists, behavior is identical to current. The default `pages/` directory continues to serve as the fallback. All existing tests pass unchanged (they don't send a `Host` header that matches any virtual host).

---

## Future Enhancements

1. **Wildcard subdomains**: `*.example.com` matching
2. **Per-virtual-host configuration**: Custom 404 pages, error pages, extension filters per host
3. **SNI-based virtual hosting**: When TLS is implemented, select virtual host based on SNI
4. **Dynamic virtual host creation**: Add/remove virtual hosts without restart (combine with hot-reload)
