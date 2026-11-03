//! The 3D view's camera in Properties: Projection turns it from Axonometric to Perspective and back, and its Focal
//! Length is typed there, both the view's (the `perspective` command); Eye Elevation raises or lowers the eye of the camera on
//! screen, looking at the same point. How a camera sees in perspective is the core's (`view3d::Camera`).

use bimcraft_i18n::msg;

use egui::{Grid, Ui};
use serde_json::json;

use crate::BimApp;
use crate::lang::t;

const TIP: &str = msg!("Perspective: see the 3D view as through a camera, its eye standing back as far as the lens and the zoom say");

/// What Projection lists: parallel, then through a camera.
const PROJECTIONS: [&str; 2] = [msg!("Axonometric"), msg!("Perspective")];

impl BimApp {
    /// The 3D view's Camera in Properties: Projection, and in perspective the lens's Focal Length and the eye's height.
    pub(crate) fn camera_properties(&mut self, ui: &mut Ui) {
        let group = msg!("Camera");
        let open = !self.collapsed.contains(group);
        if crate::properties::group_header(ui, t(group), open) && !self.collapsed.remove(group) {
            self.collapsed.insert(group);
        }
        if !open {
            return;
        }
        let model = &self.session.model;
        let lens = crate::orbit::lens_of(model);
        let units = model.units;
        let view = model.default_3d().unwrap_or_default();
        let eye = self.orbit.camera.and_then(|c| c.eye_elevation());
        let (mut set, mut raise) = (None, None);
        egui::Frame::new().inner_margin(egui::Margin { left: 8, right: 0, top: 2, bottom: 6 }).show(ui, |ui| {
            let half = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
            Grid::new("camera-properties").num_columns(2).min_col_width(half).max_col_width(half).show(ui, |ui| {
                let label = ui.label(t("Projection")).on_hover_text(t(TIP));
                let options: Vec<String> = PROJECTIONS.iter().map(|p| t(p).to_string()).collect();
                let current = &options[usize::from(lens.is_some())];
                if let Some(picked) = crate::properties::combo(ui, (view, "camera.projection"), label.id, current, &options) {
                    set = Some(json!({"on": picked == options[1]}));
                }
                ui.end_row();
                let Some(lens) = lens else { return };
                // The lens is a camera's millimetres, whatever lengths the project shows.
                let label = ui.label(t("Focal Length"));
                if let Some(typed) = self.field(ui, view, "camera.lens", format!("{lens} mm"), label.id, "") {
                    match typed.trim().trim_end_matches("mm").trim().parse::<f64>() {
                        Ok(mm) => set = Some(json!({"lens": mm})),
                        Err(_) => {
                            self.status = crate::Status { text: crate::lang::tf("`{}` is not a length", &[typed.trim()]), error: true }
                        }
                    }
                }
                ui.end_row();
                let label = ui.label(t("Eye Elevation"));
                let shown = eye.map(|z| units.length(z)).unwrap_or_default();
                if let Some(typed) = self.field(ui, view, "camera.eye", shown, label.id, "") {
                    match units.parse(&typed) {
                        Some(mm) => raise = Some(mm),
                        None => self.status = crate::Status { text: crate::lang::tf("`{}` is not a length", &[typed.trim()]), error: true },
                    }
                }
                ui.end_row();
            });
        });
        if let Some(p) = set {
            self.run("perspective", p);
        }
        if let (Some(z), Some(c)) = (raise, self.orbit.camera) {
            self.orbit.camera = Some(c.with_eye_elevation(z));
        }
    }
}
