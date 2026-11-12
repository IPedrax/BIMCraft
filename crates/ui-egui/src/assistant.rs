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

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use bimcraft_assistant::net::Net;
use bimcraft_assistant::{Chat, Line, Provider, Status, Todo, Usage, clip, key_ok, model_ok, seal};
use bimcraft_core::bridge::Access;
use bimcraft_i18n::msg;
use egui::{Color32, Context, Frame, Key, RichText, ScrollArea, Shape, Stroke, TextEdit, Ui, Vec2};

use crate::BimApp;
use crate::lang::{t, tf};
use crate::settings::{Page, heading};
use crate::style::pal;

/// What the palette and a refused send say of a key that is saved under a passphrase and not yet opened.
const LOCKED: &str = msg!("The saved API key is locked: unlock it in the Assistant settings.");

/// How many projects' token counts are kept apart.
pub const MAX_PROJECTS: usize = 100;

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
    pub(crate) chat: Option<Chat>,
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
    /// Tokens the providers have counted for this installation, by the project they were spent on (its file's path, none for a project
    /// not yet saved). `settings.json` keeps them.
    pub usage: BTreeMap<String, Usage>,
    /// The project the Settings page counts for, or all of them.
    pub(crate) usage_filter: Option<String>,
    /// When the pointer was last over the palette: files dropped there are attached to the message, not opened as a project.
    pub(crate) over_at: Option<f64>,
    /// The project the chat on screen belongs to (none until the first look, and again when a project was replaced, so the next frame
    /// reads that project's saved chat), and the revision of the chat that file holds.
    chat_for: Option<Option<std::path::PathBuf>>,
    saved_rev: usize,
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
            usage: BTreeMap::new(),
            usage_filter: None,
            over_at: None,
            chat_for: None,
            saved_rev: 0,
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
        self.chat_for = None;
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

    /// What the providers have counted for this installation, or for one project: the tokens that went in and came out, from what each
    /// reply reported (the provider's bill is what counts, not this). They are kept in `settings.json`, never in a project.
    fn usage_ui(&mut self, ui: &mut Ui, open: Option<&std::path::Path>) {
        let name_of = |key: &str| {
            if key.is_empty() {
                t("Untitled project").to_string()
            } else {
                std::path::Path::new(key).file_name().map_or_else(|| key.to_string(), |n| n.to_string_lossy().into_owned())
            }
        };
        // The open project is offered even before it has used any, so the first look says none.
        let mut keys: BTreeSet<String> = self.usage.keys().cloned().collect();
        keys.extend(open.map(|p| p.to_string_lossy().into_owned()));
        ui.horizontal(|ui| {
            ui.label(t("Show"));
            let shown = self.usage_filter.as_deref().map_or_else(|| t("All projects").to_string(), name_of);
            egui::ComboBox::from_id_salt("assistant-usage-project").selected_text(shown).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.usage_filter, None, t("All projects"));
                for key in &keys {
                    ui.selectable_value(&mut self.usage_filter, Some(key.clone()), name_of(key)).on_hover_text(key);
                }
            });
        });
        let u = self.usage_of();
        let (input, output, total) = (grouped(u.input), grouped(u.output), grouped(u.total()));
        ui.label(tf("{} in · {} out · {} total", &[&input, &output, &total]));
        ui.label(
            RichText::new(t("Counted from what each reply reports, on this computer. The provider's own page is the bill."))
                .small()
                .color(pal().muted),
        );
        if ui.small_button(t("Reset the counts")).clicked() {
            match self.usage_filter.take() {
                Some(key) => drop(self.usage.remove(&key)),
                None => self.usage.clear(),
            }
        }
    }

    /// Adds tokens the provider counted to the project's. A hundred projects are told apart; the rest count together as one not yet saved.
    fn add_usage(&mut self, project: String, used: Usage) {
        let key = if self.usage.len() >= MAX_PROJECTS && !self.usage.contains_key(&project) { String::new() } else { project };
        self.usage.entry(key).or_default().add(used);
    }

    /// The tokens counted for the filter's project, or for all.
    fn usage_of(&self) -> Usage {
        let mut sum = Usage::default();
        for (_, u) in self.usage.iter().filter(|(k, _)| self.usage_filter.as_ref().is_none_or(|f| f == *k)) {
            sum.add(*u);
        }
        sum
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
        self.sync_assistant_chat();
        if !self.assistant.busy() || self.importing() || self.installing() {
            return;
        }
        self.made_in();
        // The assistant is an agent as the control port's are: a window that lets agents only read lets it only read.
        let access = if self.agents.edit { Access::Edit } else { Access::Read };
        let Some(chat) = self.assistant.chat.as_mut() else { return };
        let changed = chat.poll(&mut self.session, access);
        let used = chat.take_usage();
        if used != Usage::default() {
            let project = self.session.path.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
            self.assistant.add_usage(project, used);
        }
        if changed {
            // The core renames and drops the option with its set.
            self.active_option = self.session.option.clone();
            ctx.request_repaint();
        }
    }

    /// Where the chats of the projects are kept: beside `settings.json`, when there is one.
    fn chats_dir(&self) -> Option<std::path::PathBuf> {
        self.settings_path.as_ref().and_then(|p| p.parent()).map(|d| d.join("chats"))
    }

    /// Once a frame: keeps a chat for each saved project. The first look at a project (and the first after it was replaced) reads its
    /// saved chat; a chat whose project got a file by Save or Save As goes with it; and once a turn is over, a chat that changed is
    /// written, or its file taken away when New chat emptied it. A project not yet saved has no file to keep a chat in. A file that
    /// cannot be written or read is passed over: the chat on screen is not lost.
    pub(crate) fn sync_assistant_chat(&mut self) {
        let Some(dir) = self.chats_dir() else { return };
        let path = self.session.path.clone();
        let a = &mut self.assistant;
        let provider = a.provider;
        let Some(chat) = a.chat.as_mut().filter(|c| !c.busy()) else { return };
        match &a.chat_for {
            None => {
                if let Some(v) = path.as_deref().and_then(|p| bimcraft_assistant::saved::read(&dir, p)) {
                    chat.restore(&v, provider);
                }
                a.saved_rev = chat.revision();
            }
            Some(was) if *was != path => a.saved_rev = 0,
            Some(_) => {}
        }
        a.chat_for = Some(path.clone());
        let rev = chat.revision();
        if let (Some(project), true) = (path, rev != a.saved_rev) {
            if rev == 0 {
                bimcraft_assistant::saved::remove(&dir, &project);
            } else {
                // shortcut: a failed write is dropped silently; the next change tries again.
                let _ = bimcraft_assistant::saved::write(&dir, &project, chat);
            }
            a.saved_rev = rev;
        }
    }

    /// What the palette shows, laid out as Claude Code's is: a header with the model, the conversation (what the user wrote in a box,
    /// the model's reasoning folded apart from its answers, each tool call with its result under it), the checklist the model is
    /// working through, and the box the user writes in, with the files attached to the message. All of it is one column that scrolls and
    /// sticks to the bottom as the conversation grows, so a short stack of palettes (BIMCraft's Navigator) still reaches every part of it.
    pub(crate) fn assistant_palette(&mut self, ui: &mut Ui) {
        if self.assistant.chat.is_none() {
            Frame::new().inner_margin(8.0).show(ui, |ui| {
                ui.label(RichText::new(t("The assistant needs the network, which this window was started without.")).color(pal().muted));
            });
            return;
        }
        let ctx = ui.ctx().clone();
        let over = ctx.pointer_hover_pos().is_some_and(|p| ui.max_rect().contains(p));
        if over {
            self.assistant.over_at = Some(ctx.input(|i| i.time));
            if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
                ui.painter().rect_stroke(ui.max_rect().shrink(2.0), 6.0, Stroke::new(2.0, pal().accent), egui::StrokeKind::Inside);
            }
        }
        let a = &self.assistant;
        let summary = format!("{} · {}", a.provider.name(), a.model);
        let no_key = (a.env_key().is_none() && !key_ok(a.key.0.trim()))
            .then(|| if a.locked() { t(LOCKED) } else { t("No API key yet. Add one in the Assistant settings.") }.to_string());
        // Sticking to the bottom is for a conversation under way: before it, the palette shows its top.
        let talking = !a.log().is_empty();
        let busy = a.busy();
        ScrollArea::vertical().id_salt("assistant").auto_shrink([false, false]).stick_to_bottom(talking).show(ui, |ui| {
            Frame::new().inner_margin(8.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(summary).small().color(pal().muted));
                    if ui.small_button(t("Assistant settings")).clicked() {
                        self.settings_page = Some(Page::Assistant);
                    }
                    if ui.add_enabled(!busy, egui::Button::new(t("New chat")).small()).clicked()
                        && let Some(chat) = self.assistant.chat.as_mut()
                    {
                        chat.clear();
                    }
                });
                if let Some(note) = no_key {
                    ui.label(RichText::new(note).small().color(pal().muted));
                }
                ui.separator();
                for (i, line) in self.assistant.log().iter().enumerate() {
                    log_line(ui, i, line);
                }
                if let Some(chat) = &self.assistant.chat {
                    checklist(ui, &chat.todos);
                }
                if busy {
                    // Not a spinner: the reply wakes the window, so nothing needs a frame after frame meanwhile.
                    ui.horizontal(|ui| {
                        marker(ui, Marker::Active);
                        ui.label(RichText::new(t("Working…")).color(pal().muted));
                    });
                }
                ui.add_space(6.0);
                self.assistant_input(ui);
            });
        });
    }

    /// File > Settings > Assistant: the provider, the model and the key, which the palette sends with. The key is the provider's
    /// environment variable when that is set, else the one typed here, kept for the session or, with Remember, in `settings.json`.
    pub(crate) fn assistant_page(&mut self, ui: &mut Ui) {
        let chats = self.chats_dir();
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
        heading(ui, msg!("Tokens used"));
        a.usage_ui(ui, self.session.path.as_deref());
        if let Some(dir) = chats {
            heading(ui, msg!("Saved chats"));
            let said = msg!(
                "A chat is kept for each saved project, on this computer, beside settings.json (never in the project). A picture sent in it is not kept."
            );
            ui.label(RichText::new(t(said)).small().color(pal().muted));
            if ui.small_button(t("Delete the saved chats")).clicked() {
                let _ = std::fs::remove_dir_all(&dir);
                // What is on screen stays, and is not written again until it changes.
                a.saved_rev = a.chat.as_ref().map_or(0, Chat::revision);
            }
        }
        ui.add_space(10.0);
        ui.label(
            RichText::new(t("The assistant sends your messages, and what it reads of the open project, to this provider, and only when you send a message. The Assistant palette (AI) asks once before the first."))
                .color(pal().muted),
        );
    }

    /// The agreement said once, then the box the user writes in: a frame holding the files picked, the text, and a row with Attach,
    /// the keys and Send (Stop while a message is worked on). Enter sends and Shift+Enter starts a new line, as in Claude Code.
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
        let (mut send, mut attach, mut stop) = (false, false, false);
        Frame::new().fill(pal().surface).stroke(Stroke::new(1.0, pal().border)).corner_radius(8.0).inner_margin(8.0).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let mut gone = None;
            if let Some(chat) = &self.assistant.chat {
                ui.horizontal_wrapped(|ui| {
                    for (i, f) in chat.attached.iter().enumerate() {
                        let chip = egui::Button::new(RichText::new(format!("{} ×", f.name)).small().monospace().color(pal().accent_ink));
                        if ui.add(chip).on_hover_text(t("Remove from the message")).clicked() {
                            gone = Some(i);
                        }
                    }
                });
                if !chat.attached.is_empty() {
                    let said = tf("Attached files go to {} with your message.", &[self.assistant.provider.name()]);
                    ui.label(RichText::new(said).small().color(pal().muted));
                }
            }
            if let (Some(i), Some(chat)) = (gone, self.assistant.chat.as_mut()) {
                chat.detach(i);
            }
            let id = ui.make_persistent_id("assistant-input");
            // Taken before the box sees the key, which would make a line of it.
            if ui.memory(|m| m.has_focus(id)) && !busy {
                send = ui.input_mut(|i| {
                    let plain = !i.modifiers.shift && i.consume_key(egui::Modifiers::NONE, Key::Enter);
                    plain || i.consume_key(egui::Modifiers::COMMAND, Key::Enter)
                });
            }
            // The keys are said in the empty box, where a narrow palette has room for them.
            let hint = format!("{}\n{}", t("Ask the assistant to look at or change the project"), t("Enter sends · Shift+Enter new line"));
            let edit = TextEdit::multiline(&mut self.assistant.input)
                .id(id)
                .frame(Frame::NONE)
                .desired_width(f32::INFINITY)
                .desired_rows(3)
                .hint_text(hint)
                .interactive(!busy);
            ui.add(edit);
            ui.horizontal(|ui| {
                attach = ui
                    .add_enabled(self.assistant.chat.is_some(), egui::Button::new(t("Attach")).small())
                    .on_hover_text(t("Pick text files or pictures, or drop them here"))
                    .clicked();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if busy {
                        stop = ui.button(t("Stop")).clicked();
                    } else {
                        let label = if agreed { t("Send") } else { t("Agree and send") };
                        send |= ui.add_enabled(!self.assistant.input.trim().is_empty(), egui::Button::new(label)).clicked();
                    }
                });
            });
        });
        if stop && let Some(chat) = self.assistant.chat.as_mut() {
            chat.stop(&mut self.session);
        }
        if attach {
            let ctx = ui.ctx().clone();
            self.ask_files(&ctx, rfd::AsyncFileDialog::new().set_title(t("Attach files")), |app, paths| {
                if let Some(chat) = app.assistant.chat.as_mut() {
                    paths.iter().for_each(|p| chat.attach(p));
                }
            });
        }
        if send && !self.assistant.input.trim().is_empty() {
            self.send_assistant(ui.ctx());
        }
    }

    /// Files dropped on the window while the pointer is over the palette go to the message; the caller opens the others as projects.
    pub(crate) fn drop_on_assistant(&mut self, ctx: &Context, dropped: &mut Vec<std::path::PathBuf>) {
        let now = ctx.input(|i| i.time);
        let Some(chat) = self.assistant.chat.as_mut() else { return };
        if self.assistant.over_at.is_some_and(|at| now - at < 0.5) {
            dropped.drain(..).for_each(|p| chat.attach(&p));
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

/// A marker drawn in code, since a font may lack the bullets and circles: what a line of the conversation begins with, and a step of
/// the checklist.
#[derive(Clone, Copy)]
enum Marker {
    Dot(Color32),
    Pending,
    Active,
    Done,
}

fn marker(ui: &mut Ui, m: Marker) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(14.0), egui::Sense::hover());
    let (c, r, painter) = (rect.center(), 5.5, ui.painter());
    match m {
        Marker::Dot(color) => {
            painter.circle_filled(c, 3.5, color);
        }
        // A dashed ring: eight short arcs.
        Marker::Pending => {
            for i in 0..8 {
                let a0 = i as f32 * std::f32::consts::TAU / 8.0 + 0.15;
                let a1 = a0 + std::f32::consts::TAU / 8.0 - 0.55;
                painter.line_segment([c + r * Vec2::angled(a0), c + r * Vec2::angled(a1)], Stroke::new(1.3, pal().muted));
            }
        }
        // Three quarters of a ring, the look of a spinner that is not spinning.
        Marker::Active => {
            let arc = (0..=24).map(|i| c + r * Vec2::angled(-1.0 + i as f32 / 24.0 * std::f32::consts::TAU * 0.75)).collect();
            painter.add(Shape::line(arc, Stroke::new(2.0, pal().text)));
        }
        Marker::Done => {
            painter.circle_filled(c, r, pal().muted);
            let tick = Stroke::new(1.6, pal().panel);
            painter.line_segment([c + Vec2::new(-2.6, 0.2), c + Vec2::new(-0.7, 2.3)], tick);
            painter.line_segment([c + Vec2::new(-0.7, 2.3), c + Vec2::new(2.8, -2.3)], tick);
        }
    }
}

