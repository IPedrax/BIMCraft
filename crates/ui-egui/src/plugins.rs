//! The Plugins window (Manage > Plugins) and the tabs the plugins put on the ribbon.
//!
//! The folder is read on a thread of its own when the app starts, so the first frame isn't kept waiting for it
//! ([`BimApp::load_plugins_soon`]); the result is put into the session's command registry on the next frame. The window
//! lists what was loaded, what was switched off and what failed and why, the plugin log, and a Refresh that reads the
//! folder again without a restart. Plugin labels, tab names and log lines are the plugins' own data and are not translated;
//! everything the window says around them is. A function that answers with a `report` has its text shown in a dialog of
//! its own ([`BimApp::plugin_report_window`]).

use std::path::PathBuf;
use std::sync::mpsc::TryRecvError;

use bimcraft_i18n::msg;
use bimcraft_plugin::{Broken, Entry, Level, Plugin, Plugins, RibbonTab, Status, UpdateRecord, UpdateSettings, update};
use egui::{Align, Context, Id as UiId, Label, Layout, RichText, ScrollArea, Ui};

use crate::popup::Modal;
use crate::{
    BimApp, Tab,
    lang::{t, tf},
    style::pal,
};

/// What a click on a plugin's function says when it needs files and the user hasn't allowed them: the plugin's name
/// fills both `{}`.
const FILE_ACCESS_ASK: &str =
    "{} isn't allowed to open files. Allow it in Manage > Plugins: the \"Allow file access\" button in the {} row.";

/// What the window asks for when a row or the footer is clicked; done after it has drawn, when it no longer borrows the app.
enum Asked {
    Refresh,
    Set(String, bool),
    /// Grant or take back a permission of a plugin, by its id and the permission's name.
    Permit(String, &'static str, bool),
    /// Read a plugin's channel now.
    Check(String),
    /// Show the confirmation before installing the version the check found.
    AskInstall(String),
    /// Swap the plugin's folder with its newest backup.
    Rollback(String),
    AskBeforeEvery(bool),
    AllowUnsigned(bool),
    AllowPrivate(bool),
    Backups(usize),
}

/// What the window shows before an install: the plugin, both versions, the channel and its key, the size, the
/// publisher's note, and why it asks at all. The plugin's own text is not translated.
#[derive(Clone, Debug)]
pub(crate) struct UpdatePrompt {
    pub id: String,
    pub name: String,
    pub current: String,
    pub next: String,
    pub released: String,
    pub size: u64,
    pub notes: String,
    pub host: String,
    pub key: String,
}

/// What a plugin's function asked the window to show: the text it wrote, under the label of the button that ran it, kept
/// until it is closed. The title and the text are the plugin's own data, so they are not translated.
pub(crate) struct PluginReport {
    title: String,
    text: String,
    /// Copy was pressed: its button says so.
    copied: bool,
}

impl BimApp {
    /// Reads the plugin folder at `folder` on a thread of its own. The window comes up at once; the plugins are put in the
    /// session's registry in a frame or two. None: no folder, no plugins.
    pub fn load_plugins_soon(&mut self, folder: Option<PathBuf>) {
        if folder.is_none() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new().name("plugins".into()).spawn(move || {
            // The window is up before this thread ends; the once-a-start channel check runs here, in the same
            // instance the window will get, so a grant the user makes can't race a second copy of the state.
            let mut plugins = Plugins::load_from(folder.as_deref());
            if plugins.has_update_grants() {
                let installed = plugins.updates_on_start();
                if !installed.is_empty() {
                    // The old commands were never registered: this is still the start, so the folder is read once
                    // more and the new version runs in this launch, not after a restart the user didn't ask for.
                    plugins = Plugins::load_from(folder.as_deref());
                }
            }
            let _ = tx.send(plugins);
        });
        if spawned.is_ok() {
            self.plugins_job = Some(rx);
        }
    }

    /// Puts plugins in place at once, without a thread: what a test or an embedder hands the window.
    pub fn use_plugins(&mut self, plugins: Plugins) {
        self.plugins_job = None;
        self.install_plugins(plugins);
    }

