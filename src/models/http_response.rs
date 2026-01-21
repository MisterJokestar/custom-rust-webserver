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
    body: Option<String>
}

// TODO:
// Build a response.
// Format Response string to send.
// Print out a response. (With, and without body) (for debugging)
// Access headers
impl HttpResponse {
    pub fn build(version: String, code: u16) -> HttpResponse {
        let headers = HashMap::<String, String>::new();
        let phrase = get_status_phrase(code);
        HttpResponse {
            version: version,
            status_code: code,
            status_phrase: phrase,
            headers: headers,
            body: None,
        }
    }

    pub fn add_header(mut self, title: String, value: String) -> HttpResponse {
        self.headers.insert(title, value);
        self
    }

    pub fn try_get_header(self, title: String) -> Option<String> {
        self.headers.get(&title).cloned()
    }

    pub fn add_body(mut self, body: String) -> HttpResponse {
        self.body = Some(body); // once told me...
        self
    }

    pub fn try_get_body(self) -> Option<String> {
        self.body
    }

    pub fn as_bytes(&self) -> Vec<u8> {
        if let Some(body) = &self.body {
            return format!("{self}{body}").as_bytes().to_vec();
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
mod response_model_tests {
    use super::*;

    #[test]
    fn test_print() {
        let response = HttpResponse::build(String::from("HTTP/1.1"), 200)
            .add_header(String::from("Test_Header"), String::from("Test_Value"));
        let string = String::from("HTTP/1.1 200 OK\r\nTest_Header: Test_Value\r\n\r\n");
        assert_eq!(string, format!("{response}"));
    }
}
