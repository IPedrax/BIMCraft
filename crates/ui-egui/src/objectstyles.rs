//! Manage > Object Styles, as Revit's dialog: the pen each model category draws its lines in, in projection and where a view
//! cuts it, edited as a draft and applied as one `object_styles` command (OK and Apply), or dropped (Cancel).

use std::collections::BTreeMap;

use bimcraft_core::graphics::MODEL;
use bimcraft_core::model::Model;
use bimcraft_core::pen::{MAX_PEN, Pen};
use bimcraft_core::styles::{ObjectStyle, default_of};
use egui::{Align, ComboBox, Context, Grid, Id as UiId, Layout, Modal, RichText, ScrollArea, Ui, WidgetInfo, WidgetType};
use serde_json::{Value, json};

use crate::BimApp;
use crate::graphics::pen_name;
use crate::lang::{category, t};

/// The Object Styles dialog: every model category's pens as edited and not yet applied.
pub(crate) struct ObjectStylesDialog {
    draft: BTreeMap<&'static str, ObjectStyle>,
}

impl ObjectStylesDialog {
    /// Open on the project's object styles.
    pub(crate) fn new(m: &Model) -> ObjectStylesDialog {
        ObjectStylesDialog { draft: MODEL.iter().map(|c| (*c, m.object_style(c))).collect() }
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
        let r = Modal::new(UiId::new("object-styles")).show(ctx, |ui| {
            ui.set_width(520.0);
            ui.label(RichText::new(t("Object Styles")).heading());
            ui.add_space(6.0);
            ui.label(t(
                "The pen each category draws its lines in, seen and cut, in every plan and section. A view's Visibility/Graphics overrides them.",
            ));
            ui.add_space(4.0);
            ScrollArea::vertical().max_height(520.0).show(ui, |ui| {
                Grid::new("object-styles").striped(true).num_columns(3).show(ui, |ui| {
                    ui.label(RichText::new(t("Category")).strong());
                    ui.label(RichText::new(t("Projection")).strong());
                    ui.label(RichText::new(t("Cut")).strong());
                    ui.end_row();
                    let mut rows: Vec<(String, &mut ObjectStyle)> = d.draft.iter_mut().map(|(c, s)| (category(c, true), s)).collect();
                    rows.sort_by(|a, b| a.0.cmp(&b.0));
                    for (name, style) in rows {
                        ui.label(&name);
                        pen_field(ui, &mut style.projection, format!("{name}: {}", t("Projection")));
                        pen_field(ui, &mut style.cut, format!("{name}: {}", t("Cut")));
                        ui.end_row();
                    }
                });
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(t("Reset")).on_hover_text(t("Back to the pens a project starts with")).clicked() {
                    for (c, s) in &mut d.draft {
                        *s = default_of(c);
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    apply = ui.button(t("Apply")).clicked();
                    cancel = ui.button(t("Cancel")).clicked();
                    ok = ui.button(t("OK")).clicked();
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
        }
    }
}