    /// Takes the folder's read when the thread is done, keeping frames coming while it runs.
    pub(crate) fn poll_plugins(&mut self, ctx: &Context) {
        if self.plugins_job.is_some() {
            ctx.request_repaint();
        }
        let mut disconnected = false;
        let received = match &self.plugins_job {
            Some(rx) => match rx.try_recv() {
                Ok(plugins) => Some(plugins),
                Err(TryRecvError::Disconnected) => {
                    disconnected = true;
                    None
                }
                Err(TryRecvError::Empty) => None,
            },
            None => None,
        };
        if let Some(plugins) = received {
            self.install_plugins(plugins);
        }
        if disconnected {
            self.plugins_job = None;
        }
    }

    fn install_plugins(&mut self, plugins: Plugins) {
        let restarts = plugins.restarts();
        plugins.register(&mut self.session);
        self.plugin_tabs = plugins.tabs();
        self.plugins = plugins;
        self.leave_a_gone_plugin_tab();
        if !restarts.is_empty() {
            self.status = crate::Status {
                text: tf("{} plugin update(s) installed; restart BIMCraft to run them.", &[&restarts.len().to_string()]),
                error: false,
            };
        }
    }

    /// Reads the folder again, keeps the plugins' state and the log, and puts their commands and tabs in place.
    fn refresh_plugins(&mut self) {
        self.plugins.refresh();
        self.plugins.register(&mut self.session);
        self.plugin_tabs = self.plugins.tabs();
        self.leave_a_gone_plugin_tab();
        self.status = crate::Status { text: t("The plugin folder was read again.").to_string(), error: false };
    }

    fn switch_plugin(&mut self, id: &str, enabled: bool) {
        match self.plugins.set_enabled(id, enabled) {
            Ok(()) => {
                self.plugins.register(&mut self.session);
                self.plugin_tabs = self.plugins.tabs();
                self.leave_a_gone_plugin_tab();
            }
            Err(e) => self.status = crate::Status { text: e, error: true },
        }
    }

    /// Grants or takes back a permission the user chose in the window, and puts the plugins' commands in place again:
    /// the grant is what lets a call reach the disk.
    fn permit_plugin(&mut self, id: &str, permission: &'static str, granted: bool) {
        match self.plugins.set_permission(id, permission, granted) {
            Ok(()) => {
                self.plugins.register(&mut self.session);
                self.plugin_tabs = self.plugins.tabs();
                self.leave_a_gone_plugin_tab();
                self.status = crate::Status {
                    text: match (permission, granted) {
                        ("update", true) => t("The plugin may now update itself.").to_string(),
                        ("update", false) => t("The plugin may no longer update itself.").to_string(),
                        (_, true) => t("The plugin may now use files.").to_string(),
                        (_, false) => t("The plugin may no longer use files.").to_string(),
                    },
                    error: false,
                };
            }
            Err(e) => self.status = crate::Status { text: e, error: true },
        }
    }

    /// Reads a plugin's channel now. The check runs on this frame, as the window's other buttons do; its answer is a
    /// line in the status bar and the log, and the row offers Install when there is something newer.
    fn check_plugin_update(&mut self, id: &str) {
        match self.plugins.check_update(id, true) {
            Ok(check) if check.available == check.current => {
                self.status = crate::Status { text: format!("{} is current ({}).", check.current, id), error: false };
            }
            Ok(check) => {
                self.status = crate::Status { text: format!("{} {} is available for {}.", id, check.available, check.host), error: false };
            }
            Err(e) => self.status = crate::Status { text: e, error: true },
        }
    }

