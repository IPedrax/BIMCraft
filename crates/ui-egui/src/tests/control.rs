//! The control port (issue 41): an agent on a TCP connection works on the project the window has open. The window answers in
//! its frames, so a test runs them while a thread of its own plays the agent.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use super::*;
use crate::control::Control;

/// A window with a control port on any free one, and the file it wrote, in a folder of its own: tests run side by side.
fn controlled(name: &str) -> (Harness<'static, BimApp>, std::path::PathBuf) {
    let file = std::env::temp_dir().join(format!("bimcraft-ui-control-{}-{name}", std::process::id())).join("bridge.json");
    let mut h = app(BOX);
    h.state_mut().use_control(Control::start(0, Some(file.clone())).unwrap());
    (h, file)
}

fn entry(h: &Harness<BimApp>) -> bimcraft_core::bridge::Entry {
    h.state().control.as_ref().unwrap().entry.clone()
}

/// An agent that sends each of `lines` and reads the reply to each, while the window goes on.
fn ask(h: &mut Harness<BimApp>, lines: Vec<String>) -> Vec<Value> {
    let port = entry(h).port;
    let agent = std::thread::spawn(move || {
        let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
        let (mut writer, mut reader) = (stream.try_clone().unwrap(), BufReader::new(stream));
        let mut replies = Vec::new();
        for line in lines {
            writeln!(writer, "{line}").unwrap();
            let mut reply = String::new();
            if reader.read_line(&mut reply).unwrap() == 0 {
                break;
            }
            replies.push(serde_json::from_str::<Value>(&reply).unwrap());
        }
        replies
    });
    while !agent.is_finished() {
        h.step();
        std::thread::sleep(Duration::from_millis(2));
    }
    agent.join().unwrap()
}

fn request(h: &Harness<BimApp>, mut body: Value) -> String {
    body["token"] = json!(entry(h).token);
    body.to_string()
}

