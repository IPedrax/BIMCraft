//! A Co-Work session on a [`Session`]: the host's ([`Hosting`]: its guests' commands run on it, and what changes goes out to
//! them) and a guest's ([`Joined`]: its copy, kept the host's model; what reads runs on it, what edits goes to the host). A window
//! calls `pump` once a pass and lets these do the rest, so the desktop and the tests drive the same thing. Each also keeps the
//! others in the session ([`Other`]: their name, colour and [`Presence`]) and sends its own window's presence (`present`).

use std::collections::BTreeMap;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bimcraft_core::cowork::{self, Delta, Place, Presence};
use bimcraft_core::engine::Session;
use bimcraft_core::model::{Id, Model};
use serde_json::{Value, json};

use crate::{Code, Guest, GuestEvent, Host, HostConfig, HostEvent, MAX_NAME, Timing, Wake};

/// The version a guest must share with its host.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// How long a guest's window waits for the host's answer to a command.
const ANSWER: Duration = Duration::from_secs(10);
/// The most commands in a guest's batch, and the longest command name and label taken.
const MAX_BATCH: usize = 10_000;
const MAX_LABEL: usize = 100;
/// The longest error from the host a guest keeps, in characters.
const MAX_ERROR: usize = 2_000;

/// What happened that the window says.
#[derive(Clone, Debug, PartialEq)]
pub enum Happened {
    Joined(String),
    /// Who left, and why.
    Left(String, String),
    /// The session is over for this window, and why: the host ended it or went.
    Over(String),
    /// Something went wrong that the window should say.
    Failed(String),
    /// Someone else changed these elements: the window lights them in their colour for a moment.
    Edited(u32, Vec<Id>),
}

/// Someone else in the session, as this window sees them: the host is number 0, the guests their numbers in join order.
#[derive(Clone, Debug, PartialEq)]
pub struct Other {
    pub name: String,
    /// Which of the session's colours is theirs: the host's is 0, a guest's the first free after.
    pub colour: usize,
    pub presence: Presence,
}

/// A presence another window sent, read strictly: none when it isn't one that can be drawn.
fn presence(v: &Value) -> Option<Presence> {
    serde_json::from_value::<Presence>(v.clone()).ok().filter(|p| p.check().is_ok())
}

/// The elements a delta put, to light: none for a whole model.
fn put(d: &Delta) -> Vec<Id> {
    match d {
        Delta::Elements { put, .. } => put.iter().map(|p| p.id).collect(),
        Delta::Whole { .. } => Vec::new(),
    }
}

/// The host's side of a session.
pub struct Hosting {
    host: Host,
    name: String,
    /// Whether guests may edit, or only look.
    pub edit: bool,
    /// The guests by number, which is join order, with the names their steps carry (no two the same, nor the host's), their
    /// colours and where they are.
    people: BTreeMap<u32, Other>,
    /// The host window's own presence, as last sent.
    mine: Option<Presence>,
    /// The model the guests have, and its number.
    sent: Arc<Model>,
    seq: u64,
    /// The project hosted, by its uid.
    project: u64,
}

impl Hosting {
    /// Starts hosting `session`'s project: the host's own steps are `config.name`'s from now on, so its undo is its own.
    pub fn start(config: HostConfig, session: &mut Session) -> io::Result<Hosting> {
        let name = config.name.clone();
        let host = Host::start(config)?;
        session.author = Some(name.clone());
        let project = session.model.uid;
        let sent = Arc::clone(&session.model);
        Ok(Hosting { host, name, edit: true, people: BTreeMap::new(), mine: None, sent, seq: 0, project })
    }

