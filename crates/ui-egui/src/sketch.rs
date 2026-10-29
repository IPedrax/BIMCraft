//! Sketch mode: Revit's Edit Boundary, and the Boundary Line of a new floor, ceiling, roof or shaft. The outline is drawn as
//! loose lines and arcs, held here until Finish Edit Mode hands them to the core, which solves them
//! (`bimcraft_core::sketch`) or names what's wrong; the lines it names are drawn in the error colour until the sketch
//! changes. A line the element kept from its last sketch keeps its id and GUID; a new one gets them at Finish.

use bimcraft_i18n::msg;

use std::collections::BTreeSet;

use bimcraft_core::geom::{self, P2};
use bimcraft_core::model::{Element, Id, Line, Model, TOL};
use bimcraft_core::render::View;
use bimcraft_core::sketch::{self as solver, Problem, SketchLine};
use bimcraft_core::units::Units;
use egui::{Align2, Color32, Modifiers, Painter, PointerButton, Pos2, Rect, Response, Shape, Stroke, StrokeKind, Ui, vec2};
use serde_json::json;

use crate::canvas::{box_shapes, tag};
use crate::icons::Icon;
use crate::lang::{t, tf};
use crate::properties::number;
use crate::style::{canvas_pal, pal};
use crate::{BimApp, Sheet, Status, Tool, big_button, draw, panel, ribbon, small_button};

/// What Finish Edit Mode makes of the lines.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Of {
    /// A new floor on the level.
    Floor(Id),
    /// A new ceiling on the level.
    Ceiling(Id),
    /// A new roof by footprint, on the level above this one.
    Roof(Id),
    /// A new shaft opening from the level.
    Shaft(Id),
    /// A new filled region in the view (a plan's key), or a masking region: any number of outlines.
    Region { view: Id, masking: bool },
    /// The element's outline and holes, redrawn.
    Edit(Id),
}

/// A drag with Modify while sketching: a grip at a line's end, or a selection box from a pixel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Drag {
    Grip(P2),
    Box((f32, f32)),
}

/// A sketch being drawn: its lines, those picked (Modify, for Delete and the grips), those the last Finish found wrong, and
/// the drag under way.
#[derive(Clone, Debug, PartialEq)]
pub struct Sketch {
    pub of: Of,
    pub lines: Vec<SketchLine>,
    pub picked: BTreeSet<usize>,
    pub wrong: Vec<usize>,
    pub drag: Option<Drag>,
    /// A new region's type and its boundary's line style (none, invisible), as the Options Bar sets them.
    pub region: (String, Option<String>),
    /// A region's sketch: each line drawn takes the Options Bar's line style.
    pub styled: bool,
    /// The plan it began in: the sketch belongs to it, and Finish Edit Mode makes its element there.
    pub plan: Sheet,
    /// The work plane a new one begun in the 3D view lies on (`workplane.rs`): what it makes stands on the level below it.
    pub surface: Option<bimcraft_core::walls::Plane>,
    /// The lines as they were before each change, oldest first, and as the last frame saw them: Ctrl+Z takes back one change
    /// (a line drawn, a trim, a fillet, a grip moved), not one line.
    history: Vec<Vec<SketchLine>>,
    seen: Vec<SketchLine>,
}

/// How many changes of a sketch Ctrl+Z can take back.
const UNDO: usize = 100;

/// Sketch lines in the plan: Revit's magenta.
const SKETCH: Color32 = Color32::from_rgb(0xC0, 0x26, 0xD3);

/// How near a grip a press takes it, px.
const GRIP: f32 = 8.0;

