//! A Co-Work session end to end with no window: a host's session on a thread of its own, pumped as its window would, and a
//! guest's on the test's, joined over this machine's address.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use bimcraft_core::engine::Session;
use bimcraft_core::model::{Element, Model};
use serde_json::{Value, json};

use crate::live::{Happened, Hosting, Joined, VERSION};
use crate::{HostConfig, Timing};

/// What the host's thread is asked to do between its passes.
type Job = Box<dyn FnOnce(&mut Session, &mut Option<Hosting>) + Send>;

struct Host {
    jobs: Sender<Job>,
    /// What the host's passes reported.
    said: Arc<Mutex<Vec<Happened>>>,
    thread: Option<JoinHandle<Session>>,
    port: u16,
    code: crate::Code,
}

impl Host {
    /// The sample house hosted by Ana, pumped every few milliseconds until it is told to stop.
    fn start() -> Host {
        let (jobs, rx) = channel::<Job>();
        let (ready, info) = channel();
        let said = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&said);
        let thread = std::thread::spawn(move || {
            let mut session = Session::new();
            session.script(bimcraft_core::sample::SAMPLE).unwrap();
            let config = HostConfig {
                addr: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
                version: VERSION.into(),
                name: "Ana".into(),
                project: "House".into(),
                discovery: None,
                timing: quick(),
                wake: Arc::new(|| {}),
            };
            let hosting = Hosting::start(config, &mut session).unwrap();
            ready.send((hosting.host().port(), hosting.host().code().clone())).unwrap();
            let mut hosting = Some(hosting);
            loop {
                match rx.recv_timeout(Duration::from_millis(3)) {
                    Ok(job) => job(&mut session, &mut hosting),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(_) => break,
                }
                match hosting.as_mut() {
                    Some(h) => log.lock().unwrap().extend(h.pump(&mut session)),
                    None => break,
                }
            }
            session
        });
        let (port, code) = info.recv().unwrap();
        Host { jobs, said, thread: Some(thread), port, code }
    }

    fn at(&self) -> SocketAddr {
        SocketAddr::from((Ipv4Addr::LOCALHOST, self.port))
    }

    /// `f` done on the host's thread, between two passes, and what it gave.
    fn on<T: Send + 'static>(&self, f: impl FnOnce(&mut Session, &mut Option<Hosting>) -> T + Send + 'static) -> T {
        let (tx, rx) = channel();
        self.jobs.send(Box::new(move |s, h| tx.send(f(s, h)).unwrap())).unwrap();
        rx.recv_timeout(Duration::from_secs(10)).unwrap()
    }

    fn model(&self) -> Arc<Model> {
        self.on(|s, _| Arc::clone(&s.model))
    }

    /// Ends the session and gives back the host's session.
    fn end(mut self, why: &'static str) -> Session {
        self.on(move |s, h| h.take().unwrap().end(s, why));
        self.thread.take().unwrap().join().unwrap()
    }
}

fn quick() -> Timing {
    Timing {
        ping: Duration::from_millis(60),
        silence: Duration::from_secs(2),
        handshake: Duration::from_secs(2),
        lockout: Duration::from_millis(500),
    }
}

/// Bo joins: his copy in a session of his own, as the window keeps it.
fn join(host: &Host, name: &str) -> (Joined, Session) {
    let (joined, model) = Joined::join_with(host.at(), &host.code, name, quick(), Arc::new(|| {})).unwrap();
    let mut s = Session::new();
    s.adopt(model);
    s.read_only = Some("a Co-Work guest's copy");
    (joined, s)
}

fn json_of(m: &Model) -> String {
    m.to_json().unwrap()
}

fn walls(m: &Model) -> usize {
    m.elements.values().filter(|e| matches!(e, Element::Wall(_))).count()
}

