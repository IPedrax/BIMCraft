//! Manage > Asset Manager: what the project draws with, a tab for each kind (materials, fill types, fill patterns, line
//! patterns, line styles and pictures), each asset with where it's used, and Import… and Export… of a `.bima` file. What it
//! lists is the core's `assets`; its buttons run `import_assets` and `export_assets`.

use std::collections::BTreeSet;

use bimcraft_i18n::msg;
use egui::{Align, Button, Color32, Context, Id as UiId, Layout, RichText, ScrollArea, Sense, vec2};
use serde_json::{Value, json};

use crate::BimApp;
use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::style::pal;

/// The tabs: the kind `assets` names each, and what the window calls it.
pub(crate) const TABS: [(&str, &str); 6] = [
    ("materials", msg!("Materials")),
    ("fill_types", msg!("Fill Types")),
    ("fill_patterns", msg!("Fill Patterns")),
    ("line_patterns", msg!("Line Patterns")),
    ("line_styles", msg!("Line Styles")),
    ("pictures", msg!("Pictures")),
];

/// How tall the list of a tab is, px: the window is of one size, its list scrolling past this.
const LIST: f32 = 340.0;

/// What an import does with a name the project has, as `import_assets` names it, and what the window calls it.
pub(crate) const EXISTING: [(&str, &str); 3] =
    [("skip", msg!("Keep the project's")), ("replace", msg!("Replace with the file's")), ("rename", msg!("Bring in renamed"))];

/// The Asset Manager: the tab shown, what `assets` said, the asset picked, those checked to export, and what an import does
/// with a name the project has.
pub(crate) struct AssetManager {
    pub(crate) tab: usize,
    pub(crate) listed: Value,
    pub(crate) picked: Option<String>,
    pub(crate) checked: BTreeSet<(String, String)>,
    pub(crate) existing: usize,
    /// Why the last Delete was refused: said inside the window, where it was asked.
    pub(crate) error: Option<String>,
}

impl AssetManager {
    /// The rows of the tab shown.
    pub(crate) fn rows(&self) -> Vec<Value> {
        self.listed[TABS[self.tab].0].as_array().cloned().unwrap_or_default()
    }

    /// What `export_assets` takes for the checked assets: by kind, their names.
    pub(crate) fn chosen(&self) -> Value {
        let mut p = json!({});
        for (kind, name) in &self.checked {
            match p[kind.as_str()].as_array_mut() {
                Some(names) => names.push(json!(name)),
                None => p[kind.as_str()] = json!([name]),
            }
        }
        p
    }
}

/// How a use reads: `Wall 100234`, `wall type Generic 200, layer 1`, `material Oak`.
pub(crate) fn use_label(u: &Value) -> String {
    let s = |k: &str| u[k].as_str().unwrap_or_default().to_string();
    match u["kind"].as_str().unwrap_or_default() {
        "element" => format!("{} {}", t(&s("category")), u["id"]),
        "type" => {
            let what = s("what");
            if what.is_empty() {
                tf("{} type {}", &[&s("category"), &s("name")])
            } else {
                tf("{} type {}, {}", &[&s("category"), &s("name"), &what])
            }
        }
        "family" => tf("family {}", &[&s("name")]),
        "material" => tf("material {}", &[&s("name")]),
        "fill_type" => tf("fill type {}", &[&s("name")]),
        "line_style" => tf("line style {}", &[&s("name")]),
        "grid_type" => tf("grid type {}", &[&s("name")]),
        "view_template" => tf("view template {}", &[&s("name")]),
        "group" => tf("group {}", &[&s("name")]),
        "phase_override" => tf("phase override {}", &[&s("name")]),
        other => other.to_string(),
    }
}

impl BimApp {
    pub(crate) fn open_asset_manager(&mut self) {
        let mut d = AssetManager { tab: 0, listed: Value::Null, picked: None, checked: BTreeSet::new(), existing: 0, error: None };
        self.relist_assets(&mut d);
        self.asset_manager = Some(d);
    }

    fn relist_assets(&mut self, d: &mut AssetManager) {
        d.listed = self.exec("assets", &json!({})).unwrap_or_default();
    }

    /// The assets `chosen` chooses (none, all of them) written to the .bima at `path`: whether it was.
    pub(crate) fn export_assets_to(&mut self, path: &std::path::Path, chosen: Value) -> bool {
        let mut p = chosen;
        p["path"] = json!(path.to_string_lossy());
        let Some(v) = self.run("export_assets", p) else { return false };
        let n: u64 = TABS.iter().map(|(k, _)| v[*k].as_u64().unwrap_or(0)).sum();
        self.status = crate::Status { text: tf("Exported {} assets.", &[&n.to_string()]), error: false };
        true
    }

