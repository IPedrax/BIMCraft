//! Snapping on the plan: where a point picked at the pointer lands. The core does the finding (`snap::snap`: what the plan has there,
//! a guide at an angle from the last point picked, else the nearest 10 mm; `AlignmentGuideService`: the temporary guides that line a
//! point up with others). This is what a front end adds, as egui's canvas does: how far the pointer reaches in pixels, which guides are
//! asked for, and the guide the last pick held, which is left only at one and a half times the distance it was entered at.
//!
//! Not here yet, from egui's canvas: the lines of a view's underlay, a camera's eye, a sketch's own lines, the background grid, Tab's
//! wall face, and the references a rest on a point marks. So the guides come from the points picked so far and, with
//! `Snaps::guides_from_whole_model`, from the whole plan; egui's default also takes the wall the chain began on and the grids near.

use bimcraft_core::alignment::{AlignmentAxis, AlignmentConfig, AlignmentGuideService, AlignmentResult, GuideContext, GuideSource};
use bimcraft_core::geom::P2;
use bimcraft_core::ids::Id;
use bimcraft_core::model::Model;
use bimcraft_core::render::View;
use bimcraft_core::snap::{self, Found, Kind, Reach, Snaps};

use crate::lang::{t, tf};

/// How near the pointer a snap reaches, pixels: to a point, and to a line (egui's `SNAP` and `PICK`).
const POINT_REACH: f64 = 10.0;
const LINE_REACH: f64 = 6.0;
/// How much farther out a guide the last pick held still holds the point than one that is entered: 12 px to leave, 8 to enter.
const HELD: f64 = 1.5;

/// Where a pick at the pointer lands, and what holds it there.
#[derive(Clone, Debug, PartialEq)]
pub struct Landing {
    /// What the core's snap found.
    pub found: Found,
    /// The temporary guides that hold the point, the first leading.
    pub guides: Vec<AlignmentResult>,
    /// Where a click puts the point: on the first guide, else the snap's own.
    pub at: P2,
}

impl Landing {
    /// What the point lands on, in the interface's words: the snap's name, or the guides that hold it (a free point or an angle step
    /// has its own marker and needs none).
    pub fn tag(&self) -> Option<String> {
        match self.guides.as_slice() {
            [] if matches!(self.found.kind, Kind::Free | Kind::Angle) => None,
            [] => Some(t(self.found.kind.name()).to_string()),
            [one] => Some(t(one.axis.name()).to_string()),
            [a, b, ..] => Some(tf("Intersection: {}, {}", &[t(a.axis.name()), t(b.axis.name())])),
        }
    }
}

/// The guides found across a whole plan, kept while the model and the switches stay as they were.
struct Whole {
    level: Id,
    config: AlignmentConfig,
    service: AlignmentGuideService,
}

#[derive(Default)]
pub struct Snapper {
    /// The snaps in force (egui's Manage > Snaps): all on until a dialog changes them.
    pub snaps: Snaps,
    /// The guide the last pick held, so leaving it takes more than entering.
    held: Option<AlignmentAxis>,
    whole: Option<Whole>,
}

impl Snapper {
    /// The model changed: what was found across the plan is found again, and no guide is held.
    pub fn forget(&mut self) {
        self.whole = None;
        self.held = None;
    }

    /// Where a pick at pixel `px` of `view` lands on `level`, after the points `from` already picked; `square` keeps the step from the
    /// last of them horizontal or vertical (Shift). None for a pointer or a view that is no number.
    pub fn land(&mut self, m: &Model, level: Id, view: &View, px: (f32, f32), from: &[P2], square: bool) -> Option<Landing> {
        let p = view.mm(px);
        if !(px.0.is_finite() && px.1.is_finite() && p.0.is_finite() && p.1.is_finite() && view.scale.is_finite() && view.scale > 0.0) {
            return None;
        }
        let reach = Reach { point: POINT_REACH / view.scale, line: LINE_REACH / view.scale };
        let found = snap::snap(m, level, p, reach, from, square, &self.snaps);
        // A rounded free point is up to 7 mm from the pointer, more than a guide reaches when zoomed in, so a guide is looked for from
        // the pointer; Shift's line and an angle step are already the point that was asked for.
        let at = if square || found.kind == Kind::Angle { found.at } else { p };
        let guides = self.align(m, level, view, &found, at, from);
        let at = guides.first().map_or(found.at, |a| a.snapped_point);
        Some(Landing { found, guides, at })
    }

