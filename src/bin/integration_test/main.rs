pub mod test_framework;
pub mod http_tests;

use std::time::Duration;
use crate::{
    test_framework::*,
    http_tests::run_http_tests,
};

fn main() {
    let port = pick_free_port();
    let addr = format!("127.0.0.1:{port}");

    println!("Starting server on {addr}...");
    let mut server = start_server(port);

    if let Err(e) = wait_for_server(&addr, Duration::from_secs(5)) {
        eprintln!("ERROR: {e}");
        let _ = server.kill();
        std::process::exit(1);
    }
    println!("Server is ready.\n");

    let mut results: Vec<TestResult> = Vec::new();
    results.extend(run_http_tests(&addr));

    let mut passed = 0;
    let mut failed = 0;
    for r in &results {
        if r.passed {
            println!("[PASS] {} -- {}", r.name, r.detail);
            passed += 1;
        } else {
            println!("[FAIL] {} -- {}", r.name, r.detail);
            failed += 1;
        }
    }

    let total = passed + failed;
    println!("\n{passed} passed, {failed} failed, {total} total");

    let _ = server.kill();
    let _ = server.wait();

    if failed > 0 {
        std::process::exit(1);
    }
}
