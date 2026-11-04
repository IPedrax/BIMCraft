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
    // A byte now and then never makes a line: it is closed 400 ms from its start, not at its next byte. The server's 400 ms begin
    // when it accepts, so the test's clock begins before it connects: a stall in between cannot make the close look early.
    let start = Instant::now();
    let mut slow = TcpStream::connect(("127.0.0.1", port)).unwrap();
    slow.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
    slow.write_all(b"{").unwrap();
    let closed = loop {
        let mut byte = [0u8; 1];
        match slow.read(&mut byte) {
            Ok(0) => break start.elapsed(),
            Ok(_) => panic!("nothing is said to a connection that has said nothing"),
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(_) => break start.elapsed(),
        }
        assert!(start.elapsed() < Duration::from_secs(20), "never closed");
        let _ = slow.write_all(b" ");
    };
    assert!(closed >= Duration::from_millis(300) && closed < Duration::from_secs(10), "closed after {closed:?}");
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
    let wall = |y: i32| bimcraft_core::bridge::Request::Execute {
        command: "wall".into(),
        params: json!({"points": [[0, y], [5000, y]]}),
        read_only: false,
    };
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

/// File > Settings at its Agents page.
fn agents_page(h: &mut Harness<BimApp>) {
    press(h, "Settings");
    press(h, "Agents");
}

/// Frames until `done` holds of the window, or ten seconds.
fn until(h: &mut Harness<BimApp>, done: impl Fn(&Harness<BimApp>) -> bool) -> bool {
    let start = std::time::Instant::now();
    while !done(h) && start.elapsed() < Duration::from_secs(10) {
        h.step();
        std::thread::sleep(Duration::from_millis(5));
    }
    done(h)
}

