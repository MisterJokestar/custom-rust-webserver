use std::{
    collections::HashMap,
    fmt,
};
use super::http_status_codes::get_status_phrase;

pub struct HttpResponse {
    version: String,
    status_code: u16,
    status_phrase: String,
    headers: HashMap<String, String>,
    body: Option<Vec<u8>>
}

impl HttpResponse {
    pub fn build(version: String, code: u16) -> HttpResponse {
        let headers = HashMap::<String, String>::new();
        let phrase = get_status_phrase(code);
        HttpResponse {
            version,
            status_code: code,
            status_phrase: phrase,
            headers,
            body: None,
        }
    }

    pub fn add_header(&mut self, title: String, value: String) -> &mut HttpResponse {
        self.headers.insert(title.to_lowercase(), value);
        self
    }

    pub fn try_get_header(&self, title: String) -> Option<String> {
        self.headers.get(&title.to_lowercase()).cloned()
    }

    pub fn add_body(&mut self, body: Vec<u8>) -> &mut HttpResponse {
        let len = body.len();
        self.body = Some(body);
        self.headers.insert("content-length".to_string(), len.to_string());
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
impl fmt::Display for HttpResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let _ = match write!(f, "{} {} {}\r\n", self.version, self.status_code, self.status_phrase) {
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

    #[test]
    fn build_creates_response_with_correct_fields() {
        let resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
        let output = format!("{resp}");
        assert!(output.starts_with("HTTP/1.1 200 OK\r\n"));
    }

    #[test]
    fn build_sets_status_phrase_for_404() {
        let resp = HttpResponse::build("HTTP/1.1".to_string(), 404);
        let output = format!("{resp}");
        assert!(output.starts_with("HTTP/1.1 404 Not Found\r\n"));
    }

    #[test]
    fn build_sets_status_phrase_for_500() {
        let resp = HttpResponse::build("HTTP/1.1".to_string(), 500);
        let output = format!("{resp}");
        assert!(output.starts_with("HTTP/1.1 500 Internal Server Error\r\n"));
    }

    #[test]
    fn add_header_stores_header() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
        resp.add_header("Content-Type".to_string(), "text/html".to_string());
        let val = resp.try_get_header("Content-Type".to_string());
        assert_eq!(val, Some("text/html".to_string()));
    }

    #[test]
    fn try_get_header_returns_none_for_missing() {
        let resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
        assert_eq!(resp.try_get_header("Missing".to_string()), None);
    }

    #[test]
    fn add_body_and_try_get_body() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
        let body = b"<h1>Hello</h1>".to_vec();
        resp.add_body(body.clone());
        assert_eq!(resp.try_get_body(), Some(body));
    }

    #[test]
    fn try_get_body_returns_none_when_empty() {
        let resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
        assert_eq!(resp.try_get_body(), None);
    }

    #[test]
    fn display_formats_status_line_and_headers() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
        resp.add_header("Server".to_string(), "rcomm".to_string());
        let output = format!("{resp}");
        assert!(output.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(output.contains("server: rcomm\r\n"));
        assert!(output.ends_with("\r\n"));
    }

    #[test]
    fn as_bytes_without_body() {
        let resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
        let bytes = resp.as_bytes();
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(text.ends_with("\r\n"));
    }

    #[test]
    fn as_bytes_with_body() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
        resp.add_body(b"body here".to_vec());
        let bytes = resp.as_bytes();
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(text.ends_with("body here"));
    }

    #[test]
    fn add_header_chaining() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
        resp.add_header("A".to_string(), "1".to_string())
            .add_header("B".to_string(), "2".to_string());
        let output = format!("{resp}");
        assert!(output.contains("a: 1\r\n"));
        assert!(output.contains("b: 2\r\n"));
    }

    #[test]
    fn unknown_status_code_has_empty_phrase() {
        let resp = HttpResponse::build("HTTP/1.1".to_string(), 999);
        let output = format!("{resp}");
        assert!(output.starts_with("HTTP/1.1 999 \r\n"));
    }

    #[test]
    fn add_body_auto_sets_content_length() {
        let mut resp = HttpResponse::build("HTTP/1.1".to_string(), 200);
        resp.add_body(b"hello world".to_vec());
        assert_eq!(resp.try_get_header("content-length".to_string()), Some("11".to_string()));
    }
}
