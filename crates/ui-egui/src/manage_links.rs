//! Manage Links, as Revit's dialog: a tab for each kind of file the project reads from elsewhere, a row for each file with its
//! status, how many instances place it, its saved path and whether that's relative, and Reload, Reload From… and Unload.
//! What it lists is the core's `links`; what its buttons do, `reload`, `reload_from`, `unload`, `reload_cad` and `reload_mesh`.

use bimcraft_i18n::msg;
use egui::{Align, Button, Context, Id as UiId, Layout, RichText, ScrollArea};
use serde_json::{Value, json};

use crate::BimApp;
use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::style::pal;

/// The tabs: the kind `links` names each, and what the dialog calls it.
pub(crate) const TABS: [(&str, &str); 7] = [
    ("projects", msg!("BIMCraft")),
    ("ifc", "IFC"),
    ("cad", msg!("CAD Formats")),
    ("pdf", msg!("PDFs")),
    ("images", msg!("Images")),
    ("clouds", msg!("Point Clouds")),
    ("models", msg!("3D Models")),
];

/// How tall a tab's list is, px: the window is of one size, its list scrolling past this.
const LIST: f32 = 320.0;

/// The Manage Links window: the tab shown, what `links` said, and the row picked in it.
pub(crate) struct ManageLinks {
    pub(crate) tab: usize,
    pub(crate) listed: Value,
    pub(crate) picked: Option<usize>,
}

impl ManageLinks {
    /// The rows of the tab shown.
    pub(crate) fn rows(&self) -> Vec<Value> {
        self.listed[TABS[self.tab].0].as_array().cloned().unwrap_or_default()
    }

    /// The row picked, if it's still there.
    fn row(&self) -> Option<Value> {
        self.picked.and_then(|i| self.rows().get(i).cloned())
    }
}

/// What a row's file can be: read again, read from another file, unloaded.
fn can(kind: &str, row: &Value) -> (bool, bool, bool) {
    match kind {
        "projects" | "ifc" => (true, true, row["status"] != "unloaded"),
        "cad" => (row["path"].is_string(), true, false),
        "models" => (true, true, false),
        "images" | "pdf" => (row["path"].is_string(), true, false),
        _ => (false, false, false),
    }
}

/// What a status is called in the window.
fn status_label(status: &str) -> String {
    match status {
        "loaded" => t("Loaded").to_string(),
        "not loaded" => t("Not Loaded").to_string(),
        "unloaded" => t("Unloaded").to_string(),
        "not found" => t("Not Found").to_string(),
        "imported" => t("Imported").to_string(),
        other => other.to_string(),
    }
}

impl BimApp {
    /// Opens Manage Links on its first tab.
    pub(crate) fn open_manage_links(&mut self) {
        let mut d = ManageLinks { tab: 0, listed: Value::Null, picked: None };
        self.relist_links(&mut d);
        self.manage_links = Some(d);
    }

    /// What `links` says now.
    fn relist_links(&mut self, d: &mut ManageLinks) {
        d.listed = self.exec("links", &json!({})).unwrap_or_default();
        if d.picked.is_some_and(|i| i >= d.rows().len()) {
            d.picked = None;
        }
    }

    /// Reload of row `row` of tab `kind`, or with `from` Reload From that file: whether it was done.
    pub(crate) fn reload_link(&mut self, kind: &str, row: &Value, from: Option<&std::path::Path>) -> bool {
        let first = row["ids"][0].clone();
        let from = from.map(|p| p.to_string_lossy().into_owned());
        let (cmd, p) = match (kind, from) {
            ("projects" | "ifc", None) => ("reload", json!({"id": first})),
            ("projects" | "ifc", Some(path)) => ("reload_from", json!({"id": first, "path": path})),
            ("cad", None) => ("reload_cad", json!({"cad": row["name"]})),
            ("cad", Some(path)) => ("reload_cad", json!({"cad": row["name"], "path": path})),
            ("models", None) => ("reload_mesh", json!({"path": row["path"]})),
            ("models", Some(path)) => ("reload_mesh", json!({"path": row["path"], "from": path})),
            ("images" | "pdf", None) => ("reload_image", json!({"id": first})),
            ("images" | "pdf", Some(path)) => ("reload_image", json!({"id": first, "path": path})),
            _ => return false,
        };
        let Some(v) = self.run(cmd, p) else { return false };
        // `reload` says what it couldn't read rather than failing.
        if let Some(e) = v["failed"].as_array().and_then(|f| f.first()) {
            self.status = crate::Status { text: e["error"].as_str().unwrap_or_default().to_string(), error: true };
            return false;
        }
        self.say_missing_fonts(&v);
        true
    }