    /// The confirmation the row opens before installing: the same facts the plugin's own call shows. It needs the
    /// user's own check first; the startup check alone doesn't offer the install.
    fn ask_install(&mut self, id: &str) {
        if !self.plugins.manual_check_ready(id) {
            self.status = crate::Status { text: t("Check for an update first.").to_string(), error: true };
            return;
        }
        match self.plugins.updater().cached(id) {
            Some(check) if check.available != check.current => {
                self.plugin_update_prompt = Some(UpdatePrompt {
                    id: id.to_string(),
                    name: self
                        .plugins
                        .plugins
                        .iter()
                        .find(|p| p.manifest.id == id)
                        .map(|p| p.manifest.name.clone())
                        .unwrap_or_else(|| id.to_string()),
                    current: check.current,
                    next: check.available,
                    released: check.released,
                    size: check.size,
                    notes: check.notes,
                    host: check.host,
                    key: check.key,
                });
            }
            _ => self.status = crate::Status { text: t("Check for an update first.").to_string(), error: true },
        }
    }

    fn install_plugin_update(&mut self, id: &str) {
        match self.plugins.apply_update(id, true) {
            Ok(installed) => {
                self.status =
                    crate::Status { text: format!("{} installed. Restart BIMCraft to run it.", installed.installed), error: false };
            }
            Err(e) => self.status = crate::Status { text: e, error: true },
        }
    }

    fn rollback_plugin(&mut self, id: &str) {
        match self.plugins.rollback_update(id, None) {
            Ok(installed) => {
                self.status =
                    crate::Status { text: format!("{} restored. Restart BIMCraft to run it.", installed.installed), error: false };
            }
            Err(e) => self.status = crate::Status { text: e, error: true },
        }
    }

    fn set_update_settings(&mut self, change: impl FnOnce(&mut UpdateSettings)) {
        let mut settings = self.plugins.update_settings();
        change(&mut settings);
        if let Err(e) = self.plugins.set_update_settings(settings) {
            self.status = crate::Status { text: e, error: true };
        }
    }