    /// The guides a point that landed as `f` takes: a free point any, a point on an angle guide one that keeps it on that guide, a
    /// point on a line only a crossing of two.
    fn align(&mut self, m: &Model, level: Id, view: &View, f: &Found, at: P2, from: &[P2]) -> Vec<AlignmentResult> {
        let o = self.snaps;
        let crossing_only = f.kind == Kind::Nearest;
        if !o.alignment_guides || !(matches!(f.kind, Kind::Free | Kind::Angle) || crossing_only) {
            return Vec::new();
        }
        let config =
            AlignmentConfig { enabled: true, endpoints: o.endpoint, midpoints: o.midpoint, faces: true, intersections: o.intersection };
        let (tol, start, held) = (f64::from(o.alignment_tolerance) / view.scale, from.last().copied(), self.held);
        let find = |s: &AlignmentGuideService| {
            let found = s.find_alignments(at, start, tol);
            match held {
                Some(held) if found.is_empty() => s.find_alignments(at, start, tol * HELD).into_iter().filter(|a| a.axis == held).collect(),
                _ => found,
            }
        };
        let found = if o.guides_from_whole_model {
            if self.whole.as_ref().is_none_or(|w| w.level != level || w.config != config) {
                self.whole = Some(Whole { level, config, service: AlignmentGuideService::from_model(m, level, config) });
            }
            self.whole.as_ref().map(|w| find(&w.service)).unwrap_or_default()
        } else {
            find(&AlignmentGuideService::from_context(&context(from), config))
        };
        let on_guide = |p: P2| {
            f.guide.is_none_or(|g| {
                let (u, d) = (P2(g.angle.to_radians().cos(), g.angle.to_radians().sin()), p - g.from);
                (u.0 * d.1 - u.1 * d.0).abs() <= 0.5
            })
        };
        let mut found: Vec<AlignmentResult> = found.into_iter().filter(|a| on_guide(a.snapped_point)).collect();
        if crossing_only && found.len() < 2 {
            found.clear();
        }
        // The held guide is one that was drawn, not one the filter took away.
        self.held = found.first().map(|a| a.axis);
        found
    }
}

