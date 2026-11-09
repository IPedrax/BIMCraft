//! An export on a thread of its own: the window goes on while it writes, the status bar shows how far it is, and Cancel stops
//! it soon after (between pages and files: [`bimcraft_core::export::Ctx`]). The job is run on a snapshot of the project, which
//! an export never changes, so editing goes on under it; what is written is the project as it was when Export was pressed.
//! What it wrote, what it couldn't and why, and whether it was stopped, is said in the status box when it is done.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bimcraft_core::export::{self, Ctx, Job, Report};
use bimcraft_i18n::msg;
use egui::{Context, ProgressBar, Ui};

use crate::lang::{t, tf};
use crate::style::pal;
use crate::{BimApp, Status};

/// How far an export is: steps done, steps there are, and what it is on.
type Progress = (usize, usize, String);

/// A job printed rather than kept: the spooler, the printer, how many copies, the paper's name if it has one the
/// spooler knows, and the title the printer's queue shows. Its files are written in a folder of their own, removed after.
pub(crate) struct PrintTo {
    pub(crate) spooler: Arc<dyn super::print::Spooler>,
    pub(crate) printer: String,
    pub(crate) copies: u32,
    pub(crate) media: Option<&'static str>,
    pub(crate) title: String,
}

/// What a job did, and when it was printed, whether the printer took it (its name) or why not.
type Answer = Result<(Report, Option<Result<String, String>>), String>;

/// An export under way.
pub(crate) struct Running {
    cancel: Arc<AtomicBool>,
    progress: Arc<Mutex<Progress>>,
    answer: Receiver<Answer>,
    /// Where its files go, for the note when it is done.
    folder: PathBuf,
}

impl Running {
    pub(crate) fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub(crate) fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// Done of all, 0 to 1, and what it is on.
    pub(crate) fn progress(&self) -> (f32, String) {
        let Ok(p) = self.progress.lock() else { return (0.0, String::new()) };
        let fraction = if p.1 == 0 { 0.0 } else { p.0 as f32 / p.1 as f32 };
        (fraction.clamp(0.0, 1.0), p.2.clone())
    }
}

/// What a writer that panicked says.
fn stopped(panic: &(dyn std::any::Any + Send)) -> String {
    let why = panic.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| panic.downcast_ref::<String>().cloned());
    match why {
        Some(why) => format!("the export stopped ({why})"),
        None => "the export stopped".to_string(),
    }
}

impl BimApp {
    /// Whether an export is being written.
    pub(crate) fn exporting(&self) -> bool {
        self.export_job.is_some()
    }

    /// Starts writing `job` on a thread of its own, unless one is being written already. Whether it started.
    pub(crate) fn start_export(&mut self, job: Job) -> bool {
        self.start_export_after(job, None)
    }

    /// [`start_export`](Self::start_export), the thread waiting for `gate` first: a test's way to cancel it before it begins.
    pub(crate) fn start_export_after(&mut self, job: Job, gate: Option<Receiver<()>>) -> bool {
        self.spawn_export(job, gate, None)
    }

    /// Writes `job` on a thread of its own, then prints what it wrote as `print` says and removes it.
    pub(crate) fn start_print(&mut self, job: Job, print: PrintTo) -> bool {
        self.spawn_export(job, None, Some(print))
    }

