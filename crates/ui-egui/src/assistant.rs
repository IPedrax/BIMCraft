//! The Assistant palette (AI): a chat with a model of the user's choice (Anthropic, OpenAI or Google) that manages the project open
//! in this window. The chat, its provider clients and its tools are `bimcraft-assistant`'s; this is the palette that holds the
//! user's provider, model and key, says before the first send that the project's contents go to the provider, and takes the
//! chat's turns in the frame ([`BimApp::poll_assistant`]), where what the model asks for runs on the window's session as a
//! command from a button does: the user sees it made, undoes it as one step, and saves it.
//!
//! The window starts with no network (`State::attach` is the desktop's, as the updater's is), so a test never reaches a provider.
//! The key is read from the environment first (the provider's own variable), else typed in the palette, and is kept for the
//! session, or, when the user ticks Remember, in `settings.json` (the one place the window keeps it, said in plain words next to the
//! box). It is never written to a project, a log or the chat, and `Secret` prints as nothing.

use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;

use bimcraft_assistant::net::Net;
use bimcraft_assistant::{Chat, Line, Provider, key_ok, model_ok, seal};
use bimcraft_core::bridge::Access;
use bimcraft_i18n::msg;
use egui::{Context, Frame, Key, RichText, ScrollArea, TextEdit, Ui};

use crate::BimApp;
use crate::lang::{t, tf};
use crate::settings::{Page, heading};
use crate::style::pal;

/// What the palette and a refused send say of a key that is saved under a passphrase and not yet opened.
const LOCKED: &str = msg!("The saved API key is locked: unlock it in the Assistant settings.");

/// A key: `Debug` shows nothing of it, so it cannot reach a log or a failed assertion by accident.
#[derive(Clone, Default, PartialEq)]
pub struct Secret(pub String);

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.0.is_empty() { "Secret(none)" } else { "Secret(..)" })
    }
}

/// What the palette keeps: the user's choices, and the chat once the desktop has given it a network.
pub struct State {
    chat: Option<Chat>,
    pub provider: Provider,
    pub model: String,
    /// The key typed in the palette; the environment's comes before it.
    pub key: Secret,
    /// Keep the key between sessions: in `settings.json`, scrambled under the secret built into BIMCraft, or, with `protect`, encrypted
    /// under a passphrase (`bimcraft_assistant::seal`).
    pub remember: bool,
    /// Also protect the saved key with a passphrase the user types, once a session, and not only with the built-in secret.
    pub protect: bool,
    /// The key as `settings.json` keeps it; none when the user does not ask for it to be kept.
    pub sealed: Option<String>,
    /// The passphrase being typed, to seal the key or to open it: cleared as soon as it is used, and never kept.
    pub passphrase: Secret,
    /// Whether `sealed` is the key now in the box, at the level asked for: it is not once the key is edited, or while a passphrase is
    /// asked for and not yet saved.
    pub(crate) key_saved: bool,
    /// What the last Save or Unlock said when it failed.
    problem: Option<String>,
    pub(crate) input: String,
    /// The providers the user has agreed to send the project's contents to, this session.
    agreed: BTreeSet<&'static str>,
    /// The environment variable `name` holds, if any: a test gives its own, so the machine's keys are never used.
    pub env: fn(&str) -> Option<String>,
}

impl Default for State {
    fn default() -> State {
        State {
            chat: None,
            provider: Provider::Anthropic,
            model: Provider::Anthropic.default_model().into(),
            key: Secret::default(),
            remember: false,
            protect: false,
            sealed: None,
            passphrase: Secret::default(),
            key_saved: false,
            problem: None,
            input: String::new(),
            agreed: BTreeSet::new(),
            env: |name| std::env::var(name).ok(),
        }
    }
}

impl State {
    /// Gives the assistant its network: the desktop's `main` does, so a window made without one never phones home.
    pub fn attach(&mut self, net: Arc<dyn Net>) {
        self.chat = Some(Chat::new(net));
    }

