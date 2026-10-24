//! The bridge (issue 41): how an agent works on the project a window has open. The window, started with `--control PORT`,
//! listens on a local port; `bimcraft-cli mcp --connect` is the MCP server of a headless session, and with a port it runs
//! its tools on the window's session instead. One JSON request a line, one JSON reply a line:
//!
//! - `{"token": T, "op": "hello"}` asks which app is there and what project it has open;
//! - `{"token": T, "op": "execute", "command": "wall", "params": {...}}` runs a command, as `Session::execute` does;
//! - `{"token": T, "op": "script", "text": "...", "atomic": false}` runs a script (`Session::script`, or all or nothing).
//!
//! The reply is `{"ok": value}` or `{"error": text}`. The token is made when the window starts and kept in a file only the user
//! can read ([`Entry`]), so another user's process on the machine, or a web page that tries the port, is turned away. This is
//! the engine's side: the parts that need a thread or a window are in the apps.

use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::engine::{Session, find};
use crate::favorites;

/// The longest request line read: a script of a whole building is a few megabytes.
pub const MAX_LINE: usize = 16 << 20;

/// What a window's agent may not do to its project: replace it. It is the user's, and the window starts another or opens one by
/// its own menus, which ask about unsaved changes first.
pub const REFUSED: &[&str] = &["new", "open", "recover", "highrise"];

/// The file the window leaves its port and token in, in the folder of the user's favorites.
pub const FILE: &str = "bridge.json";

/// A request, read.
#[derive(Debug, PartialEq)]
pub enum Request {
    Hello,
    Execute { command: String, params: Value },
    Script { text: String, atomic: bool },
}

/// Why a request line was not taken.
#[derive(Debug, PartialEq)]
pub enum Error {
    /// The token is missing or wrong: the connection is dropped.
    Token,
    /// The line is no request: what is wrong with it.
    Request(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Token => f.write_str("the token is missing or wrong: the window's bridge file has it"),
            Error::Request(why) => f.write_str(why),
        }
    }
}

/// Whether two tokens are the same, in a time that does not say how much of one was right.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

/// Reads a request line, for a window whose token is `token`.
pub fn parse(line: &str, token: &str) -> Result<Request, Error> {
    let bad = |why: &str| Error::Request(why.to_string());
    let request: Value = serde_json::from_str(line).map_err(|e| Error::Request(format!("not JSON: {e}")))?;
    let Some(o) = request.as_object() else { return Err(bad("a request is an object")) };
    if !o.get("token").and_then(Value::as_str).is_some_and(|t| same(t, token)) {
        return Err(Error::Token);
    }
    match o.get("op").and_then(Value::as_str) {
        Some("hello") => Ok(Request::Hello),
        Some("execute") => {
            let command = o.get("command").and_then(Value::as_str).ok_or_else(|| bad("`execute` needs a `command`, a string"))?;
            let params = match o.get("params") {
                None | Some(Value::Null) => json!({}),
                Some(p @ Value::Object(_)) => p.clone(),
                Some(_) => return Err(bad("`params` is an object")),
            };
            Ok(Request::Execute { command: command.to_string(), params })
        }
        Some("script") => {
            let text = o.get("text").and_then(Value::as_str).ok_or_else(|| bad("`script` needs `text`, a string"))?;
            let atomic = match o.get("atomic") {
                None | Some(Value::Null) => false,
                Some(Value::Bool(b)) => *b,
                Some(_) => return Err(bad("`atomic` is true or false")),
            };
            Ok(Request::Script { text: text.to_string(), atomic })
        }
        Some(other) => Err(Error::Request(format!("unknown op `{other}`: hello, execute or script"))),
        None => Err(bad("a request has an `op`")),
    }
}

/// Refuses a command that would replace the project, by whatever name (an id or a shortcut, in any case).
fn refuse(command: &str) -> Result<(), String> {
    match find(command) {
        Some(c) if REFUSED.contains(&c.id) => Err(format!(
            "`{}` would replace the project the window has open, which is the user's: start or open another in the window",
            c.id
        )),
        _ => Ok(()),
    }
}

/// Runs a request on `session`. A script is read through before it runs, so a line that would replace the project stops it
/// before any line has.
pub fn run(session: &mut Session, request: &Request) -> Result<Value, String> {
    match request {
        Request::Hello => Ok(json!({"app": "bimcraft", "version": env!("CARGO_PKG_VERSION"), "project": session.model.name})),
        Request::Execute { command, params } => {
            refuse(command)?;
            session.execute(command, params)
        }
        Request::Script { text, atomic } => {
            for (n, line) in text.lines().map(str::trim).enumerate() {
                if let Some(id) = line.split_whitespace().next().filter(|id| !id.starts_with('#')) {
                    refuse(id).map_err(|e| format!("line {}: {e}", n + 1))?;
                }
            }
            let results = if *atomic { session.script_atomic(text) } else { session.script(text) }?;
            Ok(json!(results))
        }
    }
}

