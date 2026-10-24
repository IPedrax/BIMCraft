//! `mcp --connect`: the MCP server's tools run on the session of a window that was started with `--control PORT`, as the user's own
//! project, instead of on a headless one of its own (issue 41). The protocol, and the file the window leaves its port and token in,
//! are `bimcraft_core::bridge`; this is the asking side. Nothing here listens, and the window is on this machine: the address is its own.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::Path;
use std::time::Duration;

use bimcraft_core::bridge::{self, Entry, MAX_LINE};
use serde_json::{Value, json};

/// How long the window has to take the connection.
const CONNECT: Duration = Duration::from_secs(5);

/// A connection to a window's control port.
pub struct Window {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    token: String,
}

/// Whether an error of the connection says the window has gone: a close with the request unread is a reset, not an end.
fn gone(e: &std::io::Error) -> bool {
    use std::io::ErrorKind::{BrokenPipe, ConnectionAborted, ConnectionReset, NotConnected, UnexpectedEof};
    matches!(e.kind(), ConnectionReset | ConnectionAborted | BrokenPipe | UnexpectedEof | NotConnected)
}

impl Window {
    /// Connects to the window that left its bridge file, or, given `port`, to that one if the file is its.
    pub fn connect(port: Option<u16>) -> Result<Window, String> {
        let path = bridge::entry_path().ok_or("there is no folder for the bridge file here: set BIMCRAFT_FAVORITES")?;
        Window::connect_with(&path, port)
    }

    /// [`Window::connect`] with the bridge file at `path`.
    pub(crate) fn connect_with(path: &Path, port: Option<u16>) -> Result<Window, String> {
        let entry = bridge::read_entry(path)
            .ok_or_else(|| format!("{}: no window has opened a control port (start one with `bimcraft --control 0`)", path.display()))?;
        if let Some(port) = port.filter(|p| *p != entry.port) {
            return Err(format!(
                "{} has the token of the window on port {}, not {port}: start that window with `--control {port}` again, or connect without a port",
                path.display(),
                entry.port
            ));
        }
        Window::connect_to(&entry)
    }

    /// Connects to the window `entry` names, and checks that it is BIMCraft that answers.
    pub fn connect_to(entry: &Entry) -> Result<Window, String> {
        let stream = TcpStream::connect_timeout(&SocketAddr::new(Ipv4Addr::LOCALHOST.into(), entry.port), CONNECT).map_err(|e| {
            format!("no window answers on port {}: {e} (is it still open? the bridge file may be left from one that closed)", entry.port)
        })?;
        stream.set_nodelay(true).map_err(|e| e.to_string())?;
        let reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
        let mut window = Window { reader, writer: stream, token: entry.token.clone() };
        let hello = window.ask(json!({"op": "hello"}))?;
        if hello["app"] != "bimcraft" {
            return Err(format!("port {} is not a BIMCraft window", entry.port));
        }
        Ok(window)
    }

