//! The chat: a conversation with a model that works on a [`Session`]. The window calls [`Chat::poll`] once a frame. A request to
//! the provider runs on a thread of its own (never the window's), and its reply is taken in the frame, where the tool calls in it
//! run on the window's session, as a command from a button does. Each batch of tool calls is one transaction named
//! [`LABEL`], and when the turn ends the batches merge into one undo step, `Assistant: <what the user asked>`.

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use bimcraft_core::bridge::Access;
use bimcraft_core::engine::Session;

use crate::attach::{self, Attached, Attachment};
use crate::net::Net;
use crate::tools::{self, TODO, Todo, to_text};
use crate::{
    Call, Done, LABEL, MAX_CALLS, MAX_FILES_TOTAL, MAX_HISTORY, MAX_ROUNDS, MAX_TEXT, Msg, Provider, Reply, Usage, clip, complete, key_ok,
    model_ok,
};

/// How many lines of the log are kept.
const MAX_LOG: usize = 500;
/// The most of an earlier conversation, in characters, that a model which didn't have it is told as text.
const MAX_FOLD: usize = 30_000;

/// What the palette shows, in order.
#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    You(String),
    /// The names of the files that went with the message just above.
    Files(Vec<String>),
    /// What the model said of its reasoning, apart from its answer.
    Thinks(String),
    Says(String),
    /// A tool the model ran: its name, its arguments and what came back, both cut short for the eye.
    Used {
        name: String,
        args: String,
        result: String,
        error: bool,
    },
    /// Something the user should know: an error from the provider, a limit reached.
    Note(String),
}

/// Asks the model on another window's key, in a Co-Work session: the provider, the model and the conversation go there and the
/// reply comes back. Called on the turn's thread; an `Err` (that window left, or its provider refused) is shown and ends the turn.
pub type Remote = Arc<dyn Fn(Provider, &str, &[Msg]) -> Result<Reply, String> + Send + Sync>;

/// Whose key a turn runs on.
#[derive(Clone)]
pub enum Pays {
    /// This window's: the tokens are counted here.
    Key(String),
    /// Another window's: it counts them.
    Remote(Remote),
}

/// A message of the user's that is being worked on.
struct Turn {
    /// The undo step it ends as.
    label: String,
    pays: Pays,
    provider: Provider,
    model: String,
    /// Who asked, in a Co-Work session: the steps of the turn are theirs.
    by: Option<String>,
    rounds: usize,
}

pub struct Chat {
    pub provider: Provider,
    pub model: String,
    pub log: Vec<Line>,
    pub(crate) history: Vec<Msg>,
    net: Arc<dyn Net>,
    /// What wakes the window when a reply has come: the window sets it before it sends.
    pub wake: Arc<dyn Fn() + Send + Sync>,
    flight: Option<Receiver<Result<Reply, String>>>,
    turn: Option<Turn>,
    /// The model's checklist, as it last wrote it: the palette shows it while the turn runs.
    pub todos: Vec<Todo>,
    /// Files picked for the next message, copied into `stage`.
    pub attached: Vec<Attachment>,
    /// The temporary folder those copies are in: made when the first file is picked, removed when the chat is cleared or dropped.
    stage: Option<tempfile::TempDir>,
    staged: usize,
    /// Tokens the provider has counted since the window last took them ([`Chat::take_usage`]).
    unreported: Usage,
    /// Who paid for the conversation the model has (empty for this window's own key) and with what provider: another starts it afresh.
    pub(crate) paid: Option<(String, Provider)>,
}

impl Chat {
    pub fn new(net: Arc<dyn Net>) -> Chat {
        Chat {
            provider: Provider::Anthropic,
            model: Provider::Anthropic.default_model().into(),
            log: Vec::new(),
            history: Vec::new(),
            net,
            wake: Arc::new(|| {}),
            flight: None,
            turn: None,
            todos: Vec::new(),
            attached: Vec::new(),
            stage: None,
            staged: 0,
            unreported: Usage::default(),
            paid: None,
        }
    }

    /// The network the chat reaches its provider through: a Co-Work guest's window makes a request on its key with it.
    pub fn net(&self) -> Arc<dyn Net> {
        Arc::clone(&self.net)
    }

