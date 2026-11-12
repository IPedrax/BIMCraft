//! A Co-Work session end to end with no window: a host's session on a thread of its own, pumped as its window would, and a
//! guest's on the test's, joined over this machine's address.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::mpsc::{Sender, channel};
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
    thread: Option<JoinHandle<Session>>,
    port: u16,
    code: crate::Code,
}

impl Host {
    /// The sample house hosted by Ana, pumped every few milliseconds until it is told to stop.
    fn start() -> Host {
        let (jobs, rx) = channel::<Job>();
        let (ready, info) = channel();
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
                    Some(h) => {
                        h.pump(&mut session);
                    }
                    None => break,
                }
            }
            session
        });
        let (port, code) = info.recv().unwrap();
        Host { jobs, thread: Some(thread), port, code }
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
    assert_eq!(bo.pump(&mut copy), vec![Happened::Over("Ana ended the session".into())]);
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
