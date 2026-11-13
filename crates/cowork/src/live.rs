//! A Co-Work session on a [`Session`]: the host's ([`Hosting`]: its guests' commands run on it, and what changes goes out to
//! them) and a guest's ([`Joined`]: its copy, kept the host's model; what reads runs on it, what edits goes to the host). A window
//! calls `pump` once a pass and lets these do the rest, so the desktop and the tests drive the same thing. Each also keeps the
//! others in the session ([`Other`]: their name, colour and [`Presence`]) and sends its own window's presence (`present`).
//!
//! The assistant's chat is the host's: a guest's message comes to it as [`Happened::ChatSay`], the window sends everyone the chat as
//! it is ([`Hosting::chat`], JSON the desktop builds), and a turn that runs on a guest's key asks that guest's window through a
//! [`Lend`], which waits on the assistant's thread while the guest's machine makes the request with its own key. No key is ever sent:
//! a guest says only that it has one, for which provider and model, and whether it lends it ([`AiKey`]).

use std::collections::BTreeMap;
use std::io;
use std::net::SocketAddr;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use bimcraft_core::cowork::{self, Delta, Place, Presence};
use bimcraft_core::engine::Session;
use bimcraft_core::model::{Id, Model};
use serde_json::{Value, json};

use crate::{Code, Guest, GuestEvent, Host, HostConfig, HostEvent, MAX_NAME, Mailer, Timing, Wake};

/// The version a guest must share with its host.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// How long a guest's window waits for the host's answer to a command.
const ANSWER: Duration = Duration::from_secs(10);
/// The most commands in a guest's batch, and the longest command name and label taken.
const MAX_BATCH: usize = 10_000;
const MAX_LABEL: usize = 100;
/// The longest error from the host a guest keeps, in characters.
const MAX_ERROR: usize = 2_000;
/// How long a turn on a guest's key waits for that guest's window: its owner may be asked first, and the provider may take a while.
pub const LENT_WAIT: Duration = Duration::from_secs(300);

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
    /// The host's window: a guest's message for the assistant, with its files as the guest sent them (to read strictly).
    ChatSay {
        peer: u32,
        name: String,
        text: String,
        files: Value,
    },
    /// The host's window: a guest stopped the assistant's turn.
    ChatStop(String),
    /// A guest's window: the chat as the host's window sent it (JSON that window builds).
    Chat(Value),
    /// A guest's window: the host asks for a request on this window's key, for a message of its own (`own`) or another's; the
    /// window answers with [`Joined::ai_reply`].
    AiRequest {
        reference: u64,
        own: bool,
        provider: String,
        model: String,
        history: Value,
    },
}

/// A step in a guest's Undo or Redo list, as the host names it: the command's label and the elements it made and took away, by
/// category, which the window names in its own language.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Named {
    pub label: String,
    pub made: Vec<(String, usize)>,
    pub removed: Vec<(String, usize)>,
}

/// What `who`'s undo and redo take back, as the host sends it.
fn history(session: &Session, who: &str) -> Value {
    let list = |redo: bool| -> Vec<Value> {
        let steps = session.own_steps(who, redo);
        steps.iter().map(|st| json!({"label": st.label, "made": st.made, "removed": st.removed})).collect()
    };
    json!({"undo": list(false), "redo": list(true)})
}

/// A list of [`Named`] read strictly: none when any of it isn't one.
fn named(v: &Value) -> Option<Vec<Named>> {
    let tally = |v: &Value| -> Option<Vec<(String, usize)>> {
        let list = v.as_array().filter(|l| l.len() <= MAX_LABEL)?;
        list.iter().map(|p| Some((text(&p[0])?, usize::try_from(p[1].as_u64()?).ok()?))).collect()
    };
    let list = v.as_array().filter(|l| l.len() <= bimcraft_core::engine::UNDO_LIMIT)?;
    list.iter().map(|s| Some(Named { label: text(&s["label"])?, made: tally(&s["made"])?, removed: tally(&s["removed"])? })).collect()
}