/// What the drawing's own points lend the guides: its chain, and the segment just placed (a direction to be parallel or square to).
fn context(from: &[P2]) -> GuideContext {
    let mut ctx = GuideContext { chain: from.to_vec(), ..GuideContext::default() };
    if let [.., a, b] = from {
        ctx.lines.push(GuideSource { a: *a, b: *b, id: None });
    }
    ctx
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;

    /// The sample house, its first level, and the view its canvas has.
    fn house() -> (App, Id, View) {
        let app = App::sample();
        let level = app.level.expect("a level");
        let view = app.camera.view(app.size.0, app.size.1);
        (app, level, view)
    }

    #[test]
    fn a_pointer_near_a_wall_corner_lands_on_what_the_plan_has_there() {
        let (app, level, view) = house();
        let corner = P2(10000.0, 7000.0);
        let (x, y) = view.px(corner);
        let l = Snapper::default().land(&app.session.model, level, &view, (x + 3.0, y - 2.0), &[], false).expect("a landing");
        assert_ne!(l.found.kind, Kind::Free, "{:?}", l.found);
        assert!(l.at.dist(corner) < 400.0, "the corner or a face's corner beside it: {:?}", l.at);
        assert!(l.tag().is_some(), "a snap on the plan has a name");
    }

    #[test]
    fn far_from_everything_a_point_rounds_to_ten_millimetres_and_has_no_name() {
        let (app, level, view) = house();
        let px = view.px(P2(-30003.0, -30004.0));
        let l = Snapper::default().land(&app.session.model, level, &view, px, &[], false).expect("a landing");
        assert_eq!(l.found.kind, Kind::Free);
        assert!(l.at.dist(P2(-30000.0, -30000.0)) < 0.1, "{:?}", l.at);
        assert!(l.guides.is_empty() && l.tag().is_none());
    }

    #[test]
    fn a_direction_near_an_angle_step_from_the_last_point_lands_on_the_step() {
        let (app, level, view) = house();
        let from = P2(-30000.0, -30000.0);
        let px = view.px(P2(-25000.0, -29960.0));
        let l = Snapper::default().land(&app.session.model, level, &view, px, &[from], false).expect("a landing");
        assert_eq!(l.found.kind, Kind::Angle, "{:?}", l.found);
        assert!((l.at.1 - from.1).abs() < 0.5, "straight east of the last point: {:?}", l.at);
        assert!(l.found.guide.is_some_and(|g| g.angle.abs() < 1e-6), "{:?}", l.found.guide);
    }

    #[test]
    fn with_the_whole_plan_a_free_point_lines_up_with_a_wall_end_and_is_held_a_little_longer() {
        let (app, level, view) = house();
        let (m, (x0, y0)) = (&app.session.model, view.px(P2(6000.0, 12000.0)));
        let mut s = Snapper::default();
        s.snaps.guides_from_whole_model = true;
        let l = s.land(m, level, &view, (x0 + 2.0, y0), &[], false).expect("a landing");
        assert_eq!(l.found.kind, Kind::Free, "{:?}", l.found);
        assert!(matches!(l.guides.first().map(|g| g.axis), Some(AlignmentAxis::X)), "a vertical guide: {:?}", l.guides);
        assert!((l.at.0 - 6000.0).abs() < 100.0, "the point is on a wall end's or face's line: {:?}", l.at);
        assert_eq!(l.tag().as_deref(), Some("Vertical"));
        // Entered within 8 px, held to 12: 10 px out it still holds this pick, but not a fresh one.
        assert!(!s.land(m, level, &view, (x0 + 10.0, y0), &[], false).expect("a landing").guides.is_empty(), "held");
        assert!(s.land(m, level, &view, (x0 + 14.0, y0), &[], false).expect("a landing").guides.is_empty(), "left");
        let mut fresh = Snapper { snaps: s.snaps, ..Snapper::default() };
        assert!(fresh.land(m, level, &view, (x0 + 10.0, y0), &[], false).expect("a landing").guides.is_empty(), "not entered");
    }

    #[test]
    fn guides_can_be_switched_off_and_the_cache_is_rebuilt_after_a_change() {
        let (app, level, view) = house();
        let (m, (x0, y0)) = (&app.session.model, view.px(P2(6000.0, 12000.0)));
        let mut s = Snapper::default();
        s.snaps.guides_from_whole_model = true;
        assert!(!s.land(m, level, &view, (x0, y0), &[], false).expect("a landing").guides.is_empty());
        s.forget();
        assert!(s.whole.is_none() && s.held.is_none());
        assert!(!s.land(m, level, &view, (x0, y0), &[], false).expect("a landing").guides.is_empty(), "found again");
        s.snaps.alignment_guides = false;
        assert!(s.land(m, level, &view, (x0, y0), &[], false).expect("a landing").guides.is_empty(), "off");
    }

    #[test]
    fn a_pointer_or_a_view_that_is_no_number_lands_nowhere() {
        let (app, level, view) = house();
        let m = &app.session.model;
        let mut s = Snapper::default();
        for px in [(f32::NAN, 1.0), (1.0, f32::INFINITY)] {
            assert!(s.land(m, level, &view, px, &[], false).is_none());
        }
        for scale in [0.0, -1.0, f64::NAN] {
            assert!(s.land(m, level, &View { scale, ..view }, (10.0, 10.0), &[], false).is_none());
        }
    }
}
