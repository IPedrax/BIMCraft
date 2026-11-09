//! The navigation triad in a corner of the 3D view (`bimcraft_core::triad`): the x, y and z axes turning with the camera, with
//! their negative ends marked. A click on an end turns the camera to look from that side (top, bottom, front, back, left, right),
//! and a second click on the same end to the opposite side. With Alt held, a click on an end sets the work plane square to that
//! axis (the `work_plane` command), and a click on the triad's empty space goes back to the level. Hovering an end names the
//! view a click turns to (with Alt, what Alt would do).
//!
//! It is drawn over the 3D view's picture, whichever painter made it, and picks from the camera alone, so it picks the same on the
//! processor and on the graphics card. Its widget lies over the view's, so a click or a drag on it never reaches the model behind
//! it, and the view under it is not hovered. It is a triad, not a cube.

use bimcraft_core::triad::{self, End, Placed};
use bimcraft_core::view3d::{Camera, P3};
use egui::{Align2, Color32, CursorIcon, FontId, Painter, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::json;

use crate::canvas::{click_modifiers, tag};
use crate::lang::{t, tc, tf};
use crate::orbit::Orbit;
use crate::style::canvas_pal;
use crate::{BimApp, Status};

/// The side of the triad's square, in points.
const SIZE: f32 = 92.0;
/// How far an end's marker stands from the middle at its farthest, in points.
const RADIUS: f32 = 30.0;
/// The markers' radii, positive and negative, and how near one a click is taken for it.
const POSITIVE: f32 = 9.0;
const NEGATIVE: f32 = 6.0;
const REACH: f32 = 10.0;
/// How far the triad stands clear of the navigation bar, and of the view's edge.
const GAP: f32 = 8.0;

/// What the triad was asked for in a frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Ask {
    /// Turn the camera to look from this end's side.
    Turn(End),
    /// Set the work plane square to this end's axis.
    Plane(End),
    /// Set the work plane back to the level.
    Level,
}

/// Where the pointer was over the triad in the last frame it was shown.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Over {
    /// On the triad's square at all.
    pub inside: bool,
    /// On one of its ends.
    pub end: Option<End>,
    /// Alt was held.
    pub alt: bool,
}

/// The colour of an axis: red for x, green for y, blue for z.
fn axis_color(axis: usize) -> Color32 {
    [Color32::from_rgb(222, 74, 68), Color32::from_rgb(86, 178, 90), Color32::from_rgb(64, 132, 232)][axis.min(2)]
}

impl Orbit {
    /// Where the triad stands in the 3D view `view`: left of the navigation bar, whose corner is `nav`, level with its top. None
    /// where the view is too small to hold it.
    pub(crate) fn triad_rect(view: Rect, nav: Pos2) -> Option<Rect> {
        let rect = Rect::from_min_size(pos2(nav.x - GAP - SIZE, nav.y), vec2(SIZE, SIZE));
        view.shrink(4.0).contains_rect(rect).then_some(rect)
    }

    /// The triad's widget for the frame, over the view's own in `view` (its navigation bar's corner at `nav`): what the pointer is
    /// over, and what a click there asks for. The camera it works from is the one on screen, which the user sees.
    pub(crate) fn triad_input(&mut self, ui: &Ui, view: Rect, nav: Pos2) -> Option<Ask> {
        self.triad = Over::default();
        let cam = self.camera?;
        let rect = Self::triad_rect(view, nav)?;
        // Over the view's widget, which senses clicks and drags: both are the triad's here and not the view's.
        let response = ui.interact(rect, ui.id().with("triad"), Sense::click_and_drag());
        let placed = triad::layout(&cam);
        let end_at = |at: Pos2| {
            let d = (at - rect.center()) / RADIUS;
            triad::pick(&placed, (f64::from(d.x), f64::from(d.y)), f64::from(REACH / RADIUS))
        };
        if let Some(at) = response.hover_pos() {
            let end = end_at(at);
            self.triad = Over { inside: true, end, alt: ui.input(|i| i.modifiers.alt) };
            if end.is_some() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
        }
        if response.clicked()
            && let Some(at) = response.interact_pointer_pos()
        {
            return match (end_at(at), click_modifiers(ui).alt) {
                (Some(end), false) => Some(Ask::Turn(end)),
                (Some(end), true) => Some(Ask::Plane(end)),
                (None, true) => Some(Ask::Level),
                (None, false) => None,
            };
        }
        None
    }

    /// What hovering the triad says: the view a click on the end turns to, with Alt the work plane it sets, and on empty space with
    /// Alt that it goes back to the level.
    pub(crate) fn triad_label(&self) -> Option<String> {
        let (over, cam) = (self.triad, self.camera?);
        match (over.end, over.alt) {
            (Some(end), false) => Some(tc("side", triad::side_after(&cam, end).name()).to_string()),
            (Some(end), true) => Some(tf("Work plane square to the {} axis", &[end.axis_name()])),
            (None, true) if over.inside => Some(t("Work plane back to the level").to_string()),
            _ => None,
        }
    }

