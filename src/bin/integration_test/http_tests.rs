use std::{
    sync::{Arc, Mutex},
    thread,
};
use crate::test_framework::*;

fn test_root_route(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    assert_contains_or_err(&resp.body, "Hello!", "body")?;
    assert_contains_or_err(&resp.body, "Hi from Rust!", "body")?;
    Ok(())
}

fn test_index_css(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/index.css")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    assert_contains_or_err(&resp.body, "background", "body")?;
    Ok(())
}

fn test_howdy_route(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/howdy")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    assert_contains_or_err(&resp.body, "Howdy!", "body")?;
    assert_contains_or_err(&resp.body, "like a cowboy", "body")?;
    Ok(())
}

fn test_howdy_page_css(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/howdy/page.css")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    assert_contains_or_err(&resp.body, "background", "body")?;
    Ok(())
}

fn test_404_does_not_exist(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/does-not-exist")?;
    assert_eq_or_err(&resp.status_code, &404, "status")?;
    assert_contains_or_err(&resp.body, "Uh oh!", "body")?;
    Ok(())
}

fn test_404_deep_path(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/a/b/c/d")?;
    assert_eq_or_err(&resp.status_code, &404, "status")?;
    Ok(())
}

fn test_content_length_matches(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    let cl: usize = resp
        .headers
        .get("content-length")
        .ok_or("missing Content-Length header")?
        .parse()
        .map_err(|_| "Content-Length not a number".to_string())?;
    let actual_len = resp.body.len();
    assert_eq_or_err(&actual_len, &cl, "content-length vs body")?;
    Ok(())
}

fn test_trailing_slash(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "/howdy/")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    assert_contains_or_err(&resp.body, "Howdy!", "body")?;
    Ok(())
}

fn test_double_slash(addr: &str) -> Result<(), String> {
    let resp = send_request(addr, "GET", "//")?;
    assert_eq_or_err(&resp.status_code, &200, "status")?;
    assert_contains_or_err(&resp.body, "Hello!", "body")?;
    Ok(())
}

fn test_concurrent_requests(addr: &str) -> Result<(), String> {
    let addr = addr.to_string();
    let results: Arc<Mutex<Vec<Result<u16, String>>>> = Arc::new(Mutex::new(Vec::new()));
    let mut handles = Vec::new();

    for _ in 0..10 {
        let addr = addr.clone();
        let results = Arc::clone(&results);
        handles.push(thread::spawn(move || {
            let result = send_request(&addr, "GET", "/").map(|r| r.status_code);
            results.lock().unwrap().push(result);
        }));
    }

    for h in handles {
        h.join().map_err(|_| "thread panicked".to_string())?;
    }

    let results = results.lock().unwrap();
    for (i, r) in results.iter().enumerate() {
        match r {
            Ok(code) => assert_eq_or_err(code, &200, &format!("thread {i} status"))?,
            Err(e) => return Err(format!("thread {i} failed: {e}")),
        }
    }
    Ok(())
}

pub fn run_http_tests(addr: &str) -> Vec<TestResult> {
    let results = vec![
        run_test("root_route", || test_root_route(&addr)),
        run_test("index_css", || test_index_css(&addr)),
        run_test("howdy_route", || test_howdy_route(&addr)),
        run_test("howdy_page_css", || test_howdy_page_css(&addr)),
        run_test("404_does_not_exist", || test_404_does_not_exist(&addr)),
        run_test("404_deep_path", || test_404_deep_path(&addr)),
        run_test("content_length_matches", || {
            test_content_length_matches(&addr)
        }),
        run_test("trailing_slash", || test_trailing_slash(&addr)),
        run_test("double_slash", || test_double_slash(&addr)),
        run_test("concurrent_requests", || test_concurrent_requests(&addr)),
    ];

    return results;
}