    pub fn host(&self) -> &Host {
        &self.host
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// The project hosted, by its uid: another put in the session's place ends the session.
    pub fn project(&self) -> u64 {
        self.project
    }

    /// The guests in join order, by number and name.
    pub fn people(&self) -> Vec<(u32, String)> {
        self.people.iter().map(|(id, o)| (*id, o.name.clone())).collect()
    }

    /// The guests, with their colours and where they are.
    pub fn others(&self) -> &BTreeMap<u32, Other> {
        &self.people
    }

    /// The host window's presence, sent to the guests when it changed.
    pub fn present(&mut self, p: Presence) {
        if self.mine.as_ref() != Some(&p) {
            self.host.broadcast(&json!({"op": "presence", "from": 0, "presence": p}), None);
            self.mine = Some(p);
        }
    }

    pub fn kick(&self, peer: u32, why: &str) {
        self.host.kick(peer, why);
    }

    /// Ends the session, telling the guests `why`; the session's steps are nobody's again.
    pub fn end(self, session: &mut Session, why: &str) {
        session.author = None;
        self.host.end(why);
    }

    /// Once a pass: guests let in with the project as it is, their commands done and answered, and what changed since the last
    /// pass, however it changed (the window's own commands, its agents, the assistant), sent to every guest.
    pub fn pump(&mut self, session: &mut Session) -> Vec<Happened> {
        let mut said = Vec::new();
        // Another project in the session's place is not the guests' to have: the window ends the session.
        if session.model.uid != self.project {
            return said;
        }
        // The host's own changes since the last pass go out as its own, before a guest's command can be taken for them.
        self.flush(session, 0, &mut said);
        while let Some(e) = self.host.try_event() {
            match e {
                HostEvent::Joined { peer, name, colour, .. } => {
                    let name = self.unique(&name);
                    // What the others have first, so the welcome and the deltas after it agree on the number.
                    self.flush(session, 0, &mut said);
                    match cowork::whole(&session.model) {
                        Ok(d) => {
                            self.host.send(peer, &json!({"op": "welcome", "you": peer, "host": self.name, "seq": self.seq, "delta": d}));
                        }
                        Err(e) => {
                            self.host.kick(peer, "the host couldn't send its project");
                            said.push(Happened::Failed(e));
                            continue;
                        }
                    }
                    self.people.insert(peer, Other { name: name.clone(), colour, presence: Presence::default() });
                    self.send_people();
                    // Then where everyone is (once the newcomer knows who they are), so it sees them before they next move.
                    let seen = self.people.iter().filter(|(id, _)| **id != peer).map(|(id, o)| (*id, &o.presence));
                    for (id, p) in seen.chain(self.mine.as_ref().map(|p| (0, p))) {
                        self.host.send(peer, &json!({"op": "presence", "from": id, "presence": p}));
                    }
                    said.push(Happened::Joined(name));
                }
                HostEvent::Left { peer, why } => {
                    if let Some(o) = self.people.remove(&peer) {
                        self.send_people();
                        said.push(Happened::Left(o.name, why));
                    }
                }
                HostEvent::Message { peer, msg } => self.answer(session, peer, &msg, &mut said),
            }
        }
        self.flush(session, 0, &mut said);
        said
    }

    /// `name`, or with a number after it when another in the session has it.
    fn unique(&self, name: &str) -> String {
        let taken = |n: &str| n == self.name || self.people.values().any(|p| p.name == n);
        (1..=MAX_NAME_TRIES)
            .map(|k| if k == 1 { name.to_string() } else { format!("{name} ({k})") })
            .find(|n| !taken(n))
            .unwrap_or_else(|| name.to_string())
    }

    fn send_people(&self) {
        let people: Vec<Value> = self.people.iter().map(|(id, o)| json!({"id": id, "name": o.name, "colour": o.colour})).collect();
        self.host.broadcast(&json!({"op": "people", "host": self.name, "people": people}), None);
    }

    /// What changed since the guests were last sent the model, sent as `by`'s (0, the host's).
    fn flush(&mut self, session: &Session, by: u32, said: &mut Vec<Happened>) {
        if Arc::ptr_eq(&self.sent, &session.model) {
            return;
        }
        match cowork::delta(&self.sent, &session.model) {
            Ok(d) => {
                self.seq += 1;
                if by != 0 {
                    said.push(Happened::Edited(by, put(&d)));
                }
                self.host.broadcast(&json!({"op": "delta", "seq": self.seq, "by": by, "delta": d}), None);
                self.sent = Arc::clone(&session.model);
            }
            Err(e) => said.push(Happened::Failed(e)),
        }
    }

    /// A guest's message: a command or a batch run as the guest and answered after what it changed has gone out, or the whole
    /// model again for a guest that lost step.
    fn answer(&mut self, session: &mut Session, peer: u32, msg: &Value, said: &mut Vec<Happened>) {
        let Some(name) = self.people.get(&peer).map(|o| o.name.clone()) else { return };
        let made_in = (text(&msg["phase"]), text(&msg["option"]));
        let result = match msg["op"].as_str() {
            Some("execute") => match (msg["command"].as_str().filter(|c| c.len() <= MAX_LABEL), params(&msg["params"])) {
                (Some(command), Some(params)) => cowork::run_for_guest(session, &name, command, &params, made_in, self.edit),
                _ => Err("an `execute` has a `command` and its `params`, an object".into()),
            },
            Some("batch") => batch(&msg["commands"]).and_then(|commands| {
                let label = msg["label"].as_str().unwrap_or("Co-Work").chars().take(MAX_LABEL).collect::<String>();
                cowork::batch_for_guest(session, &name, &label, &commands, made_in, self.edit).map(Value::from)
            }),
            Some("resync") => {
                self.flush(session, 0, said);
                if let Ok(d) = cowork::whole(&session.model) {
                    self.host.send(peer, &json!({"op": "delta", "seq": self.seq, "delta": d}));
                }
                return;
            }
            Some("presence") => {
                if let (Some(p), Some(o)) = (presence(&msg["presence"]), self.people.get_mut(&peer)) {
                    o.presence = p;
                }
                return;
            }
            // The chat is the next slice's.
            _ => return,
        };
        self.flush(session, peer, said);
        let mut done = json!({"op": "done", "ref": msg["ref"]});
        match result {
            Ok(v) => done["ok"] = v,
            Err(e) => done["error"] = json!(e),
        }
        self.host.send(peer, &done);
    }
}

/// How many numbered names are tried for a guest whose name is taken.
const MAX_NAME_TRIES: usize = 100;

fn text(v: &Value) -> Option<String> {
    v.as_str().filter(|s| s.chars().count() <= MAX_LABEL).map(String::from)
}

fn params(v: &Value) -> Option<Value> {
    match v {
        Value::Null => Some(json!({})),
        Value::Object(_) => Some(v.clone()),
        _ => None,
    }
}

/// A batch's commands, each `[command, params]`.
fn batch(v: &Value) -> Result<Vec<(String, Value)>, String> {
    let bad = || "a `batch` has `commands`, each [command, params]".to_string();
    let list = v.as_array().filter(|l| l.len() <= MAX_BATCH).ok_or_else(bad)?;
    list.iter()
        .map(|c| match (c[0].as_str().filter(|n| n.len() <= MAX_LABEL), params(&c[1])) {
            (Some(n), Some(p)) => Ok((n.to_string(), p)),
            _ => Err(bad()),
        })
        .collect()
}

/// A guest's side of a session.
pub struct Joined {
    guest: Guest,
    host: String,
    /// The project's uid: another put in the window's place leaves the session.
    project: u64,
    /// This guest's number, the guests' names in join order, and everyone else with where they are (the host is 0).
    you: u32,
    people: Vec<String>,
    others: BTreeMap<u32, Other>,
    mine: Option<Presence>,
    /// What others changed since the window last looked.
    edited: Vec<(u32, Vec<Id>)>,
    /// The number of the last delta taken in, and whether the whole model was asked for again.
    seq: u64,
    resyncing: bool,
    asked: u64,
    /// Why the session is over, once it is.
    over: Option<String>,
}

impl Joined {
    /// Joins the host at `addr` as `name`: the session and the copy of its project to put in the window's.
    pub fn join(addr: SocketAddr, code: &Code, name: &str, wake: Wake) -> Result<(Joined, Model), String> {
        Joined::join_with(addr, code, name, Timing::default(), wake)
    }