    /// The triad drawn on `p` over the 3D view in `view` (its navigation bar's corner at `nav`), as the camera `cam` shows it, the
    /// farthest end first, and what hovering it names.
    pub(crate) fn triad_draw(&self, p: &Painter, view: Rect, nav: Pos2, cam: &Camera) {
        let Some(rect) = Self::triad_rect(view, nav) else { return };
        let over = self.triad;
        let ink = canvas_pal();
        let middle = rect.center();
        // A faint ground under it, firmer while the pointer is over it, so the axes read on any model behind.
        let ground = if over.inside { 0.82 } else { 0.5 };
        p.circle_filled(middle, RADIUS + POSITIVE + 3.0, ink.panel.gamma_multiply(ground));
        for Placed { end, x, y, depth } in triad::layout(cam) {
            let at = middle + vec2(x as f32, y as f32) * RADIUS;
            if !at.is_finite() {
                continue;
            }
            let color = axis_color(end.axis());
            // Turned away from the viewer, fainter.
            let color = if depth < -0.01 { color.gamma_multiply(0.6) } else { color };
            let lit = over.end == Some(end);
            if end.positive() {
                p.line_segment([middle, at], Stroke::new(2.0, color));
                p.circle_filled(at, POSITIVE, color);
                p.text(at, Align2::CENTER_CENTER, end.letter(), FontId::proportional(11.0), Color32::WHITE);
            } else {
                p.line_segment([middle, at], Stroke::new(1.0, color));
                p.circle_filled(at, NEGATIVE, ink.panel);
                p.circle_stroke(at, NEGATIVE, Stroke::new(1.5, color));
                p.line_segment([at - vec2(2.5, 0.0), at + vec2(2.5, 0.0)], Stroke::new(1.5, color));
            }
            if lit {
                let r = if end.positive() { POSITIVE } else { NEGATIVE };
                p.circle_stroke(at, r + 2.5, Stroke::new(1.5, ink.accent));
            }
        }
        if let Some(label) = self.triad_label() {
            tag(p, rect.right_bottom() + vec2(0.0, 4.0), Align2::RIGHT_TOP, label);
        }
    }

    /// Where `end` shows on screen in the 3D view as last drawn (its navigation bar's corner at `nav`).
    #[cfg(test)]
    pub(crate) fn triad_at(&self, end: End, nav: Pos2) -> Option<Pos2> {
        let (cam, rect) = (self.camera?, Self::triad_rect(self.rect, nav)?);
        let p = triad::layout(&cam).into_iter().find(|p| p.end == end)?;
        Some(rect.center() + vec2(p.x as f32, p.y as f32) * RADIUS)
    }

    /// A point on the triad's square that no end is near, for a click on its empty space.
    #[cfg(test)]
    pub(crate) fn triad_empty(&self, nav: Pos2) -> Option<Pos2> {
        let (cam, rect) = (self.camera?, Self::triad_rect(self.rect, nav)?);
        let placed = triad::layout(&cam);
        // The square's corners, which the circle of the axes leaves clear.
        let (far, d) = (f64::from(REACH * 2.0 / RADIUS), SIZE / 2.0 - 4.0);
        [vec2(-d, d), vec2(d, d), vec2(-d, -d), vec2(d, -d)].into_iter().map(|c| rect.center() + c).find(|at| {
            triad::pick(&placed, (f64::from((at.x - rect.center().x) / RADIUS), f64::from((at.y - rect.center().y) / RADIUS)), far)
                .is_none()
        })
    }
}

/// The project's 3D view's triad: the `Orbit`'s, placed beside the window's navigation bar, and what Alt and a click on it do to
/// the project's work plane.
impl BimApp {
    /// Where the triad stands in the project's 3D view `view`.
    #[cfg(test)]
    pub(crate) fn triad_rect(&self, view: Rect) -> Option<Rect> {
        Orbit::triad_rect(view, self.nav_corner(view))
    }

    /// What hovering the project's triad says.
    #[cfg(test)]
    pub(crate) fn triad_label(&self) -> Option<String> {
        self.orbit.triad_label()
    }

    /// Alt and a click on `end`: the work plane square to its axis, through the point of the plane the view was on nearest `target`.
    pub(crate) fn triad_plane(&mut self, end: End, target: P3) {
        let m = &self.session.model;
        let current = m.work_plane(m.default_3d()).resolve(m).ok();
        let surface = triad::plane(current.as_ref(), target, end);
        if self.run("work_plane", json!({"surface": surface})).is_some() {
            self.status = Status { text: tf("Work plane square to the {} axis", &[end.axis_name()]), error: false };
        }
    }

    /// Alt and a click on the triad's empty space: the work plane back to the level, the lowest as a 3D view with none of its own
    /// sketches on. Nothing to do (and no undo step) where it is there already.
    pub(crate) fn triad_level(&mut self) {
        let m = &self.session.model;
        let own = m
            .default_3d()
            .is_some_and(|v| matches!(m.elements.get(v), Some(bimcraft_core::model::Element::View3D(v3)) if v3.work_plane.is_some()));
        if !own {
            self.status = Status { text: t("The work plane is on the level already").into(), error: false };
        } else if self.run("work_plane", json!({})).is_some() {
            self.status = Status { text: t("Work plane back to the level").into(), error: false };
        }
    }

    /// Where `end` shows on screen in the project's 3D view as last drawn.
    #[cfg(test)]
    pub(crate) fn triad_at(&self, end: End) -> Option<Pos2> {
        self.orbit.triad_at(end, self.nav_corner(self.orbit.rect))
    }

    /// A point on the project's triad's square that no end is near, for a click on its empty space.
    #[cfg(test)]
    pub(crate) fn triad_empty(&self) -> Option<Pos2> {
        self.orbit.triad_empty(self.nav_corner(self.orbit.rect))
    }
}