    /// The install confirmation, while it is open: both versions, the channel, its key and the publisher's note.
    /// Install swaps the folder; Cancel and Escape close it with nothing changed.
    pub(crate) fn plugin_update_window(&mut self, ctx: &Context) {
        let Some(prompt) = self.plugin_update_prompt.clone() else { return };
        let (mut install, mut close) = (false, false);
        let notes = prompt.notes.clone();
        let r = Modal::new(UiId::new("plugin-update"), t("Install the update?")).width(640.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(6.0);
                ui.label(RichText::new(format!("{} · {}", prompt.name, prompt.id)).strong());
                ui.label(tf("Version {} → {}", &[prompt.current.as_str(), prompt.next.as_str()]));
                ui.label(tf("Channel: {} · key {}", &[prompt.host.as_str(), prompt.key.as_str()])).on_hover_text(&prompt.key);
                ui.label(tf("Size: {} KB · released {}", &[&(prompt.size / 1024).to_string(), prompt.released.as_str()]));
                if !notes.is_empty() {
                    ui.add_space(6.0);
                    ui.label(RichText::new(t("What's new")).strong());
                    ui.label(&notes);
                }
                ui.add_space(6.0);
                ui.label(
                    RichText::new(t("The new version takes effect at the next start. The old folder is kept as a backup."))
                        .color(pal().muted),
                );
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    install = ui.button(t("Install")).clicked();
                    close = ui.button(t("Cancel")).clicked();
                });
            });
        });
        if install {
            let id = prompt.id.clone();
            self.plugin_update_prompt = None;
            self.install_plugin_update(&id);
            return;
        }
        if close || r.should_close() {
            self.plugin_update_prompt = None;
        }
    }

    /// A plugin tab that is open while the folder is read again gives way to BIMCraft's own: its index may name another tab
    /// now, or none.
    fn leave_a_gone_plugin_tab(&mut self) {
        if matches!(self.tab, Tab::Plugin(_)) {
            self.tab = Tab::Architecture;
        }
    }

    /// What a plugin tab is called: its manifest's name, or the common Plugins section for one that declared none.
    pub(crate) fn plugin_tab_name(&self, tab: &RibbonTab) -> String {
        tab.name.clone().unwrap_or_else(|| t("Plugins").to_string())
    }

    /// A plugin tab's panels: each of its functions a button, which runs it with no parameters. A function that answers
    /// with a `report` has that text opened on screen under the button's label. The labels are the plugin's data, so
    /// they aren't translated.
    pub(crate) fn plugin_tab(&mut self, ui: &mut Ui, index: usize) {
        let Some(tab) = self.plugin_tabs.get(index).cloned() else { return };
        for panel in &tab.panels {
            let commands = panel.commands.clone();
            crate::ribbon::panel(ui, &panel.name, |ui| {
                for command in &commands {
                    let tip = msg!("Runs the plugin's command");
                    if crate::plugin_button(ui, &command.label, tip).clicked() {
                        self.run_plugin_function(&command.id, command.label.clone(), ui.ctx());
                    }
                }
            });
        }
    }

    /// Keeps what a plugin's function asked the window to show, for
    /// [`plugin_report_window`](Self::plugin_report_window) to draw on the next frame.
    pub(crate) fn open_plugin_report(&mut self, title: String, text: &str) {
        self.plugin_report = Some(PluginReport { title, text: text.to_string(), copied: false });
    }

    /// Runs a plugin's function as its ribbon button and Find do: one click, no parameters typed. A function whose
    /// manifest makes `path` required asks the user for the file in the system's dialog first — that is what the plugin
    /// gets the `files` permission for — and runs with the chosen path; with nothing chosen, nothing runs.
    ///
    /// A function that needs a file needs the permission too, so the check comes before the dialog: a plugin without it
    /// says what to do in the status bar and the system's dialog never opens.
    pub(crate) fn run_plugin_function(&mut self, id: &str, label: String, ctx: &Context) {
        if let Some(field) = self.missing_path(id) {
            if let Some(name) = self.plugin_without_files(id) {
                let text = self.file_access_ask(&name);
                self.status = crate::Status { text, error: true };
                return;
            }
            let dialog = rfd::AsyncFileDialog::new().set_title(label.clone());
            self.plugin_pick = Some((id.to_string(), serde_json::Map::new()));
            self.ask_file(ctx, dialog, false, move |app, path| {
                let Some((command, mut params)) = app.plugin_pick.take() else { return };
                params.insert(field, serde_json::Value::String(path.to_string_lossy().into_owned()));
                app.finish_plugin_function(&command, label, params);
            });
            return;
        }
        self.finish_plugin_function(id, label, serde_json::Map::new());
    }

    /// The plugin's name, when its function needs a file and the manifest asks for the `files` permission but the user
    /// hasn't allowed it: the call would be refused, and the file dialog must not open first.
    fn plugin_without_files(&self, id: &str) -> Option<String> {
        let (plugin, _) = id.strip_prefix("plugin:")?.split_once(':')?;
        let p = self.plugins.plugins.iter().find(|p| p.manifest.id == plugin)?;
        let asks = p.manifest.asks_for_files();
        let granted = self.plugins.permission_granted(plugin, "files");
        (asks && !granted).then(|| p.manifest.name.clone())
    }

    /// What a click on a function that needs files says when the manifest asks for the permission and the user hasn't
    /// allowed it: the file dialog opens only after the permission is given.
    fn file_access_ask(&self, plugin: &str) -> String {
        tf(FILE_ACCESS_ASK, &[plugin, plugin])
    }

    /// Whether the function's manifest needs a file it wasn't given: a required `path` (or `file`) that a click on its
    /// button has no way to fill. It answers with that parameter's name.
    fn missing_path(&self, id: &str) -> Option<String> {
        let (plugin, function) = id.strip_prefix("plugin:")?.split_once(':')?;
        let p = self.plugins.plugins.iter().find(|p| p.manifest.id == plugin && p.status.is_loaded())?;
        let f = p.manifest.functions.iter().find(|f| f.id == function)?;
        let required = f.params.get("required")?.as_array()?;
        ["path", "file"].into_iter().find(|key| required.iter().any(|r| r.as_str() == Some(key))).map(str::to_string)
    }

    /// Runs the function with `params` and opens its `report`, as the button says what it found.
    fn finish_plugin_function(&mut self, id: &str, label: String, params: serde_json::Map<String, serde_json::Value>) {
        if let Some(value) = self.run(id, serde_json::Value::Object(params))
            && let Some(report) = value.get("report").and_then(|v| v.as_str())
        {
            self.open_plugin_report(label, report);
        }
    }

    /// A plugin function's report, while it is open: the text it wrote, a line a row in the font a report is read in,
    /// with Copy to take all of it to the clipboard. The corner's × and Escape close it, as any dialog does. A new or
    /// opened project drops it with the model it was about.
    pub(crate) fn plugin_report_window(&mut self, ctx: &Context) {
        let Some(report) = self.plugin_report.as_ref() else { return };
        let (mut close, mut copy) = (false, false);
        let r = Modal::new(UiId::new("plugin-report"), report.title.clone()).width(680.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(6.0);
                for line in report.text.lines() {
                    if line.is_empty() {
                        ui.add_space(6.0);
                    } else {
                        ui.add(Label::new(RichText::new(line).monospace()).selectable(true));
                    }
                }
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    close = ui.button(t("Close")).clicked();
                    copy = ui.button(t(if report.copied { msg!("Copied") } else { msg!("Copy") })).clicked();
                });
            });
        });
        if copy {
            ctx.copy_text(report.text.clone());
            // The button says it was pressed in the next frame, which nothing else asks for.
            ctx.request_repaint();
        }
        if let Some(report) = self.plugin_report.as_mut() {
            report.copied |= copy;
        }
        if close || r.should_close() {
            self.plugin_report = None;
        }
    }

    /// The Plugins window: what was loaded, what wasn't and why, the log, and a Refresh.
    pub(crate) fn plugins_window(&mut self, ctx: &Context) {
        if !self.plugins_dialog {
            return;
        }
        let mut asked = None;
        let mut close = false;
        let folder = self.plugins.folder.as_ref().map(|f| f.display().to_string()).unwrap_or_else(|| t("None").to_string());
        let mut plugins: Vec<PluginView> = self
            .plugins
            .plugins
            .iter()
            .map(|p| {
                PluginView::of(
                    p,
                    ViewFacts {
                        files_granted: self.plugins.permission_granted(&p.manifest.id, "files"),
                        update_granted: self.plugins.permission_granted(&p.manifest.id, "update"),
                        restart: self.plugins.restart_required(&p.manifest.id),
                        check: self.plugins.updater().cached(&p.manifest.id),
                        manual_ready: self.plugins.manual_check_ready(&p.manifest.id),
                        backup: !self.plugins.backups(&p.manifest.id).is_empty(),
                        last_update: self.plugins.update_history(&p.manifest.id).last().cloned(),
                    },
                )
            })
            .collect();
        let settings = self.plugins.update_settings();
        let _ = ctx;
        for view in &mut plugins {
            view.shown = ctx.data(|d| d.get_temp::<bool>(UiId::new(("permission", view.id.clone(), "files"))));
            view.shown_update = ctx.data(|d| d.get_temp::<bool>(UiId::new(("permission", view.id.clone(), "update"))));
        }
        let broken: Vec<Broken> = self.plugins.broken.clone();
        let log = self.plugins.log();
        let r = Modal::new(UiId::new("plugins"), t("Plugins")).width(720.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(t("Folder")).strong());
                    ui.label(folder.clone());
                });
                ui.label(
                    RichText::new(t(
                        "A plugin is a folder with plugin.json and its module; BIMCraft runs its functions in a sandbox, through commands.",
                    ))
                    .color(pal().muted),
                );
                ui.add_space(6.0);
                ui.collapsing(t("Update settings"), |ui| {
                    let (mut ask, mut unsigned, mut private) = (settings.ask_before_every, settings.allow_unsigned, settings.allow_private);
                    if ui.checkbox(&mut ask, t("Ask before every plugin update")).changed() {
                        asked = Some(Asked::AskBeforeEvery(ask));
                    }
                    if ui.checkbox(&mut unsigned, t("Allow unsigned plugin updates")).changed() {
                        asked = Some(Asked::AllowUnsigned(unsigned));
                    }
                    if ui.checkbox(&mut private, t("Allow local update channels (development only; after restart)")).changed() {
                        asked = Some(Asked::AllowPrivate(private));
                    }
                    let mut backups = settings.backups;
                    ui.horizontal(|ui| {
                        ui.label(t("Backups kept for a plugin"));
                        if ui.add(egui::DragValue::new(&mut backups).range(1..=10)).changed() {
                            asked = Some(Asked::Backups(backups));
                        }
                    });
                    ui.label(
                        RichText::new(t("A signed feed and the archive's SHA-256 are checked even when these are on."))
                            .color(pal().muted)
                            .size(12.0),
                    );
                });
                ui.add_space(6.0);
                ScrollArea::vertical().id_salt("plugins-list").max_height(240.0).auto_shrink([false, true]).show(ui, |ui| {
                    if plugins.is_empty() && broken.is_empty() {
                        ui.label(t("No plugins in the folder."));
                    }
                    for p in &plugins {
                        plugin_row(ui, p, &mut asked);
                    }
                    for b in &broken {
                        ui.add_space(4.0);
                        ui.label(RichText::new(&b.name).strong());
                        ui.label(RichText::new(tf("Couldn't be read: {}", &[b.reason.as_str()])).color(pal().error));
                    }
                });
                ui.add_space(6.0);
                ui.separator();
                ui.label(RichText::new(t("Log")).strong());
                ScrollArea::vertical().id_salt("plugins-log").max_height(180.0).auto_shrink([false, true]).stick_to_bottom(true).show(
                    ui,
                    |ui| {
                        if log.is_empty() {
                            ui.label(RichText::new(t("Nothing yet.")).color(pal().muted));
                        }
                        for e in log.iter().rev().take(200).rev() {
                            log_line(ui, e);
                        }
                    },
                );
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    close = ui.button(t("Close")).clicked();
                    if ui.button(t("Refresh")).clicked() {
                        asked = Some(Asked::Refresh);
                    }
                });
            });
        });
        if close || r.should_close() {
            self.plugins_dialog = false;
        }
        match asked {
            Some(Asked::Refresh) => self.refresh_plugins(),
            Some(Asked::Set(id, on)) => self.switch_plugin(&id, on),
            Some(Asked::Permit(id, permission, on)) => self.permit_plugin(&id, permission, on),
            Some(Asked::Check(id)) => self.check_plugin_update(&id),
            Some(Asked::AskInstall(id)) => self.ask_install(&id),
            Some(Asked::Rollback(id)) => self.rollback_plugin(&id),
            Some(Asked::AskBeforeEvery(on)) => self.set_update_settings(|s| s.ask_before_every = on),
            Some(Asked::AllowUnsigned(on)) => self.set_update_settings(|s| s.allow_unsigned = on),
            Some(Asked::AllowPrivate(on)) => self.set_update_settings(|s| s.allow_private = on),
            Some(Asked::Backups(count)) => self.set_update_settings(|s| s.backups = count),
            None => {}
        }
    }
}

