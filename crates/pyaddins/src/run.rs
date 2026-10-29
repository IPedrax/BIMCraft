//! A script running: the user's Python in a process of its own, `bimcraft.py run SCRIPT`, so nothing it does (a crash in a
//! C extension, a loop that never ends) takes the window down, and Stop kills it. It talks JSON a line at a time: the app
//! sends its inputs and the selection first; the script sends `execute` and `script` requests, each answered `{"ok": …}` or
//! `{"error": …}` on its standard input, what it prints, and at its end `done` (a message, and what to select) or `failed`
//! (the traceback). Its standard error is printed too.
//!
//! The requests run through `bimcraft_core::bridge::run`, as an agent's do: each a command, checked and rolled back on
//! failure, and nothing that would replace the user's project. Whoever runs the script answers them on the session they
//! choose (`Run::poll`): the window a frame at a time, the MCP server to the end (`run_to_end`). What comes from the
//! process is hostile: a line is capped, and one that is no message ends the run.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, Stdio};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::{Duration, Instant};

use bimcraft_core::bridge::{MAX_LINE, Request};
use serde_json::{Map, Value, json};

use crate::python::Python;

/// The longest line printed that is kept, and the most lines a run's log keeps.
pub const MAX_PRINT: usize = 4000;
pub const MAX_LOG: usize = 5000;

/// What a run has to tell.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// A line it printed; `err` for its standard error.
    Print { text: String, err: bool },
    /// It ended well: what `main` returned, and what it asked to have selected.
    Done { message: Option<String>, select: Option<Vec<u64>> },
    /// It ended badly: why, the traceback of the script's own lines.
    Failed(String),
}

enum Line {
    Out(String),
    Err(String),
    OutEnd,
    ErrEnd,
    TooLong,
}

/// Answers a script's request: a command run on some session.
pub type Ask<'a> = dyn FnMut(&Request) -> Result<Value, String> + 'a;

/// A script running.
pub struct Run {
    child: Child,
    replies: Sender<String>,
    lines: Receiver<Line>,
    pub started: Instant,
    /// Whether it has said `done` or `failed`, or was stopped.
    over: bool,
    ends: u8,
}