/// The reply line for a result.
pub fn reply(result: Result<Value, String>) -> String {
    match result {
        Ok(v) => json!({"ok": v}),
        Err(e) => json!({"error": e}),
    }
    .to_string()
}

/// A window's way in, as the file keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub port: u16,
    pub token: String,
}

/// Where the file is: beside the user's favorites.
pub fn entry_path() -> Option<PathBuf> {
    favorites::default_path().map(|p| p.with_file_name(FILE))
}

/// Writes the file for the window listening on `entry.port`, readable by the user alone where the system has such a thing.
pub fn write_entry(path: &Path, entry: &Entry) -> io::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)?.write_all(json!({"port": entry.port, "token": entry.token}).to_string().as_bytes())
}

/// Reads the file, if it is there and says a port and a token.
pub fn read_entry(path: &Path) -> Option<Entry> {
    use std::io::Read;
    let mut text = String::new();
    std::fs::File::open(path).ok()?.take(4096).read_to_string(&mut text).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    Some(Entry { port: u16::try_from(v["port"].as_u64()?).ok()?, token: v["token"].as_str().filter(|t| !t.is_empty())?.to_string() })
}

/// Removes the file, when it is still the one `entry` wrote: another window's is not this one's to remove.
pub fn remove_entry(path: &Path, entry: &Entry) {
    if read_entry(path).as_ref() == Some(entry) {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: &str = "0123-token";

    fn walls(s: &Session) -> usize {
        s.model.elements.values().filter(|e| matches!(e, crate::model::Element::Wall(_))).count()
    }

    fn line(v: Value) -> String {
        v.to_string()
    }

    #[test]
    fn a_request_is_read_with_its_token_and_what_is_wrong_with_one_is_said() {
        assert_eq!(parse(&line(json!({"token": T, "op": "hello"})), T), Ok(Request::Hello));
        assert_eq!(
            parse(&line(json!({"token": T, "op": "execute", "command": "wall", "params": {"points": [[0, 0], [1000, 0]]}})), T),
            Ok(Request::Execute { command: "wall".into(), params: json!({"points": [[0, 0], [1000, 0]]}) })
        );
        assert_eq!(
            parse(&line(json!({"token": T, "op": "execute", "command": "undo"})), T),
            Ok(Request::Execute { command: "undo".into(), params: json!({}) }),
            "no params is none"
        );
        assert_eq!(
            parse(&line(json!({"token": T, "op": "script", "text": "inspect", "atomic": true})), T),
            Ok(Request::Script { text: "inspect".into(), atomic: true })
        );
        // The token first: a request without it, or with another, says nothing else.
        for request in [
            json!({"op": "hello"}),
            json!({"token": "x", "op": "hello"}),
            json!({"token": 5, "op": "hello"}),
            json!({"token": "", "op": "bogus"}),
        ] {
            assert_eq!(parse(&line(request.clone()), T), Err(Error::Token), "{request}");
        }
        let bad = |request: Value| match parse(&line(request.clone()), T) {
            Err(Error::Request(why)) => why,
            other => panic!("{request}: {other:?}"),
        };
        assert!(bad(json!({"token": T})).contains("`op`"));
        assert!(bad(json!({"token": T, "op": "format"})).contains("unknown op `format`"));
        assert!(bad(json!({"token": T, "op": "execute"})).contains("`command`"));
        assert!(bad(json!({"token": T, "op": "execute", "command": 7})).contains("`command`"));
        assert!(bad(json!({"token": T, "op": "execute", "command": "wall", "params": [1]})).contains("`params`"));
        assert!(bad(json!({"token": T, "op": "script"})).contains("`text`"));
        assert!(bad(json!({"token": T, "op": "script", "text": "x", "atomic": "yes"})).contains("`atomic`"));
        assert!(matches!(parse("[1, 2]", T), Err(Error::Request(why)) if why.contains("object")));
        assert!(matches!(parse("{not json", T), Err(Error::Request(why)) if why.starts_with("not JSON")));
        assert!(matches!(parse("", T), Err(Error::Request(_))));
    }

    #[test]
    fn tokens_are_the_same_only_when_every_byte_is() {
        assert!(same("abc", "abc") && same("", ""));
        assert!(!same("abc", "abd") && !same("abc", "ab") && !same("ab", "abc") && !same("abc", "ABC"));
    }

    #[test]
    fn a_request_runs_on_the_session_as_a_command_does_and_the_reply_says_how_it_went() {
        let mut s = Session::new();
        let wall = Request::Execute { command: "wall".into(), params: json!({"points": [[0, 0], [4000, 0]]}) };
        let ok: Value = serde_json::from_str(&reply(run(&mut s, &wall))).unwrap();
        assert!(ok["ok"]["id"].is_u64() || ok["ok"].is_object(), "{ok}");
        assert_eq!(walls(&s), 1, "the wall is in the session");
        // A command that fails is an error reply, and the model stays.
        let door = Request::Execute { command: "door".into(), params: json!({"at": [99000, 99000]}) };
        let err: Value = serde_json::from_str(&reply(run(&mut s, &door))).unwrap();
        assert!(err["error"].as_str().is_some_and(|e| e.contains("no wall")), "{err}");
        assert_eq!(walls(&s), 1);
        // hello says what is there.
        let hello = run(&mut s, &Request::Hello).unwrap();
        assert_eq!((hello["app"].as_str(), hello["project"].as_str()), (Some("bimcraft"), Some(s.model.name.as_str())));
        // undo is a step like the others.
        run(&mut s, &Request::Execute { command: "undo".into(), params: json!({}) }).unwrap();
        assert_eq!(walls(&s), 0);
    }

    #[test]
    fn what_would_replace_the_project_is_refused_by_any_name_and_a_script_is_read_before_it_runs() {
        let mut s = Session::new();
        run(&mut s, &Request::Execute { command: "wall".into(), params: json!({"points": [[0, 0], [4000, 0]]}) }).unwrap();
        let kept = std::sync::Arc::clone(&s.model);
        for name in REFUSED {
            assert!(find(name).is_some_and(|c| c.id == *name), "`{name}` is a command id");
            for spelled in [name.to_string(), name.to_uppercase()] {
                let e = run(&mut s, &Request::Execute { command: spelled, params: json!({}) }).unwrap_err();
                assert!(e.contains("would replace the project"), "{name}: {e}");
            }
        }
        // By a shortcut: whatever an alias of a refused command is.
        for c in REFUSED.iter().filter_map(|n| find(n)) {
            for alias in c.aliases {
                let e = run(&mut s, &Request::Execute { command: alias.to_string(), params: json!({}) }).unwrap_err();
                assert!(e.contains("would replace the project"), "{alias}: {e}");
            }
        }
        // A script with one such line runs none of its lines, and says which.
        let text = "wall {\"points\": [[0, 5000], [4000, 5000]]}\n# a comment\nnew {}\n";
        for atomic in [false, true] {
            let e = run(&mut s, &Request::Script { text: text.into(), atomic }).unwrap_err();
            assert!(e.starts_with("line 3: ") && e.contains("`new`"), "{e}");
        }
        assert!(std::sync::Arc::ptr_eq(&kept, &s.model), "nothing ran");
        // The rest of a script runs, and the reply is each line's.
        let ok =
            run(&mut s, &Request::Script { text: "wall {\"points\": [[0, 5000], [4000, 5000]]}\ninspect".into(), atomic: true }).unwrap();
        assert_eq!(ok.as_array().map(Vec::len), Some(2));
        assert_eq!(walls(&s), 2);
    }

    #[test]
    fn the_file_keeps_a_port_and_a_token_for_the_user_alone_and_is_removed_only_by_its_own_window() {
        let dir = std::env::temp_dir().join(format!("bimcraft-bridge-{}", std::process::id()));
        let path = dir.join("nested").join(FILE);
        let entry = Entry { port: 40123, token: "abc".into() };
        assert_eq!(read_entry(&path), None, "no file");
        write_entry(&path, &entry).unwrap();
        assert_eq!(read_entry(&path), Some(entry.clone()));
        #[cfg(unix)]
        assert_eq!(std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(&path).unwrap().permissions()) & 0o777, 0o600);
        // Another window has written it since: this one leaves it.
        let other = Entry { port: 40124, token: "def".into() };
        write_entry(&path, &other).unwrap();
        remove_entry(&path, &entry);
        assert_eq!(read_entry(&path), Some(other.clone()));
        remove_entry(&path, &other);
        assert_eq!(read_entry(&path), None);
        // What is not a file of ours reads as none.
        for text in ["", "{", "[]", r#"{"port": 70000, "token": "x"}"#, r#"{"port": 5, "token": ""}"#, r#"{"port": "5", "token": "x"}"#] {
            std::fs::write(&path, text).unwrap();
            assert_eq!(read_entry(&path), None, "{text}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
