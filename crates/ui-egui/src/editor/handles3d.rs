//! A selected form's shape handles in {3D}, as in a plan or an elevation. With one extrusion selected, an
//! arrow at the middle of each edge of its profile at mid-depth, pointing out of the solid, and one on its End and its Start
//! at the profile's middle, pointing out along the plane's normal; with a Blend, its base's edges at its base and its First and
//! Second End. A drag moves that side (`drag_edge_to`, the plans' own) or that end along the arrow, the pointer read along the
//! arrow as the view was when the button went down: live, one step of the history once the button is up; Escape, Undo, Redo
//! and another view shown give it up, the form put back. A greyed handle refuses the drag saying why, and the view doesn't turn.
//!
//! An arrow the view sees end-on (its way within about 75 degrees of the way the view looks) can't be dragged along: it isn't
//! drawn, as a plan draws no handle on a side it sees flat. Revolves, sweeps and swept blends have no handles in any view:
//! their shapes are their profiles' and paths', changed by sketching them again.
use super::handle_params::{Drive, Target};
use super::shape_handles::ShapeHandle;
use super::sketch::reader;
use super::*;
use bimcraft_core::family::forms::FormKind;
use bimcraft_core::family::handles as hs;
use bimcraft_core::family::sketch::{FormCurve, FormLine};
use bimcraft_core::geom::P2;

/// How near a handle, in points, a press takes it: the plans' reach.
pub(crate) const REACH: f32 = 10.0;
/// How long an arrow is on screen, in points.
pub(crate) const ARROW: f32 = 11.0;
/// The least an arrow's way may show on screen, against a way across the view: less, and the view sees it end-on.
const SEEN: f32 = 0.25;

/// What a handle in {3D} drags: a side of the profile, or the End (true) or the Start.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Grab {
    Edge(ShapeHandle),
    End(bool),
}

/// A handle in {3D}: where it is in the family's axes, the way it points (and drags) there, what it drags, and why it can't be
/// dragged, if it can't (it is drawn greyed).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Handle3d {
    pub(crate) at: [f64; 3],
    pub(crate) way: [f64; 3],
    pub(crate) grab: Grab,
    pub(crate) blocked: Option<String>,
    /// The parameters its drag goes through, where its place is theirs (`handle_params.rs`).
    pub(crate) param: Option<Vec<String>>,
}

/// A handle held in {3D}: where the button went down, the points on screen one millimetre along the handle's way makes as the
/// view was then, and for an End or Start, its number before and the form's tag. A greyed handle is held too, doing nothing,
/// so the drag doesn't turn the view.
#[derive(Clone)]
pub(crate) struct Drag3d {
    press: egui::Pos2,
    /// Where the handle showed as the button went down.
    from: egui::Pos2,
    per_mm: egui::Vec2,
    end: Option<(bool, String, u64)>,
    live: bool,
    last: f64,
    /// The last pass {3D} was drawn in: shown no longer, the drag is given up.
    pub(super) seen: u64,
}

fn add(a: [f64; 3], b: [f64; 3], k: f64) -> [f64; 3] {
    [a[0] + b[0] * k, a[1] + b[1] * k, a[2] + b[2] * k]
}

/// The middle of `lines`' points (their starts, a circle's centre), read by `num`.
fn middle(lines: &[FormLine], num: &mut dyn FnMut(&Expr) -> Result<f64, String>) -> Option<P2> {
    let pts: Vec<P2> = lines
        .iter()
        .filter(|l| !l.axis)
        .map(|l| match &l.curve {
            FormCurve::Line { start, .. } => Some(P2(num(&start[0]).ok()?, num(&start[1]).ok()?)),
            FormCurve::Circle { centre, .. } => Some(P2(num(&centre[0]).ok()?, num(&centre[1]).ok()?)),
        })
        .collect::<Option<_>>()?;
    (!pts.is_empty()).then(|| bimcraft_core::geom::centroid(&pts))
}

