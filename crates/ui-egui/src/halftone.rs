//! Manage > Annotation Settings > Halftone/Underlay, as Revit's: how far toward the paper halftone draws, 1 to 100 percent,
//! on a slider. It lightens what Visibility/Graphics marks Halftone, a linked model, and a plan's underlay (what it sees
//! between its view range's bottom and its view depth). OK and Apply run `halftone`.

use egui::{Align, Button, Context, Id as UiId, Layout, RichText, Slider, vec2};
use serde_json::json;

use crate::BimApp;
use crate::lang::t;
use crate::popup::Modal;
use crate::style::pal;

/// The fade as the slider has it, until OK, Apply or Cancel; and why the last Apply was refused.
pub(crate) struct HalftoneDialog {
    pub(crate) fade: u8,
    error: Option<String>,
}

impl BimApp {
    pub(crate) fn open_halftone(&mut self) {
        self.halftone_dialog = Some(HalftoneDialog { fade: self.session.model.halftone, error: None });
    }

    pub(crate) fn halftone_window(&mut self, ctx: &Context) {
        let Some(d) = self.halftone_dialog.as_mut() else { return };
        let (mut ok, mut apply, mut close) = (false, false, false);
        let r = Modal::new(UiId::new("halftone-underlay"), t("Halftone/Underlay")).width(380.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(4.0);
                ui.label(RichText::new(t("Halftone")).strong());
                ui.horizontal(|ui| {
                    let l = ui.label(t("Fade"));
                    ui.spacing_mut().slider_width = 220.0;
                    ui.add(Slider::new(&mut d.fade, 1..=100).suffix(" %")).labelled_by(l.id);
                });
                // What it does, on a line of ink: as it is, and lightened.
                let (rect, _) = ui.allocate_exact_size(vec2(220.0, 18.0), egui::Sense::hover());
                // In the theme's ink, faded toward the panel as halftone fades toward the paper.
                let (ink, paper) = (ui.visuals().text_color(), ui.visuals().panel_fill);
                let k = f32::from(d.fade) / 100.0;
                let faded = egui::Color32::from_rgb(
                    (f32::from(ink.r()) + (f32::from(paper.r()) - f32::from(ink.r())) * k) as u8,
                    (f32::from(ink.g()) + (f32::from(paper.g()) - f32::from(ink.g())) * k) as u8,
                    (f32::from(ink.b()) + (f32::from(paper.b()) - f32::from(ink.b())) * k) as u8,
                );
                let p = ui.painter();
                let y = rect.center().y;
                p.line_segment([rect.left_center(), egui::pos2(rect.center().x - 6.0, y)], egui::Stroke::new(2.0, ink));
                p.line_segment([egui::pos2(rect.center().x + 6.0, y), rect.right_center()], egui::Stroke::new(2.0, faded));
                ui.label(
                    RichText::new(t(
                        "What a plan sees between its view range's bottom and its view depth, what Visibility/Graphics marks Halftone, and linked models: their lines and fills this far toward the paper.",
                    ))
                    .color(pal().muted)
                    .small(),
                );
                if let Some(e) = &d.error {
                    ui.add_space(6.0);
                    ui.label(RichText::new(e).color(pal().error));
                }
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    let size = vec2(76.0, 0.0);
                    apply = ui.add(Button::new(t("Apply")).min_size(size)).clicked();
                    close = ui.add(Button::new(t("Cancel")).min_size(size)).clicked();
                    ok = ui.add(Button::new(t("OK")).min_size(size)).clicked();
                });
            });
        });
        if ok || apply {
            if self.apply_halftone() && ok {
                self.halftone_dialog = None;
            }
        } else if close || r.should_close() {
            self.halftone_dialog = None;
        }
    }

    /// Runs `halftone` with the dialog's fade, unless the project has it: whether it has it now.
    fn apply_halftone(&mut self) -> bool {
        let Some(fade) = self.halftone_dialog.as_ref().map(|d| d.fade) else { return false };
        if fade == self.session.model.halftone || self.run("halftone", json!({"fade": fade})).is_some() {
            return true;
        }
        let e = std::mem::take(&mut self.status).text;
        if let Some(d) = self.halftone_dialog.as_mut() {
            d.error = Some(e);
        }
        false
    }
}
