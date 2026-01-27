use std::fmt;

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
        "GET" | "Get" | "get" => Some(HttpMethods::GET),
        "HEAD" | "Head" | "head" => Some(HttpMethods::HEAD),
        "POST" | "Post" | "post" => Some(HttpMethods::POST),
        "PUT" | "Put" | "put" => Some(HttpMethods::PUT),
        "DELETE" | "Delete" | "delete" => Some(HttpMethods::DELETE),
        "CONNECT" | "Connect" | "connect" => Some(HttpMethods::CONNECT),
        "OPTIONS" | "Options" | "options" => Some(HttpMethods::OPTIONS),
        "TRACE" | "Trace" | "trace" => Some(HttpMethods::TRACE),
        "PATCH" | "Patch" | "patch" => Some(HttpMethods::PATCH),
        _ => None,
    }
}
