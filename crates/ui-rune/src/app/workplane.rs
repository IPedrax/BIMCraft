//! The 3D view's work plane in the window, after egui's `workplane.rs`: the top bar's Work Plane button names it and opens Revit's Work Plane
//! dialog. Current Work Plane says it (`Level : Level 1`, `Reference Plane : Ramp`, a surface or three points and how steep, the offset),
//! Dissociate leaves a work plane on a reference plane where it is, and Specify a new Work Plane takes a level or a named reference plane of
//! the project with an offset along its normal. The core keeps and resolves it (`bimcraft_core::workplane`, the `work_plane` command). Not
//! here yet: Show (the plane's grid drawn into the 3D picture), the Pick a plane, Pick a line and Pick three points, and the work plane
//! of components. A child of `app` so it reaches the private handlers.

use bimcraft_core::workplane;
use rune::TextEdit;
use serde_json::json;

use super::App;
use crate::lang::{t, tf};

/// The most characters of a refusal the dialog shows.
const MAX_REFUSAL: usize = 2_000;

/// The Work Plane dialog, open.
pub struct WorkPlaneDialog {
    /// The planes the project offers by name (`workplane::choices`): its levels and its named reference planes.
    pub choices: Vec<String>,
    /// The one chosen to be set.
    pub name: String,
    /// The offset along its normal, as typed in the project's units.
    pub offset: TextEdit,
    /// The list of names is open.
    pub open: bool,
    pub error: Option<String>,
}

impl App {
    /// What the 3D view sketches on, for the button: the level, or a surface or three points and how steep, and the offset.
    pub fn work_plane_name(&self) -> String {
        let m = &self.session.model;
        let wp = m.work_plane(m.default_3d());
        let slope = || format!("{:.0}", wp.slope(m));
        let name = match (&wp.level, &wp.ref_plane, &wp.surface) {
            (Some(_), ..) | (_, Some(_), _) => wp.name(m),
            (None, None, Some(_)) => tf("Surface, {}\u{b0}", &[&slope()]),
            _ => tf("Three points, {}\u{b0}", &[&slope()]),
        };
        if wp.offset == 0.0 { name } else { format!("{name} + {}", m.length(wp.offset)) }
    }

    /// Whether the work plane follows a reference plane (what Dissociate can leave).
    pub fn work_plane_follows(&self) -> bool {
        let m = &self.session.model;
        m.work_plane(m.default_3d()).ref_plane.is_some()
    }

    /// Opens the Work Plane dialog, on the plane that is the work plane now when it is one of the names.
    pub fn open_work_plane(&mut self) {
        let m = &self.session.model;
        let wp = m.work_plane(m.default_3d());
        let choices: Vec<String> = workplane::choices(m).into_iter().map(|(_, label)| label).collect();
        let current = wp.name(m);
        let name = if choices.contains(&current) { current } else { choices.first().cloned().unwrap_or_default() };
        let offset = TextEdit::new(if wp.offset == 0.0 { String::new() } else { m.length(wp.offset) });
        self.close_menu();
        self.dialogs.clear();
        self.dialogs.workplane = Some(WorkPlaneDialog { choices, name, offset, open: false, error: None });
    }

    pub fn toggle_work_plane_list(&mut self) {
        if let Some(d) = self.dialogs.workplane.as_mut() {
            d.open = !d.open;
        }
    }

    pub fn close_work_plane_list(&mut self) {
        if let Some(d) = self.dialogs.workplane.as_mut() {
            d.open = false;
        }
    }

    /// The name chosen from the list; the list closes.
    pub fn choose_work_plane(&mut self, name: &str) {
        if let Some(d) = self.dialogs.workplane.as_mut() {
            d.name = name.to_string();
            d.open = false;
        }
    }

    /// OK: the plane named, moved by the offset typed, becomes the 3D view's work plane. What is wrong (an offset that is no length, what the
    /// core refuses) is said in the dialog, which stays; when it is taken the dialog closes.
    pub fn apply_work_plane(&mut self) -> bool {
        let Some(d) = self.dialogs.workplane.as_ref() else { return false };
        let (name, typed) = (d.name.clone(), d.offset.text.trim().to_string());
        let offset = if typed.is_empty() {
            0.0
        } else {
            match self.session.model.units.parse(&typed) {
                Some(mm) if mm.is_finite() => mm,
                _ => return self.work_plane_error(tf("`{}` is not a length", &[&typed])),
            }
        };
        if name.is_empty() {
            return self.work_plane_error(t("this project has no level or reference plane to set").to_string());
        }
        match self.session.execute("work_plane", &json!({"name": name, "offset": offset})) {
            Ok(_) => {
                self.message = None;
                self.dialogs.workplane = None;
                self.settle();
                true
            }
            Err(why) => self.work_plane_error(why),
        }
    }

    /// Dissociate: a work plane on a reference plane is left where it is, no longer following it.
    pub fn dissociate_work_plane(&mut self) -> bool {
        if !self.work_plane_follows() {
            return self.work_plane_error(
                t("Dissociate: only a work plane on a reference plane follows it; this one follows nothing").to_string(),
            );
        }
        match self.session.execute("work_plane", &json!({"dissociate": true})) {
            Ok(_) => {
                self.message = None;
                self.settle();
                true
            }
            Err(why) => self.work_plane_error(why),
        }
    }

    /// `why` is said in the dialog when it is open, else in the status bar. Always false.
    fn work_plane_error(&mut self, why: String) -> bool {
        let why: String = why.chars().take(MAX_REFUSAL).collect();
        match self.dialogs.workplane.as_mut() {
            Some(d) => d.error = Some(why),
            None => self.message = Some(why),
        }
        false
    }
}

#[cfg(test)]
#[path = "workplane_tests.rs"]
mod tests;