/// What the window needs of a plugin, copied out of it so the list draws while the window is borrowed.
struct PluginView {
    id: String,
    name: String,
    version: String,
    author: String,
    /// The plugin's own description: what it says it is for, which is what the window says when it asks for a permission.
    description: String,
    status: Status,
    functions: usize,
    /// Whether the manifest asks to reach files, and whether the user granted it.
    asks_files: bool,
    files_granted: bool,
    /// Whether the manifest asks to update itself, whether it may, and what the last check found.
    asks_update: bool,
    update_granted: bool,
    /// The user pressed Check for update: only then may Update now install what it found.
    manual_ready: bool,
    check: Option<update::Check>,
    /// The channel's host and the key of the first install: what the row says before any check.
    host: String,
    key: String,
    /// The new version is on disk and waits for a restart; there is a backup to go back to; the last event.
    restart: bool,
    backup: bool,
    last_update: Option<UpdateRecord>,
    /// What the user just answered, before the folder's state has been written: the row says it at once.
    shown: Option<bool>,
    shown_update: Option<bool>,
}

/// What the window already knows of a plugin: its grants, its last check, its restart and its backup.
struct ViewFacts {
    files_granted: bool,
    update_granted: bool,
    restart: bool,
    check: Option<update::Check>,
    manual_ready: bool,
    backup: bool,
    last_update: Option<UpdateRecord>,
}

