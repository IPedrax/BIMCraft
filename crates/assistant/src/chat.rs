//! The chat: a conversation with a model that works on a [`Session`]. The window calls [`Chat::poll`] once a frame. A request to
//! the provider runs on a thread of its own (never the window's), and its reply is taken in the frame, where the tool calls in it
//! run on the window's session, as a command from a button does. Each batch of tool calls is one transaction named
//! [`LABEL`], and when the turn ends the batches merge into one undo step, `Assistant: <what the user asked>`.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use bimcraft_core::bridge::Access;
use bimcraft_core::engine::Session;

use crate::net::Net;
use crate::tools::{self, to_text};
use crate::{Call, Done, LABEL, MAX_CALLS, MAX_HISTORY, MAX_ROUNDS, MAX_TEXT, Msg, Provider, Reply, clip, complete, key_ok, model_ok};

/// How many lines of the log are kept.
const MAX_LOG: usize = 500;

/// What the palette shows, in order.
#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    You(String),
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

/// A message of the user's that is being worked on.
struct Turn {
    /// The undo step it ends as.
    label: String,
    key: String,
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
        }
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
        let text = text.trim();
        if self.busy() || text.is_empty() {
            return;
        }
        if text.chars().count() > MAX_TEXT {
            return self.note(format!("That message is longer than {MAX_TEXT} characters: shorten it."));
        }
        if !key_ok(key) || !model_ok(&self.model) {
            return self.note("Give a key and a model id first.");
        }
        if self.history.iter().map(Msg::size).sum::<usize>() + text.len() > MAX_HISTORY {
            return self.note("This conversation is too long to send again: clear the chat and start a new one.");
        }
        self.log.push(Line::You(text.into()));
        self.history.push(Msg::User(text.into()));
        let label = format!("{LABEL}: {}", clip(&text.split_whitespace().collect::<Vec<_>>().join(" "), 40));
        self.turn = Some(Turn { label, key: key.into(), rounds: 0 });
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
        let (provider, model, key) = (self.provider, self.model.clone(), turn.key.clone());
        let (net, history, wake) = (Arc::clone(&self.net), self.history.clone(), Arc::clone(&self.wake));
        let (tx, rx) = channel();
        std::thread::Builder::new()
            .name("assistant".into())
            .spawn(move || {
                let _ = tx.send(complete(provider, net.as_ref(), &model, &key, &history));
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
        if !reply.text.trim().is_empty() {
            self.log.push(Line::Says(reply.text.trim().into()));
        }
        let calls = reply.calls.clone();
        self.history.push(Msg::Assistant(reply));
        if calls.is_empty() {
            self.finish(session);
            return true;
        }
        let done = batch(session, &calls, access);
        for (call, d) in calls.iter().zip(&done) {
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

    /// Ends the turn: its batches become one undo step.
    fn finish(&mut self, session: &mut Session) {
        if let Some(turn) = self.turn.take() {
            session.merge_steps(LABEL, &turn.label);
        }
    }
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
