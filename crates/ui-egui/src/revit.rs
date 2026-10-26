//! Revit files on the desktop. The reader, `bimcraft-rvt`, is Apache-2.0 only (docs/rvt-provenance.md), so this crate never depends
//! on it: the desktop's `main` attaches one as an [`Importer`], and a window without one (every test, a build with
//! `--no-default-features`) offers no `.rvt` and says why when it is given one. A file is read on a thread of its own while a box
//! says so and holds the window still; what comes back replaces the project as a new unsaved one, and a refusal, a panic or a
//! lost thread is said in the status box.

use bimcraft_i18n::msg;

use std::any::Any;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Duration;

use bimcraft_core::import::Imported;
use egui::{Align, Context, Id, Label, Layout, Modal, RichText, ScrollArea};

use crate::lang::{category, t, tf};
use crate::style::pal;
use crate::{BimApp, Status};

/// Reads a Revit project into a model, or says why not. It runs on a thread of its own, and may take minutes on a big file.
pub type Importer = Arc<dyn Fn(&Path) -> Result<Imported, String> + Send + Sync>;

/// The extensions File > Open offers: `.revcraft` is a project saved before the name changed, and `.rvt` is there only with a reader.
pub fn open_extensions(revit: bool) -> Vec<&'static str> {
    let mut extensions = vec!["bimcraft", "revcraft", "ifc"];
    if revit {
        extensions.push("rvt");
    }
    extensions
}

/// Whether `path` names a Revit project. Families (`.rfa`) and templates are not read here.
pub fn is_revit(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("rvt"))
}

/// An import under way: the file it reads, and where its answer comes from.
pub(crate) struct Job {
    file: String,
    answer: Receiver<Result<Imported, String>>,
}

/// What a Revit import said about itself, kept for the window to show until another project replaces it: what came in by
/// category and the importer's notes, which are what was left out and why, and what came in with a caveat.
pub(crate) struct Report {
    file: String,
    schema: String,
    /// What came in: the model's category and how many, the most first.
    pub(crate) counts: Vec<(String, usize)>,
    notes: Vec<String>,
    pub(crate) open: bool,
    /// Copy Report was pressed: its button says so.
    copied: bool,
}

impl Report {
    /// The notes of what was left out, the others, and how many elements were left out in all.
    fn split(&self) -> (Vec<&str>, Vec<&str>, usize) {
        let (left, noted): (Vec<&str>, Vec<&str>) = self.notes.iter().map(String::as_str).partition(|n| count_left_out(n).is_some());
        let left_out = left.iter().filter_map(|n| count_left_out(n)).sum();
        (left, noted, left_out)
    }

    /// The report as the window shows it, in plain text: for pasting into an issue or a file, which the window's own text can't
    /// be selected for past what is on screen.
    pub(crate) fn text(&self) -> String {
        let (left, noted, left_out) = self.split();
        let came_in: usize = self.counts.iter().map(|(_, n)| n).sum();
        let mut lines = vec![format!("{} · {}", self.file, self.schema), String::new(), tf("Came in: {}", &[&came_in.to_string()])];
        lines.extend(self.counts.iter().map(|(cat, n)| format!("{n} {}", category(cat, *n != 1))));
        lines.extend([String::new(), tf("Left out: {}", &[&left_out.to_string()])]);
        if left.is_empty() {
            lines.push(t("Nothing was left out.").to_string());
        }
        lines.extend(left.iter().map(|note| format!("• {note}")));
        if !noted.is_empty() {
            lines.extend([String::new(), t("Noted").to_string()]);
            lines.extend(noted.iter().map(|note| format!("• {note}")));
        }
        lines.join("\n") + "\n"
    }
}

