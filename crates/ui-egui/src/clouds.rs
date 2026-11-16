//! Point clouds: Insert > Point Cloud asks for scan files, then a box for how to bring them in (name, units, spacing,
//! positioning, as Revit's Link Point Cloud asks), and runs `point_cloud`.

use crate::popup::Modal;

use std::path::PathBuf;

use egui::{Align, Button, Context, Grid, Id as UiId, Layout, RichText, TextEdit};
use serde_json::json;

use crate::BimApp;
use crate::lang::{t, tf};
use crate::properties::number;
use crate::style::pal;

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
}
