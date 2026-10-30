//! The 3D view's camera in Properties, for the view the 3D tab shows or for a camera view picked on a plan: Projection
//! turns a default view from Axonometric to Perspective and back, its Focal Length is typed there, and Eye Elevation raises
//! or lowers the eye of the camera on screen, looking at the same point. A camera view (Revit's Camera tool, the `camera`
//! command) shows its Name, where its eye stands and what it looks at, each editable, and its lens. How a camera sees in
//! perspective is the core's (`view3d::Camera`).

use bimcraft_i18n::msg;

use egui::{Grid, Ui};
use serde_json::json;

use bimcraft_core::model::{Element, Id};
use bimcraft_core::units::Units;

use crate::BimApp;
use crate::lang::t;

const TIP: &str = msg!("Perspective: see the 3D view as through a camera, its eye standing back as far as the lens and the zoom say");

/// What Projection lists: parallel, then through a camera.
const PROJECTIONS: [&str; 2] = [msg!("Axonometric"), msg!("Perspective")];

/// A point typed as three lengths, `x, y, z` in the project's units, as one camera's point in mm.
fn point3(s: &str, units: Units) -> Option<[f64; 3]> {
    let parts: Vec<&str> = s.split([',', ' ']).filter(|p| !p.trim().is_empty()).collect();
    if parts.len() != 3 {
        return None;
    }
    let v: Vec<f64> = parts.iter().filter_map(|p| units.parse(p)).collect();
    (v.len() == 3).then(|| [v[0], v[1], v[2]])
}