/// A note with its element ids split off: `2 walls left out, why (ids 7, 9)` is the text and `ids 7, 9`.
pub(crate) fn split_ids(note: &str) -> (&str, Option<&str>) {
    match note.strip_suffix(')').and_then(|n| n.rsplit_once(" (")) {
        Some((what, ids)) if ids.starts_with("ids ") => (what, Some(ids)),
        _ => (note, None),
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(|| path.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned())
}

/// How many elements a note of an import says were left out: `N walls left out, why (ids …)` is N. Notes that say something
/// else (an element that came in, narrowed or moved) count none.
pub fn count_left_out(note: &str) -> Option<usize> {
    let (head, _) = note.split_once(", ")?;
    let (n, what) = head.split_once(' ')?;
    what.ends_with(" left out").then(|| n.parse().ok())?
}

/// What a reader that panicked says: the payload's text when it is one.
fn stopped(panic: &(dyn Any + Send)) -> String {
    let why = panic.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| panic.downcast_ref::<String>().cloned());
    match why {
        Some(why) => format!("the Revit reader stopped on this file ({why})"),
        None => "the Revit reader stopped on this file".to_string(),
    }
}

impl BimApp {
    /// A file to open once the window is up, as the command line names one: read like any the system asks for, so the same
    /// questions are asked first.
    pub fn open_later(&mut self, path: PathBuf) {
        self.opens.push(path);
    }

    /// Whether a Revit file is being read.
    pub fn importing(&self) -> bool {
        self.revit_job.is_some()
    }

    /// Starts reading a Revit file on a thread of its own, when this window has a reader and is not reading one already. Whether
    /// it started: the project is replaced later, by [`poll_revit`](Self::poll_revit), and only if the file comes through.
    pub(crate) fn open_revit(&mut self, path: &Path) -> bool {
        let file = file_name(path);
        let Some(reader) = self.revit.clone() else {
            let template = msg!(
                "{}: this build of BIMCraft was made without Revit support (`--no-default-features`); build with the default `rvt` feature to read Revit files"
            );
            self.status = Status { text: tf(template, &[&file]), error: true };
            return false;
        };
        if self.revit_job.is_some() {
            return false;
        }
        let (send, answer) = channel();
        let path = path.to_path_buf();
        // The reader is hostile-input code: whatever it does, the window gets an answer.
        let spawned = std::thread::Builder::new().name("revit-import".into()).spawn(move || {
            let read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| reader(&path))).unwrap_or_else(|p| Err(stopped(&*p)));
            let _ = send.send(read);
        });
        if let Err(e) = spawned {
            self.status = Status { text: tf("Couldn't open {}: {}", &[&file, &e.to_string()]), error: true };
            return false;
        }
        self.revit_job = Some(Job { file, answer });
        true
    }

    /// Takes the answer of the import, when it has come: the project replaces this one, or the status box says why not.
    pub(crate) fn poll_revit(&mut self, ctx: &Context) {
        let Some(job) = &self.revit_job else { return };
        let answer = match job.answer.try_recv() {
            Ok(answer) => answer,
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(100));
                return;
            }
            Err(TryRecvError::Disconnected) => Err("the Revit reader stopped without an answer".to_string()),
        };
        let Some(Job { file, .. }) = self.revit_job.take() else { return };
        match answer {
            Ok(imported) => self.install_revit(&file, imported),
            Err(e) => self.status = Status { text: tf("Couldn't open {}: {}", &[&file, &e]), error: true },
        }
    }

    /// A Revit project that came in is the project now: new, unsaved (saving asks for a `.bimcraft` name) and in no list of recent files.
    fn install_revit(&mut self, file: &str, imported: Imported) {
        let elements = imported.model.elements.len();
        let left = imported.skipped.iter().filter_map(|n| count_left_out(n)).sum::<usize>();
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for e in imported.model.elements.values() {
            *counts.entry(e.category()).or_default() += 1;
        }
        let mut counts: Vec<(String, usize)> = counts.into_iter().map(|(c, n)| (c.to_string(), n)).collect();
        counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let schema = imported.schema;
        // The report comes up by itself when the import has something to say: nothing pops up over a clean one.
        let report = Report {
            file: file.to_string(),
            schema: schema.clone(),
            counts,
            open: !imported.skipped.is_empty(),
            notes: imported.skipped,
            copied: false,
        };
        self.session.reset(imported.model, None);
        // Its own recovery copy, as for any new project, and none left for the one it replaces.
        self.retire_autosave();
        self.reopened();
        self.autosave_document();
        self.revit_report = Some(report);
        let count = if elements == 1 { t("1 element").to_string() } else { tf("{} elements", &[&elements.to_string()]) };
        self.status = Status { text: tf("Opened from {}: {}, {} left out.", &[&schema, &count, &left.to_string()]), error: false };
    }

    /// The box over the window while a Revit file is read: nothing under it can be touched, because the project that comes in
    /// replaces this one.
    pub(crate) fn importing_box(&self, ctx: &Context) {
        let Some(job) = &self.revit_job else { return };
        Modal::new(Id::new("revit-import")).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new(tf("Opening a Revit file: {}", &[&job.file])).strong());
            });
        });
    }
}

