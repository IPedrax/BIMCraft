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
use bimcraft_core::graphics::{ScanBy, ScanColour};
use bimcraft_core::progress::{CANCELLED, Progress};
use bimcraft_core::units::Units;
use bimcraft_i18n::msg;
use egui::{Align, Button, Context, Grid, Id as UiId, Layout, ProgressBar, RichText, TextEdit, Ui};
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
    /// Keep full detail: every point in an index beside the project's file (`point_cloud`'s `index`), the project keeping
    /// only the top of it. On at first for big files; off, and greyed out, while the project has no file to keep it beside.
    pub(crate) index: bool,
    /// Why it couldn't be brought in, shown in the box.
    error: Option<String>,
}

/// Scan files bigger than this together keep full detail at first, as `point_cloud`'s `index: "auto"` decides.
pub(crate) const INDEX_FROM: u64 = 64 << 20;

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
        let index = files_size(&paths) > INDEX_FROM;
        self.cloud_box =
            Some(CloudBox { paths, name, units: "m", spacing: String::new(), origin: "centre", acquire: false, index, error: None });
    }

    pub(crate) fn point_cloud_window(&mut self, ctx: &Context) {
        if self.cloud_reading.is_some() {
            return;
        }
        let saved = self.session.path.is_some();
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
                ui.add_space(6.0);
                ui.checkbox(&mut b.index, t("Keep full detail (index beside the project)")).on_hover_text(t(
                    "Every point goes in a file beside the project's, which views read as they come near; the project keeps a share of them",
                ));
                if !saved && b.index {
                    ui.label(RichText::new(t("The project has no file yet: the index is kept beside the scan's file.")).color(pal().muted));
                }
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

    /// Find Index… on a scan in the Project Browser: its index's file picked in the system's dialog, then
    /// `point_cloud_index` reads it there.
    pub(crate) fn find_cloud_index(&mut self, ctx: &Context, cloud: String) {
        let dialog = rfd::AsyncFileDialog::new().add_filter(t("Point cloud index"), &["bimpc"]);
        let dialog = match self.session.path.as_ref().and_then(|p| p.parent()) {
            Some(dir) => dialog.set_directory(dir),
            None => dialog,
        };
        self.ask_file(ctx, dialog, false, move |app, path| {
            app.run("point_cloud_index", json!({"cloud": cloud, "path": path.to_string_lossy()}));
        });
    }

    /// Delete Index… on a scan in the Project Browser: asked first, since its file goes for good.
    pub(crate) fn index_delete_box(&mut self, ctx: &Context) {
        let Some(cloud) = self.index_delete.clone() else { return };
        let (mut yes, mut no) = (false, false);
        let r = Modal::new(UiId::new("point-cloud-index-delete"), t("Delete Index")).width(420.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(6.0);
                ui.label(tf(
                    "Delete the index of {}? Its file is deleted for good; the scan keeps the points the project holds, and Rebuild Index can't bring it back.",
                    &[&cloud],
                ));
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    no = ui.button(t("Cancel")).clicked();
                    yes = ui.button(t("Delete")).clicked();
                });
            });
        });
        if yes {
            self.index_delete = None;
            self.run("point_cloud_index", json!({"cloud": cloud, "action": "delete"}));
        } else if no || r.should_close() {
            self.index_delete = None;
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
            "index": b.index,
        });
        // The user's folder of indexes: the read ahead runs on a session of its own, which doesn't have it.
        if let Some(dir) = &self.session.index_dir {
            p["index_dir"] = json!(dir.to_string_lossy());
        }
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
        // An index is built on a thread of its own whatever the size: it writes every point to its file.
        if files_size(&b.paths) >= IN_BACKGROUND || p["index"] == json!(true) {
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

/// The colour modes a view may draw point clouds in, as Revit's Color Mode names them.
pub(crate) const SCAN_MODES: [(ScanBy, &str); 3] =
    [(ScanBy::Normal, msg!("Normal")), (ScanBy::Elevation, msg!("Elevation")), (ScanBy::Intensity, msg!("Intensity"))];

/// The name of what `c` colours point clouds by.
pub(crate) fn scan_mode_name(c: Option<ScanColour>) -> &'static str {
    let by = c.map(|c| c.by).unwrap_or_default();
    SCAN_MODES.iter().find(|(b, _)| *b == by).map_or("", |(_, n)| n)
}

/// Point clouds' Color Mode as rows of a grid: what `scan` colours them by and, by elevation, the heights the ramp runs between
/// (blank: each scan's own foot and head), typed in `units`. What it's changed to, if it changed: none for their own colours.
pub(crate) fn scan_colour_rows(
    ui: &mut Ui,
    salt: &str,
    scan: Option<ScanColour>,
    units: Units,
    enabled: bool,
) -> Option<Option<ScanColour>> {
    let mut now = scan.unwrap_or_default();
    let before = now;
    ui.add_enabled_ui(enabled, |ui| {
        let l = ui.label(t("Color Mode"));
        let c = egui::ComboBox::from_id_salt((salt, "scan-mode")).selected_text(t(scan_mode_name(Some(now)))).show_ui(ui, |ui| {
            for (by, name) in SCAN_MODES {
                ui.selectable_value(&mut now.by, by, t(name));
            }
        });
        c.response.labelled_by(l.id);
    });
    ui.end_row();
    if now.by == ScanBy::Elevation {
        for (label, value) in [(msg!("Low"), &mut now.low), (msg!("High"), &mut now.high)] {
            let l = ui.label(t(label));
            let id = ui.id().with((salt, label));
            let mut text = ui.data_mut(|d| d.get_temp::<String>(id)).unwrap_or_else(|| value.map_or_else(String::new, |v| units.length(v)));
            let r = ui
                .add_enabled(enabled, TextEdit::singleline(&mut text).hint_text(t("the scan's own")).desired_width(140.0))
                .labelled_by(l.id);
            if r.changed() {
                // Blank is each scan's own; what isn't a length yet is kept as typed.
                if text.trim().is_empty() {
                    *value = None;
                } else if let Some(v) = units.parse(&text) {
                    *value = Some(v);
                }
            }
            if r.has_focus() {
                ui.data_mut(|d| d.insert_temp(id, text));
            } else {
                ui.data_mut(|d| d.remove::<String>(id));
            }
            ui.end_row();
        }
    }
    if now.by != ScanBy::Elevation {
        (now.low, now.high) = (None, None);
    }
    let after = (now != ScanColour::default()).then_some(now);
    (now != before || (scan.is_some() && after.is_none())).then_some(after).filter(|a| *a != scan)
}