    /// The assets of the .bima at `path` brought in, a name the project has as `existing` says: whether they were.
    pub(crate) fn import_assets_from(&mut self, path: &std::path::Path, existing: &str) -> bool {
        let Some(v) = self.run("import_assets", json!({"path": path.to_string_lossy(), "existing": existing})) else { return false };
        let count = |k: &str| v[k].as_array().map_or(0, Vec::len);
        let (added, replaced, renamed, skipped) = (count("added"), count("replaced"), count("renamed"), count("skipped"));
        let mut text = tf(
            "Imported {} assets, replaced {}, renamed {}, kept {} of the project's.",
            &[&added.to_string(), &replaced.to_string(), &renamed.to_string(), &skipped.to_string()],
        );
        if count("left_out") > 0 {
            text += " ";
            text += &tf("{} left out: this BIMCraft can't read them.", &[&count("left_out").to_string()]);
        }
        self.status = crate::Status { text, error: false };
        if let Some(mut d) = self.asset_manager.take() {
            self.relist_assets(&mut d);
            self.asset_manager = Some(d);
        }
        true
    }

    /// The assets `chosen` names deleted, the window's list read again; a refusal said in the window.
    pub(crate) fn delete_assets(&mut self, chosen: Value, d: &mut AssetManager) -> Value {
        match self.exec("delete_assets", &chosen) {
            Ok(v) => {
                d.error = None;
                d.checked.clear();
                d.picked = None;
                self.relist_assets(d);
                v
            }
            Err(e) => {
                d.error = Some(e.trim_start_matches("delete_assets: ").to_string());
                Value::Null
            }
        }
    }

