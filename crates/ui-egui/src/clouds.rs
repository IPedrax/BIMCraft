//! Point clouds: Insert > Point Cloud asks for scan files, then a box for how to bring them in (name, units, spacing,
//! positioning, as Revit's Link Point Cloud asks), and runs `point_cloud`. Big files are read first on a thread of their own
//! (`Session::read_scan_ahead`), the window drawing on with a box of how far the read has got and Cancel; `point_cloud` then
//! places what was read, one undo step on the window's session.

use crate::popup::Modal;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

use bimcraft_core::cloud::ScanRead;
use bimcraft_core::engine::Session;
use bimcraft_core::progress::{CANCELLED, Progress};
use bimcraft_i18n::msg;
use egui::{Align, Button, Context, Grid, Id as UiId, Layout, ProgressBar, RichText, TextEdit};
use serde_json::{Value, json};

use crate::BimApp;
use crate::lang::{t, tf};
use crate::properties::number;
use crate::style::pal;

/// Scan files at least this big together are read on a thread of their own, with the box: a smaller one takes less than the
/// box would be shown for (and the tests, which bring small files in, see them come in at once).
pub(crate) const IN_BACKGROUND: u64 = if cfg!(test) { 256 << 10 } else { 16 << 20 };

/// Scan files being read on a thread of their own, for `point_cloud` with `params` to place.
pub(crate) struct CloudReading {
    params: Value,
    file: String,
    progress: Arc<Progress>,
    answer: Receiver<Result<ScanRead, String>>,
    since: Instant,
}

/// The units a scan's coordinates may be in, as `point_cloud` names them.
const UNITS: [&str; 5] = ["m", "mm", "cm", "in", "ft"];

/// Bringing scan files in: what was picked and how, until OK or Cancel.
pub(crate) struct CloudBox {
    paths: Vec<PathBuf>,
    name: String,
    units: &'static str,
    /// One point kept in each cube this wide, mm, as typed; blank keeps them all.
    spacing: String,
    /// Where its file's coordinates put it (Origin to Origin), or its middle on the origin (Center to Center).
    /// Positioning: `centre`, `file` (origin to origin) or `shared` (the files' coordinates are the map's).
    origin: &'static str,
    /// By shared coordinates: the active location taken from the scan instead.
    acquire: bool,
    /// Why it couldn't be brought in, shown in the box.
    error: Option<String>,
}

impl BimApp {
    /// Insert > Point Cloud: the files the user picks, then the box for them.
    pub(crate) fn point_cloud_dialog(&mut self, ctx: &Context) {
        let formats = ["pts", "xyz", "txt", "ply", "las", "laz", "e57", "rcp", "rcs"];
        let dialog = rfd::AsyncFileDialog::new().add_filter(t("Point clouds"), &formats);
        self.ask_files(ctx, dialog, |app, paths| app.point_cloud_paths(paths));
    }

    /// Opens the box for bringing in the scan files `paths`, named for the first.
    pub(crate) fn point_cloud_paths(&mut self, paths: Vec<PathBuf>) {
        let name = paths.first().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        self.cloud_box = Some(CloudBox { paths, name, units: "m", spacing: String::new(), origin: "centre", acquire: false, error: None });
    }

