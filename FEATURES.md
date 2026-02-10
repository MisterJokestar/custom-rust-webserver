# rcomm Feature Roadmap

Features to implement for the rcomm web server, organized by category.

Each feature is rated on two metrics (1–10 scale):
- **Complexity** — how much effort/difficulty to implement (1 = trivial, 10 = major undertaking)
- **Necessity** — how important this is for a functional, correct web server (1 = nice-to-have, 10 = essential)

---

## HTTP Protocol Compliance

| Done | Feature | Complexity | Necessity |
|------|---------|:----------:|:---------:|
| [ ] | Set `Content-Type` response header based on file extension (e.g. `text/html`, `text/css`, `application/javascript`) | 2 | 10 |
| [ ] | Add `Date` response header with RFC 7231 formatted timestamp (required by HTTP/1.1) | 3 | 8 |
| [ ] | Add `Server` response header identifying the server software | 1 | 3 |
| [ ] | Support HTTP/1.1 persistent connections (`Connection: keep-alive`) — read multiple requests per TCP connection | 6 | 7 |
| [ ] | Handle `HEAD` requests correctly by returning headers only (no body) | 2 | 8 |
| [ ] | Respond to `OPTIONS` requests with an `Allow` header listing supported methods | 2 | 5 |
| [ ] | Return `405 Method Not Allowed` for unsupported methods (POST, PUT, DELETE, etc.) on static routes | 2 | 7 |
| [ ] | Parse and strip query strings (`?key=value`) from the request URI before route lookup | 2 | 8 |
| [ ] | Parse and strip URI fragments (`#section`) from the request URI | 1 | 4 |
| [ ] | Percent-decode the request URI (e.g. `%20` to space, `%2F` to `/`) | 3 | 7 |
| [ ] | Support `Transfer-Encoding: chunked` for reading chunked request bodies | 6 | 4 |
| [ ] | Support chunked transfer encoding for streaming responses | 6 | 3 |
| [ ] | Send `100 Continue` interim response when client sends `Expect: 100-continue` | 3 | 4 |
| [ ] | Return `400 Bad Request` with a descriptive body for malformed requests (currently returns empty body) | 2 | 7 |
| [ ] | Return `414 URI Too Long` for excessively long request URIs | 2 | 5 |
| [ ] | Return `431 Request Header Fields Too Large` instead of generic parse error for oversized headers | 2 | 4 |
| [ ] | Handle `TRACE` method by echoing back the received request | 2 | 2 |

## Caching & Conditional Requests

| Done | Feature | Complexity | Necessity |
|------|---------|:----------:|:---------:|
| [ ] | Send `Last-Modified` header with file modification timestamp on static file responses | 3 | 7 |
| [ ] | Generate and send `ETag` header based on file content hash | 4 | 6 |
| [ ] | Handle `If-Modified-Since` request header — return `304 Not Modified` when file hasn't changed | 3 | 7 |
| [ ] | Handle `If-None-Match` request header — return `304 Not Modified` when ETag matches | 3 | 6 |
| [ ] | Support `Cache-Control` response header with configurable max-age for static assets | 2 | 6 |
| [ ] | Support `If-Match` / `If-Unmodified-Since` precondition headers | 3 | 3 |

## Range Requests

| Done | Feature | Complexity | Necessity |
|------|---------|:----------:|:---------:|
| [ ] | Support `Range` request header for partial content delivery (e.g. `Range: bytes=0-999`) | 5 | 4 |
| [ ] | Return `206 Partial Content` with `Content-Range` header for valid range requests | 4 | 4 |
| [ ] | Return `416 Range Not Satisfiable` for out-of-bounds ranges | 2 | 3 |
| [ ] | Support multi-part range requests with `multipart/byteranges` responses | 7 | 2 |

## Security

| Done | Feature | Complexity | Necessity |
|------|---------|:----------:|:---------:|
| [ ] | Add configurable maximum request body size to prevent memory exhaustion | 2 | 9 |
| [ ] | Add configurable maximum request URI length | 2 | 6 |
| [ ] | Add configurable maximum number of request headers | 2 | 5 |
| [ ] | Add connection rate limiting per IP address | 5 | 6 |
| [ ] | Add configurable maximum concurrent connection limit | 4 | 7 |
| [ ] | Replace all `unwrap()` calls with proper error handling to prevent worker thread panics | 4 | 10 |
| [ ] | Validate that resolved file paths stay within the `pages/` directory after canonicalization | 3 | 9 |
| [ ] | Sanitize response headers to prevent header injection via CRLF sequences | 3 | 7 |
| [ ] | Add request timeout — close connections that stall during header or body transmission | 4 | 8 |
| [ ] | Add TLS/HTTPS support (either native `rustls` integration or built-in) | 8 | 6 |
| [ ] | Add `X-Content-Type-Options: nosniff` response header | 1 | 7 |
| [ ] | Add `X-Frame-Options` response header to prevent clickjacking | 1 | 6 |
| [ ] | Add `Strict-Transport-Security` (HSTS) response header when serving over TLS | 1 | 5 |
| [ ] | Add `Content-Security-Policy` response header support | 2 | 5 |
| [ ] | Reject requests with null bytes in the URI | 1 | 8 |
| [ ] | Limit the number of requests per persistent connection to prevent abuse | 2 | 4 |