#[test]
fn an_agent_works_on_the_project_the_window_has_open_and_the_user_sees_it_and_can_undo_it() {
    let (mut h, file) = controlled("works");
    assert!(h.state().status.text.contains(&entry(&h).port.to_string()), "the window says where agents come in: {:?}", h.state().status);
    let walls_before = walls(&h).len();
    let lines = vec![
        request(&h, json!({"op": "hello"})),
        request(&h, json!({"op": "execute", "command": "wall", "params": {"points": [[0, 7000], [5000, 7000]]}})),
        request(
            &h,
            json!({"op": "script", "text": "wall {\"points\": [[0, 8000], [5000, 8000]]}\nwall {\"points\": [[0, 9000], [5000, 9000]]}", "atomic": true}),
        ),
        request(&h, json!({"op": "execute", "command": "inspect"})),
    ];
    let replies = ask(&mut h, lines);
    assert_eq!(replies.len(), 4, "{replies:?}");
    assert_eq!(replies[0]["ok"]["app"], "bimcraft");
    assert_eq!(replies[0]["ok"]["project"], json!(h.state().session.model.name));
    assert!(replies[1]["ok"].is_object() && replies[2]["ok"].as_array().is_some_and(|a| a.len() == 2), "{replies:?}");
    assert_eq!(replies[3]["ok"]["counts"]["wall"], json!(walls_before + 3), "the agent sees the window's own project, with what it made");
    // In the window: three walls more, unsaved, and each step an undo step (the script's one).
    assert_eq!(walls(&h).len(), walls_before + 3);
    assert!(h.state().dirty());
    assert_eq!(h.state().session.history().iter().rev().take(2).copied().collect::<Vec<_>>(), ["Script", "Wall"]);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(walls(&h).len(), walls_before + 1, "the user's undo takes back the agent's script");
    // The file is there for as long as the window has the port, and goes with it.
    assert!(file.exists());
    h.state_mut().control = None;
    assert!(!file.exists());
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

#[test]
fn what_would_replace_the_project_an_error_and_a_wrong_token_are_each_turned_away() {
    let (mut h, file) = controlled("refused");
    let model = std::sync::Arc::clone(&h.state().session.model);
    let lines = vec![
        request(&h, json!({"op": "execute", "command": "new", "params": {}})),
        request(&h, json!({"op": "execute", "command": "OPEN", "params": {"path": "/nowhere.bimcraft"}})),
        request(&h, json!({"op": "script", "text": "wall {\"points\": [[0, 7000], [5000, 7000]]}\nnew", "atomic": false})),
        request(&h, json!({"op": "execute", "command": "door", "params": {"at": [99000, 99000]}})),
        "{not json".to_string(),
        request(&h, json!({"op": "teleport"})),
    ];
    let replies = ask(&mut h, lines);
    let errors: Vec<&str> = replies.iter().map(|r| r["error"].as_str().unwrap_or("")).collect();
    assert!(errors[0].contains("would replace the project") && errors[1].contains("would replace the project"), "{errors:?}");
    assert!(errors[2].starts_with("line 2: ") && errors[2].contains("`new`"), "{errors:?}");
    assert!(errors[3].contains("no wall") && errors[4].starts_with("not JSON") && errors[5].contains("unknown op"), "{errors:?}");
    assert!(std::sync::Arc::ptr_eq(&model, &h.state().session.model), "nothing changed in the project");
    // A wrong token, or none: the reply says so and the connection is closed, so the next line is never answered.
    let port = entry(&h).port;
    for first in [json!({"op": "hello"}).to_string(), json!({"token": "guess", "op": "hello"}).to_string()] {
        let agent = std::thread::spawn(move || {
            let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
            let (mut writer, mut reader) = (stream.try_clone().unwrap(), BufReader::new(stream));
            writeln!(writer, "{first}").unwrap();
            let mut reply = String::new();
            reader.read_line(&mut reply).unwrap();
            let _ = writeln!(writer, "{first}");
            let mut closed = String::new();
            (reply, reader.read_line(&mut closed).map_or(true, |n| n == 0))
        });
        while !agent.is_finished() {
            h.step();
            std::thread::sleep(Duration::from_millis(2));
        }
        let (reply, closed) = agent.join().unwrap();
        assert!(reply.contains("token") && closed, "{reply}");
    }
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

#[test]
fn only_a_few_agents_are_held_at_once_and_a_connection_that_says_nothing_holds_a_place_only_for_a_while() {
    let (mut h, file) = controlled("held");
    let port = entry(&h).port;
    // Eight connections that say nothing hold the eight places; the ninth is closed at once.
    let held: Vec<TcpStream> = (0..8).map(|_| TcpStream::connect(("127.0.0.1", port)).unwrap()).collect();
    std::thread::sleep(Duration::from_millis(200));
    let mut ninth = TcpStream::connect(("127.0.0.1", port)).unwrap();
    ninth.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut byte = [0u8; 1];
    assert!(ninth.read(&mut byte).map_or(true, |n| n == 0), "closed on the ninth");
    // Let go of one: a place is free, and an agent is answered again.
    drop(held);
    std::thread::sleep(Duration::from_millis(200));
    let lines = vec![request(&h, json!({"op": "hello"}))];
    assert_eq!(ask(&mut h, lines)[0]["ok"]["app"], "bimcraft");
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

#[test]
fn a_request_line_over_the_limit_is_an_error_and_a_line_at_it_is_read() {
    use std::io::Cursor;
    // The reader the connection uses: a line up to the limit comes through; one past it does not, however it ends.
    let mut ok = vec![b'a'; bimcraft_core::bridge::MAX_LINE - 1];
    ok.push(b'\n');
    assert_eq!(crate::control::read_line(&mut Cursor::new(ok)).unwrap().map(|l| l.len()), Some(bimcraft_core::bridge::MAX_LINE));
    let long = vec![b'a'; bimcraft_core::bridge::MAX_LINE + 10];
    assert!(crate::control::read_line(&mut Cursor::new(long)).is_err());
    assert_eq!(crate::control::read_line(&mut Cursor::new(Vec::new())).unwrap(), None, "the end of what an agent sends");
    assert!(crate::control::read_line(&mut Cursor::new(vec![0xff, 0xfe, b'\n'])).is_err(), "not text");
}

#[test]
fn a_port_in_use_is_an_error_and_the_bridge_file_is_left_alone() {
    let file = std::env::temp_dir().join(format!("bimcraft-ui-control-busy-{}", std::process::id())).join("bridge.json");
    let first = Control::start(0, Some(file.clone())).unwrap();
    let written = std::fs::read_to_string(&file).unwrap();
    assert!(Control::start(first.entry.port, Some(file.clone())).is_err(), "bound already");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), written, "the one that listens keeps its file");
    drop(first);
    assert!(!file.exists());
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

#[test]
fn a_connection_that_trickles_its_first_line_or_speaks_another_protocol_is_closed() {
    use std::time::Instant;
    // A short time for the first line, so that the test is short.
    let control = Control::start_within(0, None, Duration::from_millis(400)).unwrap();
    let port = control.entry.port;
    // A byte now and then never makes a line: it is closed 400 ms from its start, not at its next byte.
    let mut slow = TcpStream::connect(("127.0.0.1", port)).unwrap();
    slow.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
    let start = Instant::now();
    slow.write_all(b"{").unwrap();
    let closed = loop {
        let mut byte = [0u8; 1];
        match slow.read(&mut byte) {
            Ok(0) => break start.elapsed(),
            Ok(_) => panic!("nothing is said to a connection that has said nothing"),
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(_) => break start.elapsed(),
        }
        assert!(start.elapsed() < Duration::from_secs(5), "never closed");
        let _ = slow.write_all(b" ");
    };
    assert!(closed >= Duration::from_millis(300) && closed < Duration::from_secs(3), "closed after {closed:?}");
    // A web page's HTTP is not a request: it is told so, and closed, rather than held reading its headers.
    let mut http = TcpStream::connect(("127.0.0.1", port)).unwrap();
    http.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    http.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nOrigin: http://evil.example\r\n\r\n").unwrap();
    let mut said = String::new();
    match http.read_to_string(&mut said) {
        Ok(_) => assert!(said.lines().count() == 1 && said.contains("not JSON"), "{said}"),
        Err(e) => assert!(!matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut), "held open: {e}"),
    }
}

#[test]
fn a_request_whose_agent_has_given_up_is_not_done_and_the_next_one_is() {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc::channel;
    let mut h = app(BOX);
    let (control, tx) = Control::detached();
    h.state_mut().use_control(control);
    let wall = |y: i32| bimcraft_core::bridge::Request::Execute { command: "wall".into(), params: json!({"points": [[0, y], [5000, y]]}) };
    let before = walls(&h).len();
    let (gave_up, never) = channel();
    let (waiting, answer) = channel();
    tx.send(crate::control::Asked { request: wall(7000), reply: gave_up, abandoned: Arc::new(AtomicBool::new(true)) }).unwrap();
    tx.send(crate::control::Asked { request: wall(8000), reply: waiting, abandoned: Arc::new(AtomicBool::new(false)) }).unwrap();
    h.step();
    assert_eq!(walls(&h).len(), before + 1, "only the wall somebody was waiting for");
    assert!(answer.try_recv().unwrap().contains("\"ok\""));
    assert!(never.try_recv().is_err(), "nobody is told what nobody asked for");
}
