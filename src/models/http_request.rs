use std::{
    collections::HashMap,
    fmt,
    io::{BufReader, prelude::*},
    net::TcpStream,
};
use super::http_methods::*;

const MAX_HEADER_LINE_LEN: usize = 8192;

#[derive(Debug)]
pub enum HttpParseError {
    HeaderTooLong,
    MissingHostHeader,
    MalformedRequestLine,
    IoError(std::io::Error),
}

impl fmt::Display for HttpParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HttpParseError::HeaderTooLong => write!(f, "Header line exceeds maximum length"),
            HttpParseError::MissingHostHeader => write!(f, "Missing required Host header"),
            HttpParseError::MalformedRequestLine => write!(f, "Malformed request line"),
            HttpParseError::IoError(e) => write!(f, "IO error: {e}"),
        }
    }
}

#[derive(Debug)]
pub struct HttpRequest {
    pub method: HttpMethods,
    pub target: String,
    pub version: String,
    headers: HashMap<String, String>,
    body: Option<Vec<u8>>
}

impl HttpRequest {
    pub fn build(method: HttpMethods, target: String, version: String) -> HttpRequest {
        let headers = HashMap::<String, String>::new();
        HttpRequest {
            method,
            target,
            version,
            headers,
            body: None
        }
    }

    pub fn build_from_stream(stream: &TcpStream) -> Result<HttpRequest, HttpParseError> {
        let mut buf_reader = BufReader::new(stream);

        // Parse request line
        let mut line = String::new();
        buf_reader.read_line(&mut line).map_err(HttpParseError::IoError)?;
        let line = line.trim_end_matches(|c| c == '\r' || c == '\n');

        if line.len() > MAX_HEADER_LINE_LEN {
            return Err(HttpParseError::HeaderTooLong);
        }

        let mut iter = line.split_whitespace();
        let method_str = iter.next().ok_or(HttpParseError::MalformedRequestLine)?;
        let method = http_method_from_string(method_str).ok_or(HttpParseError::MalformedRequestLine)?;
        let target = iter.next().ok_or(HttpParseError::MalformedRequestLine)?.to_string();
        let version = iter.next().ok_or(HttpParseError::MalformedRequestLine)?.to_string();
        let mut request = HttpRequest::build(method, target, version);

        // Parse headers
        loop {
            let mut header_line = String::new();
            let len = buf_reader.read_line(&mut header_line).map_err(HttpParseError::IoError)?;
            if len == 0 {
                break;
            }
            let header_line = header_line.trim_end_matches(|c| c == '\r' || c == '\n');

            if header_line.len() > MAX_HEADER_LINE_LEN {
                return Err(HttpParseError::HeaderTooLong);
            }

            if header_line.is_empty() {
                break;
            }
            let Some((title, value)) = header_line.split_once(":") else { break; };
            request.add_header(title.to_string(), value.trim().to_string());
        }

        // Validate Host header for HTTP/1.1
        if request.version == "HTTP/1.1" && !request.headers.contains_key("host") {
            return Err(HttpParseError::MissingHostHeader);
        }

        // Parse body if Content-Length is present
        if let Some(content_length) = request.headers.get("content-length") {
            if let Ok(len) = content_length.parse::<usize>() {
                if len > 0 {
                    let mut body_buf = vec![0u8; len];
                    buf_reader.read_exact(&mut body_buf).map_err(HttpParseError::IoError)?;
                    request.add_body(body_buf);
                }
            }
        }

        Ok(request)
    }

    pub fn add_header(&mut self, title: String, value: String) -> &mut HttpRequest {
        self.headers.insert(title.to_lowercase(), value);
        self
    }

    pub fn try_get_header(&self, title: String) -> Option<String> {
        self.headers.get(&title.to_lowercase()).cloned()
    }

    pub fn add_body(&mut self, body: Vec<u8>) -> &mut HttpRequest {
        self.body = Some(body);
        self
    }

    pub fn try_get_body(&self) -> Option<Vec<u8>> {
        self.body.clone()
    }