impl PluginView {
    fn of(p: &Plugin, facts: ViewFacts) -> PluginView {
        let host = p.manifest.update_feed().map(|u| bimcraft_update::net::host_of(&u.feed)).unwrap_or_default();
        let key = p.manifest.update.as_ref().map(|u| short_key(u.key())).unwrap_or_default();
        PluginView {
            id: p.manifest.id.clone(),
            name: p.manifest.name.clone(),
            version: p.manifest.version.clone(),
            author: p.manifest.author.clone(),
            description: p.manifest.description.clone(),
            status: p.status.clone(),
            functions: p.manifest.functions.len(),
            asks_files: p.manifest.asks_for_files(),
            files_granted: facts.files_granted,
            asks_update: p.manifest.asks_for_update(),
            update_granted: facts.update_granted,
            manual_ready: facts.manual_ready,
            check: facts.check,
            host,
            key,
            restart: facts.restart,
            backup: facts.backup,
            last_update: facts.last_update,
            shown: None,
            shown_update: None,
        }
    }
}

/// `ed25519:AB12CD34…`, as the window shows a key without a line too long to read.
pub(crate) fn short_key(key: &str) -> String {
    if key.is_empty() {
        return String::new();
    }
    let body = key.strip_prefix("ed25519:").unwrap_or(key);
    let head: String = body.chars().take(16).collect();
    format!("ed25519:{head}{}", if body.chars().count() > 16 { "…" } else { "" })
}