    pub(crate) fn point_cloud_window(&mut self, ctx: &Context) {
        if self.cloud_reading.is_some() {
            return;
        }
        let Some(b) = self.cloud_box.as_mut() else { return };
        let (mut ok, mut close) = (false, false);
        let r = Modal::new(UiId::new("point-cloud"), t("Point Cloud")).width(440.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(6.0);
                let what = match b.paths.as_slice() {
                    [one] => one.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                    all => tf("{} files, brought in as one scan's stations", &[&all.len().to_string()]),
                };
                ui.label(what);
                ui.add_space(6.0);
                Grid::new("point-cloud-grid").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    let l = ui.label(t("Name"));
                    ui.add_sized([220.0, 20.0], TextEdit::singleline(&mut b.name)).labelled_by(l.id);
                    ui.end_row();
                    let l = ui.label(t("Units"));
                    let c = egui::ComboBox::from_id_salt("point-cloud-units").selected_text(b.units).show_ui(ui, |ui| {
                        for u in UNITS {
                            ui.selectable_value(&mut b.units, u, u);
                        }
                    });
                    c.response.labelled_by(l.id);
                    ui.end_row();
                    let l = ui.label(t("Spacing (mm)"));
                    ui.add_sized([220.0, 20.0], TextEdit::singleline(&mut b.spacing).hint_text(t("all the points"))).labelled_by(l.id);
                    ui.end_row();
                });
                ui.add_space(6.0);
                ui.label(t("Positioning"));
                ui.radio_value(&mut b.origin, "centre", t("Auto - Center to Center"))
                    .on_hover_text(t("Its middle on the project's origin and its foot on the level"));
                ui.radio_value(&mut b.origin, "file", t("Auto - Origin to Origin"))
                    .on_hover_text(t("Where its file's coordinates put it, as the survey placed it"));
                ui.radio_value(&mut b.origin, "shared", t("Auto - By Shared Coordinates")).on_hover_text(t(
                    "Its coordinates are the site's (a survey's): it goes where the active location puts them, turned to match",
                ));
                ui.add_enabled(b.origin == "shared", egui::Checkbox::new(&mut b.acquire, t("Acquire coordinates from it")))
                    .on_hover_text(t("The active location is set from the scan's instead, so it lands on the project's origin"));
                if let Some(e) = &b.error {
                    ui.add_space(6.0);
                    ui.label(RichText::new(e).color(pal().error));
                }
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    close = ui.button(t("Cancel")).clicked();
                    ok = ui.add_enabled(!b.name.trim().is_empty(), Button::new(t("OK"))).clicked();
                });
            });
        });
        if ok {
            self.bring_in_cloud();
        } else if close || r.should_close() {
            self.cloud_box = None;
        }
    }

    /// Runs `point_cloud` for the box's files: placed on the plan on screen's level, and selected. A refusal keeps the box
    /// open with the reason in it.
    fn bring_in_cloud(&mut self) {
        let Some(b) = self.cloud_box.as_ref() else { return };
        let mut p = json!({
            "path": b.paths.iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>(),
            "name": b.name.trim(),
            "units": b.units,
            "origin": b.origin,
        });
        if b.origin == "shared" && b.acquire {
            p["acquire"] = json!(true);
        }
        if !b.spacing.trim().is_empty() {
            match number(&b.spacing) {
                Some(s) => p["spacing"] = json!(s),
                None => {
                    let e = tf("`{}` is not a number", &[b.spacing.trim()]);
                    if let Some(b) = self.cloud_box.as_mut() {
                        b.error = Some(e);
                    }
                    return;
                }
            }
        }
        if let crate::Sheet::Plan(key) | crate::Sheet::Ceiling(key) = self.sheet
            && let Some(level) = self.session.model.plan_level(key)
        {
            p["level"] = json!(level);
        }
        if files_size(&b.paths) >= IN_BACKGROUND {
            self.read_cloud_in_background(p);
            return;
        }
        self.place_cloud(p);
    }

    /// Runs `point_cloud` with `p`: what it placed selected and the box closed, or the box keeping the reason it couldn't.
    fn place_cloud(&mut self, p: Value) {
        match self.run("point_cloud", p) {
            Some(v) => {
                self.cloud_box = None;
                self.selection = v["id"].as_u64().into_iter().collect();
            }
            // Said in the box, not on the status bar under it.
            None => {
                let e = std::mem::take(&mut self.status).text;
                if let Some(b) = self.cloud_box.as_mut() {
                    b.error = Some(e);
                }
            }
        }
    }

    /// Starts reading the box's files on a thread of their own, for `point_cloud` with `params`.
    fn read_cloud_in_background(&mut self, params: Value) {
        let file = self.cloud_box.as_ref().and_then(|b| b.paths.first()).and_then(|p| p.file_name());
        let file = file.map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let progress = Arc::new(Progress::new());
        let (send, answer) = channel();
        let (model, path, watched, asked) =
            (Arc::clone(&self.session.model), self.session.path.clone(), Arc::clone(&progress), params.clone());
        let spawned = std::thread::Builder::new().name("point cloud".into()).spawn(move || {
            // The engine runs the command under `catch_unwind`, on a session of its own that's dropped after.
            let _ = send.send(Session::read_scan_ahead(model, path, &asked, Some(watched)));
        });
        match spawned {
            Ok(_) => self.cloud_reading = Some(CloudReading { params, file, progress, answer, since: Instant::now() }),
            Err(e) => {
                if let Some(b) = self.cloud_box.as_mut() {
                    b.error = Some(e.to_string());
                }
            }
        }
    }

    /// Takes the read's answer when it has come: `point_cloud` places what was read, or the box says why it couldn't be.
    pub(crate) fn poll_cloud_reading(&mut self, ctx: &Context) {
        let Some(job) = &self.cloud_reading else { return };
        let answer = match job.answer.try_recv() {
            Ok(answer) => answer,
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(50));
                return;
            }
            Err(TryRecvError::Disconnected) => Err("the read stopped without an answer".to_string()),
        };
        let Some(CloudReading { params, file, .. }) = self.cloud_reading.take() else { return };
        match answer {
            Ok(read) => {
                self.session.give_scan(read);
                self.place_cloud(params);
            }
            Err(e) => {
                let e = if e.ends_with(CANCELLED) { tf("Reading {} was cancelled.", &[&file]) } else { e };
                if let Some(b) = self.cloud_box.as_mut() {
                    b.error = Some(e);
                }
            }
        }
    }

    /// The box over the window while scan files are read: the stage, a bar for what is counted, and Cancel.
    pub(crate) fn cloud_reading_box(&mut self, ctx: &Context) {
        let Some(job) = &self.cloud_reading else { return };
        let (stage, done) = job.progress.now();
        let cancelled = job.progress.cancelled();
        let seconds = job.since.elapsed().as_secs();
        let mut cancel = false;
        Modal::new(UiId::new("point-cloud-reading"), t("Point Cloud")).width(520.0).closable(false).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.label(RichText::new(tf("Reading {}", &[&job.file])).strong());
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
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    if ui.add_enabled(!cancelled, Button::new(t("Cancel"))).clicked() {
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

/// How big files `paths` are together, bytes.
fn files_size(paths: &[PathBuf]) -> u64 {
    paths.iter().filter_map(|f| std::fs::metadata(f).ok()).map(|m| m.len()).sum()
}