impl BimApp {
    /// The Camera properties of 3D view `view`: the view the 3D tab shows, or a camera view picked on a plan (None for the
    /// default `{3D}` before it exists). A default view shows Projection, Focal Length and the eye's height on screen; a
    /// camera view shows its Name, Eye, Target and Focal Length, each typed through the `camera` and `modify` commands.
    pub(crate) fn camera_properties(&mut self, ui: &mut Ui, view: Option<Id>) {
        let group = msg!("Camera");
        let open = !self.collapsed.contains(group);
        if crate::properties::group_header(ui, t(group), open) && !self.collapsed.remove(group) {
            self.collapsed.insert(group);
        }
        if !open {
            return;
        }
        let (lens, camera, name, is_default, units) = {
            let model = &self.session.model;
            let (lens, camera, name) = match view.and_then(|id| model.elements.get(id)) {
                // The default `{3D}` keeps its name: renaming it would leave the 3D tab without its own view.
                Some(Element::View3D(v)) => (v.lens(), v.camera, if v.name == "{3D}" { None } else { Some(v.name.clone()) }),
                _ => (None, None, None),
            };
            // The default `{3D}` (or no 3D view yet) is the one Projection, Focal Length and Eye Elevation change; once it
            // has a camera of its own it shows that camera, as a camera view does.
            let is_default = camera.is_none() && (view.is_none() || view == model.default_3d());
            (lens, camera, name, is_default, model.units)
        };
        let widget = view.unwrap_or_default();
        let eye_elevation = self.orbit.camera.and_then(|c| c.eye_elevation());
        let (mut set, mut raise, mut moved, mut renamed) = (None, None, None, None);
        egui::Frame::new().inner_margin(egui::Margin { left: 8, right: 0, top: 2, bottom: 6 }).show(ui, |ui| {
            let half = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
            Grid::new("camera-properties").num_columns(2).striped(true).min_col_width(half).max_col_width(half).show(ui, |ui| {
                // A view that exists has its name typed here, as on the plan's symbol: the engine frees the old one and
                // refuses a name another view has.
                if let Some(name) = name {
                    let label = ui.label(t("View Name"));
                    if let Some(typed) = self.field(ui, widget, "name", name, label.id, "") {
                        renamed = Some(typed);
                    }
                    ui.end_row();
                }
                if is_default {
                    let label = ui.label(t("Projection")).on_hover_text(t(TIP));
                    let options: Vec<String> = PROJECTIONS.iter().map(|p| t(p).to_string()).collect();
                    let current = &options[usize::from(lens.is_some())];
                    if let Some(picked) = crate::properties::combo(ui, (widget, "camera.projection"), label.id, current, &options) {
                        set = Some(json!({"on": picked == options[1]}));
                    }
                    ui.end_row();
                }
                // Focal Length: the lens it is seen through (its angle of view). The default view's own (`perspective`) or,
                // for a camera view, its camera's, set with the eye and target it keeps.
                if let Some(lens) = lens {
                    let label = ui.label(t("Focal Length"));
                    if let Some(typed) = self.field(ui, widget, "camera.lens", format!("{lens} mm"), label.id, "") {
                        match typed.trim().trim_end_matches("mm").trim().parse::<f64>() {
                            Ok(mm) => match camera {
                                Some(cam) => moved = Some(json!({"eye": cam.eye, "target": cam.target, "lens": mm})),
                                None => set = Some(json!({"lens": mm})),
                            },
                            Err(_) => {
                                self.status = crate::Status { text: crate::lang::tf("`{}` is not a length", &[typed.trim()]), error: true }
                            }
                        }
                    }
                    ui.end_row();
                }
                if is_default {
                    let label = ui.label(t("Eye Elevation"));
                    let shown = eye_elevation.map(|z| units.length(z)).unwrap_or_default();
                    if let Some(typed) = self.field(ui, widget, "camera.eye", shown, label.id, "") {
                        match units.parse(&typed) {
                            Some(mm) => raise = Some(mm),
                            None => {
                                self.status = crate::Status { text: crate::lang::tf("`{}` is not a length", &[typed.trim()]), error: true }
                            }
                        }
                    }
                    ui.end_row();
                }
                // A camera view: where its eye stands and what it looks at, each `x, y, z`.
                if let Some(cam) = camera {
                    let as_text = |p: [f64; 3]| p.map(|v| units.length(v)).join(", ");
                    let mut typed = (None, None);
                    let label = ui.label(t("Eye"));
                    if let Some(s) = self.field(ui, widget, "camera.eye_point", as_text(cam.eye), label.id, "") {
                        typed.0 = Some(s);
                    }
                    ui.end_row();
                    let label = ui.label(t("Target"));
                    if let Some(s) = self.field(ui, widget, "camera.target_point", as_text(cam.target), label.id, "") {
                        typed.1 = Some(s);
                    }
                    ui.end_row();
                    if typed.0.is_some() || typed.1.is_some() {
                        // A point typed must read; one left alone keeps the camera's.
                        let eye = match typed.0.as_deref() {
                            Some(s) => point3(s, units),
                            None => Some(cam.eye),
                        };
                        let target = match typed.1.as_deref() {
                            Some(s) => point3(s, units),
                            None => Some(cam.target),
                        };
                        match (eye, target) {
                            (Some(eye), Some(target)) => moved = Some(json!({"eye": eye, "target": target})),
                            _ => self.status = crate::Status { text: t("a point is three lengths, `x, y, z`").to_string(), error: true },
                        }
                    }
                }
            });
        });
        if let (Some(id), Some(new)) = (view, renamed) {
            self.run("modify", json!({"id": id, "name": new}));
        }
        if let Some(p) = moved {
            let mut p = p;
            p["view"] = view.map_or(json!("{3D}"), |id| json!(id));
            let changed = self.run("camera", p).is_some();
            // The view on screen still shows the camera as it was: let the next frame stand it from the model, so the
            // settled hand doesn't write the old eye and target back over what was just typed.
            if changed && view.is_some_and(|id| self.active_3d() == Some(id)) {
                self.orbit.camera = None;
                self.orbit.built = None;
            }
        } else if let Some(p) = set {
            self.run("perspective", p);
        }
        if let (Some(z), Some(c)) = (raise, self.orbit.camera) {
            self.orbit.camera = Some(c.with_eye_elevation(z));
        }
    }
}
