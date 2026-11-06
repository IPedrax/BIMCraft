//! Graphic Display Options (GD), as Revit's for a 3D view: whether it shows its edges, a silhouette round what stands in front
//! of what is behind and how wide, and ambient shadows where faces meet. GD opens it, and so does the view control bar's Visual
//! Style menu, in both looks of the window; OK and Apply run `graphic_display_options` on the view the 3D tab shows, which both
//! painters (the processor's and the card's) draw as it says.

use bimcraft_core::graphics::VisualStyle;
use bimcraft_core::model::Element;
use bimcraft_core::view3d::{DisplayOptions, MAX_SILHOUETTE};
use egui::{Align, Checkbox, ComboBox, Context, Id as UiId, Layout, RichText};
use serde_json::json;

use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::style::pal;
use crate::{BimApp, Sheet, Status};

/// The options as the dialog has them, until OK, Apply or Cancel; and why the last Apply was refused.
pub(crate) struct DisplayDialog {
    pub(crate) options: DisplayOptions,
    error: Option<String>,
}

/// A silhouette's width as the list shows it.
fn width(px: u32) -> String {
    if px == 0 { t("<None>").to_string() } else { tf("{} px", &[&px.to_string()]) }
}

impl BimApp {
    /// The options of the 3D view the 3D tab shows: the default's until something sets them.
    pub(crate) fn display_now(&self) -> DisplayOptions {
        let m = &self.session.model;
        match self.active_3d().and_then(|v| m.elements.get(v)) {
            Some(Element::View3D(v)) => v.display,
            _ => DisplayOptions::default(),
        }
    }

    /// Graphic Display Options (GD) for the 3D view; in another view, says they are the 3D view's.
    pub(crate) fn open_display(&mut self) {
        if self.sheet == Sheet::View3D {
            self.display_dialog = Some(DisplayDialog { options: self.display_now(), error: None });
        } else {
            let text = t("graphic display options are for the 3D view: open it from the Project Browser");
            self.status = Status { text: text.into(), error: true };
        }
    }

    pub(crate) fn display_window(&mut self, ctx: &Context) {
        // Hidden Line and Wireframe draw their edges whatever the view says, as Revit's do.
        let shaded = self.graphics_shown().style == VisualStyle::Shaded;
        let Some(d) = self.display_dialog.as_mut() else { return };
        let (mut ok, mut apply, mut close) = (false, false, false);
        let r = Modal::new(UiId::new("graphic-display-options"), t("Graphic Display Options")).width(420.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(4.0);
                ui.label(RichText::new(t("Model Display")).strong());
                ui.add_enabled(shaded, Checkbox::new(&mut d.options.edges, t("Show Edges")))
                    .on_hover_text(t("The lines where the surfaces meet"))
                    .on_disabled_hover_text(t("Hidden Line and Wireframe always draw their edges"));
                ui.horizontal(|ui| {
                    let l = ui.label(t("Silhouettes"));
                    let c = ComboBox::from_id_salt("silhouettes").selected_text(width(d.options.silhouette)).show_ui(ui, |ui| {
                        for px in 0..=MAX_SILHOUETTE {
                            ui.selectable_value(&mut d.options.silhouette, px, width(px));
                        }
                    });
                    c.response.labelled_by(l.id).on_hover_text(t("An outline round what stands in front of what is behind it"));
                });
                ui.add_space(8.0);
                ui.label(RichText::new(t("Shadows")).strong());
                ui.checkbox(&mut d.options.ambient_shadows, t("Show Ambient Shadows"))
                    .on_hover_text(t("Soft shadows where faces meet inside a corner"));
                if let Some(e) = &d.error {
                    ui.add_space(6.0);
                    ui.label(RichText::new(e).color(pal().error));
                }
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    apply = ui.button(t("Apply")).clicked();
                    close = ui.button(t("Cancel")).clicked();
                    ok = ui.button(t("OK")).clicked();
                });
            });
        });
        if ok || apply {
            if self.apply_display() && ok {
                self.display_dialog = None;
            }
        } else if close || r.should_close() {
            self.display_dialog = None;
        }
    }

    /// Runs `graphic_display_options` on the 3D view with what the dialog has, unless that is what the view has: whether the
    /// view has it now. A refusal is said in the dialog.
    fn apply_display(&mut self) -> bool {
        let Some(o) = self.display_dialog.as_ref().map(|d| d.options) else { return false };
        if o == self.display_now() {
            return true;
        }
        let view = self.active_3d().map_or_else(|| json!("{3D}"), |id| json!(id));
        let p = json!({"view": view, "edges": o.edges, "silhouette": o.silhouette, "ambient_shadows": o.ambient_shadows});
        if self.run("graphic_display_options", p).is_some() {
            return true;
        }
        let e = std::mem::take(&mut self.status).text;
        if let Some(d) = self.display_dialog.as_mut() {
            d.error = Some(e);
        }
        false
    }
}