#[test]
fn settings_open_the_port_and_close_it_and_the_view_control_bar_says_how_it_is_in_both_profiles() {
    for mut h in [app(BOX), bimcraft(BOX)] {
        assert!(h.query_by_label("Agents: off").is_some(), "the bar says no agent can come in");
        agents_page(&mut h);
        pick(&mut h, Role::CheckBox, "Let agents work on this project");
        let port = entry(&h).port;
        assert!(port != 0 && h.state().agents.listen, "listening, on a port that was free, and kept");
        assert!(h.query_by_label_contains(&format!("port {port}: agents connect with")).is_some(), "Settings says where");
        press(&mut h, "Close");
        let listening = format!("Agents: port {port}");
        assert!(h.query_by_label(&listening).is_some(), "the bar says the port");
        // An agent that has said hello is counted while it stays, and no longer once it has gone.
        let token = entry(&h).token;
        let agent = std::thread::spawn(move || {
            let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
            let (mut writer, mut reader) = (stream.try_clone().unwrap(), BufReader::new(stream));
            writeln!(writer, "{}", json!({"token": token, "op": "hello"})).unwrap();
            let mut reply = String::new();
            reader.read_line(&mut reply).unwrap();
            (writer, reply)
        });
        while !agent.is_finished() {
            h.step();
            std::thread::sleep(Duration::from_millis(2));
        }
        let (held, reply) = agent.join().unwrap();
        assert!(reply.contains("bimcraft"), "{reply}");
        assert!(until(&mut h, |h| h.query_by_label("Agents: 1 connected").is_some()), "one agent connected");
        drop(held);
        assert!(until(&mut h, |h| h.query_by_label(&listening).is_some()), "the agent has gone");
        // Closed from Settings: the bar says so, and the port is let go, so nothing answers on it.
        agents_page(&mut h);
        pick(&mut h, Role::CheckBox, "Let agents work on this project");
        assert!(h.state().control.is_none() && !h.state().agents.listen);
        press(&mut h, "Close");
        assert!(h.query_by_label("Agents: off").is_some());
        let start = std::time::Instant::now();
        while TcpStream::connect(("127.0.0.1", port)).is_ok() {
            assert!(start.elapsed() < Duration::from_secs(10), "port {port} still listens");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

#[test]
fn settings_let_agents_only_read_and_an_agents_edit_comes_back_as_an_error_that_names_the_box() {
    let (mut h, file) = controlled("read-only");
    let model = std::sync::Arc::clone(&h.state().session.model);
    let port = entry(&h).port;
    agents_page(&mut h);
    pick(&mut h, Role::CheckBox, "Allow agents to edit this project");
    assert!(!h.state().agents.edit);
    press(&mut h, "Close");
    assert!(h.query_by_label(&format!("Agents: port {port}, read-only")).is_some(), "the bar says agents only read");
    let lines = vec![
        request(&h, json!({"op": "hello"})),
        request(&h, json!({"op": "execute", "command": "wall", "params": {"points": [[0, 7000], [5000, 7000]]}})),
        request(&h, json!({"op": "script", "text": "inspect\nwall {\"points\": [[0, 8000], [5000, 8000]]}", "atomic": true})),
        request(&h, json!({"op": "execute", "command": "undo"})),
        request(&h, json!({"op": "execute", "command": "open", "params": {"path": "/nowhere.bimcraft"}})),
        request(&h, json!({"op": "execute", "command": "inspect"})),
        request(&h, json!({"op": "execute", "command": "schedule", "params": {"category": "walls"}})),
    ];
    let replies = ask(&mut h, lines);
    assert_eq!(replies[0]["ok"]["read_only"], true, "the hello says so: {replies:?}");
    for reply in &replies[1..4] {
        let e = reply["error"].as_str().unwrap_or_default();
        assert!(e.contains("read-only for agents") && e.contains("File > Settings > Agents"), "{reply}");
    }
    let e = replies[4]["error"].as_str().unwrap_or_default();
    assert!(e.contains("would replace the project") && e.contains("File > Open"), "{e}");
    assert_eq!(replies[5]["ok"]["counts"]["wall"], 4, "what reads answers: {replies:?}");
    assert_eq!(replies[6]["ok"]["rows"].as_array().map(Vec::len), Some(4));
    assert!(std::sync::Arc::ptr_eq(&model, &h.state().session.model), "nothing changed in the project");
    assert!(!h.state().dirty());
    // The user's own edits are theirs to make: the window's session is not read-only.
    h.state_mut().run("wall", json!({"points": [[0, 9000], [5000, 9000]]}));
    assert_eq!(walls(&h).len(), 5);
    // Let edit again, the agent's wall goes in.
    agents_page(&mut h);
    pick(&mut h, Role::CheckBox, "Allow agents to edit this project");
    press(&mut h, "Close");
    let wall = request(&h, json!({"op": "execute", "command": "wall", "params": {"points": [[0, 7000], [5000, 7000]]}}));
    let replies = ask(&mut h, vec![wall]);
    assert!(replies[0]["ok"].is_object(), "{replies:?}");
    assert_eq!(walls(&h).len(), 6);
    let _ = std::fs::remove_dir_all(file.parent().unwrap());
}

#[test]
fn the_settings_file_keeps_the_agents_port_and_a_window_opens_the_port_it_keeps() {
    let dir = std::env::temp_dir().join(format!("bimcraft-ui-control-kept-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let settings = dir.join("settings.json");
    let mut h = app(BOX);
    h.state_mut().use_settings(settings.clone());
    h.run();
    agents_page(&mut h);
    pick(&mut h, Role::CheckBox, "Let agents work on this project");
    pick(&mut h, Role::CheckBox, "Allow agents to edit this project");
    press(&mut h, "Close");
    let kept: Value = serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(kept["agents"], json!({"listen": true, "port": 0, "edit": false}));
    // The next window reads them, and its start opens the port they keep (here, any free one), writing its bridge file.
    let bridge = dir.join("bridge.json");
    let mut again = app(BOX);
    again.state_mut().use_settings(settings.clone());
    again.state_mut().use_bridge_file(Some(bridge.clone()));
    again.state_mut().listen_as_kept().unwrap();
    again.run();
    assert!(!again.state().agents.edit);
    let port = entry(&again).port;
    assert_eq!(bimcraft_core::bridge::read_entry(&bridge).map(|e| e.port), Some(port));
    assert!(again.query_by_label(&format!("Agents: port {port}, read-only")).is_some());
    // A port that is taken is said, and the window goes on without one.
    let (mut third, taken) = (app(BOX), std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap());
    third.state_mut().use_settings(settings.clone());
    third.state_mut().agents.port = taken.local_addr().unwrap().port();
    let e = third.state_mut().listen_as_kept().unwrap_err();
    assert!(e.contains("Couldn't listen on port"), "{e}");
    assert!(third.state().control.is_none());
    // What is not a setting of the agents' reads as the defaults: closed, on any port, agents may edit.
    let base = h.state().settings_kept.clone().unwrap();
    let read = crate::settings::Settings::from_json(&json!({"agents": {"listen": "yes", "port": 70000, "edit": 1}}), base.clone());
    assert_eq!((read.agents_listen, read.agents_port, read.agents_edit), (base.agents_listen, base.agents_port, base.agents_edit));
    drop(again);
    assert!(bimcraft_core::bridge::read_entry(&bridge).is_none(), "the window's bridge file goes with it");
    let _ = std::fs::remove_dir_all(&dir);
}
