//! A tiny HTTP server for tests: answers with whatever the test's handler returns and records every request,
//! so the whole start flow can run against it without a network.

use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex},
    thread,
};

#[derive(Clone, Debug)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)).map(|(_, value)| value.as_str())
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).unwrap_or(serde_json::Value::Null)
    }
}

pub struct Reply {
    pub status: u16,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn json(status: u16, value: serde_json::Value) -> Self {
        Self { status, body: value.to_string().into_bytes() }
    }

    pub fn bytes(body: impl Into<Vec<u8>>) -> Self {
        Self { status: 200, body: body.into() }
    }

    pub fn status(status: u16) -> Self {
        Self { status, body: Vec::new() }
    }
}

pub struct FakeServer {
    pub url: String,
    log: Arc<Mutex<Vec<Request>>>,
}

impl FakeServer {
    /// Starts listening on a free local port. The handler gets the server's own base URL, so it can put links to itself in answers.
    pub fn start(handler: impl Fn(&str, &Request) -> Reply + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind a local port");
        let url = format!("http://{}", listener.local_addr().unwrap());
        let log = Arc::new(Mutex::new(Vec::new()));
        let handler = Arc::new(handler);
        let (base, seen) = (url.clone(), log.clone());
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (base, seen, handler) = (base.clone(), seen.clone(), handler.clone());
                thread::spawn(move || serve(stream, &base, &seen, &*handler));
            }
        });
        Self { url, log }
    }

    pub fn requests(&self) -> Vec<Request> {
        self.log.lock().unwrap().clone()
    }

    /// How many requests had this method and a path starting with `prefix`.
    pub fn count(&self, method: &str, prefix: &str) -> usize {
        self.requests().iter().filter(|r| r.method == method && r.path.starts_with(prefix)).count()
    }
}

fn serve(stream: TcpStream, base: &str, seen: &Mutex<Vec<Request>>, handler: &dyn Fn(&str, &Request) -> Reply) {
    let Some(request) = read_request(&stream) else { return };
    seen.lock().unwrap().push(request.clone());
    let reply = handler(base, &request);
    let reason = match reply.status {
        200 => "OK",
        402 => "Payment Required",
        404 => "Not Found",
        _ => "Status",
    };
    let head = format!("HTTP/1.1 {} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", reply.status, reply.body.len());
    let mut stream = stream;
    let _ = stream.write_all(head.as_bytes()).and_then(|()| stream.write_all(&reply.body));
}

fn read_request(stream: &TcpStream) -> Option<Request> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let mut parts = line.split_whitespace();
    let (method, path) = (parts.next()?.to_owned(), parts.next()?.to_owned());
    let mut headers = Vec::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        let (key, value) = line.split_once(':')?;
        headers.push((key.trim().to_owned(), value.trim().to_owned()));
    }
    let length = headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = vec![0; length];
    reader.read_exact(&mut body).ok()?;
    Some(Request { method, path, headers, body })
}
