//! The assistant's chat in a Co-Work session (COWORK-PLAN.md, the shared AI chat). The host's chat is everyone's: a guest's message
//! runs in the host's window as the guest's, the chat goes out to the guests after each change, and a guest's palette shows it.
//! Whoever has an AI key pays with it; one who has none is lent one ([`payer`]): the host's while the host lends it, else the first
//! guest in join order who lends. A key never leaves its machine: a turn on a guest's key asks that guest's window, which makes the
//! request itself and asks its owner once before it pays for someone else's message.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use bimcraft_assistant::chat::{Pays, Remote};
use bimcraft_assistant::{Attached, Line, Msg, Provider, Reply, Todo, complete, key_ok, model_ok, wire};
use bimcraft_cowork::live::{AiKey, payer};
use egui::{Context, Id as UiId, RichText};
use serde_json::{Value, json};

use super::Live;
use crate::BimApp;
use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::style::pal;

/// The most requests a guest's key makes for a session in a minute: a host could otherwise run up its owner's bill.
const MAX_LENT: usize = 30;

/// Whether a guest's key pays for the messages of others in the session who have none (File > Settings > Co-Work).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Lending {
    /// The first time it would, this session.
    #[default]
    Ask,
    Always,
    Never,
}

impl Lending {
    pub const ALL: [Lending; 3] = [Lending::Ask, Lending::Always, Lending::Never];

    pub fn key(self) -> &'static str {
        match self {
            Lending::Ask => "ask",
            Lending::Always => "always",
            Lending::Never => "never",
        }
    }

    pub fn from_key(k: &str) -> Option<Lending> {
        Lending::ALL.into_iter().find(|l| l.key() == k)
    }

    pub fn label(self) -> &'static str {
        match self {
            Lending::Ask => bimcraft_i18n::msg!("Ask me first"),
            Lending::Always => bimcraft_i18n::msg!("Always"),
            Lending::Never => bimcraft_i18n::msg!("Never"),
        }
    }
}

/// The chat as a guest's palette shows it: the host's.
#[derive(Default)]
pub struct Shown {
    pub log: Vec<Line>,
    pub todos: Vec<Todo>,
    pub busy: bool,
    /// Whose key and which model the turn runs on.
    pub on: String,
}

/// A request the host asked of this window's key, waiting for its owner's answer.
pub struct Asked {
    reference: u64,
    provider: Provider,
    model: String,
    history: Value,
}

/// What the window keeps of the shared chat while it is in a session.
#[derive(Default)]
pub struct Ai {
    /// The host's: the chat's revision and whether it was busy when it last went out; whose key the turn is on; whether the turn is
    /// a guest's who may only look (it then only reads).
    sent: Option<(usize, bool)>,
    pub(crate) on: String,
    pub(crate) reads_only: bool,
    /// A guest's: the host's chat, this session's answer to lending, a request waiting for it, the requests made in the last minute,
    /// and those on their way.
    pub(crate) shown: Option<Shown>,
    lent: Option<bool>,
    pub(crate) waiting: Option<Asked>,
    made: VecDeque<Instant>,
    flights: Vec<(u64, Receiver<Result<Reply, String>>)>,
}

/// The chat as the host sends it.
fn state(log: &[Line], todos: &[Todo], busy: bool, on: &str) -> Value {
    json!({"log": wire::log(log), "todos": wire::todos(todos), "busy": busy, "on": on})
}

/// The host's chat read strictly: none when it isn't one.
fn shown(v: &Value) -> Option<Shown> {
    Some(Shown {
        log: wire::log_from(&v["log"])?,
        todos: wire::todos_from(&v["todos"])?,
        busy: v["busy"].as_bool()?,
        on: v["on"].as_str()?.chars().take(200).collect(),
    })
}