    pub fn join_with(addr: SocketAddr, code: &Code, name: &str, timing: Timing, wake: Wake) -> Result<(Joined, Model), String> {
        let hello = json!({"op": "hello", "version": VERSION, "name": name});
        let (guest, welcome) = Guest::join(addr, code, hello, timing, wake)?;
        let delta: Delta =
            serde_json::from_value(welcome["delta"].clone()).map_err(|e| format!("the host's project didn't come whole: {e}"))?;
        let model = cowork::apply(&Model::new("Project"), &delta)?;
        let host = welcome["host"].as_str().unwrap_or("the host").chars().filter(|c| !c.is_control()).take(MAX_NAME).collect();
        let seq = welcome["seq"].as_u64().unwrap_or(0);
        let you = welcome["you"].as_u64().and_then(|n| u32::try_from(n).ok()).unwrap_or(0);
        let host_other = Other { name: String::clone(&host), colour: 0, presence: Presence::default() };
        let joined = Joined {
            guest,
            host,
            project: model.uid,
            you,
            people: Vec::new(),
            others: BTreeMap::from([(0, host_other)]),
            mine: None,
            edited: Vec::new(),
            seq,
            resyncing: false,
            asked: 0,
            over: None,
        };
        Ok((joined, model))
    }

    /// The host's name.
    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn project(&self) -> u64 {
        self.project
    }