    /// The key from the provider's environment variable, when it is one.
    fn env_key(&self) -> Option<String> {
        (self.env)(self.provider.env()).map(|k| k.trim().to_string()).filter(|k| key_ok(k))
    }

    /// The key a send uses: the environment's, else the typed one.
    fn key(&self) -> String {
        self.env_key().unwrap_or_else(|| self.key.0.trim().to_string())
    }

    /// The window's project was replaced (File > New, Open, Recover): the conversation was about the old one, and a reply on its way
    /// must not act on the new one. What the user chose (the provider, the model, the key, the agreement) and the network stay.
    pub(crate) fn project_replaced(&mut self) {
        if let Some(chat) = self.chat.as_mut() {
            chat.reset();
        }
        self.input.clear();
    }

    /// A key saved under a passphrase and not yet opened: nothing to send with, but one the passphrase opens.
    fn locked(&self) -> bool {
        self.key.0.is_empty() && self.sealed.as_deref().is_some_and(seal::needs_passphrase)
    }

    /// The saved key is locked: its passphrase opens it for this session, or it is forgotten.
    fn unlock_ui(&mut self, ui: &mut Ui) {
        ui.label(t("A saved key is locked. Type its passphrase to use it in this session."));
        let mut go = false;
        ui.horizontal(|ui| {
            let l = ui.label(t("Passphrase:"));
            // From the right, so the field takes what the button leaves and the row never outgrows its page.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let typed = !self.passphrase.0.is_empty();
                go = ui.add_enabled(typed, egui::Button::new(t("Unlock"))).clicked();
                let width = ui.available_width().max(40.0);
                let r = ui.add_sized([width, 20.0], TextEdit::singleline(&mut self.passphrase.0).password(true)).labelled_by(l.id);
                go |= typed && r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            });
        });
        if go && let Some(sealed) = self.sealed.clone() {
            match seal::open_passphrase(&self.passphrase.0, &sealed) {
                Ok(key) => (self.key, self.key_saved, self.remember, self.protect, self.problem) = (Secret(key), true, true, true, None),
                Err(e) => self.problem = Some(e),
            }
            self.passphrase = Secret::default();
        }
        if let Some(problem) = &self.problem {
            ui.label(RichText::new(problem).color(pal().error));
        }
        if ui.small_button(t("Forget the saved key")).clicked() {
            (self.sealed, self.remember, self.protect, self.problem, self.passphrase) = (None, false, false, None, Secret::default());
        }
    }

    /// The key box, Remember, and the passphrase that may go with it: Remember keeps the key in `settings.json`, scrambled and coming back
    /// at the next start; the passphrase makes that encryption, which the user opens once a session.
    fn key_ui(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let l = ui.label(t("API key:"));
            let width = ui.available_width();
            let edit = TextEdit::singleline(&mut self.key.0).password(true).hint_text(self.provider.env());
            if ui.add_sized([width, 20.0], edit).labelled_by(l.id).changed() {
                self.key_saved = false;
                self.resave();
            }
        });
        if ui.checkbox(&mut self.remember, t("Remember the key")).changed() {
            self.protect &= self.remember;
            self.resave();
        }
        if !self.remember {
            return;
        }
        if ui.checkbox(&mut self.protect, t("Also protect it with a passphrase")).changed() {
            (self.passphrase, self.problem, self.key_saved) = (Secret::default(), None, false);
            self.resave();
        }
        if !self.protect {
            let said = msg!(
                "Kept in settings.json scrambled with a key built into BIMCraft: it can't be read at a glance, but anyone who has BIMCraft can unscramble it. A passphrase makes it encryption. Leave Remember off on a computer others use."
            );
            ui.label(RichText::new(t(said)).small().color(pal().muted));
            return;
        }
        let said = msg!(
            "Kept in settings.json encrypted with the passphrase alone (AES-256-GCM, PBKDF2 with 600,000 rounds): it is as safe as the passphrase is hard to guess, and it opens in any build. You type it once a session. If you forget it, the key can't be recovered: type the key again."
        );
        ui.label(RichText::new(t(said)).small().color(pal().muted));
        let mut save = false;
        ui.horizontal(|ui| {
            let l = ui.label(t("Passphrase:"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let ready = key_ok(self.key.0.trim()) && self.passphrase.0.chars().count() >= seal::PASSPHRASE_MIN;
                save = ui.add_enabled(ready, egui::Button::new(t("Save the key"))).clicked();
                let width = ui.available_width().max(40.0);
                let hint = tf("At least {} characters", &[&seal::PASSPHRASE_MIN.to_string()]);
                ui.add_sized([width, 20.0], TextEdit::singleline(&mut self.passphrase.0).password(true).hint_text(hint)).labelled_by(l.id);
            });
        });
        if save {
            match seal::seal_passphrase(&self.passphrase.0, &self.key.0) {
                Ok(sealed) => (self.sealed, self.key_saved, self.problem) = (Some(sealed), true, None),
                Err(e) => self.problem = Some(e),
            }
            self.passphrase = Secret::default();
        }
        match (&self.problem, self.key_saved) {
            (Some(problem), _) => ui.label(RichText::new(problem).color(pal().error)),
            (None, true) => ui.label(RichText::new(t("Saved, encrypted with the passphrase.")).color(pal().muted)),
            (None, false) => ui.label(RichText::new(t("Not saved with the passphrase yet.")).color(pal().muted)),
        };
    }

    /// What `settings.json` keeps of the key follows the box: nothing unless Remember is ticked; with it, the key scrambled under the
    /// built-in secret, as it is edited. With a passphrase it is sealed only when the user presses Save (stretching it takes time).
    fn resave(&mut self) {
        if !self.remember {
            (self.sealed, self.protect, self.key_saved, self.passphrase, self.problem) = (None, false, false, Secret::default(), None);
        } else if !self.protect {
            self.sealed = seal::seal(&self.key.0).ok();
            self.key_saved = self.sealed.is_some();
        }
    }

    /// Whether the chat is working on a message.
    pub fn busy(&self) -> bool {
        self.chat.as_ref().is_some_and(Chat::busy)
    }

    /// What the chat has said so far.
    pub fn log(&self) -> &[Line] {
        self.chat.as_ref().map_or(&[], |c| c.log.as_slice())
    }
}

