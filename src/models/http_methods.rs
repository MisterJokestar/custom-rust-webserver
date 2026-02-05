use std::fmt;

#[derive(Debug, PartialEq)]
pub enum HttpMethods {
    GET,
    HEAD,
    POST,
    PUT,
    DELETE,
    CONNECT,
    OPTIONS,
    TRACE,
    PATCH,
}

impl fmt::Display for HttpMethods {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let method = match self {
            HttpMethods::GET => "GET",
            HttpMethods::HEAD => "HEAD",
            HttpMethods::POST => "POST",
            HttpMethods::PUT => "PUT",
            HttpMethods::DELETE => "DELETE",
            HttpMethods::CONNECT => "CONNECT",
            HttpMethods::OPTIONS => "OPTIONS",
            HttpMethods::TRACE => "TRACE",
            HttpMethods::PATCH => "PATCH",
        };
        write!(f, "{}", method)
    }
}

pub fn http_method_from_string(method: &str) -> Option<HttpMethods> {
    match method {
        "GET" => Some(HttpMethods::GET),
        "HEAD" => Some(HttpMethods::HEAD),
        "POST" => Some(HttpMethods::POST),
        "PUT" => Some(HttpMethods::PUT),
        "DELETE" => Some(HttpMethods::DELETE),
        "CONNECT" => Some(HttpMethods::CONNECT),
        "OPTIONS" => Some(HttpMethods::OPTIONS),
        "TRACE" => Some(HttpMethods::TRACE),
        "PATCH" => Some(HttpMethods::PATCH),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_uppercase_methods() {
        assert!(http_method_from_string("GET").is_some());
        assert!(http_method_from_string("POST").is_some());
        assert!(http_method_from_string("DELETE").is_some());
    }

    #[test]
    fn rejects_non_uppercase_methods() {
        assert_eq!(http_method_from_string("get"), None);
        assert_eq!(http_method_from_string("Get"), None);
        assert_eq!(http_method_from_string("post"), None);
        assert_eq!(http_method_from_string("Post"), None);
    }
}
