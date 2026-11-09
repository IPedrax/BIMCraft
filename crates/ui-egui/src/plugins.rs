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
use bimcraft_plugin::{Broken, Entry, Level, Plugin, Plugins, RibbonTab, Status};
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
            let _ = tx.send(Plugins::load_from(folder.as_deref()));
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
        let received = match &self.plugins_job {
            Some(rx) => match rx.try_recv() {
                Ok(plugins) => Some(plugins),
                Err(TryRecvError::Disconnected) => {
                    self.plugins_job = None;
                    None
                }
                Err(TryRecvError::Empty) => None,
            },
            None => None,
        };
        if let Some(plugins) = received {
            self.plugins_job = None;
            self.install_plugins(plugins);
        }
    }

    fn install_plugins(&mut self, plugins: Plugins) {
        plugins.register(&mut self.session);
        self.plugin_tabs = plugins.tabs();
        self.plugins = plugins;
        self.leave_a_gone_plugin_tab();
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
                    text: if granted {
                        t("The plugin may now use files.").to_string()
                    } else {
                        t("The plugin may no longer use files.").to_string()
                    },
                    error: false,
                };
            }
            Err(e) => self.status = crate::Status { text: e, error: true },
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
        let mut plugins: Vec<PluginView> =
            self.plugins.plugins.iter().map(|p| PluginView::of(p, self.plugins.permission_granted(&p.manifest.id, "files"))).collect();
        let _ = ctx;
        for view in &mut plugins {
            view.shown = ctx.data(|d| d.get_temp::<bool>(UiId::new(("permission", view.id.clone(), "files"))));
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
    /// What the user just answered, before the folder's state has been written: the row says it at once.
    shown: Option<bool>,
}

impl PluginView {
    fn of(p: &Plugin, granted: bool) -> PluginView {
        PluginView {
            id: p.manifest.id.clone(),
            name: p.manifest.name.clone(),
            version: p.manifest.version.clone(),
            author: p.manifest.author.clone(),
            description: p.manifest.description.clone(),
            status: p.status.clone(),
            functions: p.manifest.functions.len(),
            asks_files: p.manifest.asks_for_files(),
            files_granted: granted,
            shown: None,
        }
    }
}

fn plugin_row(ui: &mut Ui, p: &PluginView, asked: &mut Option<Asked>) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(RichText::new(format!("{} {}", p.name, p.version)).strong().size(14.0));
            let author = if p.author.is_empty() { String::new() } else { format!(" · {}", p.author) };
            let count = if p.functions == 1 { t("1 function").to_string() } else { tf("{} functions", &[&p.functions.to_string()]) };
            ui.label(RichText::new(format!("{}{author} · {count}", p.id)).color(pal().muted));
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