impl Sketch {
    /// What the contextual tab calls it.
    pub fn title(&self) -> &'static str {
        match self.of {
            Of::Floor(_) => msg!("Create Floor Boundary"),
            Of::Ceiling(_) => msg!("Create Ceiling Boundary"),
            Of::Roof(_) => "Create Roof Footprint",
            Of::Shaft(_) => msg!("Create Shaft Opening Sketch"),
            Of::Region { masking: false, .. } => msg!("Create Filled Region Boundary"),
            Of::Region { masking: true, .. } => msg!("Create Masking Region Boundary"),
            Of::Edit(_) => "Edit Boundary",
        }
    }

    /// The line nearest `p`, within `reach` mm of it.
    pub fn line_at(&self, p: P2, reach: f64) -> Option<usize> {
        self.lines
            .iter()
            .enumerate()
            .map(|(i, l)| (i, l.dist(p)))
            .filter(|(_, d)| *d <= reach)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// The nearest actual straight segment, including one of a polyline: Align's candidate.
    pub(crate) fn alignment_at(&self, p: P2, reach: f64) -> Option<(usize, Line, f64)> {
        self.lines
            .iter()
            .enumerate()
            .flat_map(|(i, l)| {
                bimcraft_core::align::curve_lines(&l.curve).into_iter().map(move |line| (i, line, geom::dist_to_segment(p, line.0, line.1)))
            })
            .filter(|(_, _, d)| *d <= reach)
            .min_by(|a, b| a.2.total_cmp(&b.2))
    }

    /// The end of a picked line within `reach` mm of `p`: a grip.
    pub fn grip_at(&self, p: P2, reach: f64) -> Option<P2> {
        self.picked
            .iter()
            .filter_map(|&i| self.lines.get(i))
            .flat_map(|l| l.curve.handles())
            .filter(|q| q.dist(p) <= reach)
            .min_by(|a, b| a.dist(p).total_cmp(&b.dist(p)))
    }

    /// Every line end at `from` moved to `to`: the lines joined there stay joined.
    pub fn move_end(&mut self, from: P2, to: P2) {
        for l in &mut self.lines {
            l.curve = l.curve.moved(from, to);
        }
        self.wrong.clear();
    }

    /// The lines a box between pixels `a` and `b` of `view` picks: from the left, those wholly inside; from the right, those
    /// it touches too.
    pub fn in_box(&self, view: &View, a: (f32, f32), b: (f32, f32)) -> Vec<usize> {
        let (lo, hi) = (view.mm((a.0.min(b.0), a.1.max(b.1))), view.mm((a.0.max(b.0), a.1.min(b.1))));
        let inside = |p: P2| p.0 >= lo.0 && p.0 <= hi.0 && p.1 >= lo.1 && p.1 <= hi.1;
        let crossing = b.0 < a.0;
        let corners = [lo, P2(hi.0, lo.1), hi, P2(lo.0, hi.1)];
        let touches = |pts: &[P2]| {
            pts.iter().any(|p| inside(*p))
                || pts.windows(2).any(|w| {
                    geom::ring_edges(&corners)
                        .any(|(c, d)| geom::line_intersection(w[0], w[1], c, d).is_some_and(|x| on(x, w[0], w[1]) && on(x, c, d)))
                })
        };
        self.lines
            .iter()
            .enumerate()
            .filter(|(_, l)| {
                let pts = l.points();
                if crossing { touches(&pts) } else { pts.iter().all(|p| inside(*p)) }
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// A line added, unless it's too short to be one.
    pub(crate) fn add(&mut self, mut l: SketchLine) {
        if self.styled {
            l.invisible = self.region.1.is_none();
            l.style = self.region.1.clone();
        }
        if l.curve.check("").is_ok() {
            self.lines.push(l);
            self.changed();
        }
    }

    pub(crate) fn changed(&mut self) {
        self.picked.clear();
        self.wrong.clear();
    }

    /// Notes the lines if they are not as the last frame saw them: the change is a step Ctrl+Z takes back. True for a change.
    pub(crate) fn track(&mut self) -> bool {
        if self.lines == self.seen {
            return false;
        }
        let before = std::mem::replace(&mut self.seen, self.lines.clone());
        self.history.push(before);
        if self.history.len() > UNDO {
            self.history.remove(0);
        }
        true
    }

    /// The lines as they were before the last change, which is taken back; false when there's none to take back.
    pub(crate) fn undo(&mut self) -> bool {
        self.track();
        let Some(before) = self.history.pop() else { return false };
        self.lines.clone_from(&before);
        self.seen = before;
        self.changed();
        true
    }

    /// The lines, each in its colour: picked (or `lit`, a tool's picks) or under the cursor as a selection is, wrong in the error colour; a picked
    /// line's grips at its ends; the line under the cursor named by its id. While a grip is dragged to `cursor`, the lines
    /// joined there follow it.
    pub fn paint(&self, p: &Painter, at: impl Fn(P2) -> Pos2, hovered: Option<usize>, cursor: Option<P2>, lit: &[usize]) {
        let moved = |q: P2| match (self.drag, cursor) {
            (Some(Drag::Grip(from)), Some(to)) if q.dist(from) <= TOL => to,
            _ => q,
        };
        for (i, l) in self.lines.iter().enumerate() {
            let (width, colour) = match () {
                () if self.picked.contains(&i) || lit.contains(&i) => (3.0, canvas_pal().accent),
                () if hovered == Some(i) => (3.0, canvas_pal().hover),
                () if self.wrong.contains(&i) => (3.0, canvas_pal().error),
                () => (2.0, SKETCH),
            };
            let shown = match (self.drag, cursor) {
                (Some(Drag::Grip(from)), Some(to)) => SketchLine { curve: l.curve.moved(from, to), ..l.clone() },
                _ => l.clone(),
            };
            p.add(Shape::line(shown.points().into_iter().map(&at).collect(), Stroke::new(width, colour)));
        }
        for l in self.picked.iter().filter_map(|&i| self.lines.get(i)) {
            for q in l.curve.handles() {
                let r = Rect::from_center_size(at(moved(q)), vec2(8.0, 8.0));
                p.rect_filled(r, 0.0, canvas_pal().surface);
                p.rect_stroke(r, 0.0, Stroke::new(1.5, pal().accent), StrokeKind::Middle);
            }
        }
        if let Some(l) = hovered.and_then(|i| self.lines.get(i).map(|l| (i, l))).map(|(i, l)| (name(l, i), l)) {
            tag(p, at(l.1.middle()) + vec2(10.0, -10.0), Align2::LEFT_BOTTOM, l.0);
        }
    }
}

/// Whether `x`, on the line through `a` and `b`, lies between them.
fn on(x: P2, a: P2, b: P2) -> bool {
    geom::dist_to_segment(x, a, b) <= 1e-6 * a.dist(b).max(1.0)
}

/// What a line is called to whoever draws it: by its id once it has one, else new.
fn name(l: &SketchLine, i: usize) -> String {
    if l.id != 0 { tf("Line {}", &[&l.id.to_string()]) } else { tf("New line {}", &[&(i + 1).to_string()]) }
}

/// What's wrong with the lines, said for whoever drew them: what to fix, kind by kind, the first places it is, in the
/// project's units, and the lines by name.
pub fn friendly(problems: &[Problem], lines: &[SketchLine], units: Units) -> String {
    let named = |ls: &[usize]| {
        let names: Vec<String> = ls.iter().filter_map(|&i| lines.get(i).map(|l| name(l, i))).collect();
        names.join(", ")
    };
    let place = |p: &Problem| match p.at() {
        Some(q) => tf("{} at x {}, y {}", &[&named(&p.lines()), &units.with_unit(q.0), &units.with_unit(q.1)]),
        None => named(&p.lines()),
    };
    let mut out: Vec<String> = Vec::new();
    let mut kinds: Vec<u8> = problems.iter().map(Problem::rank).collect();
    kinds.dedup();
    for rank in kinds {
        let these: Vec<&Problem> = problems.iter().filter(|p| p.rank() == rank).collect();
        let Some(first) = these.first() else { continue };
        let n = these.len().to_string();
        let headline = match first {
            Problem::Empty => t("There are no lines yet: draw the outline.").to_string(),
            Problem::TooMany { .. } => t("The sketch has too many lines.").to_string(),
            Problem::OutOfRange { .. } => t("A line has a point too far out to draw.").to_string(),
            Problem::TooShort { .. } => t("A line is too short: its ends are 1 mm apart or less.").to_string(),
            Problem::Malformed { .. } => t("A line isn't one that can be drawn: two of its points lie on each other.").to_string(),
            Problem::Cross { .. } => tf("Lines intersect ({}): a loop can't cross itself or another loop.", &[&n]),
            Problem::Overlap { .. } => tf("Lines overlap ({}): one lies along another, or a line is drawn twice.", &[&n]),
            Problem::Branch { .. } => tf("Three or more lines meet at a point ({}): a loop passes each point once.", &[&n]),
            Problem::Open { .. } => tf("The loop is open: {} line ends don't meet another line.", &[&n]),
            Problem::NoArea { .. } => t("A loop encloses no area.").to_string(),
            Problem::Separate { count } => {
                tf("Draw one outline, with any openings inside it: these lines make {} separate outlines.", &[&count.to_string()])
            }
        };
        out.push(headline);
        let places: Vec<String> = these.iter().take(3).map(|p| place(p)).filter(|s| !s.is_empty()).collect();
        if !places.is_empty() {
            let more = these.len().saturating_sub(places.len());
            let mut line = places.join("; ");
            if more > 0 {
                line = tf("{}; and {} more", &[&line, &more.to_string()]);
            }
            out.push(format!("  {line}"));
        }
    }
    if problems.iter().any(|p| !p.lines().is_empty()) {
        out.push(t("The lines are shown in red.").to_string());
    }
    out.join("\n")
}

impl BimApp {
    /// Sketch mode on: the lines given (none for a new outline), drawing them with Line.
    pub(crate) fn start_sketch(&mut self, of: Of, lines: Vec<SketchLine>) {
        // A filled region in Solid Black and Thin Lines; a masking one in Masking, its lines unseen (or the first of each).
        let m = &self.session.model;
        let first = |want: &str, names: Vec<&String>| {
            let w = m.language.word(want);
            names.iter().find(|n| n.as_str() == w).or(names.first()).map(|n| n.to_string()).unwrap_or_default()
        };
        let region = match of {
            Of::Region { masking: true, .. } => {
                let masks: Vec<&String> = m.region_types.iter().filter(|(_, t)| t.masking).map(|(n, _)| n).collect();
                let masks = if masks.is_empty() { m.region_types.keys().collect() } else { masks };
                (first(bimcraft_core::fills::MASKING, masks), None)
            }
            Of::Edit(id) if matches!(m.elements.get(id), Some(Element::FilledRegion(_))) => {
                let Some(Element::FilledRegion(r)) = m.elements.get(id) else { return };
                (r.type_name.clone(), r.line_style.clone())
            }
            _ => (
                self.types
                    .get("region")
                    .filter(|n| m.region_types.contains_key(*n))
                    .cloned()
                    .unwrap_or_else(|| first(bimcraft_core::fills::SOLID_BLACK, m.region_types.keys().collect())),
                bimcraft_core::lines::default_style(&m.line_styles, m.language).cloned(),
            ),
        };
        let styled = match of {
            Of::Region { .. } => true,
            Of::Edit(id) => matches!(m.elements.get(id), Some(Element::FilledRegion(_))),
            _ => false,
        };
        let seen = lines.clone();
        let plan = self.sheet;
        self.sketch = Some(Sketch {
            of,
            lines,
            picked: BTreeSet::new(),
            wrong: Vec::new(),
            drag: None,
            region,
            styled,
            plan,
            surface: None,
            history: Vec::new(),
            seen,
        });
        self.forget_picks();
        self.selection.clear();
        self.status = Status::default();
        self.tool = Tool::SketchLine(Vec::new());
        self.raise_context();
    }

    /// Revit's Edit Boundary on the floor, ceiling, roof or shaft selected: its outline and holes as lines to edit, those it
    /// kept from its last sketch with their ids.
    pub(crate) fn edit_boundary(&mut self) {
        let &[id] = self.selection.as_slice() else { return };
        if let Some(r) = self.run("edit_boundary", json!({"id": id}))
            && let Ok(lines) = serde_json::from_value::<Vec<SketchLine>>(r["sketch"].clone())
        {
            self.start_sketch(Of::Edit(id), lines);
        }
    }

    /// The one element selected, when it has an outline Edit Boundary redraws.
    pub(crate) fn boundary_selected(&self) -> bool {
        match self.selection.as_slice() {
            [id] => match self.session.model.elements.get(*id) {
                Some(Element::Floor(_) | Element::Ceiling(_) | Element::Shaft(_) | Element::FilledRegion(_)) => true,
                Some(Element::Roof(r)) => r.extrusion.is_none(),
                _ => false,
            },
            _ => false,
        }
    }

    /// Finish Edit Mode: the lines solved and made into what the sketch is for, selected; or what's wrong said, the lines it
    /// names marked, still sketching.
    pub(crate) fn finish_sketch(&mut self) {
        let Some(sk) = &mut self.sketch else { return };
        // A region is any number of outlines; the rest, one.
        let many = match sk.of {
            Of::Region { .. } => true,
            Of::Edit(id) => matches!(self.session.model.elements.get(id), Some(Element::FilledRegion(_))),
            _ => false,
        };
        let solved = if many { solver::solve(&sk.lines).map(|_| ()) } else { solver::solve_one(&sk.lines).map(|_| ()) };
        if let Err(problems) = solved {
            sk.wrong = problems.iter().flat_map(Problem::lines).collect();
            self.status = Status { text: friendly(&problems, &sk.lines, self.session.model.units), error: true };
            return;
        }
        let lines = json!(sk.lines.iter().map(sent).collect::<Vec<_>>());
        let m = &self.session.model;
        // Begun on the 3D view's (level) work plane: it stands on the level below the plane, as high above it as the plane is.
        if let Some(surface) = sk.surface {
            let Some((level, offset)) = bimcraft_core::workplane::host(m, surface, P2(0.0, 0.0)) else { return };
            let (id, p) = match sk.of {
                Of::Floor(_) => (
                    "floor",
                    self.typed("floor", json!({"level": level, "offset": offset, "structural": self.floor_structural, "sketch": lines})),
                ),
                Of::Ceiling(_) => ("ceiling", self.typed("ceiling", json!({"level": level, "offset": offset, "sketch": lines}))),
                Of::Roof(_) => {
                    let slope = number(&self.roof_slope).unwrap_or(30.0);
                    ("roof", self.typed("roof", json!({"level": level, "offset": offset, "slope": slope, "sketch": lines})))
                }
                Of::Shaft(_) => ("shaft", json!({"level": level, "base_offset": offset, "sketch": lines})),
                _ => return,
            };
            if let Some(made) = self.run(id, p) {
                self.sketch = None;
                self.tool = Tool::Modify;
                self.forget_picks();
                self.selection = made["id"].as_u64().into_iter().collect();
            }
            return;
        }
        let (id, p) = match sk.of {
            Of::Floor(level) => {
                ("floor", self.typed("floor", json!({"level": level, "structural": self.floor_structural, "sketch": lines})))
            }
            Of::Ceiling(level) => {
                let mut p = self.typed("ceiling", json!({"level": level, "sketch": lines}));
                if let Some(offset) = m.units.parse(&self.ceiling_height) {
                    p["offset"] = json!(offset);
                }
                ("ceiling", p)
            }
            Of::Roof(level) => {
                let on = m.level_above(level).unwrap_or(m.guid(level));
                let slope = number(&self.roof_slope).unwrap_or(30.0);
                ("roof", self.typed("roof", json!({"level": on, "slope": slope, "sketch": lines})))
            }
            Of::Shaft(level) => ("shaft", json!({"level": level, "sketch": lines})),
            Of::Region { view, .. } => {
                ("filled_region", json!({"view": view, "sketch": lines, "type": sk.region.0, "line_style": sk.region.1}))
            }
            Of::Edit(id) => ("edit_boundary", json!({"id": id, "sketch": lines})),
        };
        // Solved, it may still be refused for what it makes (a shaft with a hole): the command says why.
        if let Some(made) = self.run(id, p) {
            self.sketch = None;
            self.tool = Tool::Modify;
            // The lines picked for a fillet were the sketch's: their places mean nothing in the next one.
            self.forget_picks();
            self.selection = made["id"].as_u64().into_iter().collect();
        }
    }

    /// Cancel Edit Mode: the lines dropped, the model as it was.
    pub(crate) fn cancel_sketch(&mut self) {
        let edited = match self.sketch.take() {
            Some(Sketch { of: Of::Edit(id), .. }) => Some(id),
            _ => None,
        };
        self.tool = Tool::Modify;
        self.status = Status::default();
        self.forget_picks();
        self.selection = edited.into_iter().collect();
    }

    /// Once a frame while sketching: the changes made to the lines are steps for Ctrl+Z, and the places of lines picked for
    /// a fillet or a trim (by their number in the sketch) mean nothing once the lines have changed; and the sketch stays in
    /// the plan it began in, which it takes the window back to.
    pub(crate) fn track_sketch(&mut self) {
        let Some(sk) = &mut self.sketch else { return };
        // Multiple Trim keeps an unchanged boundary while successive targets change. Other edits
        // can remove or renumber that line, so never carry an unverified index into the next frame.
        let keep_trim = matches!(self.tool, Tool::TrimTo { many: true, .. })
            && self.drawing.trim.is_some_and(|(i, _)| sk.lines.get(i).is_some() && sk.lines.get(i) == sk.seen.get(i));
        if sk.track() {
            self.drawing.fillet.retain(|(l, _)| matches!(l, draw::Picked::Element(_)));
            if !keep_trim {
                self.drawing.trim = None;
            }
            self.drawing.align_reference = None;
            if matches!(self.tool, Tool::Align(Some(_))) {
                self.tool = Tool::Align(None);
            }
        }
        if let Some(plan) = self.sketch_plan()
            && self.sheet != plan
        {
            self.open_sheet(plan);
            self.status = Status { text: t("Finish or cancel the sketch before leaving its plan.").into(), error: true };
        }
    }

    /// The view the sketch began in, while it is there to go back to: a plan (one whose level was deleted is nowhere, and holds
    /// neither the window nor its tool), or the 3D view while what it edits has a plane there (`sketch3d.rs`).
    pub(crate) fn sketch_plan(&self) -> Option<Sheet> {
        let plan = self.sketch.as_ref()?.plan;
        let there = match plan {
            Sheet::Plan(key) | Sheet::Ceiling(key) => self.session.model.plan_level(key).is_some(),
            Sheet::View3D => self.sketch_surface().is_some(),
            _ => false,
        };
        there.then_some(plan)
    }

    /// Trim/Extend's and Fillet Arc's picks so far, lit as the lines picked are.
    pub(crate) fn sketch_lit(&self) -> Vec<usize> {
        self.drawing
            .trim
            .map(|(i, _)| i)
            .into_iter()
            .chain(self.drawing.fillet.iter().filter_map(|(l, _)| match l {
                draw::Picked::Sketch(i) => Some(*i),
                draw::Picked::Element(_) => None,
            }))
            .collect()
    }

    /// Ctrl+Z while sketching: the last change to the lines taken back (a line drawn, a trim, a fillet, a grip moved).
    pub(crate) fn unsketch(&mut self) {
        if let Some(sk) = &mut self.sketch
            && sk.undo()
        {
            self.drawing.fillet.retain(|(l, _)| matches!(l, draw::Picked::Element(_)));
            self.drawing.trim = None;
            self.drawing.align_reference = None;
            if matches!(self.tool, Tool::Align(Some(_))) {
                self.tool = Tool::Align(None);
            }
        }
        if let Tool::SketchLine(chain) = &mut self.tool {
            chain.clear();
        }
    }

    /// Delete while sketching: the lines picked taken out.
    pub(crate) fn delete_sketch_lines(&mut self) {
        if let Some(sk) = &mut self.sketch {
            let picked = std::mem::take(&mut sk.picked);
            let mut i = 0;
            sk.lines.retain(|_| {
                i += 1;
                !picked.contains(&(i - 1))
            });
            sk.changed();
        }
    }

    /// Align changes only the draft until Finish; its joined ends and local undo move with it.
    pub(crate) fn align_sketch(&mut self, reference: Line, target: usize, line: Line) {
        let fixed = match self.drawing.align_reference {
            Some(draw::Picked::Sketch(i)) => Some(i),
            _ => None,
        };
        let Some(sk) = &mut self.sketch else { return };
        match bimcraft_core::align::sketch_lines(&sk.lines, target, reference, line, fixed) {
            Ok(lines) => {
                sk.lines = lines;
                sk.changed();
                self.drawing.align_reference = None;
                self.tool = Tool::Align(None);
                self.status = Status::default();
            }
            Err(text) => self.status = Status { text, error: true },
        }
    }

    /// A click while sketching, at `p` (snapped) or picking near `raw`, `reach` mm round it: Line's next point (back on
    /// the first, the loop is closed and the next line starts anew), Rectangle's corner, Arc's ends then the point it
    /// passes through, Pick Walls' wall (`wall`), or with Modify a line picked.
    pub(crate) fn sketch_click(&mut self, m: &Model, p: P2, raw: P2, reach: f64, wall: Option<Id>, mods: Modifiers) {
        // The Draw panel's other shapes: Fillet Arc picks two of the sketch's lines, Pick Lines a line of the model's;
        // the rest gather their points.
        if matches!(self.tool, Tool::SketchLine(_)) {
            match self.shape() {
                draw::Shape::Line => {}
                draw::Shape::Fillet => {
                    let hit = self.sketch.as_ref().and_then(|sk| sk.line_at(raw, reach));
                    self.fillet_pick(hit.map(draw::Picked::Sketch), raw);
                    return;
                }
                draw::Shape::PickLines => {
                    if let Some(c) = wall.and_then(|id| self.picked_curve(id, raw)) {
                        self.add_sketch_lines(vec![c]);
                    }
                    return;
                }
                _ => {
                    if let Some(made) = self.shape_click(p) {
                        self.add_sketch_lines(made);
                    }
                    return;
                }
            }
        }
        let Some(sk) = &mut self.sketch else { return };
        match &mut self.tool {
            Tool::Modify => {
                let hit = sk.line_at(raw, reach);
                if !(mods.shift || mods.command || mods.ctrl) {
                    sk.picked.clear();
                }
                if let Some(i) = hit
                    && !sk.picked.remove(&i)
                {
                    sk.picked.insert(i);
                }
            }
            Tool::SketchLine(chain) => match chain.last().copied() {
                Some(last) if last.dist(p) > TOL => {
                    let closes = chain.first().is_some_and(|f| f.dist(p) <= TOL);
                    // Back on the start after one line only, the second would lie along the first: no loop is two lines.
                    if closes && chain.len() < 3 {
                        self.status =
                            Status { text: t("A loop needs three lines: click a point that isn't the start.").into(), error: true };
                        return;
                    }
                    sk.add(SketchLine::new(last, p));
                    if closes {
                        chain.clear();
                    } else {
                        chain.push(p);
                    }
                }
                Some(_) => {}
                None => chain.push(p),
            },
            // A line along the wall's location line, round with it where it's an arc.
            Tool::SketchPick => {
                if let Some(w) = wall.and_then(|id| m.wall(id)) {
                    let axis = bimcraft_core::walls::Axis::of(w);
                    let line = match w.arc {
                        a if a != 0.0 => SketchLine::arc(w.start, w.end, axis.at(axis.length() / 2.0, 0.0)),
                        _ => SketchLine::new(w.start, w.end),
                    };
                    sk.add(line);
                }
            }
            _ => {}
        }
    }

    /// Sketch mode's drags with Modify: from a picked line's end, its grip, moving every line end there to where it's let
    /// go (snapped); from anywhere else a box, picking the lines in it (from the left, wholly inside; from the right,
    /// touching too), or a line where it began if it hardly moved. True when one ended this frame: that's no click.
    pub(crate) fn sketch_drag(&mut self, ui: &Ui, response: &Response, rect: Rect, view: &View, level: Id) -> bool {
        if self.tool != Tool::Modify || self.sketch.is_none() {
            return false;
        }
        let local = |p: Pos2| (p.x - rect.min.x, p.y - rect.min.y);
        if response.drag_started_by(PointerButton::Primary)
            && let Some(o) = ui.input(|i| i.pointer.press_origin())
            && let Some(sk) = &mut self.sketch
        {
            let q = local(o);
            sk.drag = Some(match sk.grip_at(view.mm(q), f64::from(GRIP) / view.scale) {
                Some(g) => Drag::Grip(g),
                None => Drag::Box(q),
            });
        }
        if !response.drag_stopped() {
            return false;
        }
        let Some(drag) = self.sketch.as_mut().and_then(|sk| sk.drag.take()) else { return false };
        let Some(at) = ui.input(|i| i.pointer.latest_pos()).map(local) else { return true };
        let m = std::sync::Arc::clone(&self.session.model);
        let to = self.snap(&m, level, view, at, &[], false).at;
        let mods = ui.input(|i| i.modifiers);
        let Some(sk) = &mut self.sketch else { return true };
        match drag {
            Drag::Grip(from) => sk.move_end(from, to),
            Drag::Box(from) if (from.0 - at.0).abs().max((from.1 - at.1).abs()) < 6.0 => {
                let reach = f64::from(crate::canvas::PICK) / view.scale;
                let hit = sk.line_at(view.mm(from), reach);
                if !(mods.shift || mods.command || mods.ctrl) {
                    sk.picked.clear();
                }
                sk.picked.extend(hit);
            }
            Drag::Box(from) => {
                if !(mods.shift || mods.command || mods.ctrl) {
                    sk.picked.clear();
                }
                let found = sk.in_box(view, from, at);
                sk.picked.extend(found);
            }
        }
        true
    }

    /// What a sketch tool or drag shows under the cursor at `q`: Rectangle's rectangle, Arc's arc, a selection box.
    pub(crate) fn sketch_preview(&self, p: &Painter, view: &View, origin: Pos2, q: (f32, f32), cursor: P2) {
        let stroke = Stroke::new(2.0, pal().accent);
        let at = |pt: P2| {
            let (x, y) = view.px(pt);
            origin + vec2(x, y)
        };
        match (&self.tool, self.sketch.as_ref().and_then(|sk| sk.drag)) {
            (Tool::SketchLine(_), _) => {
                for c in self.shape_preview(cursor) {
                    p.add(Shape::line(c.points().into_iter().map(at).collect(), stroke));
                }
            }
            (Tool::Modify, Some(Drag::Box(from))) => p.extend(box_shapes(p.clip_rect(), from, q)),
            _ => {}
        }
    }

    /// The contextual tab's Draw panel while sketching: every shape, and Pick Walls.
    pub(crate) fn sketch_draw(&mut self, ui: &mut Ui) {
        self.draw_panel(ui, draw::Target::Sketch);
    }

    /// Editing tools must remain available in the sketch's contextual ribbon.
    pub(crate) fn sketch_modify_panel(&mut self, ui: &mut Ui) {
        let tools = [
            (Icon::Align, msg!("Align"), msg!("Align (AL): pick a reference line, then what to line up with it"), Tool::Align(None)),
            (
                Icon::Trim,
                msg!("Trim/Extend to Corner"),
                msg!("Trim/Extend to Corner (TR): pick two walls or two lines, on the parts to keep"),
                Tool::Trim(None),
            ),
            (
                Icon::TrimSingle,
                msg!("Trim/Extend Single Element"),
                msg!("Trim/Extend Single Element: pick the line to trim or extend to, then the line to change, on the part to keep"),
                Tool::TrimTo { boundary: None, many: false },
            ),
            (
                Icon::TrimMultiple,
                msg!("Trim/Extend Multiple Elements"),
                msg!("Trim/Extend Multiple Elements: pick the line to trim or extend to, then each line to change, on the part to keep"),
                Tool::TrimTo { boundary: None, many: true },
            ),
            (Icon::Split, msg!("Split Element"), msg!("Split Element (SL): click a wall or a line where to split it"), Tool::Split),
            (Icon::Offset, msg!("Offset"), msg!("Offset (OF): pick a wall or a line; a copy goes to the cursor's side"), Tool::Offset),
        ];
        panel(ui, "Modify", |ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for row in tools.chunks(3) {
                    ui.horizontal(|ui| {
                        for (icon, label, tip, tool) in row {
                            let active = match (&self.tool, tool) {
                                (Tool::TrimTo { many: a, .. }, Tool::TrimTo { many: b, .. }) => a == b,
                                (a, b) => std::mem::discriminant(a) == std::mem::discriminant(b),
                            };
                            if small_button(ui, *icon, label, active, tip).clicked() {
                                self.set_tool(tool.clone());
                            }
                        }
                    });
                }
            });
        });
    }

    /// The Mode panel at the ribbon's right end: Finish and Cancel while sketching, Edit Boundary when one floor, ceiling,
    /// roof or shaft is selected.
    pub(crate) fn mode_panel(&mut self, ui: &mut Ui) {
        let sketching = self.sketch.is_some();
        if !sketching && !(self.boundary_selected() && self.tool == Tool::Modify) {
            return;
        }
        // Pushed to the right end by the room the rest leaves: its width as it was last drawn.
        ribbon::push_right(ui, self.mode_width);
        let drawn = ui.scope(|ui| {
            panel(ui, "Mode", |ui| {
                if sketching {
                    let tip = msg!("Finish Edit Mode: the lines, closed into loops, become the outline and its holes");
                    if big_button(ui, Icon::Check, "Finish Edit Mode", false, tip).clicked() {
                        self.finish_sketch();
                    }
                    if big_button(ui, Icon::Close, "Cancel Edit Mode", false, "Cancel Edit Mode: drop the lines, keep it as it was")
                        .clicked()
                    {
                        self.cancel_sketch();
                    }
                } else if big_button(ui, Icon::Line, "Edit Boundary", false, "Edit Boundary: sketch its outline and holes again").clicked()
                {
                    self.edit_boundary();
                }
            });
        });
        let width = drawn.response.rect.width() + ui.spacing().item_spacing.x;
        if (width - self.mode_width).abs() > 0.5 {
            self.mode_width = width;
            ui.ctx().request_repaint();
        }
    }

    /// A new floor's sketch: Walls' Outline, the outline the walls of its level enclose (and the courtyards in it) put in the
    /// sketch as lines, to edit before Finish: what the Floor tool's one click made before it sketched.
    pub(crate) fn walls_outline_panel(&mut self, ui: &mut Ui) {
        let Some(Of::Floor(level)) = self.sketch.as_ref().map(|sk| sk.of) else { return };
        panel(ui, msg!("Walls"), |ui| {
            let tip = msg!("Walls' Outline: the outline the level's walls enclose, along their centre lines, as sketch lines to edit");
            if big_button(ui, Icon::Wall, msg!("Walls' Outline"), false, tip).clicked() {
                self.walls_outline(level);
            }
        });
    }

    /// The outline level `level`'s walls enclose, and its courtyards, added to the sketch as lines; or why there's none.
    pub(crate) fn walls_outline(&mut self, level: Id) {
        let Some((outline, courtyards)) = bimcraft_core::floor_outline(&self.session.model, level) else {
            self.status = Status { text: t("No walls on this level enclose an area: sketch the outline.").into(), error: true };
            return;
        };
        let Some(sk) = &mut self.sketch else { return };
        for ring in std::iter::once(&outline).chain(&courtyards) {
            for (a, b) in geom::ring_edges(ring) {
                sk.lines.push(SketchLine::new(a, b));
            }
        }
        sk.changed();
        self.status = Status::default();
    }

    /// The Floor, Ceiling, Roof and Shaft tools' Draw panel: their outline sketched instead of found from the walls.
    pub(crate) fn sketch_starters(&mut self, ui: &mut Ui) {
        let Some(level) = self.plan_level() else { return };
        let (of, label, tip) = match self.tool {
            Tool::Floor => {
                (Of::Floor(level), msg!("Boundary Line"), msg!("Boundary Line: sketch the floor's outline, and any openings in it"))
            }
            Tool::Ceiling => {
                (Of::Ceiling(level), msg!("Sketch Ceiling"), msg!("Sketch Ceiling: sketch the ceiling's outline, and any openings in it"))
            }
            Tool::Roof => (
                Of::Roof(level),
                "Boundary Line",
                msg!("Boundary Line: sketch the roof's footprint, on the level above, and any courtyards in it"),
            ),
            Tool::Shaft(_) => (Of::Shaft(level), "Boundary Line", msg!("Boundary Line: sketch the shaft opening's outline")),
            _ => return,
        };
        panel(ui, "Draw", |ui| {
            if big_button(ui, Icon::Line, label, false, tip).clicked() {
                self.start_sketch(of, Vec::new());
            }
        });
    }

    /// The Options Bar while sketching a region: the line style the lines drawn take (or invisible lines), which the lines
    /// picked take at once.
    pub(crate) fn sketch_options(&mut self, ui: &mut Ui) {
        let m = &self.session.model;
        let Some(sk) = self.sketch.as_mut().filter(|sk| sk.styled) else { return };
        // The region's type is the Type Selector's, in the Properties palette: one place to set it.
        let l = ui.label(t("Line Style:"));
        let invisible = t("<Invisible lines>").to_string();
        let shown = sk.region.1.clone().unwrap_or_else(|| invisible.clone());
        let was = sk.region.1.clone();
        let c = egui::ComboBox::from_id_salt("region-lines").selected_text(shown).width(160.0).show_ui(ui, |ui| {
            ui.selectable_value(&mut sk.region.1, None, &invisible);
            for n in m.line_styles.keys() {
                ui.selectable_value(&mut sk.region.1, Some(n.clone()), n);
            }
        });
        c.response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, t("Line Style")));
        c.response.labelled_by(l.id);
        // A style picked restyles the lines picked.
        if sk.region.1 != was {
            let (style, picked) = (sk.region.1.clone(), sk.picked.clone());
            for i in picked {
                if let Some(line) = sk.lines.get_mut(i) {
                    line.invisible = style.is_none();
                    line.style = style.clone();
                }
            }
        }
    }

    /// Annotate's Region panel: Filled Region, Masking Region, sketched in the plan on screen.
    pub(crate) fn region_panel(&mut self, ui: &mut Ui) {
        panel(ui, msg!("Region"), |ui| {
            for (icon, label, tip, masking) in [
                (
                    Icon::FilledRegion,
                    msg!("Filled Region"),
                    msg!("Filled Region (FR): sketch an area filled with a pattern, in this view"),
                    false,
                ),
                (
                    Icon::MaskingRegion,
                    msg!("Masking Region"),
                    msg!("Masking Region: sketch an area that hides what's under it, in this view"),
                    true,
                ),
            ] {
                let on = matches!(self.sketch.as_ref().map(|s| s.of), Some(Of::Region { masking: m, .. }) if m == masking);
                if big_button(ui, icon, label, on, tip).clicked()
                    && let Some(view) = self.plan_key()
                {
                    self.start_sketch(Of::Region { view, masking }, Vec::new());
                }
            }
        });
    }

    /// What the status bar says while sketching, or None when not.
    pub(crate) fn sketch_prompt(&self) -> Option<&'static str> {
        self.sketch.as_ref()?;
        Some(match &self.tool {
            Tool::Align(None) => msg!("Pick a highlighted model edge or sketch line as the alignment reference."),
            Tool::Align(Some(_)) => msg!("Pick the highlighted sketch line to move and turn onto the reference; joined ends follow."),
            Tool::Trim(_) if self.drawing.trim.is_none() => msg!("Pick the first sketch line, on the part to keep."),
            Tool::Trim(_) => msg!("Pick the second sketch line, on the part to keep."),
            Tool::TrimTo { .. } if self.drawing.trim.is_none() => msg!("Pick a sketch line as the trim/extend boundary."),
            Tool::TrimTo { .. } => msg!("Pick a sketch line to trim or extend, on the part to keep. Esc clears the boundary."),
            Tool::SketchLine(_) if self.shape() != draw::Shape::Line => return self.shape_prompt(),
            Tool::SketchLine(chain) if chain.is_empty() => "Click where the line starts.",
            Tool::SketchLine(_) => msg!("Click the line's end; the next starts there. The first point again closes the loop; Esc stops."),
            Tool::SketchPick => msg!("Click a wall: a boundary line along it."),
            Tool::Modify if self.sketch_surface().is_some() => {
                msg!(
                    "Pick sketch lines (a box picks several); drag a picked line's end over its plane to move it; Shift and the middle button orbit; Delete removes them. Finish Edit Mode when every loop is closed."
                )
            }
            _ => {
                msg!(
                    "Pick sketch lines (a box picks several); drag a picked line's end to move it; Delete removes them. Finish Edit Mode when every loop is closed."
                )
            }
        })
    }
}

/// A line as Finish sends it: its curve's fields, and its id when it kept one, so it keeps it.
fn sent(l: &SketchLine) -> serde_json::Value {
    let mut v = serde_json::to_value(bimcraft_core::curves::Fields::of(&l.curve)).unwrap_or_default();
    if l.id != 0 {
        v["id"] = json!(l.id);
    }
    if let Some(st) = &l.style {
        v["style"] = json!(st);
    }
    if l.invisible {
        v["invisible"] = json!(true);
    }
    v
}