## Static File Serving

| Done | Feature | Complexity | Necessity |
|------|---------|:----------:|:---------:|
| [ ] | Serve binary files (images, fonts, PDFs, etc.) — replace `read_to_string()` with `read()` for `Vec<u8>` bodies | 3 | 9 |
| [ ] | Build a MIME type mapping for common file extensions (`.png`, `.jpg`, `.gif`, `.svg`, `.woff2`, `.pdf`, `.json`, etc.) | 2 | 9 |
| [ ] | Support serving files with no extension by defaulting to `application/octet-stream` | 1 | 4 |
| [ ] | Add directory index fallback — serve `index.html` when a directory path is requested without a matching `page.html` | 3 | 5 |
| [ ] | Support configurable document root directory (not hardcoded to `pages/`) | 2 | 6 |
| [ ] | Add `.gz` / `.br` pre-compressed file serving — serve `file.js.gz` when `Accept-Encoding: gzip` is present and the compressed file exists | 4 | 3 |
| [ ] | Support `Accept-Encoding` and respond with `Content-Encoding: gzip` or `deflate` for on-the-fly compression | 7 | 4 |
| [ ] | Add favicon.ico support (serve a default or custom favicon) | 1 | 3 |
| [ ] | Support serving dotfiles with opt-in configuration (hidden by default) | 2 | 3 |
| [ ] | Add configurable file extension whitelist/blacklist for served files | 3 | 4 |

## Routing

| Done | Feature | Complexity | Necessity |
|------|---------|:----------:|:---------:|
| [ ] | Support route matching for file types beyond `.html`, `.css`, `.js` (e.g. `.json`, `.xml`, `.svg`, `.wasm`) | 2 | 7 |
| [ ] | Add trailing slash redirect — `GET /howdy/` returns `301` to `/howdy` (or vice versa) instead of silently handling both | 3 | 4 |
| [ ] | Add configurable custom 404 page path (not hardcoded to `pages/not_found.html`) | 2 | 4 |
| [ ] | Add configurable custom error pages for other status codes (500, 403, etc.) | 3 | 4 |
| [ ] | Support route aliases / rewrite rules (e.g. `/old-path` -> `/new-path`) | 5 | 3 |
| [ ] | Add redirect support (301, 302, 307, 308) via configuration | 4 | 4 |
| [ ] | Hot-reload routes when files in `pages/` are added, modified, or deleted (file system watching) | 7 | 3 |
| [ ] | Support virtual host routing (different route trees per `Host` header value) | 6 | 2 |

## Logging & Observability

| Done | Feature | Complexity | Necessity |
|------|---------|:----------:|:---------:|
| [ ] | Add structured access logging in Common Log Format (CLF): `host ident authuser [date] "request" status bytes` | 4 | 8 |
| [ ] | Add configurable log levels (error, warn, info, debug, trace) | 4 | 7 |
| [ ] | Log request duration (time from connection accept to response sent) | 2 | 6 |
| [ ] | Add configurable log output destination (stdout, stderr, file, or both) | 3 | 5 |
| [ ] | Log client IP address and port on each request | 1 | 7 |
| [ ] | Log error details when file reads or connection handling fails | 2 | 8 |
| [ ] | Add request ID generation and logging for correlation | 2 | 4 |
| [ ] | Add health check endpoint (`/health` or `/healthz`) that returns server status | 2 | 5 |

## Configuration

| Done | Feature | Complexity | Necessity |
|------|---------|:----------:|:---------:|
| [ ] | Make thread pool size configurable via `RCOMM_THREADS` environment variable | 1 | 7 |
| [ ] | Add a configuration file format (TOML or JSON) as an alternative to environment variables | 5 | 4 |
| [ ] | Add command-line argument parsing for port, address, document root, and thread count | 4 | 6 |
| [ ] | Support `--help` and `--version` command-line flags | 2 | 5 |
| [ ] | Add configurable request timeouts (header read, body read, keep-alive idle) | 4 | 6 |
| [ ] | Add configurable response buffer sizes | 3 | 3 |