    fn spawn_export(&mut self, job: Job, gate: Option<Receiver<()>>, print: Option<PrintTo>) -> bool {
        if self.export_job.is_some() {
            return false;
        }
        let model = Arc::clone(&self.session.model);
        let registry = self.session.exports.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(Mutex::new((0, job.items.len(), String::new())));
        let (send, answer) = channel();
        let folder = job.folder.clone();
        // The window looks again every tenth of a second while it runs (`poll_export`).
        let (stop, tell) = (Arc::clone(&cancel), Arc::clone(&progress));
        let spawned = std::thread::Builder::new().name("export".into()).spawn(move || {
            if let Some(gate) = gate {
                let _ = gate.recv();
            }
            let cancelled = || stop.load(Ordering::Relaxed);
            let progress = |done: usize, total: usize, what: &str| {
                if let Ok(mut p) = tell.lock() {
                    *p = (done, total, what.to_string());
                }
            };
            let ctx = Ctx { cancelled: &cancelled, progress: &progress };
            // The writers read a project that may be anyone's: whatever they do, the window gets an answer.
            let report = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| export::run(&model, &registry, &job, &ctx)))
                .unwrap_or_else(|p| Err(stopped(&*p)));
            let answer = report.map(|report| {
                let printed = print.map(|to| {
                    let sent = if report.cancelled {
                        Err("cancelled: nothing was sent to the printer".to_string())
                    } else if report.written.is_empty() {
                        Err("nothing was made to print".to_string())
                    } else {
                        report
                            .written
                            .iter()
                            .try_for_each(|w| to.spooler.print(&to.printer, &w.path, to.copies, to.media, &to.title))
                            .map(|()| to.printer.clone())
                    };
                    // The spooler has its own copy: the file was only for it.
                    let _ = std::fs::remove_dir_all(&job.folder);
                    sent
                });
                (report, printed)
            });
            let _ = send.send(answer);
        });
        if let Err(e) = spawned {
            self.status = Status { text: tf("Couldn't export: {}", &[&e.to_string()]), error: true };
            return false;
        }
        self.export_job = Some(Running { cancel, progress, answer, folder });
        true
    }

    /// Once a frame: what a finished export did, in the status box.
    pub(crate) fn poll_export(&mut self, ctx: &Context) {
        let Some(job) = &self.export_job else { return };
        let answer = match job.answer.try_recv() {
            Ok(answer) => answer,
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(100));
                return;
            }
            Err(TryRecvError::Disconnected) => Err("the export stopped without an answer".to_string()),
        };
        let Some(job) = self.export_job.take() else { return };
        self.status = match answer {
            Ok((report, None)) => said(&report, &job.folder),
            Ok((report, Some(Ok(printer)))) => {
                let mut status = said(&report, &job.folder);
                // What wasn't made stays said, under it.
                let failed = status.text.split_once('\n').map(|(_, rest)| format!("\n{rest}")).unwrap_or_default();
                status.text = tf(msg!("Sent to {}."), &[&printer]) + &failed;
                status
            }
            Ok((_, Some(Err(why)))) => Status { text: tf(msg!("Couldn't print: {}"), &[&why]), error: true },
            Err(e) => Status { text: tf("Couldn't export: {}", &[&e]), error: true },
        };
    }

    /// The export's bar and its Cancel, in the status bar while one is written.
    pub(crate) fn export_progress(&mut self, ui: &mut Ui) {
        let Some(job) = &self.export_job else { return };
        let mut cancel = false;
        if job.cancelled() {
            ui.label(egui::RichText::new(t("Cancelling…")).color(pal().muted));
        } else {
            cancel = ui.button(t("Cancel Export")).clicked();
        }
        let (fraction, what) = job.progress();
        let bar = ProgressBar::new(fraction).desired_width(200.0).fill(pal().accent).text(if what.is_empty() {
            t("Exporting…").to_string()
        } else {
            what
        });
        ui.add(bar);
        if cancel {
            job.cancel();
        }
    }
}

/// What a job did, as the status box says it.
fn said(report: &Report, folder: &Path) -> Status {
    let n = report.written.len();
    let files = match (n, report.written.first()) {
        (1, Some(w)) => w.path.file_name().map_or_else(|| w.path.display().to_string(), |f| f.to_string_lossy().into_owned()),
        _ => tf("{} files", &[&n.to_string()]),
    };
    let place = folder.display().to_string();
    let mut text = if report.cancelled {
        tf(msg!("Export cancelled: {} written to {} before it stopped."), &[&files, &place])
    } else {
        tf(msg!("Exported {} to {}."), &[&files, &place])
    };
    for f in report.failed.iter().take(5) {
        text.push_str(&format!("\n{}", tf(msg!("{} ({}): {}"), &[&f.of, &f.format, &f.error])));
    }
    if report.failed.len() > 5 {
        text.push_str(&format!("\n{}", tf(msg!("and {} more not written"), &[&(report.failed.len() - 5).to_string()])));
    }
    Status { text, error: !report.failed.is_empty() || (n == 0 && !report.cancelled) }
}