    /// Sends a request and waits for its reply: the value it gives, or the error it says.
    pub fn ask(&mut self, mut request: Value) -> Result<Value, String> {
        request["token"] = json!(self.token);
        writeln!(self.writer, "{request}")
            .and_then(|()| self.writer.flush())
            .map_err(|e| format!("the window closed the connection ({e})"))?;
        let mut line = String::new();
        let read = self.reader.by_ref().take(MAX_LINE as u64 + 1).read_line(&mut line).map_err(|e| {
            if gone(&e) { format!("the window closed the connection ({e})") } else { format!("the window did not answer: {e}") }
        })?;
        if read == 0 {
            return Err("the window closed the connection".into());
        }
        let reply: Value = serde_json::from_str(&line).map_err(|e| format!("the window's reply is not JSON: {e}"))?;
        match (reply.get("ok"), reply["error"].as_str()) {
            (Some(ok), _) => Ok(ok.clone()),
            (None, Some(error)) => Err(error.to_string()),
            _ => Err("the window's reply says neither ok nor error".into()),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use bimcraft_core::engine::Session;
    use std::net::TcpListener;

    /// A window's control port for a test: a thread that answers each line as the desktop does, on a session of its own, until
    /// the agent hangs up or `after` requests have been answered (the window closing).
    pub(crate) fn stand_in(after: usize) -> Entry {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let entry = Entry { port: listener.local_addr().unwrap().port(), token: "test-token".into() };
        let token = entry.token.clone();
        std::thread::spawn(move || {
            let Ok((stream, _)) = listener.accept() else { return };
            let mut session = Session::new();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut writer = stream;
            for _ in 0..after {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                let result = bridge::parse(&line, &token).map_err(|e| e.to_string()).and_then(|r| bridge::run(&mut session, &r));
                if writeln!(writer, "{}", bridge::reply(result)).is_err() {
                    break;
                }
            }
        });
        entry
    }

    #[test]
    fn a_window_is_asked_and_what_it_says_is_a_value_or_an_error() {
        let entry = stand_in(usize::MAX);
        let mut w = Window::connect_to(&entry).unwrap();
        let wall = w.ask(json!({"op": "execute", "command": "wall", "params": {"points": [[0, 0], [4000, 0]]}})).unwrap();
        assert!(wall.is_object(), "{wall}");
        let counts = w.ask(json!({"op": "execute", "command": "inspect"})).unwrap();
        assert_eq!(counts["counts"]["wall"], 1);
        assert!(w.ask(json!({"op": "execute", "command": "door", "params": {"at": [90000, 90000]}})).unwrap_err().contains("no wall"));
        assert!(w.ask(json!({"op": "execute", "command": "new"})).unwrap_err().contains("would replace the project"));
        assert!(w.ask(json!({"op": "nonsense"})).unwrap_err().contains("unknown op"));
    }

    #[test]
    fn a_wrong_token_a_closed_window_and_a_port_nobody_has_are_said_in_words() {
        let mut entry = stand_in(usize::MAX);
        entry.token = "guess".into();
        assert!(Window::connect_to(&entry).err().is_some_and(|e| e.contains("token")), "a wrong token");
        // A window that closes after answering the hello: the next request finds the connection closed.
        let entry = stand_in(1);
        let mut w = Window::connect_to(&entry).unwrap();
        // Given time to have closed, with the next request unread: the answer to it is a reset, or nothing, or a broken pipe.
        std::thread::sleep(std::time::Duration::from_millis(200));
        for _ in 0..2 {
            let said = w.ask(json!({"op": "hello"})).unwrap_err();
            assert!(said.starts_with("the window closed the connection"), "{said}");
        }
        // A port nothing listens on: the first, which no program of a user's does.
        let gone = Entry { port: 1, token: "x".into() };
        assert!(Window::connect_to(&gone).err().is_some_and(|e| e.contains("no window answers")));
        // Something that answers, and is no BIMCraft: a line that is no JSON.
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let other = Entry { port: listener.local_addr().unwrap().port(), token: "x".into() };
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut line = String::new();
                let _ = BufReader::new(stream.try_clone().unwrap()).read_line(&mut line);
                let _ = writeln!(stream, "HTTP/1.1 400 Bad Request");
            }
        });
        assert!(Window::connect_to(&other).err().is_some_and(|e| e.contains("not JSON")));
    }

    #[test]
    fn the_bridge_file_names_the_window_and_a_port_that_is_not_its_own_is_refused() {
        let dir = std::env::temp_dir().join(format!("bimcraft-cli-bridge-{}", std::process::id()));
        let file = dir.join(bridge::FILE);
        assert!(Window::connect_with(&file, None).err().is_some_and(|e| e.contains("no window has opened a control port")));
        let entry = stand_in(usize::MAX);
        bridge::write_entry(&file, &entry).unwrap();
        let other = if entry.port == 9 { 10 } else { 9 };
        assert!(
            Window::connect_with(&file, Some(other)).err().is_some_and(|e| e.contains(&format!("not {other}"))),
            "the file is another window's"
        );
        assert!(Window::connect_with(&file, Some(entry.port)).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