impl FamilyEditor {
    /// The selected extrusion's (or Blend's) handles in {3D}, as the type previewed flexes it; none while a tool is in hand.
    pub(crate) fn handles_3d(&self) -> Vec<Handle3d> {
        if !self.handles_shown() {
            return Vec::new();
        }
        let Some(i) = self.selected_form() else { return Vec::new() };
        // A form the view hides (Hide in View, Temporary Hide/Isolate): no handles.
        if self.temp_hidden_form(i) {
            return Vec::new();
        }
        let Ok(drawn) = self.family() else { return Vec::new() };
        let Some(form) = drawn.forms.get(i) else { return Vec::new() };
        let kind = form.kind();
        if !matches!(kind, FormKind::Extrusion | FormKind::Blend) || !form.mesh.is_empty() {
            return Vec::new();
        }
        let values = self.values();
        let shown = drawn.flexed(&values).map(std::borrow::Cow::into_owned).unwrap_or_else(|_| drawn.clone());
        let flexed = if form.guid.is_nil() { shown.forms.get(i) } else { shown.forms.iter().find(|f| f.guid == form.guid) };
        let Some(flexed) = flexed else { return Vec::new() };
        let Ok(frame) = shown.frame(&flexed.host) else { return Vec::new() };
        let mut num = reader(&values);
        let (Ok(from), Ok(to)) = (num(&flexed.from), num(&flexed.to)) else { return Vec::new() };
        let mut out = Vec::new();
        // Tied to a parameter, a drag goes through it; a lock or EQ holding it, no handle.
        let key = self.drive_key(&drawn);
        let drive = |target| match self.drive(key, &drawn, i, target) {
            Drive::Free => Some((None, None)),
            Drive::Param(names) => Some((None, Some(names))),
            Drive::Refused(why) => Some((Some(why), None)),
            Drive::Hidden => None,
        };
        // The sides: an extrusion's at mid-depth, a Blend's base's at its base.
        let depth = if kind == FormKind::Extrusion { (from + to) / 2.0 } else { from };
        for h in hs::edge_handles(&flexed.sketch, &mut num) {
            let way: [f64; 3] = std::array::from_fn(|k| frame.x[k] * h.out.0 + frame.y[k] * h.out.1);
            let Some((blocked, param)) = drive(Target::Edge(h.line, h.out)) else { continue };
            let at = frame.point(h.at, depth);
            let edge = ShapeHandle { at: h.at, way, line: h.line, out: h.out, blocked: blocked.clone(), param: param.clone() };
            out.push(Handle3d { at, way, grab: Grab::Edge(edge), blocked, param });
        }
        // The End and the Start, at the middle of the profile there (a Blend's top at its End).
        let base = middle(&flexed.sketch, &mut num);
        let top = if flexed.top.is_empty() { base } else { middle(&flexed.top, &mut num) };
        for (end, at, mid) in [(false, from, base), (true, to, top)] {
            let Some(mid) = mid else { continue };
            let Some((blocked, param)) = drive(Target::End(end)) else { continue };
            let k = if end { 1.0 } else { -1.0 };
            out.push(Handle3d { at: frame.point(mid, at), way: frame.z.map(|z| z * k), grab: Grab::End(end), blocked, param });
        }
        out
    }

    /// The handles in {3D} where `project` shows them (through `cam`), with the way each points on screen; those the view sees
    /// end-on, or on a face turned away from it, left out.
    pub(crate) fn handles_3d_on(
        &self,
        project: &dyn Fn([f64; 3]) -> Option<egui::Pos2>,
        cam: &bimcraft_core::view3d::Camera,
    ) -> Vec<(egui::Pos2, egui::Vec2, Handle3d)> {
        // A millimetre, as the view is scaled: what a way across the view shows as.
        let k = 40.0 / cam.scale.max(1e-9);
        self.handles_3d()
            .into_iter()
            .filter_map(|h| {
                // Its face turned away: the way out goes further from the eye (the projection's depth grows toward it).
                let depth = |q: [f64; 3]| cam.project(q).2;
                if depth(add(h.at, h.way, k)) < depth(h.at) {
                    return None;
                }
                let at = project(h.at)?;
                let along = project(add(h.at, h.way, k))? - at;
                // Across: the longer of two ways square to the arrow, as the view shows them.
                let across = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
                    .into_iter()
                    .map(|a: [f64; 3]| {
                        let d = a[0] * h.way[0] + a[1] * h.way[1] + a[2] * h.way[2];
                        add(a, h.way, -d)
                    })
                    .filter_map(|a| {
                        let n = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
                        (n > 1e-6).then(|| project(add(h.at, a, k / n)).map(|p| (p - at).length()))?
                    })
                    .fold(0.0_f32, f32::max);
                (along.length() > SEEN * across && along.length() > 1e-3).then(|| (at, along / k as f32, h))
            })
            .collect()
    }