impl BimApp {
    /// Opens or closes the palette, as AI does.
    pub(crate) fn toggle_assistant(&mut self) {
        let p = crate::dock::Palette::Assistant;
        self.show_palette(p, !self.docking.shown(p));
    }

    /// Once a frame: takes what the provider has answered, runs the tools it asked for on the project, as a command from a button
    /// is run (while a file is being read or an update installed the project is about to be replaced: they wait), and asks again.
    pub(crate) fn poll_assistant(&mut self, ctx: &Context) {
        if !self.assistant.busy() || self.importing() || self.installing() {
            return;
        }
        self.made_in();
        // The assistant is an agent as the control port's are: a window that lets agents only read lets it only read.
        let access = if self.agents.edit { Access::Edit } else { Access::Read };
        let Some(chat) = self.assistant.chat.as_mut() else { return };
        if chat.poll(&mut self.session, access) {
            // The core renames and drops the option with its set.
            self.active_option = self.session.option.clone();
            ctx.request_repaint();
        }
    }

    /// What the palette shows: which model it talks to (chosen in File > Settings > Assistant, which a button opens), what the chat
    /// said, and the box the user writes in. All of it is one column that scrolls and sticks to the bottom as the conversation grows,
    /// so a short stack of palettes (BIMCraft's Navigator) still reaches every part of it.
    pub(crate) fn assistant_palette(&mut self, ui: &mut Ui) {
        if self.assistant.chat.is_none() {
            Frame::new().inner_margin(8.0).show(ui, |ui| {
                ui.label(RichText::new(t("The assistant needs the network, which this window was started without.")).color(pal().muted));
            });
            return;
        }
        let a = &self.assistant;
        let summary = format!("{} · {}", a.provider.name(), a.model);
        let no_key = (a.env_key().is_none() && !key_ok(a.key.0.trim()))
            .then(|| if a.locked() { t(LOCKED) } else { t("No API key yet. Add one in the Assistant settings.") }.to_string());
        // Sticking to the bottom is for a conversation under way: before it, the palette shows its top.
        let talking = !a.log().is_empty();
        ScrollArea::vertical().id_salt("assistant").auto_shrink([false, false]).stick_to_bottom(talking).show(ui, |ui| {
            Frame::new().inner_margin(8.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(summary).small().color(pal().muted));
                    if ui.small_button(t("Assistant settings")).clicked() {
                        self.settings_page = Some(Page::Assistant);
                    }
                });
                if let Some(note) = no_key {
                    ui.label(RichText::new(note).small().color(pal().muted));
                }
                ui.separator();
                for line in self.assistant.log() {
                    log_line(ui, line);
                }
                self.assistant_input(ui);
            });
        });
    }

    /// File > Settings > Assistant: the provider, the model and the key, which the palette sends with. The key is the provider's
    /// environment variable when that is set, else the one typed here, kept for the session or, with Remember, in `settings.json`.
    pub(crate) fn assistant_page(&mut self, ui: &mut Ui) {
        let a = &mut self.assistant;
        heading(ui, msg!("Provider and model"));
        ui.horizontal(|ui| {
            ui.label(t("Provider"));
            let before = a.provider;
            egui::ComboBox::from_id_salt("assistant-provider").selected_text(a.provider.name()).show_ui(ui, |ui| {
                for p in Provider::ALL {
                    ui.selectable_value(&mut a.provider, p, p.name());
                }
            });
            if a.provider != before {
                a.model = a.provider.default_model().into();
            }
        });
        // The model's id is typed (any the provider has), with a drop-down of the ones worth suggesting. Laid out from the right, so the
        // field takes exactly what the drop-down leaves: a row wider than its page would widen the window, frame after frame.
        ui.horizontal(|ui| {
            let l = ui.label(t("Model"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                egui::ComboBox::from_id_salt("assistant-models").selected_text("").width(0.0).show_ui(ui, |ui| {
                    for m in a.provider.models() {
                        if ui.selectable_label(a.model == *m, *m).clicked() {
                            a.model = (*m).into();
                        }
                    }
                });
                let width = ui.available_width().max(40.0);
                ui.add_sized([width, 20.0], TextEdit::singleline(&mut a.model).hint_text(a.provider.default_model())).labelled_by(l.id);
            });
        });
        heading(ui, msg!("API key"));
        if let Some(var) = a.env_key().map(|_| a.provider.env()) {
            ui.label(RichText::new(tf("Using the key in {}", &[var])).color(pal().muted));
        } else if a.locked() {
            a.unlock_ui(ui);
        } else {
            a.key_ui(ui);
        }
        ui.add_space(10.0);
        ui.label(
            RichText::new(t("The assistant sends your messages, and what it reads of the open project, to this provider, and only when you send a message. The Assistant palette (AI) asks once before the first."))
                .color(pal().muted),
        );
    }

    fn assistant_input(&mut self, ui: &mut Ui) {
        let busy = self.assistant.busy();
        let agreed = self.assistant.agreed.contains(self.assistant.provider.key());
        if !agreed {
            ui.label(
                RichText::new(tf(
                    "Your messages, and what the assistant reads of this project (its levels, elements, schedules), are sent to {}. Nothing is sent until you send a message.",
                    &[self.assistant.provider.name()],
                ))
                .small()
                .color(pal().muted),
            );
        }
        let hint = t("Ask the assistant to look at or change the project");
        let edit =
            TextEdit::multiline(&mut self.assistant.input).desired_width(f32::INFINITY).desired_rows(2).hint_text(hint).interactive(!busy);
        let r = ui.add(edit);
        let mut send = r.has_focus() && ui.input(|i| i.modifiers.command && i.key_pressed(Key::Enter));
        ui.horizontal(|ui| {
            let label = if agreed { t("Send") } else { t("Agree and send") };
            send |= ui.add_enabled(!busy && !self.assistant.input.trim().is_empty(), egui::Button::new(label)).clicked();
            if busy {
                if ui.button(t("Stop")).clicked()
                    && let Some(chat) = self.assistant.chat.as_mut()
                {
                    chat.stop(&mut self.session);
                }
                // Not a spinner: the reply wakes the window, so nothing needs a frame after frame meanwhile.
                ui.label(RichText::new(t("Working…")).color(pal().muted));
            } else if ui.button(t("New chat")).clicked()
                && let Some(chat) = self.assistant.chat.as_mut()
            {
                chat.clear();
            }
        });
        if send {
            self.send_assistant(ui.ctx());
        }
    }

    /// Sends what is in the box: the agreement is the Send of the first time, and the model and provider are the palette's.
    fn send_assistant(&mut self, ctx: &Context) {
        let a = &mut self.assistant;
        let key = a.key();
        let locked = a.locked() && a.env_key().is_none();
        let text = std::mem::take(&mut a.input);
        let Some(chat) = a.chat.as_mut() else { return };
        if !model_ok(a.model.trim()) {
            chat.log.push(Line::Note(t("Give a model id: letters, digits, dots and dashes only.").to_string()));
            a.input = text;
            return;
        }
        if !key_ok(&key) {
            let note = if locked {
                t(LOCKED).to_string()
            } else {
                tf("Give an API key in the Assistant settings, or set {} in the environment.", &[a.provider.env()])
            };
            chat.log.push(Line::Note(note));
            a.input = text;
            return;
        }
        a.agreed.insert(a.provider.key());
        (chat.provider, chat.model) = (a.provider, a.model.trim().to_string());
        let ctx = ctx.clone();
        chat.wake = Arc::new(move || ctx.request_repaint());
        chat.send(&text, &key);
        if !chat.busy() {
            // Refused (the conversation is too long, say): the message is still there to send, and the note says why.
            a.input = text;
        }
    }
}

