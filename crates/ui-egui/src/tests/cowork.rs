//! Co-Work (COWORK-PLAN.md): the interface changes the project only through `BimApp::exec` and `exec_batch`, so that a guest's
//! commands go to the host instead of to a copy that would drift from it; and the window hosting a session and joining one, each
//! against the other side run with no window, as `bimcraft_cowork::live` runs it.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

use bimcraft_core::model::Model;
use bimcraft_cowork::live::{Happened, Hosting, Joined, VERSION};
use bimcraft_cowork::{Code, HostConfig, Timing};
use serde_json::json;

use super::*;
use crate::cowork::{GUEST, Live, Prefs};

/// The interface's own sources, outside the tests: each file's path from `src` and its text.
fn sources() -> Vec<(String, String)> {
    fn walk(dir: &std::path::Path, root: &std::path::Path, out: &mut Vec<(String, String)>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
            if name == "tests" || name == "tests.rs" {
                continue;
            }
            if path.is_dir() {
                walk(&path, root, out);
            } else if name.ends_with(".rs") {
                out.push((name, std::fs::read_to_string(&path).unwrap()));
            }
        }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    walk(&root, &root, &mut out);
    out
}

#[test]
fn the_interface_changes_the_project_only_through_exec() {
    let files = sources();
    assert!(files.len() > 50, "the sources were found: {}", files.len());
    let mut found = Vec::new();
    for (name, text) in &files {
        // A file's own tests, at its end, drive the session as a test does.
        let text = text.split("#[cfg(test)]\nmod tests {").next().unwrap_or(text);
        // Joined, so a call split over lines (`self.session` and then `.transaction(`) is still one.
        let joined: String = text.split_whitespace().collect();
        for call in ["self.session.execute(", "self.session.transaction(", "self.session.script("] {
            found.extend(std::iter::repeat_n(format!("{name}: {call}"), joined.matches(call).count()));
        }
    }
    // The funnel itself, once each.
    let funnel = ["lib.rs: self.session.execute(", "lib.rs: self.session.transaction("];
    for f in funnel {
        let at = found.iter().position(|x| x == f).unwrap_or_else(|| panic!("the funnel is there: {f}"));
        found.remove(at);
    }
    assert_eq!(found, Vec::<String>::new(), "call BimApp::exec or exec_batch instead");
}

fn quick() -> Timing {
    Timing {
        ping: Duration::from_millis(100),
        silence: Duration::from_secs(3),
        handshake: Duration::from_secs(3),
        lockout: Duration::from_millis(500),
    }
}

/// A window that hosts as Ana, on this machine's address, out of sight of the network.
fn host_window() -> Harness<'static, BimApp> {
    let mut h = app(BOX);
    h.state_mut().cowork_prefs = Prefs { name: "Ana".into(), addr: "127.0.0.1".into(), port: 0, visible: false, edit: true };
    shortcut(&mut h, "CW");
    press(&mut h, "Start Co-Work");
    h
}

fn hosted(h: &Harness<BimApp>) -> (SocketAddr, Code) {
    match &h.state().cowork {
        Some(Live::Host(hosting)) => (SocketAddr::from((Ipv4Addr::LOCALHOST, hosting.host().port())), hosting.host().code().clone()),
        _ => panic!("the window hosts"),
    }
}

/// Runs the window's frames until `done` holds, or fails.
fn frames_until(h: &mut Harness<BimApp>, done: impl Fn(&Harness<BimApp>) -> bool) {
    let until = Instant::now() + Duration::from_secs(20);
    while !done(h) {
        assert!(Instant::now() < until, "the window never got there: {:?}", h.state().status);
        h.step();
        std::thread::sleep(Duration::from_millis(3));
    }
}

fn wall_count(m: &Model) -> usize {
    m.elements.values().filter(|e| e.category() == "wall").count()
}

#[test]
fn co_work_opens_from_its_shortcut_and_the_collaborate_tab_in_both_profiles() {
    for mut h in [app(BOX), bimcraft(BOX)] {
        shortcut(&mut h, "CW");
        assert!(h.state().cowork_dialog.is_some());
        h.get_by_label("Start Co-Work");
        h.get_by_label("Join");
        h.state_mut().cowork_dialog = None;
        h.run();
    }
    let mut h = app(BOX);
    press(&mut h, "Collaborate");
    press(&mut h, "Co-Work");
    assert!(h.state().cowork_dialog.is_some());
}