    /// The window, while it's open.
    pub(crate) fn manage_links_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.manage_links.take() else { return };
        let (mut close, mut reload, mut from, mut unload, mut add) = (false, false, false, false, false);
        // Of one size whatever its tab lists, so changing tabs never moves it.
        let r = Modal::new(UiId::new("manage-links"), t("Manage Links")).width(760.0).min_height(LIST + 120.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (i, (kind, label)) in TABS.iter().enumerate() {
                        let n = d.listed[*kind].as_array().map_or(0, Vec::len);
                        let text = if n > 0 { format!("{} ({n})", t(label)) } else { t(label).to_string() };
                        if ui.selectable_label(d.tab == i, text).clicked() && d.tab != i {
                            d.tab = i;
                            d.picked = None;
                        }
                    }
                });
                ui.separator();
                let rows = d.rows();
                ScrollArea::vertical().min_scrolled_height(LIST).max_height(LIST).auto_shrink([false, false]).show(ui, |ui| {
                    // A long name or path is cut short, its whole in its tip, so the window keeps its width.
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                    if rows.is_empty() {
                        ui.label(RichText::new(t("Nothing of this kind is linked or imported.")).color(pal().muted));
                        return;
                    }
                    egui::Grid::new("manage-links-rows").striped(true).num_columns(5).spacing([16.0, 4.0]).max_col_width(300.0).show(
                        ui,
                        |ui| {
                            for head in [msg!("Name"), msg!("Status"), msg!("Instances"), msg!("Saved Path"), msg!("Path Type")] {
                                ui.label(RichText::new(t(head)).strong());
                            }
                            ui.end_row();
                            for (i, row) in rows.iter().enumerate() {
                                let name = row["name"].as_str().unwrap_or_default();
                                if ui.selectable_label(d.picked == Some(i), name).clicked() {
                                    d.picked = Some(i);
                                }
                                let status = row["status"].as_str().unwrap_or_default();
                                let colour = if status == "not found" { pal().error } else { pal().text };
                                ui.label(RichText::new(status_label(status)).color(colour));
                                ui.label(row["instances"].as_u64().unwrap_or(0).to_string());
                                let path = row["path"].as_str().or(row["source"].as_str()).unwrap_or_default();
                                let shown = ui.label(path);
                                if let Some(full) = row["resolved"].as_str() {
                                    shown.on_hover_text(full);
                                }
                                let kind = match row["path_type"].as_str() {
                                    Some("relative") => t("Relative").to_string(),
                                    Some(_) => t("Absolute").to_string(),
                                    None => String::new(),
                                };
                                ui.label(kind);
                                ui.end_row();
                            }
                        },
                    );
                });
            });
            crate::popup::footer(ui, |ui| {
                let kind = TABS[d.tab].0;
                let (reloads, froms, unloads) = d.row().map_or((false, false, false), |row| can(kind, &row));
                ui.horizontal(|ui| {
                    reload = ui.add_enabled(reloads, Button::new(t("Reload"))).clicked();
                    from = ui.add_enabled(froms, Button::new(t("Reload From…"))).clicked();
                    unload = ui.add_enabled(unloads, Button::new(t("Unload"))).clicked();
                    let adds = matches!(kind, "projects" | "ifc" | "cad" | "clouds" | "models" | "images" | "pdf");
                    add = ui.add_enabled(adds, Button::new(t("Add…"))).clicked();
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        close = ui.button(t("Close")).clicked();
                    });
                });
            });
        });
        let kind = TABS[d.tab].0;
        if let Some(row) = d.row() {
            if reload && self.reload_link(kind, &row, None) {
                self.status = crate::Status { text: tf("Reloaded {}.", &[row["name"].as_str().unwrap_or_default()]), error: false };
            }
            if unload {
                self.run("unload", json!({"id": row["ids"][0]}));
            }
            if from {
                self.reload_from_dialog(ctx, kind, row);
            }
        }
        if add {
            match kind {
                "projects" | "ifc" => self.link_dialog = true,
                "cad" => self.cad_dialog = Some(true),
                "clouds" => self.cloud_dialog = true,
                "images" => self.image_dialog(ctx, true, false),
                "pdf" => self.image_dialog(ctx, true, true),
                _ => self.import_mesh_dialog(ctx),
            }
            close = true;
        }
        if reload || unload {
            self.relist_links(&mut d);
        }
        if !(close || r.should_close()) {
            self.manage_links = Some(d);
        }
    }

    /// Reload From…: the file the user picks, read in place of row `row`'s.
    fn reload_from_dialog(&mut self, ctx: &Context, kind: &'static str, row: Value) {
        let extensions: Vec<&str> = match kind {
            "cad" => vec!["dwg", "dxf"],
            "models" => vec!["obj", "gltf", "glb"],
            "images" => vec!["png", "jpg", "jpeg"],
            "pdf" => vec!["pdf"],
            _ => crate::links::link_extensions(),
        };
        let dialog = rfd::AsyncFileDialog::new().add_filter(t("Files of this kind"), &extensions);
        self.ask_file(ctx, dialog, false, move |app, path| {
            app.reload_link(kind, &row, Some(&path));
            if let Some(mut d) = app.manage_links.take() {
                app.relist_links(&mut d);
                app.manage_links = Some(d);
            }
        });
    }
}
