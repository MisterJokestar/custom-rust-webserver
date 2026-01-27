use std::{
    collections::HashMap,
    fs,
    io::prelude::*,
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
};
use rcomm::ThreadPool;
use rcomm::models::{
    http_response::HttpResponse,
    http_request::HttpRequest,
};

const PORT: &str = "7879";
const ADDRESS: &str = "127.0.0.1";

fn main() {
    let full_address = format!("{ADDRESS}:{PORT}");
    let listener = TcpListener::bind(full_address).unwrap();

    let pool = ThreadPool::new(4);

    let path = Path::new("./pages");
    let routes = build_routes(String::from(""), path);

    println!("Routes:\n{routes:#?}\n\n");
    println!("Listening on {ADDRESS}:{PORT}");

    for stream in listener.incoming() {
        let routes_clone = routes.clone();
        let stream = stream.unwrap();

        pool.execute(move || {
            handle_connection(stream, routes_clone);
        });
    }
}

fn handle_connection(mut stream: TcpStream, routes: HashMap<String, PathBuf>) {
    let http_request = HttpRequest::build_from_stream(&stream);
    let clean_target = clean_route(&http_request.target);

    println!("Request: {http_request}");

    let (mut response, filename) = if routes.contains_key(&clean_target) {
        (HttpResponse::build(String::from("HTTP/1.1"), 200),
            routes.get(&clean_target).unwrap().to_str().unwrap())
    } else {
        (HttpResponse::build(String::from("HTTP/1.1"), 404),
            "pages/not_found.html")
    };

    let contents = fs::read_to_string(filename).unwrap();
    let length = contents.len();

    let _ = response.add_header(String::from("Content-Length"), format!("{length}"))
        .add_body(contents.into());

    println!("Response: {response}");
    stream.write_all(&response.as_bytes()).unwrap();
}

fn clean_route(route: &String) -> String {
    let mut clean_route = String::from("");
    for part in route.split("/").collect::<Vec<_>>() {
        if part == "" || part == "." || part == ".." {
            continue;
        }
        clean_route.push_str(format!("/{part}").as_str());
    }
    if clean_route == "" {
        clean_route = String::from("/");
    }
    clean_route
}

fn build_routes(route: String, directory: &Path) -> HashMap<String, PathBuf> {
    let mut routes: HashMap<String, PathBuf> = HashMap::new();

    for entry in fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let name = path.file_name().unwrap().to_str().unwrap();
        if path.is_dir() {
            routes.extend(
                build_routes(format!("{route}/{name}"), &path)
            );
        } else if path.is_file() {
            match path.extension().unwrap().to_str().unwrap() {
                "html" | "css" | "js" => {
                    if name == "index.html" || name == "page.html" {
                        if route == "" {
                            routes.insert(String::from("/"), path);
                        } else {
                            routes.insert(route.clone(), path);
                        }
                    } else if name == "not_found.html" {
                        continue;
                    } else {
                        routes.insert(format!("{route}/{name}"), path);
                    }
                }
                _ => {continue;}
            }
        }
    }

    routes
}