#[test]
fn a_window_hosts_and_a_guest_edits_its_project_even_with_no_frame_drawn() {
    let mut h = host_window();
    let (addr, code) = hosted(&h);
    assert_eq!(h.state().session.author.as_deref(), Some("Ana"), "the host's steps are Ana's");
    assert!(h.state().title.contains("Co-Work"), "{}", h.state().title);
    h.get_by_label(&code.to_string());
    // It says where it listens: here, this machine's own address.
    assert!(h.query_by_label_contains("Address: 127.0.0.1, port").is_some());
    let n = wall_count(&h.state().session.model);
    // Bo, with no window, joins and draws a wall while the host's window runs its frames.
    let (ready, go) = (channel(), channel::<()>());
    let bo = std::thread::spawn(move || {
        let (mut bo, model) = Joined::join(addr, &code, "Bo", Arc::new(|| {})).unwrap();
        let mut copy = bimcraft_core::engine::Session::new();
        copy.adopt(model);
        copy.read_only = Some("a guest's copy");
        bo.exec(&mut copy, "wall", &json!({"points": [[0, 9000], [5000, 9000]]})).unwrap();
        ready.0.send(wall_count(&copy.model)).unwrap();
        // Then once more, while the window draws nothing: `pump` alone, as `logic` runs it for a minimised window.
        go.1.recv().unwrap();
        bo.exec(&mut copy, "wall", &json!({"points": [[0, 12000], [5000, 12000]]})).unwrap();
        wall_count(&copy.model)
    });
    let seen = loop {
        h.step();
        if let Ok(n) = ready.1.try_recv() {
            break n;
        }
        std::thread::sleep(Duration::from_millis(3));
    };
    assert_eq!((seen, wall_count(&h.state().session.model)), (n + 1, n + 1));
    assert_eq!(h.state().session.steps(false)[0].by.as_deref(), Some("Bo"));
    assert!(h.state().status.text.contains("Bo joined"), "{:?}", h.state().status);
    go.0.send(()).unwrap();
    let ctx = h.ctx.clone();
    while !bo.is_finished() {
        h.state_mut().pump(&ctx);
        std::thread::sleep(Duration::from_millis(3));
    }
    assert_eq!(bo.join().unwrap(), n + 2);
    assert_eq!(wall_count(&h.state().session.model), n + 2);
    // The host's own undo is its own: Bo's walls stay.
    h.state_mut().exec("wall", &json!({"points": [[0, 15000], [5000, 15000]]})).unwrap();
    h.state_mut().exec("undo", &json!({})).unwrap();
    assert_eq!(wall_count(&h.state().session.model), n + 2);
}

#[test]
fn a_window_that_opens_another_project_ends_the_session_it_hosts() {
    let mut h = host_window();
    let (addr, code) = hosted(&h);
    let bo = std::thread::spawn(move || {
        let (mut bo, model) = Joined::join(addr, &code, "Bo", Arc::new(|| {})).unwrap();
        let mut copy = bimcraft_core::engine::Session::new();
        copy.adopt(model);
        let until = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(Happened::Over(why)) = bo.pump(&mut copy).pop() {
                return why;
            }
            assert!(Instant::now() < until, "the session never ended");
            std::thread::sleep(Duration::from_millis(5));
        }
    });
    frames_until(&mut h, |h| h.state().status.text.contains("Bo joined"));
    h.state_mut().session.reset(Model::new("Other"), None);
    frames_until(&mut h, |_| bo.is_finished());
    let why = bo.join().unwrap();
    assert!(why.contains("Ana opened another project"), "{why}");
    assert!(h.state().cowork.is_none());
    assert_eq!(h.state().session.author, None);
}