/// One line of the conversation: what the user said and the model said in words, each tool call in the small type of a log.
fn log_line(ui: &mut Ui, line: &Line) {
    let wrap = |ui: &mut Ui, text: RichText| {
        ui.add(egui::Label::new(text).wrap());
    };
    match line {
        Line::You(text) => {
            ui.label(RichText::new(t("You")).strong().color(pal().accent));
            wrap(ui, RichText::new(text));
        }
        Line::Says(text) => {
            ui.label(RichText::new(t("Assistant")).strong());
            wrap(ui, RichText::new(text));
        }
        Line::Used { name, args, result, error } => {
            let ink = if *error { pal().error } else { pal().muted };
            wrap(ui, RichText::new(format!("▸ {name} {args}")).small().monospace().color(pal().muted));
            wrap(ui, RichText::new(result).small().monospace().color(ink));
        }
        Line::Note(text) => wrap(ui, RichText::new(text).color(pal().error)),
    }
    ui.add_space(4.0);
}

/// Names the palette and its choices for Undo's list: an undo step the assistant made, `Assistant: <what was asked>`.
pub(crate) fn step_label(label: &str) -> Option<String> {
    label.strip_prefix(&format!("{}: ", bimcraft_assistant::LABEL)).map(|asked| tf(msg!("Assistant: {}"), &[asked]))
}
