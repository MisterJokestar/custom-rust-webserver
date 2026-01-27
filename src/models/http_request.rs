use std::{
    collections::HashMap,
    fmt,
    io::{BufReader, prelude::*},
    net::TcpStream,
};
use super::http_methods::*;

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
            method: method,
            target: target,
            version: version,
            headers: headers,
            body: None
        }
    }

    pub fn build_from_stream(stream: &TcpStream) -> HttpRequest {
        let mut buf_reader = BufReader::new(stream);
        let mut line = String::new();
        let _ = buf_reader.read_line(&mut line);
        let mut iter = line.split_whitespace();
        let method = http_method_from_string(iter.next().unwrap()).unwrap();
        let target = iter.next().unwrap().to_string();
        let version = iter.next().unwrap().to_string();
        let mut request = HttpRequest::build(method, target, version);

        loop {
            let mut header_line = String::new();
            let len = buf_reader.read_line(&mut header_line).unwrap();
            let _ = header_line.truncate(len - 2);
            if len == 0 || header_line.is_empty() || header_line == "" {
                break;
            }
            let Some((title, value)) = header_line.split_once(":") else { break; };
            let _ = request.add_header(title.to_string(), value.to_string());
        }
        if let Some(content_length) = request.headers.get("Content-Length") {
            if let Ok(len) = content_length.trim().parse::<usize>() {
                if len > 0 {
                    let mut body_buf = vec![0u8; len];
                    buf_reader.read_exact(&mut body_buf).unwrap();
                    let _ = request.add_body(body_buf);
                }
            }
        }

        request
    }

    pub fn add_header(&mut self, title: String, value: String) -> &mut HttpRequest {
        self.headers.insert(title, value);
        self
    }

    pub fn try_get_header(self, title: String) -> Option<String> {
        self.headers.get(&title).cloned()
    }

    pub fn add_body(&mut self, body: Vec<u8>) -> &mut HttpRequest {
        self.body = Some(body); // once told me...
        self
    }

    pub fn try_get_body(self) -> Option<Vec<u8>> {
        self.body
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
