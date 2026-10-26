//! Manage > Line Weights, as Revit's dialog: what each pen draws on paper, the model's at each of a list of scales and the
//! annotation's at any, edited as a draft and applied as one `line_weights` command (OK and Apply), or dropped (Cancel).

use bimcraft_i18n::msg;

use bimcraft_core::model::Model;
use bimcraft_core::pen::{LineWeights, MAX_PEN, MAX_SCALES, ScalePens, WIDTH_RANGE};
use bimcraft_core::units::Units;
use egui::{Align, ComboBox, Context, DragValue, Grid, Id as UiId, Layout, Modal, RichText, ScrollArea, Ui, WidgetInfo, WidgetType};
use serde_json::json;

use crate::BimApp;
use crate::lang::{t, tf};

/// The dialog's tabs.
const TABS: [&str; 2] = [msg!("Model Line Weights"), msg!("Annotation Line Weights")];

/// The Line Weights dialog: its tab, and the line weights as edited and not yet applied.
pub(crate) struct LineWeightsDialog {
    tab: usize,
    draft: LineWeights,
}

impl LineWeightsDialog {
    /// Open on the project's line weights.
    pub(crate) fn new(m: &Model) -> LineWeightsDialog {
        LineWeightsDialog { tab: 0, draft: m.line_weights().into_owned() }
    }
}

/// A pen's width, in mm on paper, in a field named `name` for screen readers.
fn width_field(ui: &mut Ui, w: &mut f64, name: String) {
    let r = ui.add(DragValue::new(w).range(WIDTH_RANGE).speed(0.005).fixed_decimals(2));
    r.widget_info(|| WidgetInfo::labeled(WidgetType::DragValue, true, name.clone()));
}

/// The model's pens: a column for each scale, finest first, each with its Delete; then Add Scale, which copies the column
/// nearest the scale picked.
fn model_tab(ui: &mut Ui, d: &mut LineWeightsDialog, units: Units) {
    ui.label(t(
        "Each pen's width on paper, mm, in views at each scale: a view between two takes the nearer one's. Pen 0 is always a hairline.",
    ));
    ui.add_space(4.0);
    let mut delete = None;
    ScrollArea::both().max_height(560.0).show(ui, |ui| {
        Grid::new("line-weights-model").striped(true).min_col_width(64.0).show(ui, |ui| {
            ui.label(RichText::new(t("Pen")).strong());
            let last = d.draft.model.len() == 1;
            for (i, col) in d.draft.model.iter().enumerate() {
                let name = units.scale(col.scale);
                ui.vertical(|ui| {
                    ui.label(RichText::new(&name).strong());
                    let r = ui.add_enabled(!last, egui::Button::new(t("Delete")).small().wrap_mode(egui::TextWrapMode::Extend));
                    r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, !last, tf("Delete {}", &[&name])));
                    if r.clicked() {
                        delete = Some(i);
                    }
                });
            }
            ui.end_row();
            for pen in 0..usize::from(MAX_PEN) {
                ui.label((pen + 1).to_string());
                for col in &mut d.draft.model {
                    let name = tf("Pen {} at {}", &[&(pen + 1).to_string(), &units.scale(col.scale)]);
                    if let Some(w) = col.widths.get_mut(pen) {
                        width_field(ui, w, name);
                    }
                }
                ui.end_row();
            }
        });
    });
    if let Some(i) = delete {
        d.draft.model.remove(i);
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let l = ui.label(t("Add Scale:"));
        let free: Vec<f64> = units.scales().iter().copied().filter(|n| d.draft.column(*n).is_none()).collect();
        let full = d.draft.model.len() >= MAX_SCALES;
        let mut add = None;
        ui.add_enabled_ui(!full && !free.is_empty(), |ui| {
            let c = ComboBox::from_id_salt("line-weights-add").selected_text("").width(160.0).show_ui(ui, |ui| {
                for n in &free {
                    if ui.selectable_label(false, units.scale(*n)).clicked() {
                        add = Some(*n);
                    }
                }
            });
            c.response.labelled_by(l.id);
        });
        if let Some(n) = add {
            let widths = d.draft.at(n).model;
            let mut col = ScalePens { scale: n, widths: [0.0; MAX_PEN as usize] };
            col.widths.copy_from_slice(&widths[1..]);
            d.draft.model.push(col);
            d.draft.model.sort_by(|a, b| a.scale.total_cmp(&b.scale));
        }
    });
}

/// The annotation's pens, at any scale.
fn annotation_tab(ui: &mut Ui, d: &mut LineWeightsDialog) {
    ui.label(t(
        "Each pen's width on paper, mm, for dimensions, tags, text, grids, levels and marks, at any scale. Pen 0 is always a hairline.",
    ));
    ui.add_space(4.0);
    Grid::new("line-weights-annotation").striped(true).show(ui, |ui| {
        ui.label(RichText::new(t("Pen")).strong());
        ui.label(RichText::new(t("Width")).strong());
        ui.end_row();
        for (pen, w) in d.draft.annotation.iter_mut().enumerate() {
            ui.label((pen + 1).to_string());
            width_field(ui, w, tf("Annotation pen {}", &[&(pen + 1).to_string()]));
            ui.end_row();
        }
    });
}

impl BimApp {
    /// The Line Weights dialog, while it's open.
    pub(crate) fn line_weights_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.line_weights_dialog.take() else { return };
        let units = self.session.model.units;
        let (mut ok, mut apply, mut cancel) = (false, false, false);
        let r = Modal::new(UiId::new("line-weights")).show(ctx, |ui| {
            ui.set_width(760.0);
            ui.label(RichText::new(t("Line Weights")).heading());
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                for (i, label) in TABS.iter().enumerate() {
                    if ui.selectable_label(d.tab == i, t(label)).clicked() {
                        d.tab = i;
                    }
                }
            });
            ui.separator();
            match d.tab {
                0 => model_tab(ui, &mut d, units),
                _ => annotation_tab(ui, &mut d),
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(t("Reset")).on_hover_text(t("Back to the line weights a project in these units starts with")).clicked() {
                    d.draft = LineWeights::default_for(units);
                }
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    apply = ui.button(t("Apply")).clicked();
                    cancel = ui.button(t("Cancel")).clicked();
                    ok = ui.button(t("OK")).clicked();
                });
            });
        });
        // What changed, as one command: the project's start again is a reset, else the whole table. A refusal keeps the
        // dialog open to fix it.
        let mut failed = false;
        if (ok || apply) && *self.session.model.line_weights() != d.draft {
            let p = if d.draft == LineWeights::default_for(units) {
                json!({"reset": true})
            } else {
                json!({"model": d.draft.model, "annotation": d.draft.annotation})
            };
            failed = self.run("line_weights", p).is_none();
        }
        if failed || !(ok || cancel || r.should_close()) {
            self.line_weights_dialog = Some(d);
        }
    }
}
