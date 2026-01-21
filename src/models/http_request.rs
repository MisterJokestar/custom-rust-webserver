use std::collections::HashMap;
use super::http_methods::HttpMethods;

pub struct HttpRequest {
    method: HttpMethods,
    target: String,
    version: String,
    headers: HashMap<String, String>,
    body: Option<String>
}

// TODO:
// Build a Request from a stream.
// Print out a Request. (With, and without body) (for debugging)
// Access headers
