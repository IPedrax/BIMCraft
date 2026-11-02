//! What the tests stand in for the network with, and the desktop's tests too (the `testing` feature): a table of answers, and a
//! server on this machine for the real client to talk to.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use crate::net::{Net, NetError, Reply};

/// A request as a server saw it.
#[derive(Clone, Debug)]
pub struct Seen {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Seen {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

/// A whole HTTP response, as bytes.
pub fn http(status: u16, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
    let mut out = format!("HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n", body.len());
    for (k, v) in headers {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str("\r\n");
    let mut bytes = out.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

/// A server on this machine that answers each request with what `route` says (a whole response, see [`http`]) and remembers it.
pub struct Server {
    /// `http://127.0.0.1:port`.
    pub base: String,
    pub seen: Arc<Mutex<Vec<Seen>>>,
}

impl Server {
    pub fn start(route: impl Fn(&Seen, &str) -> Vec<u8> + Send + 'static) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (log, own) = (seen.clone(), base.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut head = Vec::new();
                let mut byte = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") {
                    if stream.read(&mut byte).unwrap_or(0) == 0 {
                        break;
                    }
                    head.push(byte[0]);
                }
                let text = String::from_utf8_lossy(&head).to_string();
                let mut lines = text.lines();
                let mut first = lines.next().unwrap_or("").split(' ');
                let (method, path) = (first.next().unwrap_or("").to_string(), first.next().unwrap_or("").to_string());
                let headers: Vec<(String, String)> =
                    lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).collect();
                let length =
                    headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-length")).and_then(|(_, v)| v.parse().ok()).unwrap_or(0);
                let mut body = vec![0u8; length];
                let _ = stream.read_exact(&mut body);
                let request = Seen { method, path, headers, body };
                let answer = route(&request, &own);
                log.lock().unwrap().push(request);
                let _ = stream.write_all(&answer);
            }
        });
        Server { base, seen }
    }

    pub fn requests(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}

/// A network that answers from a table, and remembers what it was asked. Each address has a list of answers, given in turn, the
/// last one for ever after (so a 429 and then a 200 is two answers).
#[derive(Default)]
pub struct Fake {
    answers: Mutex<BTreeMap<(String, String), Vec<Result<Reply, NetError>>>>,
    asked: Mutex<Vec<(String, String, Vec<u8>)>>,
}

impl Fake {
    pub fn new() -> Fake {
        Fake::default()
    }

    /// `body` for GET `url`, after what was given before.
    pub fn get(self, url: &str, body: &[u8]) -> Fake {
        self.push("GET", url, Ok(Reply { body: body.to_vec(), content_type: None }))
    }

    /// `body` for POST `url`.
    pub fn post(self, url: &str, body: &[u8]) -> Fake {
        self.push("POST", url, Ok(Reply { body: body.to_vec(), content_type: Some("application/json".into()) }))
    }

    /// An HTTP error for `method` `url`.
    pub fn status(self, method: &str, url: &str, status: u16, retry_after: Option<u64>) -> Fake {
        self.push(method, url, Err(NetError { message: format!("{url}: HTTP {status}"), status: Some(status), retry_after }))
    }

    fn push(self, method: &str, url: &str, answer: Result<Reply, NetError>) -> Fake {
        self.answers.lock().unwrap().entry((method.into(), url.into())).or_default().push(answer);
        self
    }

    /// What was asked, in order: method, address, body.
    pub fn asked(&self) -> Vec<(String, String, Vec<u8>)> {
        self.asked.lock().unwrap().clone()
    }

    fn answer(&self, method: &str, url: &str, body: &[u8]) -> Result<Reply, NetError> {
        self.asked.lock().unwrap().push((method.into(), url.into(), body.to_vec()));
        let mut table = self.answers.lock().unwrap();
        match table.get_mut(&(method.to_string(), url.to_string())) {
            Some(list) if list.len() > 1 => list.remove(0),
            Some(list) => list.first().cloned().unwrap_or_else(|| Err(NetError::new(format!("404 for {url}")))),
            None => Err(NetError { message: format!("404 for {url}"), status: Some(404), retry_after: None }),
        }
    }
}

impl Net for Fake {
    fn get(&self, url: &str, _limit: u64) -> Result<Reply, NetError> {
        self.answer("GET", url, &[])
    }

    fn get_range(&self, url: &str, start: u64, len: u64) -> Result<Reply, NetError> {
        let reply = self.answer("GET", url, &[])?;
        let (start, end) = (start as usize, (start + len) as usize);
        Ok(Reply { body: reply.body.get(start..end.min(reply.body.len())).unwrap_or_default().to_vec(), content_type: reply.content_type })
    }

    fn post(&self, url: &str, _content_type: &str, body: &[u8], _limit: u64) -> Result<Reply, NetError> {
        self.answer("POST", url, body)
    }
}