/// A guest's AI key as it tells the host, which never has the key itself: the provider and model it is for, and whether it pays for
/// others' messages (its owner said Always, or Ask and hasn't said no).
#[derive(Clone, Debug, PartialEq)]
pub struct AiKey {
    pub provider: String,
    pub model: String,
    pub lends: bool,
}

/// Whose key pays for a message of `sender`'s (0, the host): the sender's own when it has one; for one who has none, a key lent to
/// it: the host's (0) while the host lends it (`own_keys` false), else the first guest in join order who lends. None when nobody can.
pub fn payer(sender: u32, host_key: bool, own_keys: bool, keys: &BTreeMap<u32, AiKey>) -> Option<u32> {
    let has = if sender == 0 { host_key } else { keys.contains_key(&sender) };
    if has {
        return Some(sender);
    }
    if host_key && !own_keys {
        return Some(0);
    }
    keys.iter().find(|(_, k)| k.lends).map(|(id, _)| *id)
}

/// The requests asked of guests' windows that are waiting for an answer, by reference: whose they are and where the answer goes.
#[derive(Default)]
struct Asks {
    next: u64,
    waiting: BTreeMap<u64, (u32, Sender<Result<Value, String>>)>,
}

fn lock(asks: &Mutex<Asks>) -> std::sync::MutexGuard<'_, Asks> {
    asks.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A guest's key, lent for one turn: [`Lend::ask`] runs on the assistant's thread.
#[derive(Clone)]
pub struct Lend {
    mailer: Mailer,
    asks: Arc<Mutex<Asks>>,
    peer: u32,
    name: String,
    own: bool,
}

impl Lend {
    /// The guest's window makes the request with its key (`provider`, `model` and the conversation, `history`) and the provider's
    /// reply comes back, or why not: it refused, left, or took longer than [`LENT_WAIT`].
    pub fn ask(&self, provider: &str, model: &str, history: Value) -> Result<Value, String> {
        let (tx, rx) = channel();
        let reference = {
            let mut asks = lock(&self.asks);
            asks.next += 1;
            let n = asks.next;
            asks.waiting.insert(n, (self.peer, tx));
            n
        };
        let msg = json!({"op": "ai_request", "ref": reference, "own": self.own, "provider": provider, "model": model, "history": history});
        let got = if self.mailer.send(self.peer, &msg) {
            rx.recv_timeout(LENT_WAIT).unwrap_or_else(|_| Err(format!("{} didn't answer in time: ask again", self.name)))
        } else {
            Err(format!("{} left: ask again", self.name))
        };
        lock(&self.asks).waiting.remove(&reference);
        got
    }
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
    /// The guests' AI keys, as they told them, and the chat as the window last sent it (a newcomer has it at once).
    keys: BTreeMap<u32, AiKey>,
    chat: Value,
    asks: Arc<Mutex<Asks>>,
    /// Each guest's Undo and Redo lists as last sent.
    histories: BTreeMap<u32, Value>,
}

