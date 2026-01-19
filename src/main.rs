use std::{
    collections::HashMap,
    fs,
    io::{BufReader, prelude::*},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
};
use rcomm::ThreadPool;

const PORT: &str = "7879";
const ADDRESS: &str = "127.0.0.1";

fn main() {
    let full_address = format!("{ADDRESS}:{PORT}");
    let listener = TcpListener::bind(full_address).unwrap();

    let pool = ThreadPool::new(4);

    let path = Path::new("./pages");
    let routes = build_routes(String::from("/"), path);
    println!("Routes:\n{routes:#?}\n\n");

    for stream in listener.incoming() {
        let routes_clone = routes.clone();
        let stream = stream.unwrap();

        pool.execute(move || {
            handle_connection(stream, routes_clone);
        });
    }
}

fn handle_connection(mut stream: TcpStream, routes: HashMap<String, PathBuf>) {
    let buf_reader = BufReader::new(&stream);
    let http_request: Vec<_> = buf_reader
        .lines()
        .map(|result| result.unwrap())
        .take_while(|line| !line.is_empty())
        .collect();
    let route = http_request[0].split_whitespace().collect::<Vec<_>>()[1];

    println!("Request: {http_request:#?}");

    let (status_line, filename) = if routes.contains_key(route) {
        ("HTTP/1.1 200 OK", routes.get(route).unwrap().to_str().unwrap())
    } else {
        ("HTTP/1.1 404 NOT FOUND", "pages/not_found.html")
    };

    let contents = fs::read_to_string(filename).unwrap();
    let length = contents.len();

    let response =
        format!("{status_line}\r\nContent-Length: {length}\r\n\r\n{contents}");

    stream.write_all(response.as_bytes()).unwrap();
}

fn build_routes(route: String, directory: &Path) -> HashMap<String, PathBuf> {
    let mut routes: HashMap<String, PathBuf> = HashMap::new();

    for entry in fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let name = path.file_name().unwrap().to_str().unwrap();
        if path.is_dir() {
            routes.extend(
                build_routes(format!("{route}{name}/"), &path)
            );
        } else if path.is_file() {
            match path.extension().unwrap().to_str().unwrap() {
                "html" | "css" | "js" => {
                    if name == "index.html" {
                        routes.insert(route.clone(), path);
                    } else if name == "not_found.html" {
                        continue;
                    } else {
                        routes.insert(format!("{route}{name}"), path);
                    }
                }
                _ => {continue;}
            }
        }
    }

    routes
}
