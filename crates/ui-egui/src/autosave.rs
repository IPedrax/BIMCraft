//! Desktop policy over the core's background writer. Tests opt in with their own temp directory;
//! constructing a BimApp by itself never touches recovery files.
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};
use web_time::{Duration, Instant};

use bimcraft_core::autosave::{Backup, Notice, Worker, backup_path};
use bimcraft_core::ids::Guid;
use bimcraft_core::model::Model;
use egui::{Context, Id, Modal, RichText};
use serde_json::json;

use crate::lang::{t, tf};
use crate::{BimApp, Pending, Status};

pub(crate) struct State {
    worker: Option<Worker>,
    root: PathBuf,
    temporary: PathBuf,
    project: Option<PathBuf>,
    pub active: Option<PathBuf>,
    pub interval_minutes: u32,
    pub next_due: Instant,
    snapshot: Weak<Model>,
    generation: u64,
    pub discovering: usize,
    pub busy: bool,
    pub candidates: Vec<Backup>,
    pub show_recovery: bool,
    deleting: Option<PathBuf>,
    recovery_error: Option<String>,
    retired: Vec<PathBuf>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            worker: None,
            root: PathBuf::new(),
            temporary: PathBuf::new(),
            project: None,
            active: None,
            interval_minutes: 2,
            next_due: Instant::now() + Duration::from_secs(120),
            snapshot: Weak::new(),
            generation: 0,
            discovering: 0,
            busy: false,
            candidates: Vec::new(),
            show_recovery: false,
            deleting: None,
            recovery_error: None,
            retired: Vec::new(),
        }
    }
}

impl State {
    fn protected(&self, path: &Path) -> bool {
        self.candidates.iter().any(|b| b.path == path)
    }

    fn reset_timer(&mut self) {
        self.next_due = Instant::now() + Duration::from_secs(u64::from(self.interval_minutes.max(1)) * 60);
    }

    fn discover(&mut self, project: Option<PathBuf>) -> Result<(), String> {
        if let Some(worker) = &self.worker {
            worker.discover(project, self.generation)?;
            self.discovering += 1;
        }
        Ok(())
    }

    fn select_destination(&mut self, project: Option<&Path>) {
        let named = project.map(backup_path);
        let next = named.filter(|p| !self.protected(p)).unwrap_or_else(|| self.temporary.clone());
        if let Some(old) = self.active.replace(next.clone())
            && old != next
            && !self.protected(&old)
            && !self.retired.contains(&old)
        {
            // Keep a temporary fallback until a replacement snapshot or manual Save succeeds.
            self.retired.push(old);
        }
    }
}

impl BimApp {
    /// Enable desktop recovery after the egui context exists. All discovery and disk writes run
    /// on the worker; only an Arc reference to the committed model crosses the UI boundary.
    pub fn enable_autosave(&mut self, temporary_directory: PathBuf, ctx: &Context) {
        let wake = ctx.clone();
        match Worker::start(temporary_directory.clone(), move || wake.request_repaint()) {
            Ok(worker) => {
                self.autosave.worker = Some(worker);
                self.autosave.root = temporary_directory;
                self.autosave.temporary = backup_path(&self.autosave.root.join(format!("untitled-{}", Guid::new())));
                self.autosave_document();
                if let Err(e) = self.autosave.discover(None) {
                    self.autosave_error(&e);
                }
            }
            Err(e) => self.autosave_error(&e),
        }
    }

    /// Also used when the original project failed to open at startup.
    pub fn inspect_recovery(&mut self, project: PathBuf) {
        if let Err(e) = self.autosave.discover(Some(project)) {
            self.autosave_error(&e);
        }
    }