    pub(crate) fn asset_manager_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.asset_manager.take() else { return };
        let (mut close, mut import, mut export_some, mut export_all, mut delete) = (false, false, false, false, false);
        // Of one size whatever it lists or shows of the asset picked, so nothing it shows moves it on the screen.
        let r = Modal::new(UiId::new("asset-manager"), t("Asset Manager")).width(760.0).min_height(LIST + 220.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (i, (kind, label)) in TABS.iter().enumerate() {
                        let n = d.listed[*kind].as_array().map_or(0, Vec::len);
                        if ui.selectable_label(d.tab == i, format!("{} ({n})", t(label))).clicked() && d.tab != i {
                            d.tab = i;
                            d.picked = None;
                        }
                    }
                });
                ui.separator();
                let kind = TABS[d.tab].0;
                let rows = d.rows();
                ui.columns(2, |cols| {
                    ScrollArea::vertical()
                        .id_salt("asset-rows")
                        .min_scrolled_height(LIST)
                        .max_height(LIST)
                        .auto_shrink([false, false])
                        .show(&mut cols[0], |ui| {
                            if rows.is_empty() {
                                ui.label(RichText::new(t("The project has none.")).color(pal().muted));
                            }
                            // A long name is cut short, its whole in its tip, so the window keeps its width.
                            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                            for row in &rows {
                                let name = row["name"].as_str().unwrap_or_default().to_string();
                                ui.horizontal(|ui| {
                                    let key = (kind.to_string(), name.clone());
                                    let mut on = d.checked.contains(&key);
                                    if ui.checkbox(&mut on, "").on_hover_text(t("Check it to export or delete it")).changed() {
                                        if on {
                                            d.checked.insert(key);
                                        } else {
                                            d.checked.remove(&key);
                                        }
                                    }
                                    swatch(ui, &self.session.model, kind, &name);
                                    let shown = row["label"].as_str().map_or_else(|| name.clone(), |l| format!("{l} ({name})"));
                                    let uses = row["uses"].as_u64().unwrap_or(0);
                                    let text = if uses == 0 { t("unused").to_string() } else { tf("used {}×", &[&uses.to_string()]) };
                                    // The name, cut short to leave its uses room beside it.
                                    let room = (ui.available_width() - 72.0).max(40.0);
                                    ui.scope(|ui| {
                                        ui.set_max_width(room);
                                        let picked = d.picked.as_deref() == Some(name.as_str());
                                        let r = ui.selectable_label(picked, &shown);
                                        // Its whole name in a tip where the row cuts it short.
                                        let r = if shown.chars().count() > 40 { r.on_hover_text(&shown) } else { r };
                                        if r.clicked() {
                                            d.picked = Some(name.clone());
                                        }
                                    });
                                    ui.label(RichText::new(text).color(pal().muted));
                                });
                            }
                        });
                    let ui = &mut cols[1];
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                    let picked = d.picked.as_ref().and_then(|p| rows.iter().find(|r| r["name"] == p.as_str()));
                    match picked {
                        None => {
                            ui.label(RichText::new(t("Pick an asset to see where it's used.")).color(pal().muted));
                        }
                        Some(row) => {
                            ui.label(RichText::new(row["label"].as_str().or(row["name"].as_str()).unwrap_or_default()).strong());
                            if let Some([w, h]) = row["size"].as_array().map(|s| [s[0].as_u64().unwrap_or(0), s[1].as_u64().unwrap_or(0)]) {
                                ui.label(tf("{} × {} px", &[&w.to_string(), &h.to_string()]));
                            }
                            if let Some(g) = row["guid"].as_str() {
                                ui.label(RichText::new(format!("GUID {g}")).color(pal().muted).small());
                            }
                            ui.add_space(6.0);
                            ui.label(t("Used by"));
                            let by = row["used_by"].as_array().cloned().unwrap_or_default();
                            ScrollArea::vertical().id_salt("asset-uses").max_height(LIST - 100.0).auto_shrink([false, true]).show(
                                ui,
                                |ui| {
                                    if by.is_empty() {
                                        ui.label(RichText::new(t("Nothing in the project uses it.")).color(pal().muted));
                                    }
                                    for u in &by {
                                        ui.label(use_label(u));
                                    }
                                },
                            );
                        }
                    }
                });
                if let Some(e) = &d.error {
                    ui.add(egui::Label::new(RichText::new(e).color(pal().error)).wrap());
                }
                ui.add_space(6.0);
                ui.separator();
                import = ui.button(t("Import…")).clicked();
                ui.horizontal_wrapped(|ui| {
                    ui.label(t("When importing duplicates:"));
                    for (i, (_, label)) in EXISTING.iter().enumerate() {
                        ui.radio_value(&mut d.existing, i, t(label));
                    }
                });
            });
            crate::popup::footer(ui, |ui| {
                ui.horizontal(|ui| {
                    let n = d.checked.len();
                    export_some = ui.add_enabled(n > 0, Button::new(tf("Export {} Checked…", &[&n.to_string()]))).clicked();
                    export_all = ui.button(t("Export All…")).clicked();
                    // The checked assets, or with none checked the one picked.
                    let doomed = if n > 0 { n } else { usize::from(d.picked.is_some()) };
                    let tip = t("Delete the checked assets, or the one picked; refused while something uses one");
                    delete = ui.add_enabled(doomed > 0, Button::new(tf("Delete {}", &[&doomed.to_string()]))).on_hover_text(tip).clicked();
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        close = ui.button(t("Close")).clicked();
                    });
                });
            });
        });
        if delete {
            let p = if d.checked.is_empty() {
                let mut p = json!({});
                if let Some(n) = &d.picked {
                    p[TABS[d.tab].0] = json!([n]);
                }
                p
            } else {
                d.chosen()
            };
            if p.as_object().is_some_and(|o| !o.is_empty()) {
                self.delete_assets(p, &mut d);
            }
        }
        if import {
            let existing = EXISTING[d.existing].0;
            let dialog = rfd::AsyncFileDialog::new().add_filter(t("BIMCraft assets"), &[bimcraft_core::files::ASSETS]);
            self.ask_file(ctx, dialog, false, move |app, path| {
                app.import_assets_from(&path, existing);
            });
        }
        if export_some || export_all {
            let chosen = if export_all { json!({}) } else { d.chosen() };
            let dialog = rfd::AsyncFileDialog::new()
                .add_filter(t("BIMCraft assets"), &[bimcraft_core::files::ASSETS])
                .set_file_name(format!("{}.{}", self.session.model.name, bimcraft_core::files::ASSETS));
            self.ask_file(ctx, dialog, true, move |app, path| {
                app.export_assets_to(&path, chosen);
            });
        }
        if !(close || r.should_close()) {
            self.asset_manager = Some(d);
        }
    }
}

/// A small square of what asset `name` of `kind` looks like: a material's colour, a line style's.
fn swatch(ui: &mut egui::Ui, m: &bimcraft_core::model::Model, kind: &str, name: &str) {
    let colour = match kind {
        "materials" => m.materials.get(name).map(|x| x.color),
        "line_styles" => m.line_styles.get(name).map(|x| x.color),
        "fill_types" => m.region_types.get(name).map(|x| x.foreground_color),
        _ => None,
    };
    let (rect, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
    if let Some([r, g, b]) = colour {
        ui.painter().rect_filled(rect, 2.0, Color32::from_rgb(r, g, b));
        ui.painter().rect_stroke(rect, 2.0, (1.0, pal().muted), egui::StrokeKind::Inside);
    }
}
