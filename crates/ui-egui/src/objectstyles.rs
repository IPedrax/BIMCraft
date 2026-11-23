//! Manage > Object Styles, as Revit's dialog: the pen each model category draws its lines in, in projection and where a view
//! cuts it, edited as a draft and applied as one `object_styles` command (OK and Apply), or dropped (Cancel).

use crate::popup::Modal;

use std::collections::BTreeMap;

use bimcraft_core::graphics::{CURTAIN_PARTS, MODEL};
use bimcraft_core::model::Model;
use bimcraft_core::pen::{MAX_PEN, Pen};
use bimcraft_core::styles::ObjectStyle;
use egui::{Align, ComboBox, Context, Grid, Id as UiId, Layout, RichText, ScrollArea, Ui, WidgetInfo, WidgetType};
use serde_json::{Value, json};

use crate::BimApp;
use crate::graphics::pen_name;
use crate::lang::{category, t};

/// The Object Styles dialog: every model category's pens as edited and not yet applied.
pub(crate) struct ObjectStylesDialog {
    draft: BTreeMap<&'static str, ObjectStyle>,
    /// Opened from a family document (FE-19): its category, whose rows come first, and the note that the project changes.
    family: Option<&'static str>,
}

impl ObjectStylesDialog {
    /// Open on the project's object styles.
    pub(crate) fn new(m: &Model) -> ObjectStylesDialog {
        // Every model category, the subcategories that have rows of their own (Doors > Opening), and a curtain wall's parts.
        let keys = MODEL.iter().copied().chain(bimcraft_core::styles::model_subs()).chain(CURTAIN_PARTS);
        ObjectStylesDialog { draft: keys.map(|c| (c, m.object_style(c))).collect(), family: None }
    }

    /// Open from a family document of category `category` (its graphics category's key): its rows first.
    pub(crate) fn for_family(m: &Model, category: &'static str) -> ObjectStylesDialog {
        ObjectStylesDialog { family: Some(category), ..ObjectStylesDialog::new(m) }
    }
}

/// What a row is called: its category's name, a subcategory's under its category's (Doors > Opening), so it sorts beside it.
fn row_name(key: &str) -> String {
    match bimcraft_core::graphics::sub(key) {
        Some(s) => format!("{} > {}", category(s.parent, true), t(s.name)),
        None => category(key, true),
    }
}

/// A drop-down of the pens, named `name` for screen readers.
fn pen_field(ui: &mut Ui, pen: &mut Pen, name: String) {
    let c = ComboBox::from_id_salt(&name).selected_text(pen_name(*pen)).width(120.0).show_ui(ui, |ui| {
        for each in (0..=MAX_PEN).map(Pen) {
            ui.selectable_value(pen, each, pen_name(each));
        }
    });
    c.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, name.clone()));
}

impl BimApp {
    /// The Object Styles dialog, while it's open.
    pub(crate) fn object_styles_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.object_styles_dialog.take() else { return };
        let (mut ok, mut apply, mut cancel) = (false, false, false);
        let r = Modal::new(UiId::new("object-styles"), t("Object Styles")).width(520.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(6.0);
                ui.label(t(
                    "The pen each category draws its lines in, seen and cut, in every plan and section. A view's Visibility/Graphics overrides them.",
                ));
                if d.family.is_some() {
                    ui.add_space(4.0);
                    ui.label(RichText::new(t(super::editor::PROJECT_NOTE_STYLES)).strong());
                }
                ui.add_space(4.0);
                ScrollArea::vertical().max_height(520.0).show(ui, |ui| {
                    Grid::new("object-styles").striped(true).num_columns(3).show(ui, |ui| {
                        ui.label(RichText::new(t("Category")).strong());
                        ui.label(RichText::new(t("Projection")).strong());
                        ui.label(RichText::new(t("Cut")).strong());
                        ui.end_row();
                        // A family's category, and its subcategories, first (FE-19); the rest by name.
                        let family = d.family;
                        let theirs =
                            |c: &str| family.is_some_and(|f| c == f || bimcraft_core::graphics::sub(c).is_some_and(|s| s.parent == f));
                        let mut rows: Vec<(bool, String, &mut ObjectStyle)> =
                            d.draft.iter_mut().map(|(c, s)| (!theirs(c), row_name(c), s)).collect();
                        rows.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
                        let rows: Vec<(String, &mut ObjectStyle)> = rows.into_iter().map(|(_, n, s)| (n, s)).collect();
                        for (name, style) in rows {
                            ui.label(&name);
                            pen_field(ui, &mut style.projection, format!("{name}: {}", t("Projection")));
                            pen_field(ui, &mut style.cut, format!("{name}: {}", t("Cut")));
                            ui.end_row();
                        }
                    });
                });
            });
            crate::popup::footer(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.button(t("Reset")).on_hover_text(t("Back to the pens a project starts with")).clicked() {
                        for (c, s) in &mut d.draft {
                            // As a project starts: a subcategory as its category does, a curtain wall's part with its own.
                            *s = bimcraft_core::styles::inherited(&BTreeMap::new(), c);
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                        apply = ui.button(t("Apply")).clicked();
                        cancel = ui.button(t("Cancel")).clicked();
                        ok = ui.button(t("OK")).clicked();
                    });
                });
            });
        });
        // What changed, as one command: every category whose pens aren't the project's now. A refusal keeps the dialog
        // open to fix it.
        let mut failed = false;
        if ok || apply {
            let m = &self.session.model;
            let changed: serde_json::Map<String, Value> = (d.draft.iter())
                .filter(|(c, s)| m.object_style(c) != **s)
                .map(|(c, s)| (c.to_string(), json!({"projection": s.projection, "cut": s.cut})))
                .collect();
            if !changed.is_empty() {
                failed = self.run("object_styles", json!({"categories": changed})).is_none();
            }
        }
        if failed || !(ok || cancel || r.should_close()) {
            self.object_styles_dialog = Some(d);
        } else if self.vg_dialog.is_some() {
            // Visibility/Graphics, which opened it, comes back on the next frame.
            ctx.request_repaint();
        }
    }
}