impl Run {
    /// Starts the script at `path` with `inputs` (checked with `inputs::resolve`) and `selection`, in `python`, through the
    /// module at `client`. `wake` is called whenever it has something to say, so a window that sleeps between frames looks.
    pub fn start(
        python: &Python,
        client: &Path,
        path: &Path,
        inputs: &Map<String, Value>,
        selection: &[u64],
        wake: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Result<Run, String> {
        let mut c = python.command();
        c.arg(client).arg("run").arg(path);
        if let Some(dir) = path.parent() {
            c.current_dir(dir);
        }
        c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = c.spawn().map_err(|e| format!("Python could not start: {e}"))?;
        let (Some(stdin), Some(stdout), Some(stderr)) = (child.stdin.take(), child.stdout.take(), child.stderr.take()) else {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Python started without its pipes".into());
        };
        let (tx, lines) = channel();
        let wake = move || {
            if let Some(w) = &wake {
                w();
            }
        };
        let wake = Arc::new(wake);
        // Its output, a line at a time.
        let (out, w) = (tx.clone(), Arc::clone(&wake));
        std::thread::Builder::new()
            .name("pyaddin-out".into())
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                loop {
                    let mut line = String::new();
                    let line = match reader.by_ref().take(MAX_LINE as u64 + 1).read_line(&mut line) {
                        Ok(0) | Err(_) => Line::OutEnd,
                        Ok(n) if n > MAX_LINE => Line::TooLong,
                        Ok(_) => Line::Out(line),
                    };
                    match line {
                        Line::Out(_) => {
                            let _ = out.send(line);
                            w();
                        }
                        Line::TooLong => {
                            let _ = out.send(Line::TooLong);
                            // What is left is read and dropped, so the process is never stuck writing it.
                            let _ = std::io::copy(&mut reader, &mut std::io::sink());
                            let _ = out.send(Line::OutEnd);
                            w();
                            return;
                        }
                        _ => {
                            let _ = out.send(Line::OutEnd);
                            w();
                            return;
                        }
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        let (err, w) = (tx, Arc::clone(&wake));
        std::thread::Builder::new()
            .name("pyaddin-err".into())
            .spawn(move || {
                let mut reader = BufReader::new(stderr);
                loop {
                    let mut line = Vec::new();
                    match reader.by_ref().take(64 * 1024).read_until(b'\n', &mut line) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            let _ = err.send(Line::Err(String::from_utf8_lossy(&line).trim_end().to_string()));
                            w();
                        }
                    }
                }
                let _ = err.send(Line::ErrEnd);
                w();
            })
            .map_err(|e| e.to_string())?;
        // Replies, from a thread of their own: a script that stops reading can't stop the window.
        let (replies, to_send) = channel::<String>();
        std::thread::Builder::new()
            .name("pyaddin-in".into())
            .spawn(move || {
                let mut stdin = stdin;
                for line in to_send {
                    if stdin.write_all(line.as_bytes()).and_then(|()| stdin.write_all(b"\n")).and_then(|()| stdin.flush()).is_err() {
                        return;
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        let first = json!({"inputs": inputs, "selection": selection}).to_string();
        let _ = replies.send(first);
        Ok(Run { child, replies, lines, started: Instant::now(), over: false, ends: 0 })
    }

    /// Whether it is over, and all it said has been read.
    pub fn finished(&self) -> bool {
        self.over && self.ends >= 2
    }

    /// Kills it.
    pub fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.over = true;
    }

    /// What it said since the last poll, at most `budget` requests answered with `ask`; waits up to `wait` for the first.
    pub fn poll(&mut self, ask: &mut Ask, budget: usize, wait: Duration) -> Vec<Event> {
        let mut events = Vec::new();
        let mut answered = 0;
        let mut first = true;
        while answered < budget && !self.finished() {
            let line = if first && !wait.is_zero() {
                match self.lines.recv_timeout(wait) {
                    Ok(l) => l,
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => Line::ErrEnd,
                }
            } else {
                match self.lines.try_recv() {
                    Ok(l) => l,
                    Err(_) => break,
                }
            };
            first = false;
            match line {
                Line::Err(text) => events.push(Event::Print { text: cap(text), err: true }),
                Line::OutEnd | Line::ErrEnd => {
                    self.ends += 1;
                    if self.ends >= 2 && !self.over {
                        self.over = true;
                        let status = self.child.wait().ok().and_then(|s| s.code());
                        let how = status.map_or_else(|| "it was stopped".to_string(), |c| format!("its exit code {c}"));
                        events.push(Event::Failed(format!("Python ended before the script did ({how})")));
                    }
                }
                Line::TooLong => {
                    self.stop();
                    events.push(Event::Failed(format!("the script sent a line longer than {} MB", MAX_LINE >> 20)));
                }
                Line::Out(text) if self.over => drop(text),
                Line::Out(text) => match message(&text) {
                    Ok(Message::Ask(request)) => {
                        answered += 1;
                        let reply = match ask(&request) {
                            Ok(v) => json!({"ok": v}),
                            Err(e) => json!({"error": e}),
                        };
                        let _ = self.replies.send(reply.to_string());
                    }
                    Ok(Message::Event(e)) => {
                        if matches!(e, Event::Done { .. } | Event::Failed(_)) {
                            self.over = true;
                        }
                        events.push(e);
                    }
                    Err(e) => {
                        self.stop();
                        events.push(Event::Failed(e));
                    }
                },
            }
        }
        events
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        if !self.over || self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}

fn cap(text: String) -> String {
    if text.chars().count() <= MAX_PRINT { text } else { text.chars().take(MAX_PRINT).chain("…".chars()).collect() }
}

enum Message {
    Ask(Request),
    Event(Event),
}

/// A line from the script's process.
fn message(line: &str) -> Result<Message, String> {
    let bad = || format!("the script's process said what is no message: {}", line.chars().take(200).collect::<String>());
    let v: Value = serde_json::from_str(line).map_err(|_| bad())?;
    let text = |key: &str| v[key].as_str().map(str::to_string);
    match v["op"].as_str() {
        Some("execute") => {
            let command = text("command").ok_or_else(bad)?;
            let params = match &v["params"] {
                Value::Null => json!({}),
                p @ Value::Object(_) => p.clone(),
                _ => return Err(bad()),
            };
            Ok(Message::Ask(Request::Execute { command, params }))
        }
        Some("script") => {
            Ok(Message::Ask(Request::Script { text: text("text").ok_or_else(bad)?, atomic: v["atomic"].as_bool().unwrap_or(false) }))
        }
        Some("print") => {
            Ok(Message::Event(Event::Print { text: cap(text("text").unwrap_or_default()), err: v["err"].as_bool() == Some(true) }))
        }
        Some("done") => {
            let select = v["select"].as_array().map(|a| a.iter().filter_map(Value::as_u64).take(crate::inputs::MAX_IDS).collect());
            Ok(Message::Event(Event::Done { message: text("message").map(cap), select }))
        }
        Some("failed") => Ok(Message::Event(Event::Failed(cap(text("error").unwrap_or_default())))),
        _ => Err(bad()),
    }
}

/// How a run to its end went.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outcome {
    /// What it printed, `err` for its standard error: the last [`MAX_LOG`] lines.
    pub log: Vec<(String, bool)>,
    pub message: Option<String>,
    pub select: Option<Vec<u64>>,
    /// Why it failed, or none.
    pub error: Option<String>,
}

impl Outcome {
    pub fn to_json(&self) -> Value {
        let log: Vec<Value> = self.log.iter().map(|(t, err)| if *err { json!({"text": t, "err": true}) } else { json!(t) }).collect();
        json!({"ok": self.error.is_none(), "message": self.message, "select": self.select, "error": self.error, "log": log})
    }
}

/// Runs a script to its end, or until `limit` (then it is stopped), answering its requests with `ask`.
pub fn run_to_end(
    python: &Python,
    client: &Path,
    path: &Path,
    inputs: &Map<String, Value>,
    selection: &[u64],
    ask: &mut Ask,
    limit: Duration,
) -> Outcome {
    let mut out = Outcome::default();
    let mut run = match Run::start(python, client, path, inputs, selection, None) {
        Ok(r) => r,
        Err(e) => {
            out.error = Some(e);
            return out;
        }
    };
    while !run.finished() {
        if run.started.elapsed() > limit {
            run.stop();
            out.error = Some(format!("the script ran longer than {} seconds and was stopped", limit.as_secs()));
            break;
        }
        for e in run.poll(ask, 256, Duration::from_millis(50)) {
            match e {
                Event::Print { text, err } => {
                    if out.log.len() >= MAX_LOG {
                        out.log.remove(0);
                    }
                    out.log.push((text, err));
                }
                Event::Done { message, select } => (out.message, out.select) = (message, select),
                Event::Failed(e) => out.error = Some(e),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::python::tests::python;
    use crate::store::Store;
    use bimcraft_core::bridge;
    use bimcraft_core::engine::Session;

    fn walls(s: &Session) -> usize {
        s.model.elements.values().filter(|e| matches!(e, bimcraft_core::model::Element::Wall(_))).count()
    }

    fn run(py: &Python, code: &str, inputs: Value, s: &mut Session) -> Outcome {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        let path = store.save("test", code).unwrap();
        let inputs = inputs.as_object().cloned().unwrap_or_default();
        run_to_end(py, &store.client().unwrap(), &path, &inputs, &[100001], &mut |r| bridge::run(s, r), Duration::from_secs(60))
    }

    #[test]
    fn a_script_runs_commands_on_the_session_prints_and_returns() {
        let Some(py) = python() else { return };
        let mut s = Session::new();
        let code = r#"
import bimcraft as bc

def main(length: bc.Length = 4000, n: int = 2):
    print("selected", bc.selection())
    for i in range(n):
        bc.wall(points=[[0, i * 3000], [length, i * 3000]])
    with bc.batch() as b:
        bc.execute("wall", {"points": [[0, 9000], [length, 9000]]})
        bc.wall(points=[[0, 12000], [length, 12000]])
    walls = bc.query(category="wall")
    bc.select([w["id"] for w in walls])
    return "%d walls, %d in the batch" % (len(walls), len(b.results))
"#;
        let out = run(&py, code, json!({"length": 5000.0, "n": 3}), &mut s);
        assert_eq!(out.error, None, "{out:?}");
        assert_eq!(out.message.as_deref(), Some("5 walls, 2 in the batch"));
        assert_eq!(out.log, [("selected [100001]".to_string(), false)]);
        assert_eq!(walls(&s), 5);
        assert_eq!(out.select.as_ref().map(Vec::len), Some(5));
    }

    #[test]
    fn a_command_refused_raises_in_python_and_a_failure_says_the_script_line() {
        let Some(py) = python() else { return };
        let mut s = Session::new();
        let code = "import bimcraft as bc\n\ndef main():\n    bc.wall(points=[[0, 0], [4000, 0]])\n    bc.door(at=[99000, 99000])\n";
        let out = run(&py, code, json!({}), &mut s);
        let e = out.error.unwrap();
        assert!(e.starts_with("line 5: CommandError:") && e.contains("no wall"), "{e}");
        assert_eq!(walls(&s), 1, "what it did before stays done");
        // Caught, the script goes on.
        let code = "import bimcraft as bc\ntry:\n    bc.new()\nexcept bc.CommandError as e:\n    print('refused:', e)\n";
        let out = run(&py, code, json!({}), &mut s);
        assert_eq!(out.error, None);
        assert!(out.log[0].0.starts_with("refused:") && out.log[0].0.contains("would replace the project"), "{:?}", out.log);
        // Python's own errors: the traceback of the script's lines, not the module's.
        let out = run(&py, "x = 1\ny = x / 0\n", json!({}), &mut s);
        let e = out.error.unwrap();
        assert!(e.contains("line 2") && e.ends_with("ZeroDivisionError: division by zero") && !e.contains("bimcraft.py"), "{e}");
        let out = run(&py, "import sys\nsys.exit('gave up')\n", json!({}), &mut s);
        assert_eq!(out.error.as_deref(), Some("gave up"));
        let out = run(&py, "import sys\nprint('bye')\nsys.exit(0)\n", json!({}), &mut s);
        assert_eq!((out.error, out.log.len()), (None, 1));
    }

    #[test]
    fn a_script_that_dies_hangs_or_babbles_is_stopped_and_said_to_have() {
        let Some(py) = python() else { return };
        let mut s = Session::new();
        // Out of its process with no word: os._exit skips everything.
        let out = run(&py, "import os, sys\nsys.__stderr__.write('dying\\n')\nos._exit(3)\n", json!({}), &mut s);
        assert_eq!(out.error.as_deref(), Some("Python ended before the script did (its exit code 3)"), "{out:?}");
        assert_eq!(out.log, [("dying".to_string(), true)]);
        // What is no message on the protocol's stream ends it.
        let out = run(
            &py,
            "import sys\nsys.__stdout__.write('hello\\n')\nsys.__stdout__.flush()\nimport time\ntime.sleep(30)\n",
            json!({}),
            &mut s,
        );
        assert!(out.error.unwrap().contains("no message: hello"));
        // Past its time.
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        let path = store.save("slow", "import time\ntime.sleep(30)\n").unwrap();
        let start = Instant::now();
        let out =
            run_to_end(&py, &store.client().unwrap(), &path, &Map::new(), &[], &mut |r| bridge::run(&mut s, r), Duration::from_millis(500));
        assert!(out.error.unwrap().contains("was stopped"));
        assert!(start.elapsed() < Duration::from_secs(10));
        // input() reads nothing and print goes to the log, not to the protocol.
        let out = run(&py, "print(repr(input() if False else __import__('sys').stdin.read()))\n", json!({}), &mut s);
        assert_eq!(out.log, [("''".to_string(), false)]);
    }

    fn types(s: &mut Session) -> Vec<String> {
        let reply = s.execute("elements", &json!({"category": "wall", "limit": 100000})).unwrap();
        reply["elements"].as_array().unwrap().iter().map(|w| w["type"].as_str().unwrap().to_string()).collect()
    }

    #[test]
    fn the_sample_changes_walls_from_one_type_to_another() {
        let Some(py) = python() else { return };
        let (_, code) = crate::store::SAMPLES[0];
        let mut s = Session::new();
        s.script("wall {\"points\": [[0, 0], [4000, 0]]}\nwall {\"points\": [[0, 3000], [4000, 3000]]}\nwall {\"points\": [[0, 6000], [4000, 6000]], \"type\": \"Interior - 100mm\"}").unwrap();
        let walls = s.execute("elements", &json!({"category": "wall"})).unwrap();
        let first = walls["elements"][0]["id"].as_u64().unwrap();
        let steps = s.history().len();
        // Only the selected one of that type.
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        let path = store.save("Change Wall Type", code).unwrap();
        let client = store.client().unwrap();
        let go = |s: &mut Session, inputs: Value, selection: &[u64]| {
            let inputs = inputs.as_object().cloned().unwrap();
            run_to_end(&py, &client, &path, &inputs, selection, &mut |r| bridge::run(s, r), Duration::from_secs(60))
        };
        let from_to = |only: bool| json!({"from_type": "Generic - 200mm", "to_type": "Exterior - 300mm", "only_selected": only});
        let out = go(&mut s, from_to(true), &[first]);
        assert_eq!(out.message.as_deref(), Some("1 walls changed from Generic - 200mm to Exterior - 300mm"), "{out:?}");
        assert_eq!(types(&mut s), ["Exterior - 300mm", "Generic - 200mm", "Interior - 100mm"]);
        // All of them; the other type is left alone.
        let out = go(&mut s, from_to(false), &[]);
        assert_eq!(out.message.as_deref(), Some("1 walls changed from Generic - 200mm to Exterior - 300mm"));
        assert_eq!(out.select, Some(vec![first + 1]));
        assert_eq!(types(&mut s), ["Exterior - 300mm", "Exterior - 300mm", "Interior - 100mm"]);
        assert_eq!(s.history().len(), steps + 2, "a step a run (one modify each)");
        // None left, and the same type twice.
        assert_eq!(go(&mut s, from_to(false), &[]).message.as_deref(), Some("No walls of type Generic - 200mm."));
        let same = json!({"from_type": "Generic - 200mm", "to_type": "Generic - 200mm"});
        assert_eq!(go(&mut s, same, &[]).message.as_deref(), Some("Pick two different types."));
    }

    #[test]
    fn query_reads_every_page() {
        let Some(py) = python() else { return };
        let mut s = Session::new();
        let lines: Vec<String> = (0..520).map(|i| format!("wall {{\"points\": [[0, {y}], [1000, {y}]]}}", y = i * 1000)).collect();
        s.script_atomic(&lines.join("\n")).unwrap();
        let code = "import bimcraft as bc\ndef main():\n    return '%d %d' % (len(bc.query(category='wall')), len(bc.query(category='wall', limit=10)))\n";
        let out = run(&py, code, json!({}), &mut s);
        assert_eq!(out.message.as_deref(), Some("520 10"), "{out:?}");
    }

    #[test]
    fn stop_kills_a_run_at_once() {
        let Some(py) = python() else { return };
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        let path = store.save("forever", "while True:\n    pass\n").unwrap();
        let mut r = Run::start(&py, &store.client().unwrap(), &path, &Map::new(), &[], None).unwrap();
        let mut s = Session::new();
        assert!(r.poll(&mut |q| bridge::run(&mut s, q), 10, Duration::from_millis(100)).is_empty());
        assert!(!r.finished());
        r.stop();
        // Its streams close, and it is over.
        let start = Instant::now();
        while !r.finished() && start.elapsed() < Duration::from_secs(10) {
            r.poll(&mut |q| bridge::run(&mut s, q), 10, Duration::from_millis(50));
        }
        assert!(r.finished());
    }

    #[test]
    fn what_is_no_message_is_refused() {
        for line in ["", "[]", "{}", r#"{"op": "fly"}"#, r#"{"op": "execute"}"#, r#"{"op": "execute", "command": "x", "params": [1]}"#] {
            assert!(message(line).is_err(), "{line}");
        }
        assert!(matches!(message(r#"{"op": "execute", "command": "wall"}"#), Ok(Message::Ask(Request::Execute { .. }))));
        let long = json!({"op": "print", "text": "x".repeat(MAX_PRINT * 2)}).to_string();
        let Ok(Message::Event(Event::Print { text, .. })) = message(&long) else { panic!() };
        assert_eq!(text.chars().count(), MAX_PRINT + 1);
    }
}