    /// The guests, in join order (this one among them).
    pub fn people(&self) -> &[String] {
        &self.people
    }

    /// Everyone else in the session, the host first, with their colours and where they are.
    pub fn others(&self) -> &BTreeMap<u32, Other> {
        &self.others
    }

    /// This window's presence, sent to the host (which hands it on) when it changed.
    pub fn present(&mut self, p: Presence) {
        if self.mine.as_ref() != Some(&p) && self.guest.send(&json!({"op": "presence", "presence": p})) {
            self.mine = Some(p);
        }
    }

    /// Runs a command as the window's [`Session::execute`] would: on the copy when it only reads, on the host's model when it
    /// changes it (waiting for the answer, and taking in what changed first), or not at all.
    pub fn exec(&mut self, session: &mut Session, command: &str, params: &Value) -> Result<Value, String> {
        match cowork::place(command, params) {
            Place::Here => session.execute(command, params),
            Place::Refused(why) => Err(why),
            Place::Host => {
                let msg = json!({"op": "execute", "command": command, "params": params, "phase": session.phase, "option": session.option});
                self.ask(session, msg)
            }
        }
    }

    /// Several commands as one undo step named `label`, as [`Session::transaction`] runs them.
    pub fn batch(&mut self, session: &mut Session, label: &str, commands: &[(&str, Value)]) -> Result<Vec<Value>, String> {
        let places: Vec<Place> = commands.iter().map(|(c, p)| cowork::place(c, p)).collect();
        if let Some(Place::Refused(why)) = places.iter().find(|p| matches!(p, Place::Refused(_))) {
            return Err(why.clone());
        }
        if places.iter().all(|p| *p == Place::Here) {
            return session.transaction(label, |s| commands.iter().map(|(c, p)| s.execute(c, p)).collect());
        }
        let list: Vec<Value> = commands.iter().map(|(c, p)| json!([c, p])).collect();
        let msg = json!({"op": "batch", "label": label, "commands": list, "phase": session.phase, "option": session.option});
        Ok(self.ask(session, msg)?.as_array().cloned().unwrap_or_default())
    }

    /// Sends `msg` and waits for its answer, taking in the deltas that come before it.
    fn ask(&mut self, session: &mut Session, mut msg: Value) -> Result<Value, String> {
        if let Some(why) = &self.over {
            return Err(why.clone());
        }
        self.asked += 1;
        msg["ref"] = json!(self.asked);
        if !self.guest.send(&msg) {
            return Err("the connection to the host is gone".into());
        }
        let until = Instant::now() + ANSWER;
        loop {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(format!("{} didn't answer in time: the change may still come", self.host));
            }
            match self.guest.wait_event(left) {
                Some(GuestEvent::Message(m)) if m["op"] == "done" && m["ref"] == json!(self.asked) => {
                    return match m["error"].as_str() {
                        Some(e) => Err(e.chars().take(MAX_ERROR).collect()),
                        None => Ok(m["ok"].clone()),
                    };
                }
                Some(GuestEvent::Message(m)) => self.take(session, &m),
                Some(GuestEvent::Ended(why) | GuestEvent::Lost(why)) => {
                    self.over = Some(why.clone());
                    return Err(why);
                }
                None => {}
            }
        }
    }