    /// A press at `press` on handle `h`, which `per_mm` (points on screen a millimetre along it makes) says how to read the
    /// pointer along, in the pass `seen`: held. A greyed one is held doing nothing, saying why.
    pub(crate) fn grab_3d(
        &mut self,
        h: &Handle3d,
        (press, from): (egui::Pos2, egui::Pos2),
        per_mm: egui::Vec2,
        seen: u64,
    ) -> Result<(), String> {
        let mut drag = Drag3d { press, from, per_mm, end: None, live: false, last: 0.0, seen };
        let result = match (&h.blocked, &h.grab) {
            (Some(why), _) => Err(why.clone()),
            (None, Grab::Edge(sh)) => {
                let values = self.values();
                let shown = self.family().map(|f| f.flexed(&values).map(std::borrow::Cow::into_owned).unwrap_or(f));
                match shown.and_then(|f| self.edge_drag(sh, (None, None, seen), &f)) {
                    Ok(d) => {
                        self.views.edge_drag = Some(d);
                        // Through a parameter: its drag begun, else why none can move the side.
                        let through = match (&sh.param, self.selected_form()) {
                            (Some(names), Some(i)) => self.start_param_drag(i, Target::Edge(sh.line, sh.out), names, false),
                            _ => Ok(()),
                        };
                        if through.is_err() {
                            self.views.edge_drag = None;
                        }
                        drag.live = through.is_ok();
                        through
                    }
                    Err(e) => Err(e),
                }
            }
            (None, Grab::End(end)) if h.param.is_some() => {
                let names = h.param.clone().unwrap_or_default();
                let started = match self.selected_form() {
                    Some(i) => self.start_param_drag(i, Target::End(*end), &names, false),
                    None => Err(t("Select the form whose end to drag.").to_string()),
                };
                drag.live = started.is_ok();
                started
            }
            (None, Grab::End(end)) => match self.selected_form().and_then(|i| self.forms.get(i)) {
                Some(ft) => {
                    drag.end = Some((*end, if *end { ft.to.clone() } else { ft.from.clone() }, ft.tag));
                    drag.live = true;
                    Ok(())
                }
                None => Err(String::new()),
            },
        };
        // What it can line up with along its arrow, as near as the plans take it.
        let scale = f64::from(per_mm.length());
        if drag.live && scale > 1e-9 {
            let target = match &h.grab {
                Grab::Edge(sh) => Target::Edge(sh.line, sh.out),
                Grab::End(end) => Target::End(*end),
            };
            self.begin_snap(target, h.way, f64::from(super::shape_handles::SNAP) / scale, h.param.is_none());
        }
        self.views.drag3d = Some(drag);
        result
    }

    /// The handle held in {3D}, the pointer at `now`: how far along its arrow it has gone, kept, and what the drag would make
    /// said in the status bar (a parameter's value found as it goes); nothing is written until the button is up, so the view
    /// isn't made again under the hand. A parameter's value refused says why.
    pub(crate) fn drag_3d_to(&mut self, now: egui::Pos2) -> Result<(), String> {
        let param = self.dragging_param();
        let Some((press, per_mm, last)) = self.views.drag3d.as_ref().filter(|d| d.live).map(|d| (d.press, d.per_mm, d.last)) else {
            return Ok(());
        };
        let n = per_mm.length_sq();
        if n <= 1e-12 {
            return Ok(());
        }
        // Up to something that faces its way, it lines up with it.
        let d = self.snap_to(f64::from((now - press).dot(per_mm) / n));
        if !d.is_finite() || (d - last).abs() < 1e-9 {
            return Ok(());
        }
        let Some(drag) = self.views.drag3d.as_mut() else { return Ok(()) };
        drag.last = d;
        let end = drag.end.clone();
        if param {
            let done = self.param_to(d);
            if done.is_err() {
                self.snap_refused();
            }
            return done;
        }
        let said = match end {
            Some((true, ..)) => tf("{} = {}", &[t("Extrusion End"), &number_text(self.end_after(true, d).unwrap_or(d))]),
            Some((false, ..)) => tf("{} = {}", &[t("Extrusion Start"), &number_text(self.end_after(false, d).unwrap_or(d))]),
            // Said to a hundredth of a millimetre: a pointer read through {3D} lands a hair off a round number.
            None => tf("Moved {} mm", &[&number_text((d * 100.0).round() / 100.0)]),
        };
        self.views.said = Some(if self.lined_up() { format!("{said} {}", t("Aligned.")) } else { said });
        Ok(())
    }

    /// The End's (or Start's) number held in {3D} after a drag `d` along its arrow.
    fn end_after(&self, end: bool, d: f64) -> Option<f64> {
        let (_, before, _) = self.views.drag3d.as_ref()?.end.clone()?;
        let x = before.trim().parse::<f64>().ok().filter(|x| x.is_finite())?;
        Some(x + if end { d } else { -d })
    }