## Connection Handling

| Done | Feature | Complexity | Necessity |
|------|---------|:----------:|:---------:|
| [ ] | Share the route map via `Arc` instead of cloning the `HashMap` per connection | 2 | 8 |
| [ ] | Implement HTTP/1.1 pipelining — process multiple requests on a single connection without waiting for each response | 7 | 3 |
| [ ] | Add TCP keepalive socket options | 2 | 4 |
| [ ] | Set `TCP_NODELAY` for lower-latency responses | 1 | 5 |
| [ ] | Add configurable listen backlog size for `TcpListener` | 2 | 3 |
| [ ] | Support binding to multiple addresses/ports simultaneously | 4 | 2 |
| [ ] | Support Unix domain socket listening | 4 | 2 |

## Thread Pool & Concurrency

| Done | Feature | Complexity | Necessity |
|------|---------|:----------:|:---------:|
| [ ] | Fix the mutex-blocking-recv pattern — only one worker can wait for a job at a time; consider per-worker channels or a work-stealing scheduler | 7 | 6 |
| [ ] | Add thread pool size auto-tuning based on available CPU cores | 2 | 4 |
| [ ] | Add task queue depth monitoring and backpressure (reject connections when queue is full with `503 Service Unavailable`) | 5 | 5 |
| [ ] | Name worker threads for easier debugging (e.g. `rcomm-worker-0`) | 1 | 4 |
| [ ] | Add worker thread panic recovery — restart crashed workers instead of losing pool capacity | 5 | 8 |
| [ ] | Add graceful shutdown via signal handling (SIGTERM, SIGINT) with in-flight request draining | 5 | 7 |

## Error Handling

| Done | Feature | Complexity | Necessity |
|------|---------|:----------:|:---------:|
| [ ] | Replace `unwrap()` in `handle_connection()` file read with fallback to `500 Internal Server Error` | 2 | 10 |
| [ ] | Replace `unwrap()` in `build_routes()` extension check with a skip-and-log for extensionless files | 1 | 7 |
| [ ] | Return structured error responses (HTML or JSON) for 4xx and 5xx errors | 3 | 5 |
| [ ] | Add connection-level error handling so a single bad request doesn't affect other connections | 3 | 8 |
| [ ] | Log and recover from TCP write errors (client disconnect mid-response) instead of panicking | 2 | 8 |

## Testing

| Done | Feature | Complexity | Necessity |
|------|---------|:----------:|:---------:|
| [ ] | Add integration tests for `400 Bad Request` responses (malformed request lines) | 2 | 7 |
| [ ] | Add integration tests for path traversal attempts (`GET /../etc/passwd`) | 2 | 9 |
| [ ] | Add integration tests for non-GET methods (POST, PUT, DELETE returning 405) | 2 | 6 |
| [ ] | Add integration tests for binary file serving | 2 | 6 |
| [ ] | Add integration tests for `HEAD` requests (response has headers but no body) | 2 | 6 |
| [ ] | Add integration tests for large file responses | 3 | 5 |
| [ ] | Add integration tests for concurrent connection limits | 3 | 5 |
| [ ] | Add integration tests for request timeout behavior | 4 | 5 |
| [ ] | Add integration tests for `Connection: keep-alive` / persistent connections | 3 | 5 |
| [ ] | Add benchmark tests for requests-per-second throughput | 5 | 3 |
| [ ] | Fix TOCTOU race in `pick_free_port()` — bind the listener and pass it to the server instead of passing a port number | 3 | 4 |
| [ ] | Add unit tests for `get_status_phrase()` covering all status codes | 1 | 4 |
| [ ] | Add unit tests for `clean_route()` with adversarial inputs | 2 | 7 |

## Developer Experience

| Done | Feature | Complexity | Necessity |
|------|---------|:----------:|:---------:|
| [ ] | Add a `--watch` mode that auto-restarts the server when source files change | 6 | 3 |
| [ ] | Add colored terminal output for request log lines (green for 2xx, yellow for 3xx, red for 4xx/5xx) | 3 | 2 |
| [ ] | Print the full URL (including address and port) on startup for easy click-to-open | 1 | 5 |
| [ ] | Add a `--verbose` flag for debug-level output | 2 | 5 |
| [ ] | Fix the hardcoded port mismatch in HTML pages (`localhost:7879` vs. default port `7878`) | 1 | 8 |
| [ ] | Support relative CSS/JS paths in HTML pages instead of hardcoded absolute URLs | 1 | 7 |