impl BimApp {
    /// Once a pass, in a session: the host sends its chat when it changed; a guest tells the host its key and answers the requests
    /// on it that are done.
    pub(crate) fn pump_chat(&mut self, ctx: &Context) {
        let a = &self.assistant;
        match &mut self.cowork {
            Some(Live::Host(h)) => {
                let Some(chat) = &a.chat else { return };
                let now = (chat.revision(), chat.busy());
                if self.cowork_ai.sent != Some(now) {
                    h.chat(state(&chat.log, &chat.todos, chat.busy(), &self.cowork_ai.on));
                    self.cowork_ai.sent = Some(now);
                }
            }
            Some(Live::Guest(j)) => {
                let lends = self.cowork_prefs.lending != Lending::Never && self.cowork_ai.lent != Some(false);
                let has = a.chat.is_some() && key_ok(&a.key()) && model_ok(a.model.trim());
                j.ai_key(has.then(|| AiKey { provider: a.provider.key().into(), model: a.model.trim().into(), lends }));
                let mut used = bimcraft_assistant::Usage::default();
                self.cowork_ai.flights.retain(|(reference, rx)| match rx.try_recv() {
                    Ok(got) => {
                        if let Ok(r) = &got {
                            used.add(r.usage);
                        }
                        j.ai_reply(*reference, got.map(|r| wire::reply(&r)));
                        false
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => true,
                    Err(_) => {
                        j.ai_reply(*reference, Err("the request stopped without an answer".into()));
                        false
                    }
                });
                if used != bimcraft_assistant::Usage::default() {
                    let project = self.session.path.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
                    self.assistant.add_usage(project, used);
                }
                if !self.cowork_ai.flights.is_empty() {
                    ctx.request_repaint_after(Duration::from_millis(200));
                }
            }
            None => {}
        }
    }

    /// The host's window: a message of `by`'s (`sender`, 0 for the host) for the session's chat, on the key [`payer`] picks. `Err`
    /// says why it wasn't taken.
    pub(crate) fn chat_on_host(&mut self, ctx: &Context, sender: u32, by: &str, text: &str, files: Vec<Attached>) -> Result<(), String> {
        let Some(Live::Host(h)) = &self.cowork else { return Err(t("This window hosts no session").into()) };
        let a = &self.assistant;
        let Some(chat) = &a.chat else {
            return Err(t("The assistant needs the network, which the host's window was started without.").into());
        };
        if chat.busy() {
            return Err(t("The assistant is still answering: ask again when it is done").into());
        }
        let host_key = key_ok(&a.key()) && model_ok(a.model.trim());
        let nobody = || t("Nobody in the session has an AI key to answer with: set one in File > Settings > Assistant").to_string();
        let paying = payer(sender, host_key, !self.cowork_prefs.lend_key, h.keys()).ok_or_else(nobody)?;
        let (name, provider, model, pays) = if paying == 0 {
            (h.name().to_string(), a.provider, a.model.trim().to_string(), Pays::Key(a.key()))
        } else {
            let k = h.keys().get(&paying).ok_or_else(nobody)?;
            let provider = Provider::from_key(&k.provider).ok_or_else(nobody)?;
            let name = h.others().get(&paying).map_or_else(String::new, |o| o.name.clone());
            let lend = h.lend(paying, paying == sender);
            let remote: Remote = Arc::new(move |p: Provider, m: &str, history: &[Msg]| {
                let v = lend.ask(p.key(), m, wire::history(history))?;
                wire::reply_from(&v).ok_or_else(|| "what came back was no reply".to_string())
            });
            (name, provider, k.model.clone(), Pays::Remote(remote))
        };
        let reads_only = sender != 0 && !h.edit;
        let on = tf("{} on {}'s key", &[&format!("{} {model}", provider.name()), &name]);
        let Some(chat) = self.assistant.chat.as_mut() else { return Err(nobody()) };
        let wake = ctx.clone();
        chat.wake = Arc::new(move || wake.request_repaint());
        chat.send_shared(text, files, by, &name, provider, &model, pays);
        if !chat.busy() {
            // Refused: the chat's note says why.
            return Err(match chat.log.last() {
                Some(Line::Note(n)) => n.clone(),
                _ => t("The assistant didn't take the message").into(),
            });
        }
        (self.cowork_ai.on, self.cowork_ai.reads_only) = (on, reads_only);
        Ok(())
    }

    /// Sends what is in the palette's box to the session's chat: on the host's window, as the host's; on a guest's, to the host.
    pub(crate) fn send_in_session(&mut self, ctx: &Context) {
        let text = std::mem::take(&mut self.assistant.input);
        let Some(files) = self.assistant.chat.as_mut().and_then(|c| c.load_attached()) else {
            self.assistant.input = text;
            return;
        };
        let sent = match &self.cowork {
            Some(Live::Guest(j)) => {
                let busy = self.cowork_ai.shown.as_ref().is_some_and(|s| s.busy);
                if busy {
                    Err(t("The assistant is still answering: ask again when it is done").to_string())
                } else if j.say(&text, wire::files(&files)) {
                    Ok(())
                } else {
                    Err(t("the connection to the host is gone").to_string())
                }
            }
            Some(Live::Host(h)) => {
                let name = h.name().to_string();
                self.chat_on_host(ctx, 0, &name, &text, files)
            }
            None => return,
        };
        match sent {
            Ok(()) => {
                if let Some(chat) = self.assistant.chat.as_mut() {
                    chat.attached.clear();
                }
                self.assistant.agree_session();
            }
            Err(why) => {
                self.assistant.input = text;
                self.status = crate::Status { text: why, error: true };
            }
        }
    }

    /// A guest's window: what the host sent of the chat, a message it refused, or a request on this window's key.
    pub(crate) fn chat_happened(&mut self, state: &Value) {
        if let Some(s) = shown(state) {
            self.cowork_ai.shown = Some(s);
        }
    }

    /// A guest's window: the host asks for a request on this window's key, for a message of this window's own (`own`) or another's.
    /// Another's waits for its owner's yes, once a session, unless they said Always or Never.
    pub(crate) fn lend_for(&mut self, reference: u64, own: bool, provider: &str, model: String, history: Value) {
        let me = self.cowork_prefs.name.clone();
        let refuse = |app: &BimApp, why: String| {
            if let Some(Live::Guest(j)) = &app.cowork {
                j.ai_reply(reference, Err(why));
            }
        };
        let a = &self.assistant;
        let Some(provider) = Provider::from_key(provider).filter(|p| *p == a.provider && key_ok(&a.key())) else {
            return refuse(self, tf("{}'s AI key is for another provider now: ask again", &[&me]));
        };
        let asked = Asked { reference, provider, model, history };
        if !own {
            match (self.cowork_prefs.lending, self.cowork_ai.lent) {
                (Lending::Never, _) | (_, Some(false)) => return refuse(self, tf("{} doesn't lend their AI key", &[&me])),
                (Lending::Ask, None) if self.cowork_ai.waiting.is_some() => {
                    return refuse(self, tf("{} is still being asked to lend their key: ask again", &[&me]));
                }
                (Lending::Ask, None) => {
                    self.cowork_ai.waiting = Some(asked);
                    return;
                }
                _ => {}
            }
        }
        self.run_lent(asked);
    }

    /// Makes a request the host asked on this window's key, on a thread; its answer goes back at a later pass ([`BimApp::pump_chat`]).
    fn run_lent(&mut self, asked: Asked) {
        let me = self.cowork_prefs.name.clone();
        let ai = &mut self.cowork_ai;
        ai.made.retain(|at| at.elapsed() < Duration::from_secs(60));
        let history = wire::history_from(&asked.history);
        let refused = if ai.made.len() >= MAX_LENT {
            Some(tf("{}'s key has made {} requests this minute: ask again in a moment", &[&me, &MAX_LENT.to_string()]))
        } else if self.assistant.chat.is_none() {
            Some(tf("{}'s window has no network for the assistant", &[&me]))
        } else if history.is_none() {
            Some(t("That conversation is more than a request may carry").to_string())
        } else {
            None
        };
        let (Some(chat), Some(history), None) = (&self.assistant.chat, history, refused.clone()) else {
            let why = refused.unwrap_or_default();
            if let Some(Live::Guest(j)) = &self.cowork {
                j.ai_reply(asked.reference, Err(why));
            }
            return;
        };
        let (net, key, provider, model) = (chat.net(), self.assistant.key(), asked.provider, asked.model);
        let (tx, rx) = channel();
        let ran = std::thread::Builder::new().name("assistant-lent".into()).spawn(move || {
            let _ = tx.send(complete(provider, net.as_ref(), &model, &key, &history));
        });
        match ran {
            Ok(_) => {
                ai.made.push_back(Instant::now());
                ai.flights.push((asked.reference, rx));
            }
            Err(e) => {
                if let Some(Live::Guest(j)) = &self.cowork {
                    j.ai_reply(asked.reference, Err(format!("could not start the request: {e}")));
                }
            }
        }
    }

    /// A guest's window, the first time this session its key would pay for someone else's message: asked once.
    pub(crate) fn lend_window(&mut self, ctx: &Context) {
        if self.cowork_ai.waiting.is_none() {
            return;
        }
        let (mut yes, mut no) = (false, false);
        let r = Modal::new(UiId::new("cowork-lend"), t("Lend your AI key?")).width(420.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.label(t("Let this session's chat use your AI key for others who have none? What they ask is billed to your account."));
                ui.label(
                    RichText::new(t("The key stays on this computer: the request is made from here. The answer holds for this session; File > Settings > Co-Work keeps a standing one."))
                        .small()
                        .color(pal().muted),
                );
            });
            crate::popup::footer(ui, |ui| {
                ui.horizontal(|ui| {
                    yes = ui.button(t("Lend it")).clicked();
                    no = ui.button(t("Don't")).clicked();
                });
            });
        });
        if !(yes || no || r.should_close()) {
            return;
        }
        let Some(asked) = self.cowork_ai.waiting.take() else { return };
        self.cowork_ai.lent = Some(yes);
        if yes {
            self.run_lent(asked);
        } else if let Some(Live::Guest(j)) = &self.cowork {
            j.ai_reply(asked.reference, Err(tf("{} doesn't lend their AI key", &[&self.cowork_prefs.name])));
        }
    }
}
