//! CAD drawings: Insert > Import CAD and Link CAD ask for a DXF file, then a box for how to bring it in (name, colours,
//! layers, units, positioning, current view only, as Revit's Import CAD Formats asks), and run `import_cad`.

use crate::popup::Modal;

use bimcraft_i18n::msg;

use std::path::PathBuf;

use egui::{Align, Button, Checkbox, ComboBox, Context, Grid, Id as UiId, Layout, RichText, TextEdit};
use serde_json::{Value, json};

use crate::BimApp;
use crate::lang::{t, tc};
use crate::style::pal;

/// The units a drawing may be in, as `import_cad` names them: auto reads its file's.
const UNITS: [&str; 6] = ["auto", "mm", "cm", "m", "in", "ft"];

/// How its colours come in, as `import_cad` names them, and their labels.
const COLORS: [(&str, &str); 3] = [("preserve", msg!("Preserve")), ("invert", "Invert"), ("black", msg!("Black and White"))];

/// Bringing a drawing in: what was picked and how, until OK or Cancel.
pub(crate) struct CadBox {
    path: PathBuf,
    /// Link CAD: the file is remembered and read again on Reload and on opening the project.
    link: bool,
    name: String,
    colors: &'static str,
    /// All its layers, or those its file has on (Visible).
    all_layers: bool,
    units: &'static str,
    /// Its origin on the project's (Origin to Origin), or its middle (Center to Center).
    by_origin: bool,
    /// In the plan on screen only, not in every plan of its level.
    this_view: bool,
    /// Why it couldn't be brought in, shown in the box.
    error: Option<String>,
}

impl CadBox {
    /// Whether it was opened by Link CAD.
    pub(crate) fn linked(&self) -> bool {
        self.link
    }
}

impl BimApp {
    /// Insert > Import CAD or Link CAD: the file the user picks, then the box for it.
    pub(crate) fn import_cad_dialog(&mut self, ctx: &Context, link: bool) {
        let dialog = rfd::AsyncFileDialog::new().add_filter(t("CAD drawings (DWG, DXF)"), &["dxf", "dwg"]);
        self.ask_files(ctx, dialog, move |app, paths| {
            if let Some(path) = paths.into_iter().next() {
                app.import_cad_path(path, link);
            }
        });
    }

    /// Opens the box for bringing in the drawing at `path`, named for its file.
    pub(crate) fn import_cad_path(&mut self, path: PathBuf, link: bool) {
        let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        self.cad_box = Some(CadBox {
            path,
            link,
            name,
            colors: "preserve",
            all_layers: false,
            units: "auto",
            by_origin: false,
            this_view: false,
            error: None,
        });
    }

    pub(crate) fn import_cad_window(&mut self, ctx: &Context) {
        let plan = matches!(self.sheet, crate::Sheet::Plan(_));
        let Some(b) = self.cad_box.as_mut() else { return };
        let (mut ok, mut close) = (false, false);
        let r = Modal::new(UiId::new("import-cad"), t(if b.link { "Link CAD" } else { "Import CAD" })).width(440.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(6.0);
                ui.label(b.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
                ui.add_space(6.0);
                Grid::new("import-cad-grid").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    let l = ui.label(t("Name"));
                    ui.add_sized([220.0, 20.0], TextEdit::singleline(&mut b.name)).labelled_by(l.id);
                    ui.end_row();
                    let l = ui.label(t("Colors"));
                    let now = COLORS.iter().find(|(k, _)| *k == b.colors).map_or("Preserve", |(_, n)| n);
                    let c = ComboBox::from_id_salt("import-cad-colors").selected_text(t(now)).show_ui(ui, |ui| {
                        for (k, n) in COLORS {
                            ui.selectable_value(&mut b.colors, k, t(n));
                        }
                    });
                    c.response.labelled_by(l.id);
                    ui.end_row();
                    let l = ui.label(tc("cad", "Layers"));
                    let now = if b.all_layers { "All" } else { "Visible" };
                    let c = ComboBox::from_id_salt("import-cad-layers").selected_text(t(now)).show_ui(ui, |ui| {
                        ui.selectable_value(&mut b.all_layers, false, t("Visible"));
                        ui.selectable_value(&mut b.all_layers, true, t("All"));
                    });
                    c.response.labelled_by(l.id);
                    ui.end_row();
                    let l = ui.label(t("Import units"));
                    let shown = |u: &str| if u == "auto" { t("Auto-Detect").to_string() } else { u.to_string() };
                    let c = ComboBox::from_id_salt("import-cad-units").selected_text(shown(b.units)).show_ui(ui, |ui| {
                        for u in UNITS {
                            ui.selectable_value(&mut b.units, u, shown(u));
                        }
                    });
                    c.response.labelled_by(l.id);
                    ui.end_row();
                });
                ui.add_space(6.0);
                ui.label(t("Positioning"));
                ui.radio_value(&mut b.by_origin, false, t("Auto - Center to Center"))
                    .on_hover_text(t("Its middle on the project's origin"));
                ui.radio_value(&mut b.by_origin, true, t("Auto - Origin to Origin"))
                    .on_hover_text(t("Its origin on the project's origin, where its file's coordinates put it"));
                ui.add_space(6.0);
                ui.add_enabled(plan, Checkbox::new(&mut b.this_view, t("Current view only")))
                    .on_hover_text(t("In this plan only, not in every plan of its level"));
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
            self.bring_in_cad();
        } else if close || r.should_close() {
            self.cad_box = None;
        }
    }

    /// Runs `import_cad` for the box's file: on the plan on screen's level (or in that plan only), and selected. A refusal
    /// keeps the box open with the reason in it.
    fn bring_in_cad(&mut self) {
        let Some(b) = self.cad_box.as_ref() else { return };
        let mut p = json!({
            "path": b.path.to_string_lossy(),
            "name": b.name.trim(),
            "colors": b.colors,
            "layers": if b.all_layers { "all" } else { "visible" },
            "units": b.units,
            "position": if b.by_origin { "origin" } else { "center" },
        });
        if b.link {
            p["link"] = json!(true);
        }
        if let crate::Sheet::Plan(key) | crate::Sheet::Ceiling(key) = self.sheet {
            if b.this_view && matches!(self.sheet, crate::Sheet::Plan(_)) {
                p["view"] = json!(key);
            } else if let Some(level) = self.session.model.plan_level(key) {
                p["level"] = json!(level);
            }
        }
        match self.run("import_cad", p) {
            Some(v) => {
                self.cad_box = None;
                self.selection = v["id"].as_u64().into_iter().collect();
                // What the drawing's read left out, and entities far from the rest, on the status bar.
                let said: Vec<&str> =
                    v["notes"].as_array().into_iter().flatten().chain([&v["warning"]]).filter_map(Value::as_str).collect();
                if !said.is_empty() {
                    self.status = crate::Status { text: said.join("; "), error: false };
                }
            }
            // Said in the box, not on the status bar under it.
            None => {
                let e = std::mem::take(&mut self.status).text;
                if let Some(b) = self.cad_box.as_mut() {
                    b.error = Some(e);
                }
            }
        }
    }
}