/// The model's checklist: one step to a row, a ring, an arc or a tick before it, in a bordered panel.
fn checklist(ui: &mut Ui, todos: &[Todo]) {
    if todos.is_empty() {
        return;
    }
    Frame::new().fill(pal().surface).stroke(Stroke::new(1.0, pal().border)).corner_radius(8.0).inner_margin(10.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        for todo in todos {
            let (m, ink) = match todo.status {
                Status::Completed => (Marker::Done, pal().muted),
                Status::InProgress => (Marker::Active, pal().text),
                Status::Pending => (Marker::Pending, pal().text),
            };
            ui.horizontal_top(|ui| {
                marker(ui, m);
                ui.add(egui::Label::new(RichText::new(&todo.content).monospace().color(ink)).wrap());
            });
        }
    });
    ui.add_space(4.0);
}

/// One line of the conversation, as Claude Code draws it: what the user wrote in a box, the model's reasoning folded under
/// Thinking, its words after a dot, each tool call a dot, its name and arguments, and its result indented under it.
fn log_line(ui: &mut Ui, i: usize, line: &Line) {
    let wrap = |ui: &mut Ui, text: RichText| {
        ui.add(egui::Label::new(text).wrap());
    };
    match line {
        Line::You(text) => {
            Frame::new().fill(pal().bar).corner_radius(6.0).inner_margin(6.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_top(|ui| {
                    ui.label(RichText::new("›").strong().color(pal().accent_ink));
                    wrap(ui, RichText::new(text));
                });
            });
        }
        Line::Files(names) => {
            ui.horizontal_wrapped(|ui| {
                for name in names {
                    ui.label(RichText::new(format!("@{name}")).small().monospace().color(pal().accent_ink));
                }
            });
        }
        Line::Thinks(text) => {
            egui::CollapsingHeader::new(RichText::new(t("Thinking")).italics().color(pal().muted))
                .id_salt(("assistant-think", i))
                .default_open(false)
                .show(ui, |ui| {
                    wrap(ui, RichText::new(text).italics().small().color(pal().muted));
                });
        }
        Line::Says(text) => {
            ui.horizontal_top(|ui| {
                marker(ui, Marker::Dot(pal().text));
                wrap(ui, RichText::new(text));
            });
        }
        Line::Used { name, args, result, error } => {
            let (dot, ink) = if *error { (pal().error, pal().error) } else { (pal().context, pal().muted) };
            ui.horizontal_top(|ui| {
                marker(ui, Marker::Dot(dot));
                ui.add(egui::Label::new(RichText::new(name).strong()).wrap());
                wrap(ui, RichText::new(clip(args, 120)).small().monospace().color(pal().muted));
            });
            ui.indent(("assistant-result", i), |ui| wrap(ui, RichText::new(result).small().monospace().color(ink)));
        }
        Line::Note(text) => wrap(ui, RichText::new(text).color(pal().error)),
    }
    ui.add_space(4.0);
}

/// Names the palette and its choices for Undo's list: an undo step the assistant made, `Assistant: <what was asked>`.
pub(crate) fn step_label(label: &str) -> Option<String> {
    label.strip_prefix(&format!("{}: ", bimcraft_assistant::LABEL)).map(|asked| tf(msg!("Assistant: {}"), &[asked]))
}

/// A count with its thousands told apart: 1,234,567.
fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}