    /// The tokens counted since the last call, which the window adds to what it keeps for the project.
    pub fn take_usage(&mut self) -> Usage {
        std::mem::take(&mut self.unreported)
    }

    /// Copies a file the user picked into the chat's temporary folder, to go with the next message. Refused, with a note, when it is
    /// not text or a picture, is too big, or is one too many.
    pub fn attach(&mut self, from: &Path) {
        let held = self.attached.len();
        let dir = match &self.stage {
            Some(dir) => Ok(dir.path().to_path_buf()),
            None => tempfile::Builder::new()
                .prefix("bimcraft-assistant-")
                .tempdir()
                .map(|d| (d.path().to_path_buf(), self.stage = Some(d)).0)
                .map_err(|e| format!("could not make a temporary folder for the file: {e}")),
        };
        self.staged += 1;
        match dir.and_then(|dir| attach::stage(from, &dir, self.staged, held)) {
            Ok(a) => self.attached.push(a),
            Err(e) => self.note(e),
        }
    }

    /// Takes the picked file `i` off the next message and deletes its copy.
    pub fn detach(&mut self, i: usize) {
        if i < self.attached.len() {
            let _ = fs::remove_file(self.attached.remove(i).path);
        }
    }

    /// Where the copies of the files picked are, while there are any.
    pub fn stage_dir(&self) -> Option<&Path> {
        self.stage.as_ref().map(tempfile::TempDir::path)
    }

    /// Forgets the files: the folder and every copy in it go.
    fn drop_files(&mut self) {
        (self.attached, self.stage, self.staged) = (Vec::new(), None, 0);
    }

    /// Whether a message is being worked on.
    pub fn busy(&self) -> bool {
        self.turn.is_some()
    }

    /// Starts a new conversation. What the model was told is forgotten; the project is as it is.
    pub fn clear(&mut self) {
        if !self.busy() {
            self.log.clear();
            self.history.clear();
            self.todos.clear();
            self.drop_files();
            self.paid = None;
        }
    }

    fn note(&mut self, text: impl Into<String>) {
        self.log.push(Line::Note(text.into()));
        if self.log.len() > MAX_LOG {
            self.log.remove(0);
        }
    }

    /// Sends the user's message with `key`, and begins the turn. Refused, with a note in the log, when a message is being worked on,
    /// the message or the key is no good, or the conversation has outgrown [`MAX_HISTORY`].
    pub fn send(&mut self, text: &str, key: &str) {
        if self.busy() || text.trim().is_empty() {
            return;
        }
        if !key_ok(key) {
            return self.note("Give a key and a model id first.");
        }
        let Some(files) = self.load_attached() else { return };
        let (provider, model) = (self.provider, self.model.clone());
        self.begin(text, files, None, "", provider, &model, Pays::Key(key.into()));
        if self.busy() {
            self.attached.clear();
        }
    }

    /// Reads the files picked for the next message, which stay picked; `None`, with a note, when one can no longer be read.
    pub fn load_attached(&mut self) -> Option<Vec<Attached>> {
        let mut files = Vec::with_capacity(self.attached.len());
        for a in &self.attached {
            match a.load() {
                Ok(f) => files.push(f),
                Err(e) => {
                    self.note(e);
                    return None;
                }
            }
        }
        Some(files)
    }

    /// Sends a message of `by`'s in a Co-Work session, asked on `payer`'s key with `provider` and `model`: the tools run here, as
    /// `by`'s edits. Another payer or provider than the last starts the model afresh, as its reasoning means nothing to another.
    #[allow(clippy::too_many_arguments)]
    pub fn send_shared(&mut self, text: &str, files: Vec<Attached>, by: &str, payer: &str, provider: Provider, model: &str, pays: Pays) {
        self.begin(text, files, Some(by), payer, provider, model, pays);
    }