impl Hosting {
    /// Starts hosting `session`'s project: the host's own steps are `config.name`'s from now on, so its undo is its own.
    pub fn start(config: HostConfig, session: &mut Session) -> io::Result<Hosting> {
        let name = config.name.clone();
        let host = Host::start(config)?;
        session.author = Some(name.clone());
        let project = session.model.uid;
        let sent = Arc::clone(&session.model);
        let (keys, chat, asks) = (BTreeMap::new(), Value::Null, Arc::default());
        let histories = BTreeMap::new();
        Ok(Hosting { host, name, edit: true, people: BTreeMap::new(), mine: None, sent, seq: 0, project, keys, chat, asks, histories })
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

    /// The guests' AI keys, by number, as they said (join order).
    pub fn keys(&self) -> &BTreeMap<u32, AiKey> {
        &self.keys
    }

    /// Sends everyone the chat as it is now (JSON the window builds), and keeps it for whoever joins next.
    pub fn chat(&mut self, state: Value) {
        self.host.broadcast(&json!({"op": "chat", "state": state}), None);
        self.chat = state;
    }

    /// Tells guest `peer` its message to the assistant wasn't taken, and why.
    pub fn refuse(&self, peer: u32, why: &str) {
        self.host.send(peer, &json!({"op": "chat_refused", "why": why}));
    }

    /// Guest `peer`'s key for one turn, for a message of its own (`own`) or someone else's.
    pub fn lend(&self, peer: u32, own: bool) -> Lend {
        let name = self.people.get(&peer).map_or_else(|| "the guest".into(), |o| o.name.clone());
        Lend { mailer: self.host.mailer(), asks: Arc::clone(&self.asks), peer, name, own }
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
                    if !self.chat.is_null() {
                        self.host.send(peer, &json!({"op": "chat", "state": self.chat}));
                    }
                    said.push(Happened::Joined(name));
                }
                HostEvent::Left { peer, why } => {
                    self.keys.remove(&peer);
                    if let Some(o) = self.people.remove(&peer) {
                        // A turn on this guest's key can't go on.
                        for (who, tx) in lock(&self.asks).waiting.values() {
                            if *who == peer {
                                let _ = tx.send(Err(format!("{} left: ask again", o.name)));
                            }
                        }
                        self.send_people();
                        said.push(Happened::Left(o.name, why));
                    }
                }
                HostEvent::Message { peer, msg } => self.answer(session, peer, &msg, &mut said),
            }
        }
        self.flush(session, 0, &mut said);
        // Each guest's own Undo and Redo lists, when they changed.
        self.histories.retain(|id, _| self.people.contains_key(id));
        for (id, o) in &self.people {
            let now = history(session, &o.name);
            if self.histories.get(id) != Some(&now) {
                self.host.send(*id, &json!({"op": "history", "history": now}));
                self.histories.insert(*id, now);
            }
        }
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
            Some("import") => match (msg["command"].as_str(), params(&msg["params"]), msg["name"].as_str(), msg["bytes"].as_str()) {
                (Some(c), Some(p), Some(n), Some(b)) => cowork::import_for_guest(session, &name, c, &p, n, b, made_in, self.edit),
                _ => Err("an `import` has a `command`, its `params`, the file's `name` and its `bytes`".into()),
            },
            Some("script") => match msg["text"].as_str() {
                Some(text) => cowork::script_for_guest(session, &name, text, made_in, self.edit).map(Value::from),
                None => Err("a `script` has its `text`".into()),
            },
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
            Some("chat_say") => {
                if let Some(text) = msg["text"].as_str() {
                    said.push(Happened::ChatSay { peer, name, text: text.into(), files: msg["files"].clone() });
                }
                return;
            }
            Some("chat_stop") => return said.push(Happened::ChatStop(name)),
            Some("ai_key") => {
                match (text(&msg["provider"]), text(&msg["model"])) {
                    (Some(provider), Some(model)) => {
                        self.keys.insert(peer, AiKey { provider, model, lends: msg["lends"] == json!(true) });
                    }
                    _ => {
                        self.keys.remove(&peer);
                    }
                }
                return;
            }
            Some("ai_reply") => {
                let reference = msg["ref"].as_u64().unwrap_or(0);
                // Only the guest that was asked answers.
                if let Some((_, tx)) = lock(&self.asks).waiting.get(&reference).filter(|(who, _)| *who == peer) {
                    let _ = tx.send(match msg["error"].as_str() {
                        Some(e) => Err(e.chars().take(MAX_ERROR).collect()),
                        None => Ok(msg["reply"].clone()),
                    });
                }
                return;
            }
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
    /// This window's AI key as last told.
    key: Option<AiKey>,
    /// What this guest's undo and redo would take back, as the host last said.
    undo: Vec<Named>,
    redo: Vec<Named>,
    /// What else the host said that the window hears at its next pass.
    said: Vec<Happened>,
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
            key: None,
            said: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
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

    /// What this guest's undo (or with `redo`, redo) would take back, the next first: its Undo and Redo lists.
    pub fn history(&self, redo: bool) -> &[Named] {
        if redo { &self.redo } else { &self.undo }
    }

    /// Sends a message for the assistant, with its files (JSON the window builds).
    pub fn say(&self, text: &str, files: Value) -> bool {
        self.guest.send(&json!({"op": "chat_say", "text": text, "files": files}))
    }

    /// Stops the assistant's turn.
    pub fn stop_chat(&self) {
        self.guest.send(&json!({"op": "chat_stop"}));
    }

    /// Tells the host this window's AI key, when it changed: none, or which provider and model it is for and whether it lends it.
    pub fn ai_key(&mut self, key: Option<AiKey>) {
        if self.key == key {
            return;
        }
        let msg = match &key {
            Some(k) => json!({"op": "ai_key", "provider": k.provider, "model": k.model, "lends": k.lends}),
            None => json!({"op": "ai_key"}),
        };
        if self.guest.send(&msg) {
            self.key = key;
        }
    }

    /// The answer to the host's [`Happened::AiRequest`]: the provider's reply, or why there is none.
    pub fn ai_reply(&self, reference: u64, result: Result<Value, String>) {
        let msg = match result {
            Ok(reply) => json!({"op": "ai_reply", "ref": reference, "reply": reply}),
            Err(e) => json!({"op": "ai_reply", "ref": reference, "error": e}),
        };
        self.guest.send(&msg);
    }

    /// Runs a command as the window's [`Session::execute`] would: on the copy when it only reads, on the host's model when it
    /// changes it (waiting for the answer, and taking in what changed first), or not at all.
    pub fn exec(&mut self, session: &mut Session, command: &str, params: &Value) -> Result<Value, String> {
        match cowork::place(command, params) {
            Place::Here => session.execute(command, params),
            // A file of this guest's own goes to the host with the command, which imports it there.
            Place::Refused(why) => match cowork::guest_import(command, params) {
                Some(path) => self.import(session, command, params, &path),
                None => Err(why),
            },
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

    /// An import of a file on this guest's machine: its bytes go to the host with the command, at most [`cowork::MAX_IMPORT`].
    fn import(&mut self, session: &mut Session, command: &str, params: &Value, path: &str) -> Result<Value, String> {
        let too_big = || format!("{path} is bigger than the {} MB a guest sends the host to import", cowork::MAX_IMPORT >> 20);
        if std::fs::metadata(path).map_err(|e| format!("couldn't read {path}: {e}"))?.len() > cowork::MAX_IMPORT as u64 {
            return Err(too_big());
        }
        // Read once more within the cap: the file may have grown since.
        let bytes = std::fs::read(path).map_err(|e| format!("couldn't read {path}: {e}"))?;
        if bytes.len() > cowork::MAX_IMPORT {
            return Err(too_big());
        }
        let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let msg = json!({"op": "import", "command": command, "params": params, "name": name,
            "bytes": cowork::base64(&bytes), "phase": session.phase, "option": session.option});
        self.ask(session, msg)
    }

    /// A script (Manage > Run Script) run on the host's model as one undo step of this guest's, once its dry run passes there.
    pub fn script(&mut self, session: &mut Session, text: &str) -> Result<Vec<Value>, String> {
        let msg = json!({"op": "script", "text": text, "phase": session.phase, "option": session.option});
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
        let said = edited.chain(self.said.drain(..)).collect::<Vec<_>>();
        said.into_iter().chain(self.over.iter().map(|why| Happened::Over(why.clone()))).collect()
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
            Some("chat") => self.said.push(Happened::Chat(m["state"].clone())),
            Some("history") => {
                if let (Some(undo), Some(redo)) = (named(&m["history"]["undo"]), named(&m["history"]["redo"])) {
                    (self.undo, self.redo) = (undo, redo);
                }
            }
            Some("chat_refused") => {
                let why = m["why"].as_str().unwrap_or("the assistant didn't take the message");
                self.said.push(Happened::Failed(why.chars().take(MAX_ERROR).collect()));
            }
            Some("ai_request") => {
                if let (Some(reference), Some(provider), Some(model)) = (m["ref"].as_u64(), text(&m["provider"]), text(&m["model"])) {
                    let (own, history) = (m["own"] == json!(true), m["history"].clone());
                    self.said.push(Happened::AiRequest { reference, own, provider, model, history });
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