    /// Once a pass: what the host sent taken in. `Over` once the session is.
    pub fn pump(&mut self, session: &mut Session) -> Vec<Happened> {
        while self.over.is_none() {
            match self.guest.try_event() {
                Some(GuestEvent::Message(m)) => self.take(session, &m),
                Some(GuestEvent::Ended(why) | GuestEvent::Lost(why)) => self.over = Some(why),
                None => break,
            }
        }
        let edited = self.edited.drain(..).map(|(by, ids)| Happened::Edited(by, ids));
        edited.chain(self.over.iter().map(|why| Happened::Over(why.clone()))).collect()
    }

    /// One message of the host's: a delta taken in, in order, or the whole model asked for again when one is missing or doesn't
    /// fit; the people.
    pub(crate) fn take(&mut self, session: &mut Session, m: &Value) {
        match m["op"].as_str() {
            Some("delta") => {
                let (Some(seq), Ok(d)) = (m["seq"].as_u64(), serde_json::from_value::<Delta>(m["delta"].clone())) else {
                    return self.resync();
                };
                let whole = matches!(d, Delta::Whole { .. });
                if seq <= self.seq && !(whole && self.resyncing) {
                    return;
                }
                if !whole && (self.resyncing || seq != self.seq + 1) {
                    return self.resync();
                }
                match cowork::apply(&session.model, &d) {
                    Ok(model) => {
                        session.adopt(model);
                        (self.seq, self.resyncing) = (seq, false);
                        if let Some(by) = m["by"].as_u64().and_then(|b| u32::try_from(b).ok()).filter(|b| *b != self.you)
                            && self.others.contains_key(&by)
                        {
                            self.edited.push((by, put(&d)));
                        }
                    }
                    Err(_) => self.resync(),
                }
            }
            Some("people") => {
                let clean = |n: &str| n.chars().filter(|c| !c.is_control()).take(MAX_NAME).collect::<String>();
                let listed: Vec<(u32, String, usize)> = m["people"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .take(crate::MAX_GUESTS)
                    .filter_map(|p| {
                        let id = p["id"].as_u64().and_then(|n| u32::try_from(n).ok()).filter(|n| *n != 0)?;
                        let colour = p["colour"].as_u64().map_or(0, |c| (c as usize).min(crate::MAX_GUESTS));
                        Some((id, clean(p["name"].as_str()?), colour))
                    })
                    .collect();
                self.people = listed.iter().map(|(_, n, _)| n.clone()).collect();
                // The others now, each keeping where it was.
                let mut others = BTreeMap::from([(
                    0,
                    self.others.remove(&0).unwrap_or(Other { name: self.host.clone(), colour: 0, presence: Presence::default() }),
                )]);
                for (id, name, colour) in listed.into_iter().filter(|(id, ..)| *id != self.you) {
                    let presence = self.others.remove(&id).map(|o| o.presence).unwrap_or_default();
                    others.insert(id, Other { name, colour, presence });
                }
                self.others = others;
            }
            Some("presence") => {
                let from = m["from"].as_u64().and_then(|n| u32::try_from(n).ok());
                if let (Some(p), Some(o)) = (presence(&m["presence"]), from.and_then(|f| self.others.get_mut(&f))) {
                    o.presence = p;
                }
            }
            _ => {}
        }
    }

    fn resync(&mut self) {
        if !std::mem::replace(&mut self.resyncing, true) {
            self.guest.send(&json!({"op": "resync"}));
        }
    }
}