    #[allow(clippy::too_many_arguments)]
    fn begin(&mut self, text: &str, files: Vec<Attached>, by: Option<&str>, payer: &str, provider: Provider, model: &str, pays: Pays) {
        let text = text.trim();
        if self.busy() || text.is_empty() {
            return;
        }
        if text.chars().count() > MAX_TEXT {
            return self.note(format!("That message is longer than {MAX_TEXT} characters: shorten it."));
        }
        if !model_ok(model) {
            return self.note("Give a key and a model id first.");
        }
        let paid = (payer.to_string(), provider);
        // Another key or provider can't take the earlier turns as they were made (reasoning and calls are valid only where they
        // were): it has them told as text in the message.
        let mut told = text.to_string();
        if self.paid.as_ref().is_some_and(|p| *p != paid) && !self.history.is_empty() {
            told = format!(
                "What was said earlier in this chat, on another key (its reasoning is not here):\n{}\n\nNow: {text}",
                fold(&self.history)
            );
            self.history.clear();
            let whose = if payer.is_empty() { "your key".into() } else { format!("{payer}'s key") };
            self.note(format!("Asked on {whose} with {}: the model starts afresh, told what was said before as text.", provider.name()));
        }
        if self.history.iter().map(Msg::size).sum::<usize>() + told.len() > MAX_HISTORY {
            return self.note("This conversation is too long to send again: clear the chat and start a new one.");
        }
        let sent: usize = self.history.iter().map(Msg::file_bytes).sum::<usize>() + files.iter().map(|f| f.len()).sum::<usize>();
        if sent > MAX_FILES_TOTAL {
            return self.note("The files in this conversation are more than can be sent again: clear the chat, or send fewer.");
        }
        self.paid = Some(paid);
        self.log.push(Line::You(text.into()));
        if !files.is_empty() {
            self.log.push(Line::Files(files.iter().map(|f| f.name.clone()).collect()));
        }
        self.todos.clear();
        self.history.push(Msg::User(told, files));
        let what = clip(&text.split_whitespace().collect::<Vec<_>>().join(" "), 40);
        let label = match by {
            Some(by) => format!("{LABEL} ({by}): {what}"),
            None => format!("{LABEL}: {what}"),
        };
        let by = by.map(str::to_string);
        self.turn = Some(Turn { label, pays, provider, model: model.into(), by, rounds: 0 });
        if let Err(e) = self.launch() {
            self.note(e);
            self.turn = None;
        }
    }

    /// Asks the model again, on a thread.
    fn launch(&mut self) -> Result<(), String> {
        let Some(turn) = &mut self.turn else { return Ok(()) };
        turn.rounds += 1;
        if turn.rounds > MAX_ROUNDS {
            return Err(format!("Stopped after {MAX_ROUNDS} replies to one message. What was done stays; ask again to go on."));
        }
        if self.history.iter().map(Msg::size).sum::<usize>() > MAX_HISTORY {
            return Err("Stopped: the conversation has grown past what is sent at once. Clear the chat to start a new one.".into());
        }
        let (provider, model, pays) = (turn.provider, turn.model.clone(), turn.pays.clone());
        let (net, history, wake) = (Arc::clone(&self.net), self.history.clone(), Arc::clone(&self.wake));
        let (tx, rx) = channel();
        std::thread::Builder::new()
            .name("assistant".into())
            .spawn(move || {
                let reply = match pays {
                    Pays::Key(key) => complete(provider, net.as_ref(), &model, &key, &history),
                    Pays::Remote(ask) => ask(provider, &model, &history),
                };
                let _ = tx.send(reply);
                wake();
            })
            .map_err(|e| format!("could not start the request: {e}"))?;
        self.flight = Some(rx);
        Ok(())
    }