impl BimApp {
    /// The report of the last Revit import, while it is open: what came in, what was left out and why, with the element ids.
    pub(crate) fn report_window(&mut self, ctx: &Context) {
        let Some(report) = self.revit_report.as_ref().filter(|r| r.open) else { return };
        let (left, noted, left_out) = report.split();
        let came_in: usize = report.counts.iter().map(|(_, n)| n).sum();
        let (mut close, mut copy) = (false, false);
        // As tall as the window allows, so that a long list is read without scrolling where it can be.
        let height = (ctx.content_rect().height() * 0.65).max(200.0);
        let r = Modal::new(Id::new("revit-report")).show(ctx, |ui| {
            ui.set_width(560.0);
            ui.label(RichText::new(t("Revit import report")).heading());
            ui.label(RichText::new(format!("{} · {}", report.file, report.schema)).color(pal().muted));
            ui.add_space(6.0);
            ScrollArea::vertical().max_height(height).show(ui, |ui| {
                ui.label(RichText::new(tf("Came in: {}", &[&came_in.to_string()])).strong());
                for (cat, n) in &report.counts {
                    ui.label(format!("{n} {}", category(cat, *n != 1)));
                }
                ui.add_space(8.0);
                ui.label(RichText::new(tf("Left out: {}", &[&left_out.to_string()])).strong());
                if left.is_empty() {
                    ui.label(t("Nothing was left out."));
                }
                for note in left {
                    note_row(ui, note);
                }
                if !noted.is_empty() {
                    ui.add_space(8.0);
                    ui.label(RichText::new(t("Noted")).strong());
                    for note in noted {
                        note_row(ui, note);
                    }
                }
            });
            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                close = ui.button(t("Close")).clicked();
                copy = ui.button(t(if report.copied { msg!("Copied") } else { msg!("Copy Report") })).clicked();
            });
        });
        if copy {
            ctx.copy_text(report.text());
            // The button says it was pressed in the next frame, which nothing else asks for.
            ctx.request_repaint();
        }
        if let Some(report) = self.revit_report.as_mut() {
            report.copied |= copy;
            report.open &= !(close || r.should_close());
        }
    }

    /// File > Revit import report, which is there while the project is the one the report is about.
    pub(crate) fn report_menu(&mut self, ui: &mut egui::Ui) {
        if let Some(report) = self.revit_report.as_mut()
            && ui.button(t("Revit import report")).clicked()
        {
            report.open = true;
        }
    }

    /// Whether the report is up, which holds the keys as every dialog does.
    pub(crate) fn report_open(&self) -> bool {
        self.revit_report.as_ref().is_some_and(|r| r.open)
    }
}

/// A note of the import, its element ids under it in the muted ink.
fn note_row(ui: &mut egui::Ui, note: &str) {
    let (what, ids) = split_ids(note);
    ui.add(Label::new(format!("• {what}")).wrap());
    if let Some(ids) = ids {
        ui.add(Label::new(RichText::new(ids).small().monospace().color(pal().muted)).wrap());
    }
}
