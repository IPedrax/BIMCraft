//! Updates: the banner under the Options Bar that says a newer BIMCraft is out (or coming down, or ready), the box that holds the
//! window while one is put in place, and the File menu's two items. Checking, downloading and installing are `bimcraft-update`'s;
//! this draws what it says and passes the user's clicks on. A window starts with no updater, so a test never phones home: the
//! desktop's `main` attaches one.

use bimcraft_i18n::msg;

use std::sync::atomic::Ordering;
use std::time::Duration;

use bimcraft_update::{Note, State};
use egui::{Button, Color32, Context, Frame, Id, Modal, OpenUrl, ProgressBar, RichText, Stroke, Ui, ViewportCommand, vec2};

use crate::lang::{t, tf};
use crate::style::pal;
use crate::{BimApp, Pending, Status};

/// What a click on the banner asks for, done once it's drawn.
enum Click {
    Update,
    Later,
    Skip,
    Restart,
    Page(String),
}

pub(crate) fn failed(text: String) -> Status {
    Status { text, error: true }
}

impl BimApp {
    /// Takes what the updater's threads have finished, and says in the status box what a check, a download or an install came to; an
    /// install that went through closes the window.
    pub(crate) fn poll_updates(&mut self, ctx: &Context) {
        let Some(updates) = self.updates.as_mut() else { return };
        let note = updates.poll();
        if matches!(updates.state(), State::Downloading { .. }) {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        let current = updates.current().to_string();
        self.status = match note {
            None => return,
            Some(Note::UpToDate) => Status { text: tf("BIMCraft {} is up to date.", &[&current]), error: false },
            Some(Note::CheckFailed(e)) => failed(tf("Couldn't check for updates: {}", &[&e])),
            Some(Note::DownloadFailed(e)) => failed(tf("Couldn't download the update: {}", &[&e])),
            Some(Note::InstallFailed(e)) => failed(tf("Couldn't install the update: {}", &[&e])),
            // It's in, and this window has to go for the new version to take its place: the project was saved or given up before.
            Some(Note::Installed) => {
                self.closing = true;
                ctx.send_viewport_cmd(ViewportCommand::Close);
                return;
            }
        };
    }

    /// Whether an update is being put in place, which holds the window until it has gone through or failed.
    pub(crate) fn installing(&self) -> bool {
        self.updates.as_ref().is_some_and(|u| matches!(u.state(), State::Installing { .. }))
    }

    /// Whether the banner has something to say.
    pub(crate) fn has_update_banner(&self) -> bool {
        self.updates.as_ref().is_some_and(|u| !matches!(u.state(), State::Idle | State::Installing { .. }))
    }

    /// The box over the window while an update is put in place: nothing under it can be touched, because the window closes when it's in.
    pub(crate) fn installing_box(&self, ctx: &Context) {
        let Some(State::Installing { release, .. }) = self.updates.as_ref().map(|u| u.state()) else { return };
        let text = tf("Installing BIMCraft {}…", &[&release.version.to_string()]);
        Modal::new(Id::new("installing")).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new(text).strong());
            });
        });
    }

    /// The banner: the offer with its three answers, the download's progress, or the update ready to install.
    pub(crate) fn update_banner(&mut self, ui: &mut Ui) {
        let Some(updates) = self.updates.as_ref() else { return };
        let current = updates.current().to_string();
        let mut click = None;
        let frame = Frame::new().fill(pal().hover_fill).inner_margin(vec2(8.0, 4.0)).stroke(Stroke::new(1.0, pal().border));
        frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| match updates.state() {
                State::Idle | State::Installing { .. } => {}
                State::Available(release) => {
                    ui.label(
                        RichText::new(tf("BIMCraft {} is available. You have {}.", &[&release.version.to_string(), &current])).strong(),
                    );
                    if release.installer.is_some() {
                        if primary(ui, "Update now", msg!("Downloads and checks the update; BIMCraft then closes, updates and starts again.")) {
                            click = Some(Click::Update);
                        }
                    } else if primary(ui, "Download", msg!("Opens the release page on GitHub. This copy can't update itself.")) {
                        click = Some(Click::Page(release.page.clone()));
                    }
                    if ui.button(t("Later")).clicked() {
                        click = Some(Click::Later);
                    }
                    if ui.button(t("Skip this version")).clicked() {
                        click = Some(Click::Skip);
                    }
                    if ui.link(t("What's new")).clicked() {
                        click = Some(Click::Page(release.page.clone()));
                    }
                }
                State::Downloading { release, done } => {
                    ui.label(tf("Downloading BIMCraft {}…", &[&release.version.to_string()]));
                    let size = release.installer.as_ref().map_or(1, |i| i.size).max(1);
                    let fraction = (done.load(Ordering::Relaxed) as f32 / size as f32).clamp(0.0, 1.0);
                    ui.add(ProgressBar::new(fraction).desired_width(220.0).fill(pal().accent));
                    ui.label(format!("{}%", (fraction * 100.0) as u32));
                }
                State::Ready { release, .. } => {
                    ui.label(
                        RichText::new(tf("BIMCraft {} is ready. It will close, update and start again.", &[&release.version.to_string()]))
                            .strong(),
                    );
                    if primary(
                        ui,
                        "Restart and install",
                        msg!("Asks to save first if there are unsaved changes, then closes BIMCraft, installs the update and starts it again."),
                    ) {
                        click = Some(Click::Restart);
                    }
                    if ui.button(t("Later")).clicked() {
                        click = Some(Click::Later);
                    }
                }
            });
        });
        let ctx = ui.ctx().clone();
        let Some(click) = click else { return };
        let Some(updates) = self.updates.as_mut() else { return };
        match click {
            Click::Update => updates.update_now(),
            Click::Later => updates.later(),
            Click::Skip => {
                if let Err(e) = updates.skip() {
                    self.status = failed(tf("Couldn't save the setting: {}", &[&e]));
                }
            }
            Click::Restart => self.guard(Pending::Update, &ctx),
            Click::Page(url) => ctx.open_url(OpenUrl::new_tab(url)),
        }
    }

    /// The File menu's updates items: the switch for the check at startup, and a check now.
    pub(crate) fn update_menu(&mut self, ui: &mut Ui) {
        let Some(updates) = self.updates.as_mut() else { return };
        ui.separator();
        let mut on = updates.checks_at_startup();
        if ui.checkbox(&mut on, t("Check for updates at startup")).changed()
            && let Err(e) = updates.set_checks_at_startup(on)
        {
            self.status = failed(tf("Couldn't save the setting: {}", &[&e]));
        }
        if ui.button(t("Check for Updates Now")).clicked() {
            updates.check_now();
        }
    }

    /// Starts putting the update in place, with this project to open again. The window closes when that has gone through
    /// ([`poll_updates`](Self::poll_updates)): it has to, for the new version to take its place.
    pub(crate) fn install_update(&mut self) {
        let project = self.session.path.clone();
        if let Some(updates) = self.updates.as_mut() {
            updates.install(project.as_deref());
        }
    }
}

/// The banner's main button, in the accent as the File tab is: whether it was clicked.
fn primary(ui: &mut Ui, label: &str, tip: &str) -> bool {
    ui.add(Button::new(RichText::new(t(label)).color(Color32::WHITE).strong()).fill(pal().accent)).on_hover_text(t(tip)).clicked()
}
