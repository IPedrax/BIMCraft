//! The Plugins window (Manage > Plugins) and the tabs the plugins put on the ribbon.
//!
//! The folder is read on a thread of its own when the app starts, so the first frame isn't kept waiting for it
//! ([`BimApp::load_plugins_soon`]); the result is put into the session's command registry on the next frame. The window
//! lists what was loaded, what was switched off and what failed and why, the plugin log, and a Refresh that reads the
//! folder again without a restart. Plugin labels, tab names and log lines are the plugins' own data and are not translated;
//! everything the window says around them is.

use std::path::PathBuf;
use std::sync::mpsc::TryRecvError;

use bimcraft_i18n::msg;
use bimcraft_plugin::{Broken, Entry, Level, Plugin, Plugins, RibbonTab, Status};
use egui::{Align, Context, Id as UiId, Layout, RichText, ScrollArea, Ui};

use crate::popup::Modal;
use crate::{
    BimApp, Tab,
    lang::{t, tf},
    style::pal,
};

/// What the window asks for when a row or the footer is clicked; done after it has drawn, when it no longer borrows the app.
enum Asked {
    Refresh,
    Set(String, bool),
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

    /// A plugin tab's panels: each of its functions a button, which runs it with no parameters. The labels are the
    /// plugin's data, so they aren't translated.
    pub(crate) fn plugin_tab(&mut self, ui: &mut Ui, index: usize) {
        let Some(tab) = self.plugin_tabs.get(index).cloned() else { return };
        for panel in &tab.panels {
            let commands = panel.commands.clone();
            crate::ribbon::panel(ui, &panel.name, |ui| {
                for command in &commands {
                    let tip = msg!("Runs the plugin's command");
                    if crate::plugin_button(ui, &command.label, tip).clicked() {
                        self.run(&command.id, serde_json::json!({}));
                    }
                }
            });
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
        let plugins: Vec<PluginView> = self.plugins.plugins.iter().map(PluginView::of).collect();
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
    status: Status,
    functions: usize,
}

impl PluginView {
    fn of(p: &Plugin) -> PluginView {
        PluginView {
            id: p.manifest.id.clone(),
            name: p.manifest.name.clone(),
            version: p.manifest.version.clone(),
            author: p.manifest.author.clone(),
            status: p.status.clone(),
            functions: p.manifest.functions.len(),
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