    pub(crate) fn autosave_document(&mut self) {
        if self.autosave.worker.is_none() {
            return;
        }
        self.autosave.generation += 1;
        self.autosave.discovering = 0;
        self.autosave.snapshot = Weak::new();
        self.autosave.project = self.session.path.clone();
        self.autosave.recovery_error = None;
        // Saved projects are rediscovered when opened again. Postponed copies stay on disk.
        if let Some(worker) = &self.autosave.worker {
            for backup in &self.autosave.candidates {
                if backup.project.is_some() {
                    let _ = worker.release(backup.path.clone());
                }
            }
        }
        self.autosave.candidates.retain(|b| b.project.is_none());
        self.autosave.show_recovery = !self.autosave.candidates.is_empty();
        self.autosave.temporary = backup_path(&self.autosave.root.join(format!("untitled-{}", Guid::new())));
        self.autosave.select_destination(self.session.path.as_deref());
        self.autosave.reset_timer();
        if let Some(project) = self.session.path.clone() {
            self.inspect_recovery(project);
        }
    }

    fn autosave_error(&mut self, e: &str) {
        self.status = Status { text: tf("Recovery copy: {}", &[e]), error: true };
    }

    fn remove_autosave(&mut self, path: PathBuf) {
        if let Some(worker) = &self.autosave.worker
            && let Err(e) = worker.remove(path)
        {
            self.autosave_error(&e);
        }
    }

    /// Ordered after any pending snapshot; unresolved recovery copies are always preserved.
    pub(crate) fn retire_autosave(&mut self) {
        self.cleanup_retired_autosaves();
        if let Some(path) = self.autosave.active.take()
            && !self.autosave.protected(&path)
            && self.autosave.discovering == 0
        {
            self.remove_autosave(path);
        }
    }

    fn cleanup_retired_autosaves(&mut self) {
        for path in std::mem::take(&mut self.autosave.retired) {
            if !self.autosave.protected(&path) {
                self.remove_autosave(path);
            }
        }
    }

    pub(crate) fn autosave_saved(&mut self) {
        if self.autosave.worker.is_none() {
            return;
        }
        self.retire_autosave();
        if self.autosave.project != self.session.path {
            self.autosave.project = self.session.path.clone();
            // Save As can target a project with a recovery copy of its own. Discover it before
            // starting new snapshots; never delete a backup this window has not inspected.
            if let Some(project) = self.session.path.clone() {
                self.inspect_recovery(project);
            }
        }
        self.autosave.select_destination(self.session.path.as_deref());
        self.autosave.snapshot = Weak::new();
        self.autosave.reset_timer();
    }

    pub(crate) fn poll_autosave(&mut self, ctx: &Context) {
        while let Some(note) = self.autosave.worker.as_mut().and_then(Worker::poll) {
            match note {
                Notice::Saved { path, snapshot, result } => {
                    self.autosave.busy = false;
                    if self.autosave.active.as_ref() == Some(&path) && self.autosave.snapshot.ptr_eq(&snapshot) {
                        match result {
                            Ok(()) => self.cleanup_retired_autosaves(),
                            Err(e) => {
                                self.autosave.snapshot = Weak::new();
                                self.autosave_error(&e);
                            }
                        }
                    }
                }
                Notice::Removed { path, result } => {
                    if self.autosave.deleting.as_ref() == Some(&path) {
                        self.autosave.deleting = None;
                        if result.is_ok() {
                            self.autosave.recovery_error = None;
                            self.autosave.candidates.retain(|b| b.path != path);
                            self.autosave.show_recovery = !self.autosave.candidates.is_empty();
                            self.autosave.select_destination(self.session.path.as_deref());
                            self.autosave.snapshot = Weak::new();
                        } else if let Err(e) = &result {
                            self.autosave.recovery_error = Some(e.clone());
                        }
                    }
                    if let Err(e) = result {
                        self.autosave_error(&e);
                    }
                }
                Notice::Discovered { generation, result } if generation == self.autosave.generation => {
                    self.autosave.discovering = self.autosave.discovering.saturating_sub(1);
                    match result {
                        Ok(backups) => {
                            for backup in backups {
                                if !self.autosave.candidates.contains(&backup) {
                                    self.autosave.candidates.push(backup);
                                    self.autosave.show_recovery = true;
                                }
                            }
                            self.autosave.select_destination(self.session.path.as_deref());
                        }
                        Err(e) => self.autosave_error(&e),
                    }
                }
                Notice::Discovered { result: Ok(backups), .. } => {
                    if let Some(worker) = &self.autosave.worker {
                        for backup in backups {
                            if self.autosave.active.as_ref() != Some(&backup.path) && !self.autosave.protected(&backup.path) {
                                let _ = worker.release(backup.path);
                            }
                        }
                    }
                }
                Notice::Discovered { .. } => {}
            }
        }
        if self.autosave.worker.is_none() || self.autosave.interval_minutes == 0 || self.closing {
            return;
        }
        let now = Instant::now();
        if now >= self.autosave.next_due {
            // Capture only committed model state. An unfinished sketch is kept by its tool.
            if self.dirty() && self.autosave.discovering == 0 && !self.autosave.busy {
                let same = self.autosave.snapshot.upgrade().is_some_and(|m| Arc::ptr_eq(&m, &self.session.model));
                if !same && let Some(path) = self.autosave.active.clone() {
                    let snapshot = Arc::clone(&self.session.model);
                    let result = self.autosave.worker.as_mut().map(|w| w.save(path, Arc::clone(&snapshot)));
                    match result {
                        Some(Ok(true)) => {
                            self.autosave.snapshot = Arc::downgrade(&snapshot);
                            self.autosave.busy = true;
                        }
                        Some(Err(e)) => self.autosave_error(&e),
                        _ => {}
                    }
                }
            }
            self.autosave.reset_timer();
        }
        ctx.request_repaint_after(self.autosave.next_due.saturating_duration_since(now));
    }