#[test]
fn a_window_joins_a_session_edits_through_the_host_and_keeps_a_read_only_copy_when_it_ends() {
    // Ana hosts with no window, on a thread of her own.
    let (info, stop) = (channel(), channel::<&'static str>());
    let ana = std::thread::spawn(move || {
        let mut s = bimcraft_core::engine::Session::new();
        s.script(BOX).unwrap();
        let config = HostConfig {
            addr: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
            version: VERSION.into(),
            name: "Ana".into(),
            project: "Box".into(),
            discovery: None,
            timing: quick(),
            wake: Arc::new(|| {}),
        };
        let mut hosting = Hosting::start(config, &mut s).unwrap();
        info.0.send((hosting.host().port(), hosting.host().code().clone())).unwrap();
        loop {
            hosting.pump(&mut s);
            if let Ok(why) = stop.1.try_recv() {
                hosting.end(&mut s, why);
                return s;
            }
            std::thread::sleep(Duration::from_millis(3));
        }
    });
    let (port, code) = info.1.recv().unwrap();
    let mut h = app(r#"wall {"points": [[0,0],[3000,0]]}"#);
    h.state_mut().cowork_prefs.name = "Bo".into();
    // A project with unsaved changes isn't replaced by joining.
    shortcut(&mut h, "CW");
    {
        let d = h.state_mut().cowork_dialog.as_mut().unwrap();
        (d.code, d.address) = (code.to_string().to_lowercase(), format!("127.0.0.1:{port}"));
    }
    h.state_mut().exec("wall", &json!({"points": [[0, 3000], [3000, 3000]]})).unwrap();
    h.run();
    press(&mut h, "Join");
    let error = h.state().cowork_dialog.as_ref().and_then(|d| d.error.clone()).unwrap_or_default();
    assert!(error.contains("Save"), "{error}");
    assert!(!h.state().guest());
    // Saved, it joins.
    h.state_mut().saved = Arc::clone(&h.state().session.model);
    press(&mut h, "Join");
    frames_until(&mut h, |h| h.state().guest());
    // The title follows on the next frame.
    h.run();
    assert!(h.state().title.contains("Ana's Co-Work"), "{}", h.state().title);
    assert_eq!(h.state().session.read_only, Some(GUEST));
    let n = wall_count(&h.state().session.model);
    assert_eq!(n, 4, "the host's project, not the window's own");
    // An edit goes to Ana; what only reads runs here; the copy alone can't change.
    h.state_mut().exec("wall", &json!({"points": [[0, 9000], [5000, 9000]]})).unwrap();
    assert_eq!(wall_count(&h.state().session.model), n + 1);
    assert!(h.state_mut().exec("inspect", &json!({})).is_ok());
    assert!(h.state_mut().session.execute("wall", &json!({"points": [[0, 12000], [5000, 12000]]})).is_err());
    assert!(h.state_mut().exec("new", &json!({})).unwrap_err().contains("leave"));
    // Ana ends it: the window says so and keeps the project, read-only.
    stop.0.send("Ana ended the session").unwrap();
    let hosted = ana.join().unwrap();
    assert_eq!(wall_count(&hosted.model), n + 1);
    frames_until(&mut h, |h| !h.state().guest());
    assert!(h.state().status.text.contains("Ana ended the session"), "{:?}", h.state().status);
    assert_eq!(h.state().session.read_only, Some(GUEST));
    // A project of the user's own is theirs to change.
    h.state_mut().session.reset(Model::new("Mine"), None);
    h.run();
    assert_eq!(h.state().session.read_only, None);
}

#[test]
fn a_join_address_is_read_at_once_and_a_name_is_looked_up_on_the_join_thread() {
    let code = Code::new().unwrap();
    let here = |typed: &str| super::super::cowork::tests_address(typed, &code);
    assert_eq!(here("192.168.1.20").unwrap(), Some(SocketAddr::from(([192, 168, 1, 20], bimcraft_cowork::PORT))));
    assert_eq!(here(" 10.0.0.5:5000 ").unwrap(), Some(SocketAddr::from(([10, 0, 0, 5], 5000))));
    assert_eq!(here("[::1]:41730").unwrap().map(|a| a.port()), Some(41730));
    assert_eq!(here("ana-laptop.local").unwrap(), None, "a name waits for the thread");
    assert!(here("").unwrap_err().contains("address"), "no host on this network has the code");
}

/// What Co-Work looks like, for a person to look at: `BIMCRAFT_COWORK_DUMP=<folder>` writes the Join dialog, the hosting one
/// and Settings > Co-Work, light and dark.
#[test]
fn co_work_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_COWORK_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    for (dark, look) in [(false, "light"), (true, "dark")] {
        let save = |h: &mut Harness<BimApp>, name: &str| {
            if dark {
                h.ctx.set_theme(egui::Theme::Dark);
            }
            h.run();
            h.render().unwrap().save(std::path::Path::new(&folder).join(format!("cowork-{name}-{look}.png"))).unwrap();
        };
        let mut h = app(BOX);
        shortcut(&mut h, "CW");
        h.state_mut().cowork_dialog.as_mut().unwrap().code = "7KQ2-M9XD-4TPA-R3HC".into();
        save(&mut h, "join");
        let mut h = host_window();
        save(&mut h, "host");
        let mut h = bimcraft(BOX);
        h.state_mut().settings_page = Some(crate::settings::Page::Cowork);
        save(&mut h, "settings");
        // Bo in the session: his pointer and his selection on the plan, his badge on the bar.
        let mut h = host_window();
        h.state_mut().cowork_dialog = None;
        let key = level(&h, "Level 1");
        let wall = ids(&h, "wall")[0];
        let (plan, guid) = (h.state().session.model.elements.guid(key), h.state().session.model.elements.guid(wall).unwrap());
        let (_, tx, thread) = bo_at(&mut h, move |_| bimcraft_core::cowork::Presence {
            view: Some(bimcraft_core::cowork::On { kind: "plan".into(), of: plan }),
            cursor: Some(P2(4000.0, 3000.0)),
            selection: vec![guid],
            ..Default::default()
        });
        save(&mut h, "presence");
        drop(tx);
        thread.join().unwrap();
    }
}

/// Bo, with no window, in `h`'s session: joins, says where he is (`p`, made from his copy of the project), and stays until the
/// sender is dropped. His number in the session comes back.
fn bo_at(
    h: &mut Harness<'static, BimApp>,
    p: impl FnOnce(&Model) -> bimcraft_core::cowork::Presence + Send + 'static,
) -> (u32, std::sync::mpsc::Sender<Option<Value>>, std::thread::JoinHandle<()>) {
    let (addr, code) = hosted(h);
    let (tx, rx) = channel::<Option<Value>>();
    let bo = std::thread::spawn(move || {
        let (mut bo, model) = Joined::join(addr, &code, "Bo", Arc::new(|| {})).unwrap();
        let mut copy = bimcraft_core::engine::Session::new();
        let p = p(&model);
        copy.adopt(model);
        bo.present(p);
        loop {
            match rx.recv_timeout(Duration::from_millis(5)) {
                // A wall to draw, as Bo.
                Ok(Some(params)) => drop(bo.exec(&mut copy, "wall", &params)),
                Ok(None) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                Err(_) => drop(bo.pump(&mut copy)),
            }
        }
    });
    frames_until(h, |h| crate::cowork::others_of(&h.state().cowork).is_some_and(|o| o.values().any(|o| o.presence != Default::default())));
    let id = *crate::cowork::others_of(&h.state().cowork).unwrap().keys().next().unwrap();
    (id, tx, bo)
}

fn level(h: &Harness<BimApp>, name: &str) -> Id {
    h.state().session.model.levels().into_iter().find(|(_, l)| l.name == name).map(|(id, _)| id).unwrap()
}

#[test]
fn the_others_show_on_the_plan_they_are_on_in_their_colours_and_in_properties() {
    let mut h = host_window();
    h.state_mut().cowork_dialog = None;
    let key = level(&h, "Level 1");
    let wall = ids(&h, "wall")[0];
    let (plan, wall_guid) = (h.state().session.model.elements.guid(key), h.state().session.model.elements.guid(wall).unwrap());
    let (bo, tx, thread) = bo_at(&mut h, move |_| bimcraft_core::cowork::Presence {
        view: Some(bimcraft_core::cowork::On { kind: "plan".into(), of: plan }),
        cursor: Some(P2(2000.0, 3000.0)),
        selection: vec![wall_guid],
        ..Default::default()
    });
    let marks = |h: &Harness<BimApp>, kind: &str| {
        let s = h.state();
        let here = bimcraft_core::cowork::On { kind: kind.into(), of: plan };
        crate::cowork::marks(crate::cowork::others_of(&s.cowork).unwrap(), &s.session.model, &here, &s.cowork_seen)
    };
    let on_plan = marks(&h, "plan");
    assert_eq!(on_plan.pointers, vec![("Bo".to_string(), 1, P2(2000.0, 3000.0))], "his pointer, in his colour");
    assert_eq!(on_plan.lit.get(&wall), Some(&1), "his selection, in his colour");
    assert!(marks(&h, "ceiling").pointers.is_empty(), "his pointer is on another view");
    // Selected here too: Properties says he has it.
    h.state_mut().selection = vec![wall];
    h.run();
    assert!(h.query_by_label("Bo has this selected").is_some());
    // What he draws lights in his colour for a moment, selected or not.
    let before = ids(&h, "wall");
    tx.send(Some(json!({"points": [[0, 9000], [5000, 9000]]}))).unwrap();
    frames_until(&mut h, |h| ids(h, "wall").len() > before.len());
    let made = *ids(&h, "wall").iter().find(|id| !before.contains(id)).unwrap();
    frames_until(&mut h, |h| marks(h, "plan").lit.get(&made) == Some(&1));
    // The window's own badge for him on the view control bar.
    assert!(h.query_by_label("Bo").is_some());
    let _ = bo;
    drop(tx);
    thread.join().unwrap();
}

#[test]
fn following_someone_shows_their_view_until_you_move_it_yourself() {
    let mut h = host_window();
    h.state_mut().cowork_dialog = None;
    let up = level(&h, "Level 2");
    let plan = h.state().session.model.elements.guid(up);
    let (bo, tx, thread) = bo_at(&mut h, move |_| bimcraft_core::cowork::Presence {
        view: Some(bimcraft_core::cowork::On { kind: "plan".into(), of: plan }),
        zoom: Some((1000.0, 500.0, 0.2)),
        ..Default::default()
    });
    // As the other tests click the view control bar, which floats over the view.
    h.get_by_label("Bo").click_accesskit();
    h.run();
    assert_eq!(h.state().cowork_seen.following, Some(bo));
    frames_until(&mut h, |h| h.state().sheet == Sheet::Plan(up));
    let v = h.state().canvas.view(up).unwrap();
    assert_eq!((v.cx, v.cy, v.scale), (1000.0, 500.0, 0.2), "his zoom");
    // Moving the view stops it.
    h.state_mut().canvas.set_view(up, 0.0, 0.0, 0.1);
    frames_until(&mut h, |h| h.state().cowork_seen.following.is_none());
    assert!(h.state().status.text.contains("stopped following Bo"), "{:?}", h.state().status);
    drop(tx);
    thread.join().unwrap();
}

#[test]
fn a_window_says_where_it_is_and_a_3d_view_boxes_what_the_others_selected() {
    let mut h = host_window();
    h.state_mut().cowork_dialog = None;
    let wall = ids(&h, "wall")[0];
    let guid = h.state().session.model.elements.guid(wall).unwrap();
    let camera = bimcraft_core::view3d::Camera {
        azimuth: 225.0,
        elevation: 30.0,
        scale: 0.05,
        target: [4000.0, 3000.0, 1500.0],
        width: 800,
        height: 600,
        lens: Some(35.0),
    };
    let (_, tx, thread) = bo_at(&mut h, move |_| bimcraft_core::cowork::Presence {
        view: Some(bimcraft_core::cowork::On { kind: "3d".into(), of: None }),
        camera: Some(bimcraft_core::cowork::Look::of(&camera)),
        selection: vec![guid],
        ..Default::default()
    });
    // The host's own presence: its plan and its selection.
    h.state_mut().selection = vec![wall];
    h.run();
    let mine = h.state().my_presence();
    assert_eq!(mine.view.as_ref().map(|v| v.kind.as_str()), Some("plan"));
    assert_eq!(mine.selection, vec![guid]);
    assert!(mine.zoom.is_some() && mine.camera.is_none());
    // In 3D, Bo's selection is boxed in his colour.
    shortcut(&mut h, "3D");
    frames_until(&mut h, |h| h.state().cowork_seen.boxes.is_some());
    let (_, _, boxes) = h.state().cowork_seen.boxes.clone().unwrap();
    assert_eq!(boxes.len(), 1, "his one wall");
    assert_eq!(boxes[0].0, 1, "in his colour");
    assert!(h.state().my_presence().camera.is_some(), "the camera goes with a 3D view");
    drop(tx);
    thread.join().unwrap();
}