    /// Once a frame: takes the reply if it has come, runs the tools it asks for on `session`, and asks again, or ends the turn.
    /// Whether anything changed (the window repaints then).
    pub fn poll(&mut self, session: &mut Session, access: Access) -> bool {
        let got = match self.flight.as_ref().map(Receiver::try_recv) {
            None | Some(Err(TryRecvError::Empty)) => return false,
            Some(Ok(reply)) => reply,
            Some(Err(TryRecvError::Disconnected)) => Err("the request stopped without an answer".into()),
        };
        self.flight = None;
        let reply = match got {
            Ok(reply) => reply,
            Err(e) => {
                self.note(e);
                self.finish(session);
                return true;
            }
        };
        let (by, here) = match &self.turn {
            Some(turn) => (turn.by.clone(), matches!(turn.pays, Pays::Key(_))),
            None => (None, true),
        };
        if here {
            self.unreported.add(reply.usage);
        }
        if !reply.thinking.trim().is_empty() {
            self.log.push(Line::Thinks(clip(reply.thinking.trim(), 20_000)));
        }
        if !reply.text.trim().is_empty() {
            self.log.push(Line::Says(reply.text.trim().into()));
        }
        let calls = reply.calls.clone();
        self.history.push(Msg::Assistant(reply));
        if calls.is_empty() {
            self.finish(session);
            return true;
        }
        // The calls are the edits of whoever asked: undo takes them back as theirs.
        let author = by.map(|by| session.author.replace(by));
        let done = batch(session, &calls, access);
        if let Some(author) = author {
            session.author = author;
        }
        for (call, d) in calls.iter().zip(&done) {
            // The checklist is drawn as a list, not as a call in the log; one that was refused is shown, so the user sees why it is not.
            if call.name == TODO && !d.error {
                self.todos = tools::todos(&call.args).unwrap_or_default();
                continue;
            }
            self.log.push(Line::Used {
                name: call.name.clone(),
                args: clip(&call.args.to_string(), 300),
                result: clip(&d.text, 600),
                error: d.error,
            });
        }
        self.history.push(Msg::Results(done));
        if let Err(e) = self.launch() {
            self.note(e);
            self.finish(session);
        }
        true
    }

    /// Gives up on the request in flight; what the turn already did stays, as one undo step.
    pub fn stop(&mut self, session: &mut Session) {
        self.flight = None;
        if self.busy() {
            self.note("Stopped.");
            self.finish(session);
        }
    }

    /// Forgets the conversation and whatever is in flight, for a window whose project was replaced: the reply to a request made about
    /// the old one must not run its tools on the new one, and nothing of the old turn is left to merge. The provider, the model and
    /// the network stay.
    pub fn reset(&mut self) {
        (self.flight, self.turn) = (None, None);
        self.log.clear();
        self.history.clear();
        self.todos.clear();
        self.drop_files();
        self.unreported = Usage::default();
        self.paid = None;
    }

    /// Ends the turn: its batches become one undo step.
    fn finish(&mut self, session: &mut Session) {
        if let Some(turn) = self.turn.take() {
            session.merge_steps(LABEL, &turn.label);
        }
    }
}

/// The conversation as text, for a model that didn't have it: the words, the tools called and what they gave, without reasoning or
/// pictures; its last [`MAX_FOLD`] characters.
fn fold(history: &[Msg]) -> String {
    let mut out = String::new();
    for m in history {
        match m {
            Msg::User(text, files) => {
                out += &format!("\n[user] {text}");
                for f in files {
                    out += &format!(" [file {}]", f.name);
                }
            }
            Msg::Assistant(r) => {
                out += &format!("\n[assistant] {}", r.text.trim());
                for c in &r.calls {
                    out += &format!("\n[called {} {}]", c.name, clip(&c.args.to_string(), 300));
                }
            }
            Msg::Results(done) => {
                for d in done {
                    out += &format!("\n[{} gave] {}", d.name, clip(&d.text, 600));
                }
            }
        }
    }
    let n = out.chars().count();
    out.chars().skip(n.saturating_sub(MAX_FOLD)).collect()
}

/// Runs a reply's calls as one transaction: a call that fails is told so and the others go on (a failed command has changed
/// nothing), and a panic anywhere takes the whole batch back.
fn batch(session: &mut Session, calls: &[Call], access: Access) -> Vec<Done> {
    let mut done = Vec::with_capacity(calls.len());
    let ran = session.transaction(LABEL, |s| {
        for (i, call) in calls.iter().enumerate() {
            let result = if i < MAX_CALLS {
                tools::run(s, call, access)
            } else {
                Err(format!("only {MAX_CALLS} tool calls are run in one reply: ask again for the rest"))
            };
            let (text, error) = match result {
                Ok(v) => (to_text(&v), false),
                Err(e) => (e, true),
            };
            done.push(Done { id: call.id.clone(), name: call.name.clone(), text, error });
        }
        Ok(())
    });
    if let Err(e) = ran {
        return calls
            .iter()
            .map(|c| Done { id: c.id.clone(), name: c.name.clone(), text: format!("taken back: {e}"), error: true })
            .collect();
    }
    done
}