/// Pumps the guest until `ok` holds of its copy.
fn until(joined: &mut Joined, s: &mut Session, ok: impl Fn(&Session) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ok(s) {
        assert!(Instant::now() < deadline, "the guest's copy never got there");
        joined.pump(s);
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn wall(y: i64) -> Value {
    json!({"points": [[0, y], [6000, y]]})
}

#[test]
fn a_guest_edits_the_hosts_model_and_both_keep_the_same_one() {
    let host = Host::start();
    let (mut bo, mut copy) = join(&host, "Bo");
    assert_eq!(bo.host(), "Ana");
    assert_eq!(json_of(&copy.model), json_of(&host.model()), "the welcome is the host's project");
    // An edit goes to the host; its answer comes after what it changed, so the copy has it at once.
    let n = walls(&copy.model);
    let made = bo.exec(&mut copy, "wall", &wall(40000)).unwrap();
    assert!(made.is_object() || made.is_array(), "{made}");
    assert_eq!(walls(&copy.model), n + 1);
    assert_eq!(json_of(&copy.model), json_of(&host.model()));
    let top = host.on(|s, _| s.steps(false)[0].by.clone());
    assert_eq!(top.as_deref(), Some("Bo"), "the step is Bo's");
    // A batch is one step of his.
    bo.batch(&mut copy, "Draw", &[("wall", wall(43000)), ("wall", wall(46000))]).unwrap();
    assert_eq!(walls(&copy.model), n + 3);
    assert_eq!(host.on(|s, _| (s.steps(false)[0].label.clone(), s.steps(false)[0].by.clone())), ("Draw".into(), Some("Bo".into())));
    // What reads runs on the copy; what would replace the project isn't done; the copy alone can't change.
    assert!(bo.exec(&mut copy, "inspect", &json!({})).unwrap()["counts"]["wall"].is_number());
    assert!(bo.exec(&mut copy, "open", &json!({"path": "/nowhere.bimc"})).unwrap_err().contains("leave"));
    assert!(copy.execute("wall", &wall(52000)).is_err(), "the copy is read-only");
    // The host's own edit reaches him between commands.
    host.on(|s, _| s.execute("wall", &wall(55000)).map(|_| ()).unwrap());
    until(&mut bo, &mut copy, |s| walls(&s.model) == n + 4);
    // His undo takes back his batch, not the host's wall after it.
    bo.exec(&mut copy, "undo", &json!({})).unwrap();
    assert_eq!(walls(&copy.model), n + 2);
    assert_eq!(walls(&host.model()), n + 2);
    // An error comes back as the host said it.
    let e = bo.exec(&mut copy, "door", &json!({"at": [99000, 99000]})).unwrap_err();
    assert!(e.contains("wall"), "{e}");
    let mut session = host.end("Ana ended the session");
    assert_eq!(session.author, None, "the host's steps are nobody's again");
    assert!(session.execute("undo", &json!({})).is_ok(), "and its undo is the plain one");
    // The ending is on its way: it comes when the guest's reader has it.
    let deadline = Instant::now() + Duration::from_secs(10);
    while !bo.pump(&mut copy).contains(&Happened::Over("Ana ended the session".into())) {
        assert!(Instant::now() < deadline, "the ending never came");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(bo.exec(&mut copy, "wall", &wall(60000)).unwrap_err().contains("ended"));
}

#[test]
fn guests_see_each_other_by_name_and_a_taken_name_gets_a_number() {
    let host = Host::start();
    let (mut a, mut sa) = join(&host, "Bo");
    let (_b, _sb) = join(&host, "Bo");
    let (_c, _sc) = join(&host, "Ana");
    let deadline = Instant::now() + Duration::from_secs(10);
    while a.people().len() < 3 {
        assert!(Instant::now() < deadline, "{:?}", a.people());
        a.pump(&mut sa);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(a.people(), ["Bo", "Bo (2)", "Ana (2)"]);
    let people = host.on(|_, h| h.as_ref().unwrap().people().into_iter().map(|(_, n)| n).collect::<Vec<_>>());
    assert_eq!(people, ["Bo", "Bo (2)", "Ana (2)"]);
}

#[test]
fn a_guest_that_loses_step_asks_for_the_whole_model_and_has_it() {
    let host = Host::start();
    let (mut bo, mut copy) = join(&host, "Bo");
    // A delta far ahead of the last he took: one is missing.
    bo.take(&mut copy, &json!({"op": "delta", "seq": 99, "delta": {"kind": "elements", "next_id": 1, "put": [], "gone": []}}));
    // Meanwhile the host goes on; the whole model he asked for has it.
    host.on(|s, _| s.execute("wall", &wall(40000)).map(|_| ()).unwrap());
    let want = json_of(&host.model());
    until(&mut bo, &mut copy, |s| json_of(&s.model) == want);
    // A delta that is no delta is a lost step too.
    bo.take(&mut copy, &json!({"op": "delta", "seq": 1000, "delta": {"kind": "nonsense"}}));
    host.on(|s, _| s.execute("wall", &wall(43000)).map(|_| ()).unwrap());
    let want = json_of(&host.model());
    until(&mut bo, &mut copy, |s| json_of(&s.model) == want);
}

#[test]
fn guests_only_look_when_the_host_says_so() {
    let host = Host::start();
    host.on(|_, h| h.as_mut().unwrap().edit = false);
    let (mut bo, mut copy) = join(&host, "Bo");
    let n = walls(&copy.model);
    let e = bo.exec(&mut copy, "wall", &wall(40000)).unwrap_err();
    assert!(e.contains("only look"), "{e}");
    assert_eq!((walls(&copy.model), walls(&host.model())), (n, n));
    assert!(bo.exec(&mut copy, "inspect", &json!({})).is_ok());
}

#[test]
fn another_project_in_the_hosts_session_never_reaches_the_guests() {
    let host = Host::start();
    let (mut bo, mut copy) = join(&host, "Bo");
    let joined = json_of(&copy.model);
    host.on(|s, _| s.reset(Model::new("Other"), None));
    host.on(|s, _| s.execute("wall", &wall(40000)).map(|_| ()).unwrap());
    std::thread::sleep(Duration::from_millis(200));
    bo.pump(&mut copy);
    assert_eq!(json_of(&copy.model), joined, "the guest keeps the project it joined");
    assert_ne!(host.on(|_, h| h.as_ref().unwrap().project()), host.model().uid);
}

/// Pumps `who` until `ok` holds of it.
fn pump_until(who: &mut Joined, s: &mut Session, ok: impl Fn(&Joined) -> bool) -> Vec<Happened> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut said = Vec::new();
    while !ok(who) {
        assert!(Instant::now() < deadline, "never got there: {:?}", who.others());
        said.extend(who.pump(s));
        std::thread::sleep(Duration::from_millis(5));
    }
    said
}

#[test]
fn everyone_sees_where_the_others_are_and_who_changed_what_in_their_colours() {
    use bimcraft_core::cowork::Presence;
    use bimcraft_core::geom::P2;
    let host = Host::start();
    let (mut bo, mut sb) = join(&host, "Bo");
    let (mut cy, mut sc) = join(&host, "Cy");
    let ids = host.on(|_, h| h.as_ref().unwrap().people().into_iter().map(|(id, _)| id).collect::<Vec<_>>());
    let (bo_id, cy_id) = (ids[0], ids[1]);
    let w = sb.model.elements.iter().find(|(_, e)| matches!(e, Element::Wall(_))).map(|(id, _)| *id).unwrap();
    let p = Presence { cursor: Some(P2(1000.0, 2000.0)), selection: vec![sb.model.elements.guid(w).unwrap()], ..Default::default() };
    bo.present(p.clone());
    // Cy has it from the host, under Bo's name and colour; so has the host.
    pump_until(&mut cy, &mut sc, |c| c.others().get(&bo_id).is_some_and(|o| o.presence == p));
    let o = &cy.others()[&bo_id];
    assert_eq!((o.name.as_str(), o.colour), ("Bo", 1));
    assert_eq!((cy.others()[&0].name.as_str(), cy.others()[&0].colour), ("Ana", 0), "the host is 0, in colour 0");
    let deadline = Instant::now() + Duration::from_secs(10);
    while host.on(move |_, h| h.as_ref().unwrap().others()[&bo_id].presence.clone()) != p {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    // The host's own, to both.
    let q = Presence { cursor: Some(P2(-500.0, 0.0)), ..Default::default() };
    let sent = q.clone();
    host.on(move |_, h| h.as_mut().unwrap().present(sent));
    pump_until(&mut bo, &mut sb, |b| b.others()[&0].presence == q);
    // What can't be drawn isn't kept: the host keeps Bo where he was.
    bo.present(Presence { cursor: Some(P2(f64::NAN, 0.0)), ..Default::default() });
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(host.on(move |_, h| h.as_ref().unwrap().others()[&bo_id].presence.clone()), p);
    // Someone who comes in late sees everyone at once.
    let (mut di, mut sd) = join(&host, "Di");
    pump_until(&mut di, &mut sd, |d| d.others().get(&bo_id).is_some_and(|o| o.presence == p) && d.others()[&0].presence == q);
    assert_eq!(di.others()[&cy_id].name, "Cy");
    // Bo's wall lights in Bo's colour for the others, and not for Bo.
    let made = bo.exec(&mut sb, "wall", &wall(40000)).unwrap();
    let made = made["id"].as_u64().or_else(|| made["ids"][0].as_u64()).unwrap();
    let edited = Happened::Edited(bo_id, vec![made]);
    assert!(!bo.pump(&mut sb).contains(&edited), "his own edit doesn't light for him");
    let mut said = pump_until(&mut cy, &mut sc, |c| c.others().len() == 3);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !said.contains(&edited) {
        assert!(Instant::now() < deadline, "{said:?}");
        said.extend(cy.pump(&mut sc));
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(host.said.lock().unwrap().contains(&edited), "nor for the host's window");
    // The host's own edit lights for the guests in the host's colour.
    host.on(|s, _| s.execute("wall", &wall(43000)).map(|_| ()).unwrap());
    let deadline = Instant::now() + Duration::from_secs(10);
    while !said.iter().any(|h| matches!(h, Happened::Edited(0, ids) if ids.len() == 1)) {
        assert!(Instant::now() < deadline, "{said:?}");
        said.extend(cy.pump(&mut sc));
        std::thread::sleep(Duration::from_millis(5));
    }
    // One who leaves is no longer among the others.
    drop(di);
    pump_until(&mut cy, &mut sc, |c| c.others().len() == 2);
}

#[test]
fn a_guests_script_runs_on_the_hosts_model_as_one_step_of_theirs() {
    let host = Host::start();
    let (mut bo, mut sb) = join(&host, "Bo");
    let n = walls(&sb.model);
    let script = "wall {\"points\": [[0, 40000], [6000, 40000]]}\nwall {\"points\": [[6000, 40000], [6000, 46000]]}";
    assert_eq!(bo.script(&mut sb, script).unwrap().len(), 2);
    assert_eq!(walls(&sb.model), n + 2, "his copy has them at once");
    let top = host.on(|s, _| (s.steps(false)[0].label.clone(), s.steps(false)[0].by.clone()));
    assert_eq!(top, ("Script".to_string(), Some("Bo".to_string())));
    assert!(bo.script(&mut sb, "new {}").is_err(), "never another project");
    // His Undo list is his own script, from the host.
    pump_until(&mut bo, &mut sb, |b| b.history(false).len() == 1);
    assert_eq!((bo.history(false)[0].label.as_str(), &bo.history(false)[0].made), ("Script", &vec![("wall".to_string(), 2)]));
    bo.exec(&mut sb, "undo", &json!({})).unwrap();
    pump_until(&mut bo, &mut sb, |b| b.history(false).is_empty() && b.history(true).len() == 1);
    assert_eq!(walls(&sb.model), n);
}

#[test]
fn a_guests_import_goes_to_the_host_with_its_file() {
    let host = Host::start();
    let (mut bo, mut sb) = join(&host, "Bo");
    let dir = std::env::temp_dir().join(format!("bimcraft-guest-import-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pat = dir.join("mine.pat");
    std::fs::write(&pat, "*Bo Diagonal, his\n45, 0,0, 0,3.175\n").unwrap();
    bo.exec(&mut sb, "import_fill_patterns", &json!({"path": pat.to_string_lossy()})).unwrap();
    assert!(sb.model.fill_patterns.contains_key("Bo Diagonal"), "his copy has it");
    assert!(host.model().fill_patterns.contains_key("Bo Diagonal"), "the host's project has it");
    // A link stays the host's.
    let e = bo.exec(&mut sb, "import_cad", &json!({"path": pat.to_string_lossy(), "link": true})).unwrap_err();
    assert!(e.contains("host"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn whoever_has_a_key_pays_with_it_and_a_lent_one_is_for_those_who_have_none() {
    use crate::live::{AiKey, payer};
    use std::collections::BTreeMap;
    let key = |lends| AiKey { provider: "anthropic".into(), model: "m".into(), lends };
    let keys = BTreeMap::from([(1, key(false)), (2, key(true)), (4, key(true))]);
    for own_keys in [false, true] {
        assert_eq!(payer(1, true, own_keys, &keys), Some(1), "a guest with a key pays with it");
        assert_eq!(payer(0, true, own_keys, &keys), Some(0));
    }
    // One without: the host's while the host lends it, else the first guest in join order who lends.
    assert_eq!(payer(3, true, false, &keys), Some(0));
    assert_eq!(payer(3, true, true, &keys), Some(2));
    assert_eq!(payer(3, false, false, &keys), Some(2));
    assert_eq!(payer(0, false, false, &keys), Some(2), "the host without a key too");
    // The first lender left: the next.
    assert_eq!(payer(3, false, false, &BTreeMap::from([(1, key(false)), (4, key(true))])), Some(4));
    assert_eq!(payer(3, false, false, &BTreeMap::from([(1, key(false))])), None, "nobody lends");
}

/// Pumps `who` until it hears something `pick` takes.
fn hear<T>(who: &mut Joined, s: &mut Session, pick: impl Fn(&Happened) -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(t) = who.pump(s).iter().find_map(&pick) {
            return t;
        }
        assert!(Instant::now() < deadline, "never heard it");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn the_chat_is_the_hosts_and_a_turn_on_a_guests_key_asks_that_guests_window() {
    use crate::live::AiKey;
    let host = Host::start();
    let (mut bo, mut sb) = join(&host, "Bo");
    let bo_id = host.on(|_, h| h.as_ref().unwrap().people()[0].0);
    bo.ai_key(Some(AiKey { provider: "openai".into(), model: "gpt".into(), lends: true }));
    let deadline = Instant::now() + Duration::from_secs(10);
    while host.on(|_, h| h.as_ref().unwrap().keys().len()) == 0 {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(host.on(move |_, h| h.as_ref().unwrap().keys()[&bo_id].clone()).model, "gpt", "what the host has: never the key");
    // The chat as the host's window sends it, to everyone, and to whoever comes later at once.
    let state = json!({"log": [{"t": "you", "text": "hello"}], "busy": false});
    let sent = state.clone();
    host.on(move |_, h| h.as_mut().unwrap().chat(sent));
    let chat = |h: &Happened| match h {
        Happened::Chat(s) => Some(s.clone()),
        _ => None,
    };
    assert_eq!(hear(&mut bo, &mut sb, chat), state);
    let (mut cy, mut sc) = join(&host, "Cy");
    assert_eq!(hear(&mut cy, &mut sc, chat), state);
    // Bo's message comes to the host's window, and a refusal back to him.
    bo.say("draw a wall", json!([]));
    let deadline = Instant::now() + Duration::from_secs(10);
    while !host
        .said
        .lock()
        .unwrap()
        .iter()
        .any(|h| matches!(h, Happened::ChatSay { peer, name, text, .. } if *peer == bo_id && name == "Bo" && text == "draw a wall"))
    {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    host.on(move |_, h| h.as_ref().unwrap().refuse(bo_id, "the assistant is still answering Ana"));
    hear(&mut bo, &mut sb, |h| matches!(h, Happened::Failed(w) if w.contains("still answering")).then_some(()));
    // A turn on Bo's key: the assistant's thread waits while Bo's window answers.
    let lend = host.on(move |_, h| h.as_ref().unwrap().lend(bo_id, false));
    let asking = std::thread::spawn({
        let lend = lend.clone();
        move || lend.ask("openai", "gpt", json!([{"t": "user", "text": "hi", "files": []}]))
    });
    let (reference, history) = hear(&mut bo, &mut sb, |h| match h {
        Happened::AiRequest { reference, own: false, provider, model, history } if provider == "openai" && model == "gpt" => {
            Some((*reference, history.clone()))
        }
        _ => None,
    });
    assert_eq!(history[0]["text"], "hi");
    bo.ai_reply(reference, Ok(json!({"t": "assistant", "text": "hello"})));
    assert_eq!(asking.join().unwrap().unwrap()["text"], "hello");
    // Bo leaves while a turn waits on him: it ends, saying so.
    let asking = std::thread::spawn(move || lend.ask("openai", "gpt", json!([])));
    hear(&mut bo, &mut sb, |h| matches!(h, Happened::AiRequest { .. }).then_some(()));
    drop(bo);
    let got = asking.join().unwrap();
    assert_eq!(got, Err("Bo left: ask again".into()));
}

#[test]
fn a_knock_comes_to_the_hosts_window_which_lets_it_in_with_the_code() {
    let host = Host::start();
    let addr = host.at();
    let (tx, rx) = std::sync::mpsc::channel();
    let bo = std::thread::spawn(move || crate::knock(addr, "Bo", VERSION, quick(), move |d| drop(tx.send(d.to_string()))));
    let deadline = Instant::now() + Duration::from_secs(10);
    let (k, digits) = loop {
        let found = host.said.lock().unwrap().iter().find_map(|h| match h {
            Happened::Knock { knock, name, digits } if name == "Bo" => Some((*knock, digits.clone())),
            _ => None,
        });
        if let Some(f) = found {
            break f;
        }
        assert!(Instant::now() < deadline, "the knock never came");
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), digits);
    host.on(move |_, h| h.as_ref().unwrap().answer_knock(k, true));
    let code = bo.join().unwrap().unwrap();
    assert_eq!(code, host.code);
    let (joined, _) = Joined::join_with(addr, &code, "Bo", quick(), Arc::new(|| {})).unwrap();
    assert_eq!(joined.host(), "Ana");
}

#[test]
fn a_host_hands_the_session_to_a_guest_and_the_others_move_there() {
    let host = Host::start();
    let (mut bo, mut sb) = join(&host, "Bo");
    let (mut cy, mut sc) = join(&host, "Cy");
    let bo_id = host.on(|_, h| h.as_ref().unwrap().people().into_iter().find(|(_, n)| n == "Bo").unwrap().0);
    host.on(|s, _| s.execute("wall", &wall(40000)).map(|_| ()).unwrap());
    let theirs = json_of(&host.model());
    host.on(move |s, h| h.as_mut().unwrap().hand_over(s, bo_id)).unwrap();
    // Bo is asked to host, and hosts from his copy, which has everything the host's had.
    let deadline = Instant::now() + Duration::from_secs(10);
    while !bo.pump(&mut sb).contains(&Happened::HandOver) {
        assert!(Instant::now() < deadline, "Bo was never asked");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(json_of(&sb.model), theirs);
    sb.read_only = None;
    let config = HostConfig {
        addr: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        version: VERSION.into(),
        name: "Bo".into(),
        project: "House".into(),
        discovery: None,
        timing: quick(),
        wake: Arc::new(|| {}),
    };
    let mut hosting = Hosting::start(config, &mut sb).unwrap();
    bo.hosting(Ok((hosting.host().port(), hosting.host().code().clone())));
    // Cy is told where, and joins Bo there while Bo's window runs.
    let moved = loop {
        let said = cy.pump(&mut sc);
        if let Some(m) = said.iter().find_map(|h| match h {
            Happened::Moved { host, addr, code } => Some((host.clone(), *addr, code.clone())),
            _ => None,
        }) {
            break m;
        }
        assert!(Instant::now() < deadline, "Cy was never told");
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!((moved.0.as_str(), moved.1.port()), ("Bo", hosting.host().port()));
    let rejoin = std::thread::spawn(move || Joined::join_with(moved.1, &moved.2, "Cy", quick(), Arc::new(|| {})));
    while !rejoin.is_finished() {
        hosting.pump(&mut sb);
        std::thread::sleep(Duration::from_millis(3));
    }
    let (_, model) = rejoin.join().unwrap().unwrap();
    assert_eq!(json_of(&model), theirs, "the project as it was, from its new host");
    assert!(host.said.lock().unwrap().contains(&Happened::HandedOver("Bo".into())), "the old host's window hears it is done");
}