    /// Where the handle held in {3D} has been dragged to on screen, and the way it points: what the view draws following the
    /// pointer.
    pub(crate) fn held_3d(&self) -> Option<(egui::Pos2, egui::Vec2)> {
        let d = self.views.drag3d.as_ref().filter(|d| d.live)?;
        Some((d.from + d.per_mm * d.last as f32, d.per_mm.normalized()))
    }

    /// The handle held in {3D} let go: the drag written, the side, the end or the parameter where the hand left it, one step of
    /// the history; refused, as it was, saying why.
    pub(crate) fn let_go_3d(&mut self) -> Result<(), String> {
        let Some(d) = self.views.drag3d.take() else { return Ok(()) };
        // Where it was let go is where it lined up: nothing more to take it to.
        self.views.handle_snap = None;
        if !d.live {
            self.views.edge_drag = None;
            return Ok(());
        }
        if self.views.param_drag.is_some() {
            self.views.edge_drag = None;
            self.let_param_go();
            return Ok(());
        }
        let done = match d.end.clone() {
            None => {
                let way = self.views.edge_drag.as_ref().map(super::shape_handles::EdgeDrag::way);
                match way {
                    Some(way) if d.last.abs() > 1e-9 => self.drag_edge_to(way.map(|w| w * d.last)),
                    _ => Ok(()),
                }
            }
            Some((end, before, tag)) => {
                let after = before.trim().parse::<f64>().ok().filter(|x| x.is_finite()).map(|x| x + if end { d.last } else { -d.last });
                match (after, self.forms.iter_mut().find(|f| f.tag == tag)) {
                    (_, None) => Ok(()),
                    (Some(x), Some(ft)) => {
                        if d.last.abs() > 1e-9 {
                            *(if end { &mut ft.to } else { &mut ft.from }) = number_text(x);
                        }
                        Ok(())
                    }
                    (None, Some(_)) => {
                        Err(t("This end is tied to a parameter or worked out by a formula: change it in Family Types.").to_string())
                    }
                }
            }
        };
        self.views.edge_drag = None;
        done
    }

    /// The handle held in {3D} given up: nothing was written. Whether there was one.
    pub(super) fn cancel_3d(&mut self) -> bool {
        self.views.handle_snap = None;
        self.views.drag3d.take().is_some()
    }

    /// Whether a handle is held in {3D}, and so the view doesn't turn.
    pub(crate) fn holding_3d(&self) -> bool {
        self.views.drag3d.is_some()
    }

    /// The pass a handle held in {3D} was last drawn in, kept up; a drag of another view's given up.
    pub(crate) fn keep_3d(&mut self, pass: u64) {
        if self.views.drag3d.as_ref().is_some_and(|d| d.seen + 1 < pass)
            || self.views.edge_drag.as_ref().is_some_and(|d| d.which.is_some())
            || self.views.end_drag.is_some()
            || self.views.moving.is_some()
        {
            self.cancel_drags();
        }
        if let Some(d) = self.views.drag3d.as_mut() {
            d.seen = pass;
        }
    }
}

/// A handle's arrow at `centre` pointing `way` (on screen, a unit): blue where it can be dragged, grey where it can't, and lit
/// (larger, the hover colour) under the pointer that would take it.
pub(crate) fn paint_arrow(painter: &egui::Painter, centre: egui::Pos2, way: egui::Vec2, free: bool, lit: bool) {
    let k = if lit { 1.4 } else { 1.0 };
    let side = egui::vec2(-way.y, way.x);
    let tip = |grow: f32| {
        let back = centre - way * grow;
        vec![centre + way * (ARROW * k + grow * 1.6), back + side * (7.0 * k + grow * 1.6), back - side * (7.0 * k + grow * 1.6)]
    };
    let colour = match (free, lit) {
        (true, true) => crate::style::canvas_pal().hover,
        (true, false) => super::views::PICKED,
        (false, _) => pal().muted,
    };
    // A rim of the canvas's opposite under it, so it reads over a shaded form as over paper: dark on a light canvas, light
    // on a dark one.
    let rim = if crate::style::canvas_dark() { Color32::from_gray(235) } else { Color32::from_gray(25) };
    painter.add(egui::Shape::convex_polygon(tip(1.5), rim, egui::Stroke::NONE));
    painter.add(egui::Shape::convex_polygon(tip(0.0), colour, egui::Stroke::new(1.0, Color32::WHITE)));
}
