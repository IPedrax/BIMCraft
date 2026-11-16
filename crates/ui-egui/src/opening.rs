//! A big project opened on a thread of its own: the window keeps drawing, a box shows how far the read has got (the core's
//! [`Progress`], stage by stage, the elements counted), and Cancel stops it where the read next checks its memory, the project
//! that was open staying as it was. What a small file takes is less than a frame's worth of waiting, so it still opens in the
//! frame (and the tests, which open small files, see it open at once).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

use bimcraft_core::engine::Session;
use bimcraft_core::model::Model;
use bimcraft_core::progress::{CANCELLED, Progress};
use bimcraft_i18n::msg;
use egui::{Context, Id, ProgressBar, RichText};
use serde_json::{Value, json};

use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::style::pal;
use crate::{BimApp, Status};

/// Files at least this big open on a thread of their own, with the box.
pub(crate) const IN_BACKGROUND: u64 = 16 << 20;

/// What the thread hands back: the model read, the path it was read from, and `open`'s reply; or why not.
type Answer = Result<(Arc<Model>, Option<PathBuf>, Value), String>;

/// A project being read.
pub(crate) struct Opening {
    path: PathBuf,
    file: String,
    progress: Arc<Progress>,
    answer: Receiver<Answer>,
    since: Instant,
}

impl BimApp {
    /// Starts reading the project at `path` on a thread of its own. Whether it started: the project is replaced later, by
    /// [`poll_opening`](Self::poll_opening), and only if the file comes through.
    pub(crate) fn open_in_background(&mut self, path: &Path) -> bool {
        let file = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        if self.opening.is_some() {
            self.status = Status { text: tf("{} is still being opened.", &[&file]), error: true };
            return false;
        }
        let progress = Arc::new(Progress::new());
        let (send, answer) = channel();
        let (to, watched, dwg) = (path.to_path_buf(), Arc::clone(&progress), self.session.dwg.clone());
        let spawned = std::thread::Builder::new().name("open".into()).spawn(move || {
            let mut s = Session::new();
            s.dwg = dwg;
            s.progress = Some(watched);
            // The engine runs the command under `catch_unwind` and leaves the scratch session as it was on any failure.
            let read = s.execute("open", &json!({"path": to.to_string_lossy()})).map(|reply| (Arc::clone(&s.model), s.path.clone(), reply));
            drop(s);
            let _ = send.send(read);
        });
        if let Err(e) = spawned {
            self.status = Status { text: tf("Couldn't open {}: {}", &[&file, &e.to_string()]), error: true };
            return false;
        }
        self.opening = Some(Opening { path: path.to_path_buf(), file, progress, answer, since: Instant::now() });
        true
    }

    /// Whether a project is being read.
    pub(crate) fn opening_busy(&self) -> bool {
        self.opening.is_some()
    }

    /// Takes the read's answer when it has come: the project replaces this one, or the status says why not.
    pub(crate) fn poll_opening(&mut self, ctx: &Context) {
        let Some(job) = &self.opening else { return };
        let answer = match job.answer.try_recv() {
            Ok(answer) => answer,
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(50));
                return;
            }
            Err(TryRecvError::Disconnected) => Err("the read stopped without an answer".to_string()),
        };
        let Some(Opening { path, file, .. }) = self.opening.take() else { return };
        match answer {
            Ok((model, at, reply)) => {
                self.session.reset_shared(model, at);
                self.status = Status::default();
                self.opened(&path, &reply);
            }
            Err(e) if e.ends_with(CANCELLED) => {
                self.status = Status { text: tf("Opening {} was cancelled; the project stays as it was.", &[&file]), error: false };
            }
            Err(e) => {
                self.status = Status { text: e, error: true };
                self.open_failed(&path);
            }
        }
    }

    /// The box over the window while a project is read: its stage, a bar for what is counted, and Cancel.
    pub(crate) fn opening_box(&mut self, ctx: &Context) {
        let Some(job) = &self.opening else { return };
        let (stage, done) = job.progress.now();
        let cancelled = job.progress.cancelled();
        let seconds = job.since.elapsed().as_secs();
        let mut cancel = false;
        Modal::new(Id::new("opening"), t("Opening")).width(520.0).closable(false).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.label(RichText::new(tf("Opening {}", &[&job.file])).strong());
                ui.add_space(6.0);
                let words = if cancelled {
                    t("Cancelling…").to_string()
                } else {
                    t(if stage.is_empty() { msg!("Starting") } else { stage }).to_string()
                };
                ui.label(RichText::new(format!("{words} · {seconds} s")).color(pal().muted));
                ui.add_space(4.0);
                let bar = match done {
                    Some(f) => ProgressBar::new(f).show_percentage(),
                    None => ProgressBar::new(0.0).animate(true),
                };
                ui.add(bar.desired_width(480.0));
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    if ui.add_enabled(!cancelled, egui::Button::new(t("Cancel"))).clicked() {
                        cancel = true;
                    }
                });
            });
        });
        if cancel {
            job.progress.cancel();
        }
        ctx.request_repaint_after(Duration::from_millis(100));
    }
}