    pub fn as_bytes(&self) -> Vec<u8> {
        if let Some(body) = &self.body {
            let mut bytes = format!("{self}").as_bytes().to_vec();
            bytes.append(&mut body.clone());
            return bytes
        } else {
            return format!("{self}").as_bytes().to_vec();
        }
    }
}

// Will not display body.
impl fmt::Display for HttpRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let _ = match write!(f, "{} {} {}\r\n", self.method, self.target, self.version) {
            Ok(result) => result,
            Err(e) => return Err(e),
        };
        for (title, value) in &self.headers {
            let _ = match write!(f, "{}: {}\r\n", title, value) {
                Ok(result) => result,
                Err(e) => return Err(e),
            };
        }
        write!(f, "\r\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::http_methods::HttpMethods;

    #[test]
    fn build_creates_request_with_correct_fields() {
        let req = HttpRequest::build(
            HttpMethods::GET,
            "/index".to_string(),
            "HTTP/1.1".to_string(),
        );
        assert_eq!(req.method.to_string(), "GET");
        assert_eq!(req.target, "/index");
        assert_eq!(req.version, "HTTP/1.1");
    }

    #[test]
    fn add_header_stores_header() {
        let mut req = HttpRequest::build(
            HttpMethods::GET,
            "/".to_string(),
            "HTTP/1.1".to_string(),
        );
        req.add_header("Content-Type".to_string(), "text/html".to_string());
        let val = req.try_get_header("Content-Type".to_string());
        assert_eq!(val, Some("text/html".to_string()));
    }

    #[test]
    fn try_get_header_returns_none_for_missing() {
        let req = HttpRequest::build(
            HttpMethods::GET,
            "/".to_string(),
            "HTTP/1.1".to_string(),
        );
        assert_eq!(req.try_get_header("Missing".to_string()), None);
    }

    #[test]
    fn add_body_and_try_get_body() {
        let mut req = HttpRequest::build(
            HttpMethods::POST,
            "/submit".to_string(),
            "HTTP/1.1".to_string(),
        );
        let body = b"hello world".to_vec();
        req.add_body(body.clone());
        assert_eq!(req.try_get_body(), Some(body));
    }

    #[test]
    fn try_get_body_returns_none_when_empty() {
        let req = HttpRequest::build(
            HttpMethods::GET,
            "/".to_string(),
            "HTTP/1.1".to_string(),
        );
        assert_eq!(req.try_get_body(), None);
    }

    #[test]
    fn display_formats_request_line_and_headers() {
        let mut req = HttpRequest::build(
            HttpMethods::GET,
            "/path".to_string(),
            "HTTP/1.1".to_string(),
        );
        req.add_header("Host".to_string(), "localhost".to_string());
        let output = format!("{req}");
        assert!(output.starts_with("GET /path HTTP/1.1\r\n"));
        assert!(output.contains("host: localhost\r\n"));
        assert!(output.ends_with("\r\n"));
    }

    #[test]
    fn as_bytes_without_body() {
        let req = HttpRequest::build(
            HttpMethods::GET,
            "/".to_string(),
            "HTTP/1.1".to_string(),
        );
        let bytes = req.as_bytes();
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.starts_with("GET / HTTP/1.1\r\n"));
    }

    #[test]
    fn as_bytes_with_body() {
        let mut req = HttpRequest::build(
            HttpMethods::POST,
            "/data".to_string(),
            "HTTP/1.1".to_string(),
        );
        req.add_body(b"body content".to_vec());
        let bytes = req.as_bytes();
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.starts_with("POST /data HTTP/1.1\r\n"));
        assert!(text.ends_with("body content"));
    }

    #[test]
    fn build_from_stream_parses_get_request() {
        use std::io::Write;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let handle = std::thread::spawn(move || {
            let mut client = std::net::TcpStream::connect(addr).unwrap();
            client
                .write_all(b"GET /hello HTTP/1.1\r\nHost: localhost\r\n\r\n")
                .unwrap();
            client.shutdown(std::net::Shutdown::Write).unwrap();
        });

        let (stream, _) = listener.accept().unwrap();
        let req = HttpRequest::build_from_stream(&stream).unwrap();

        assert_eq!(req.method.to_string(), "GET");
        assert_eq!(req.target, "/hello");
        assert_eq!(req.version, "HTTP/1.1");
        handle.join().unwrap();
    }

    #[test]
    fn build_from_stream_parses_post_with_body() {
        use std::io::Write;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let body = b"key=value";
        let handle = std::thread::spawn(move || {
            let mut client = std::net::TcpStream::connect(addr).unwrap();
            let msg = format!(
                "POST /form HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                std::str::from_utf8(body).unwrap()
            );
            client.write_all(msg.as_bytes()).unwrap();
            client.shutdown(std::net::Shutdown::Write).unwrap();
        });

        let (stream, _) = listener.accept().unwrap();
        let req = HttpRequest::build_from_stream(&stream).unwrap();

        assert_eq!(req.method.to_string(), "POST");
        assert_eq!(req.target, "/form");
        assert_eq!(req.try_get_body(), Some(b"key=value".to_vec()));
        handle.join().unwrap();
    }

    #[test]
    fn build_from_stream_trims_header_ows() {
        use std::io::Write;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let handle = std::thread::spawn(move || {
            let mut client = std::net::TcpStream::connect(addr).unwrap();
            client
                .write_all(b"GET / HTTP/1.1\r\nHost:   localhost   \r\nX-Custom:  hello  \r\n\r\n")
                .unwrap();
            client.shutdown(std::net::Shutdown::Write).unwrap();
        });

        let (stream, _) = listener.accept().unwrap();
        let req = HttpRequest::build_from_stream(&stream).unwrap();

        assert_eq!(req.try_get_header("host".to_string()), Some("localhost".to_string()));
        assert_eq!(req.try_get_header("x-custom".to_string()), Some("hello".to_string()));
        handle.join().unwrap();
    }

    #[test]
    fn build_from_stream_handles_bare_lf() {
        use std::io::Write;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let handle = std::thread::spawn(move || {
            let mut client = std::net::TcpStream::connect(addr).unwrap();
            // Use bare \n instead of \r\n
            client
                .write_all(b"GET / HTTP/1.1\nHost: localhost\n\n")
                .unwrap();
            client.shutdown(std::net::Shutdown::Write).unwrap();
        });

        let (stream, _) = listener.accept().unwrap();
        let req = HttpRequest::build_from_stream(&stream).unwrap();

        assert_eq!(req.method.to_string(), "GET");
        assert_eq!(req.target, "/");
        assert_eq!(req.try_get_header("host".to_string()), Some("localhost".to_string()));
        handle.join().unwrap();
    }

    #[test]
    fn build_from_stream_rejects_oversized_header() {
        use std::io::Write;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let handle = std::thread::spawn(move || {
            let mut client = std::net::TcpStream::connect(addr).unwrap();
            let long_value = "x".repeat(MAX_HEADER_LINE_LEN + 1);
            let msg = format!("GET / HTTP/1.1\r\nHost: localhost\r\nX-Big: {long_value}\r\n\r\n");
            client.write_all(msg.as_bytes()).unwrap();
            client.shutdown(std::net::Shutdown::Write).unwrap();
        });

        let (stream, _) = listener.accept().unwrap();
        let result = HttpRequest::build_from_stream(&stream);

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), HttpParseError::HeaderTooLong));
        handle.join().unwrap();
    }

    #[test]
    fn build_from_stream_rejects_http11_without_host() {
        use std::io::Write;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let handle = std::thread::spawn(move || {
            let mut client = std::net::TcpStream::connect(addr).unwrap();
            client
                .write_all(b"GET / HTTP/1.1\r\n\r\n")
                .unwrap();
            client.shutdown(std::net::Shutdown::Write).unwrap();
        });

        let (stream, _) = listener.accept().unwrap();
        let result = HttpRequest::build_from_stream(&stream);

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), HttpParseError::MissingHostHeader));
        handle.join().unwrap();
    }
}