    pub(crate) fn recovery_window(&mut self, ctx: &Context) {
        if !self.autosave.show_recovery || self.installing() {
            return;
        }
        let Some(backup) = self.autosave.candidates.first().cloned() else { return };
        let mut choice = None;
        let response = Modal::new(Id::new("recover-autosave")).show(ctx, |ui| {
            ui.set_max_width(520.0);
            ui.label(RichText::new(t("Recovery copy found")).heading());
            ui.label(
                backup
                    .project
                    .as_deref()
                    .and_then(Path::file_name)
                    .map_or_else(|| t("Untitled project").to_owned(), |n| n.to_string_lossy().into_owned()),
            );
            ui.label(t("Recover opens the backup as unsaved work. Discard deletes it. Postpone keeps it for later without replacing it."));
            ui.label(backup.path.to_string_lossy()).on_hover_text(backup.path.to_string_lossy());
            if let Some(error) = &self.autosave.recovery_error {
                ui.colored_label(crate::style::pal().error, error);
            }
            ui.add_space(8.0);
            ui.add_enabled_ui(self.autosave.deleting.is_none(), |ui| {
                ui.horizontal(|ui| {
                    for label in ["Recover", "Discard", "Postpone"] {
                        if ui.button(t(label)).clicked() {
                            choice = Some(label);
                        }
                    }
                })
            });
        });
        match choice {
            Some("Recover") => {
                self.autosave.show_recovery = false;
                self.guard(Pending::Recover(backup), ctx);
            }
            Some("Discard") => {
                self.autosave.deleting = Some(backup.path.clone());
                self.remove_autosave(backup.path);
            }
            Some("Postpone") => self.autosave.show_recovery = false,
            _ if response.should_close() => self.autosave.show_recovery = false,
            _ => {}
        }
    }

    pub(crate) fn recover_backup(&mut self, backup: Backup) {
        let saved = Arc::clone(&self.saved);
        if self.run("recover", json!({"path": backup.path, "project": backup.project})).is_none() {
            self.autosave.recovery_error = Some(self.status.text.clone());
            self.autosave.show_recovery = true;
            return;
        }
        self.retire_autosave();
        // Opening a recovered model uses the same view setup as Open, but it remains dirty.
        self.reopened();
        self.saved = saved;
        self.autosave.candidates.retain(|b| b != &backup);
        self.autosave.recovery_error = None;
        self.autosave.show_recovery = !self.autosave.candidates.is_empty();
        self.autosave.generation += 1;
        self.autosave.discovering = 0;
        self.autosave.active = Some(backup.path.clone());
        self.autosave.project = backup.project.clone();
        if backup.project.is_none() {
            self.autosave.temporary = backup.path;
        }
        self.autosave.snapshot = Arc::downgrade(&self.session.model);
        self.autosave.reset_timer();
    }
}