fn plugin_row(ui: &mut Ui, p: &PluginView, asked: &mut Option<Asked>) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            // A manual install in a running session keeps the old module until the restart; the header says which
            // version will run then instead of looking as if nothing changed.
            let version = match (&p.last_update, p.restart) {
                (Some(record), true) => format!("{} → {}", p.version, record.to),
                _ => p.version.clone(),
            };
            ui.label(RichText::new(format!("{} {version}", p.name)).strong().size(14.0));
            let author = if p.author.is_empty() { String::new() } else { format!(" · {}", p.author) };
            let count = if p.functions == 1 { t("1 function").to_string() } else { tf("{} functions", &[&p.functions.to_string()]) };
            ui.label(RichText::new(format!("{}{author} · {count}", p.id)).color(pal().muted));
            if p.restart {
                ui.label(RichText::new(t("Update installed; restart BIMCraft to run it.")).color(pal().muted).size(12.0));
            }
            if let Some(check) = &p.check
                && check.available != check.current
            {
                ui.label(
                    RichText::new(tf("{} is available from {}", &[check.available.as_str(), check.host.as_str()]))
                        .color(pal().muted)
                        .size(12.0),
                );
            }
            if let Some(record) = &p.last_update {
                ui.label(
                    RichText::new(tf("Last update: {} → {} · {}", &[record.from.as_str(), record.to.as_str(), record.at.as_str()]))
                        .color(pal().muted)
                        .size(12.0),
                );
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| match &p.status {
            Status::Loaded => {
                if ui.button(t("Disable")).clicked() {
                    *asked = Some(Asked::Set(p.id.clone(), false));
                }
                ui.label(RichText::new(t("Loaded")).color(pal().muted));
            }
            Status::Disabled => {
                if ui.button(t("Enable")).clicked() {
                    *asked = Some(Asked::Set(p.id.clone(), true));
                }
                ui.label(RichText::new(t("Disabled")).color(pal().muted));
            }
            Status::Failed(why) => {
                if ui.button(t("Enable")).clicked() {
                    *asked = Some(Asked::Set(p.id.clone(), true));
                }
                ui.label(RichText::new(t("Error")).color(pal().error)).on_hover_text(why);
            }
        });
    });
    if p.asks_files {
        // The manifest's request, which only the user can answer: the plugin's own description says why it wants it.
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            ui.label(RichText::new(t("File access")).strong().size(12.0)).on_hover_text(t(
                "This plugin asks to read and write files on your computer. Its commands reach the disk only while you allow it.",
            ));
            let granted = p.shown.unwrap_or(p.files_granted);
            if ui
                .add_enabled(
                    p.status.is_loaded(),
                    egui::Button::new(if granted { t("Revoke file access") } else { t("Allow file access") }),
                )
                .clicked()
            {
                // The user's answer, shown at once: the row and its button change on this frame, as they would
                // when the window read the answer; the folder's state is written after the window closes.
                ui.ctx().data_mut(|d| d.insert_temp(UiId::new(("permission", p.id.clone(), "files")), !granted));

                *asked = Some(Asked::Permit(p.id.clone(), "files", !granted));
            }
            ui.label(RichText::new(if granted { t("Allowed") } else { t("Not allowed") }).color(pal().muted).size(12.0));
        });
        if !p.description.is_empty() {
            ui.horizontal(|ui| {
                ui.add_space(4.0);
                ui.label(RichText::new(&p.description).color(pal().muted).size(12.0));
            });
        }
    }
    if p.asks_update {
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            ui.label(RichText::new(t("Plugin updates")).strong().size(12.0));
            let granted = p.shown_update.unwrap_or(p.update_granted);
            if ui
                .add_enabled(
                    p.status.is_loaded(),
                    egui::Button::new(if granted { t("Disable automatic updates") } else { t("Enable automatic updates") }),
                )
                .clicked()
            {
                ui.ctx().data_mut(|d| d.insert_temp(UiId::new(("permission", p.id.clone(), "update")), !granted));
                *asked = Some(Asked::Permit(p.id.clone(), "update", !granted));
            }
            ui.label(
                RichText::new(if granted { t("Automatic updates: on") } else { t("Automatic updates: off") }).color(pal().muted).size(12.0),
            );
        });
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            let host = if p.host.is_empty() { t("No channel declared").to_string() } else { p.host.clone() };
            ui.label(RichText::new(tf("Channel: {}", &[&host])).color(pal().muted).size(12.0));
            if !p.key.is_empty() {
                ui.label(RichText::new(tf("Publisher key: {}", &[&p.key])).color(pal().muted).size(12.0));
            }
        });
        if p.update_granted {
            ui.horizontal(|ui| {
                ui.add_space(4.0);
                if ui.button(t("Check for update")).clicked() {
                    *asked = Some(Asked::Check(p.id.clone()));
                }
                let install = p.manual_ready && p.check.as_ref().is_some_and(|c| c.available != c.current);
                if ui.add_enabled(install, egui::Button::new(t("Update now"))).clicked() {
                    *asked = Some(Asked::AskInstall(p.id.clone()));
                }
                if ui.add_enabled(p.backup, egui::Button::new(t("Roll back"))).clicked() {
                    *asked = Some(Asked::Rollback(p.id.clone()));
                }
            });
            if !p.manual_ready && p.check.as_ref().is_some_and(|c| c.available != c.current) {
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(t("A new version is ready: press Check for update to enable Update now."))
                            .color(pal().muted)
                            .size(12.0),
                    );
                });
            }
        }
        if !p.description.is_empty() {
            ui.horizontal(|ui| {
                ui.add_space(4.0);
                ui.label(RichText::new(&p.description).color(pal().muted).size(12.0));
            });
        }
        if !p.update_granted {
            ui.horizontal(|ui| {
                ui.add_space(4.0);
                ui.label(
                    RichText::new(t(
                        "A plugin allowed to update can replace its own code without further questions. Allow it only for a publisher you trust.",
                    ))
                    .color(pal().muted)
                    .size(12.0),
                );
            });
        }
    }
    if let Status::Failed(why) = &p.status {
        ui.label(RichText::new(why).color(pal().error));
    }
}

/// The window's rows ask what to do after it has drawn: switching a plugin changes the session's registry.
fn log_line(ui: &mut Ui, e: &Entry) {
    let colour = match e.level {
        Level::Info => pal().muted,
        Level::Warning | Level::Error => pal().error,
    };
    ui.label(RichText::new(format!("{}: {}", e.plugin, e.text)).color(colour).size(12.0));
}
