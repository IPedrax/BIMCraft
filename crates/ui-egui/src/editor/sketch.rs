//! F5: sketch mode in the family document, as Revit's Create Extrusion and Create Revolve. Create > Extrusion or Revolve (or
//! Void Forms') sketches on the view's work plane, in a plan or an elevation that sees the plane flat, or in {3D}; Edit
//! Extrusion and Edit Revolve sketch a selected form's lines again. The lines are the project's sketch's (`crate::sketch::Sketch`:
//! its lines, picks, grips, local undo and paint), in the host plane's own axes, drawn with the Draw panel's shapes
//! (`bimcraft_core::draw`); a revolve's Axis Line is one more line, marked. Finish Edit Mode checks them as the core does
//! (`family::sketch::finish_check`), refusing with what to fix and leaving the draft as it was, and on success puts the form
//! in the draft, one step of its history; Cancel Edit Mode leaves the draft as it was. A line kept from the form keeps its id,
//! its GUID and, unmoved, its formulas.
use super::*;
use bimcraft_core::curves::Curve;
use bimcraft_core::draw::{self as shapes, Options, Shape};
use bimcraft_core::family::forms::{self as forms, FormKind, Swept};
use bimcraft_core::family::sketch::{self as fs, FormLine};
use bimcraft_core::family::views::{Staged, Which};
use bimcraft_core::family::{DimKind, DimRef, DimView, FamilyDimension, Frame, Val};
use bimcraft_core::geom::P2;
use bimcraft_core::ids::Guid;
use bimcraft_core::sketch::SketchLine;
use egui::Pos2;

use crate::icons::Icon;
use crate::ribbon;
use crate::sketch::{Drag, Of, Sketch};

/// What marks a sketch line as a revolve's Axis Line while it is drawn: never kept, the form's axis line is marked its own way.
const AXIS: &str = "\u{0}axis";

/// The green the family's views draw reference planes and lines in (the core's `render::REFERENCE_PLANE`).
const REFERENCE: Color32 = Color32::from_rgb(70, 140, 90);

/// Revit's green, a revolve's Axis Line.
const AXIS_COLOUR: Color32 = Color32::from_rgb(20, 150, 60);

/// The parts of a form not being sketched (F6: a Blend's base while its top is, a sweep's path while its profile is): grey.
const GHOST: Color32 = Color32::from_rgb(140, 140, 150);

/// How near a line, in points, a click picks it.
const REACH: f32 = 8.0;

/// The shapes a form's Draw panel offers, in its order: lines and arcs, as Revit's Create Extrusion has them (no spline or
/// ellipse), and Pick Lines.
pub(crate) const GALLERY: [Shape; 9] = [
    Shape::Line,
    Shape::Rectangle,
    Shape::Inscribed,
    Shape::Circumscribed,
    Shape::Circle,
    Shape::Arc,
    Shape::CentreArc,
    Shape::TangentArc,
    Shape::Fillet,
];

/// Which of a form's sketches is on screen (F6): its sketch (an extrusion's or a revolve's profile, a Blend's base, a sweep's
/// path), a Blend's top, a sweep's profile, a Swept Blend's second profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Part {
    Main = 0,
    Top = 1,
    Profile = 2,
    End = 3,
}

/// A form being sketched: the lines, which form (none, a new one), what it is, the plane it is drawn on and the view it was
/// begun in, its numbers as typed, and the Draw panel's tool and what it has been clicked so far.
pub(crate) struct FormSketch {
    pub(crate) sketch: Sketch,
    /// The form redrawn; none, a new one.
    pub(crate) index: Option<usize>,
    pub(crate) revolve: bool,
    /// What it is (F6): an extrusion, a revolve, a Blend, a Sweep or a Swept Blend.
    pub(crate) kind: FormKind,
    /// Which of its sketches is on screen, in `sketch`; the others' lines wait in `parts`, by [`Part`].
    pub(crate) part: Part,
    pub(crate) parts: [Vec<SketchLine>; 4],
    /// Edit Vertices' Twist, a Blend's or a Swept Blend's.
    pub(crate) twist: i32,
    pub(crate) void: bool,
    pub(crate) host: PlaneRef,
    /// Extrusion Start and End, or a revolve's Start and End Angle, as typed (a number or a formula).
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) material: String,
    /// The Draw panel's shape; none, Modify: a click picks a line, a picked line's end drags.
    pub(crate) tool: Option<Shape>,
    /// Axis Line rather than Boundary Line: what is drawn is the revolve's axis.
    pub(crate) axis_tool: bool,
    /// The points clicked of the shape being drawn, and the last line's end and the way it ran there (Chain, Tangent End Arc).
    pub(crate) points: Vec<P2>,
    last: Option<(P2, P2)>,
    /// Fillet Arc's first line.
    fillet: Option<usize>,
    /// The Options Bar: a polygon's sides, a fixed radius (on, typed), and Offset.
    pub(crate) sides: String,
    pub(crate) radius_on: bool,
    pub(crate) radius: String,
    pub(crate) offset: String,
    /// Where the cursor is on the plane, for the preview, snapped.
    hover: Option<P2>,
    /// What the cursor snapped to, for its marker.
    found: Option<bimcraft_core::snap::Found>,
    /// Chain: a line or an arc drawn starts the next where it ends (the Options Bar's, for the session).
    pub(crate) chain: bool,
    /// What the sketch snaps to (`Family::sketch_targets`), and the draft it was found for.
    targets: Option<(u64, fs::Targets)>,
    /// The form's GUID: its own when edited, a new one for a new form, so its lines can be locked before it is made (F7b).
    pub(crate) guid: Guid,
    /// The form's lines as the type previewed showed them when Edit began, each part's (by [`Part`]): a line that comes back
    /// there keeps its formulas.
    shown: [Vec<FormLine>; 4],
    /// Align (F7b): the reference picked first, its line on the plane, and which of the sketch's lines it is, if one.
    pub(crate) align_from: Option<(DimRef, (P2, P2), Option<usize>)>,
    /// Align's padlock, offered until the next click: the lock it adds.
    pub(crate) offer: Option<FamilyDimension>,
    /// Locks added while sketching, and those the form had that were taken off: written at Finish, dropped by Cancel.
    pub(crate) locks: Vec<FamilyDimension>,
    pub(crate) unlocked: Vec<Guid>,
    /// Pick Lines' Lock, on the Options Bar (F10a): a line picked from a reference is locked to it, as Align's padlock locks.
    pub(crate) lock: bool,
    /// Trim/Extend to Corner (TR), on: the first line picked, where it was clicked (the part kept).
    pub(crate) trim: Option<Option<(usize, P2)>>,
}

/// What Pick Lines picks: the line, and the reference it lies on, if the view found one, as a lock waiting for the sketch's
/// line (its `refs` the reference alone): what Lock adds.
pub(crate) type Picked = (Curve, Option<FamilyDimension>);

/// What Pick Lines picks in a plan or an elevation: the line in the view's millimetres, and the lock waiting, as [`Picked`].
pub(crate) type PickedInView = ((P2, P2), Option<FamilyDimension>);

/// A padlock shown while a form is sketched (F7b): Align's offer (open), a lock added while sketching, one the form had.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Padlock {
    Offer,
    Added(usize),
    Had(Guid),
}

impl FormSketch {
    /// The lines of part `p`, on screen or waiting.
    pub(crate) fn part_lines(&self, p: Part) -> &[SketchLine] {
        if p == self.part { &self.sketch.lines } else { &self.parts[p as usize] }
    }

    /// Part `p` put on screen, the one there put by (its lines, its own Ctrl+Z gone with it); the tool back to Line.
    pub(crate) fn set_part(&mut self, p: Part) {
        if p == self.part {
            return;
        }
        let now = std::mem::take(&mut self.sketch.lines);
        self.parts[self.part as usize] = now;
        let lines = std::mem::take(&mut self.parts[p as usize]);
        self.sketch = Sketch::of_form(self.sketch.of, lines);
        self.part = p;
        self.points.clear();
        self.last = None;
        self.fillet = None;
        self.align_from = None;
        self.offer = None;
        self.targets = None;
        self.set_tool(Some(Shape::Line), false);
    }

    /// The path (a sweep's sketch) as drawn, chained, its numbers as they are.
    pub(crate) fn path(&self) -> Result<forms::Path, String> {
        let lines = fs::lines_of(self.part_lines(Part::Main));
        forms::path(&lines, &mut |e| match e {
            Expr::Number(x) => Ok(*x),
            Expr::Formula(f) => Err(f.clone()),
        })
    }

    /// What the contextual tab is called: Revit's words for the form and the part on screen.
    pub(crate) fn title(&self) -> &'static str {
        match (self.kind, self.part, self.void) {
            (FormKind::Extrusion | FormKind::Revolve, _, _) => self.sketch.title(),
            (FormKind::Blend, Part::Top, false) => msg!("Modify | Create Blend Top Boundary"),
            (FormKind::Blend, Part::Top, true) => msg!("Modify | Create Void Blend Top Boundary"),
            (FormKind::Blend, _, false) => msg!("Modify | Create Blend Base Boundary"),
            (FormKind::Blend, _, true) => msg!("Modify | Create Void Blend Base Boundary"),
            (FormKind::Sweep, Part::Main, _) => msg!("Modify | Sweep > Sketch Path"),
            (FormKind::Sweep, _, _) => msg!("Modify | Sweep > Edit Profile"),
            (FormKind::SweptBlend, Part::Main, _) => msg!("Modify | Swept Blend > Sketch Path"),
            (FormKind::SweptBlend, Part::End, _) => msg!("Modify | Swept Blend > Edit Profile 2"),
            (FormKind::SweptBlend, _, _) => msg!("Modify | Swept Blend > Edit Profile 1"),
        }
    }

    /// What the cursor snaps to now, if anything.
    #[cfg(test)]
    pub(crate) fn snapped(&self) -> Option<bimcraft_core::snap::Found> {
        self.found.filter(|f| f.kind != bimcraft_core::snap::Kind::Free)
    }

    /// The lines of the profile: all but the axis.
    fn boundary(&self) -> Vec<SketchLine> {
        self.sketch.lines.iter().filter(|l| !is_axis(l)).cloned().collect()
    }

    /// The Axis Line's ends.
    fn axis(&self) -> Option<[P2; 2]> {
        self.sketch.lines.iter().find(|l| is_axis(l)).map(|l| [l.start(), l.end()])
    }

    /// The Options Bar's settings, as the Draw panel's shapes take them.
    pub(crate) fn options(&self) -> Options {
        let number = |s: &str| s.trim().parse::<f64>().ok().filter(|x| x.is_finite());
        Options {
            sides: self.sides.trim().parse::<usize>().unwrap_or(6).clamp(3, shapes::MAX_SIDES),
            radius: self.radius_on.then(|| number(&self.radius)).flatten().filter(|r| *r > 0.0),
            offset: number(&self.offset).unwrap_or(0.0),
        }
    }

    /// What the shape being drawn would make with the cursor at `cursor`.
    fn preview(&self, cursor: P2) -> Vec<Curve> {
        match self.tool {
            Some(Shape::Fillet | Shape::PickLines) | None => Vec::new(),
            Some(shape) if self.points.is_empty() && shape.clicks(&self.options()) > 1 => Vec::new(),
            Some(shape) => shapes::preview(shape, &self.points, cursor, &self.options(), self.last.map(|l| l.1)),
        }
    }

    /// Curves drawn: the Axis Line (its first straight line) or lines of the profile, a polyline cut into its pieces. The
    /// next chained line starts at the last one's end.
    fn add(&mut self, curves: Vec<Curve>) {
        if self.axis_tool {
            if let Some(c) = curves.into_iter().find(|c| c.kind() == "line") {
                self.sketch.lines.retain(|l| !is_axis(l));
                self.sketch.lines.push(SketchLine { style: Some(AXIS.into()), ..SketchLine::from(c) });
                self.sketch.changed();
            }
            return;
        }
        for c in curves {
            for piece in bimcraft_core::curves::explode(&c) {
                let end = piece.end();
                let way = piece.tangent(piece.domain().1);
                self.sketch.add(SketchLine::from(piece));
                self.last = Some((end, way));
            }
        }
    }

    /// A click at `p` on the plane: the next point of the shape (made once it has its points), or with Modify a line picked;
    /// `pick` is what Pick Lines finds there. What went wrong, if anything.
    fn click(&mut self, p: P2, reach: f64, shift: bool, pick: &dyn Fn() -> Result<Picked, String>) -> Option<String> {
        // Trim/Extend to Corner: two lines, each on the part to keep.
        if let Some(first) = self.trim {
            let Some(i) = self.sketch.line_at(p, reach) else {
                return Some(t("Pick a line of the sketch, on the part to keep.").to_string());
            };
            match first {
                None => self.trim = Some(Some((i, p))),
                Some((a, keep)) if a != i => {
                    self.trim = Some(None);
                    let (Some(la), Some(lb)) = (self.sketch.lines.get(a), self.sketch.lines.get(i)) else { return None };
                    match shapes::corner(&la.curve, keep, &lb.curve, p) {
                        Some((ca, cb)) => {
                            self.sketch.lines[a].curve = ca;
                            self.sketch.lines[i].curve = cb;
                            self.sketch.changed();
                        }
                        None => return Some(t("These lines don't meet: pick two that would, on the parts to keep.").to_string()),
                    }
                }
                Some(_) => {}
            }
            return None;
        }
        match self.tool {
            None => {
                match self.sketch.line_at(p, reach) {
                    Some(i) if shift => {
                        if !self.sketch.picked.remove(&i) {
                            self.sketch.picked.insert(i);
                        }
                    }
                    Some(i) => self.sketch.picked = [i].into(),
                    None => self.sketch.picked.clear(),
                }
                None
            }
            Some(Shape::PickLines) => match pick() {
                Ok((c, lock)) => {
                    let by = self.options().offset;
                    let n = self.sketch.lines.len();
                    self.add(vec![shapes::pick(&c, p, by).unwrap_or(c)]);
                    // Lock (F10a): the line picked locked to the reference it lies on, as Align's padlock locks it.
                    if self.lock
                        && by == 0.0
                        && let Some(mut d) = lock
                        && self.sketch.lines.len() == n + 1
                    {
                        let l = &mut self.sketch.lines[n];
                        if l.guid.is_nil() {
                            l.guid = Guid::new();
                        }
                        d.refs.push(DimRef::Sketch(self.guid, l.guid));
                        self.locks.push(d);
                    }
                    None
                }
                Err(e) => Some(e),
            },
            Some(Shape::Fillet) => {
                let i = self.sketch.line_at(p, reach)?;
                match self.fillet.take() {
                    None => {
                        self.fillet = Some(i);
                        None
                    }
                    Some(first) if first != i => {
                        let radius = self.options().radius.unwrap_or(100.0);
                        let (Some(a), Some(b)) = (self.sketch.lines.get(first), self.sketch.lines.get(i)) else { return None };
                        match shapes::fillet_lines(&a.curve, &b.curve, radius) {
                            Some((la, arc, lb)) => {
                                let (ka, kb) = (a.clone(), b.clone());
                                self.sketch.lines[first] = SketchLine { curve: la, ..ka };
                                self.sketch.lines[i] = SketchLine { curve: lb, ..kb };
                                self.sketch.add(SketchLine::from(arc));
                                None
                            }
                            None => Some(t("These lines can't be rounded: pick two straight lines that meet.").to_string()),
                        }
                    }
                    Some(_) => None,
                }
            }
            Some(shape) => {
                self.points.push(p);
                let o = self.options();
                if let Some(made) = shapes::make(shape, &self.points, &o, self.last.map(|l| l.1)) {
                    self.add(made);
                    self.points.clear();
                    // Chained: the next starts where this one ended.
                    if self.chain && shape.chains() && !self.axis_tool && o.offset == 0.0 {
                        self.points.extend(self.last.map(|l| l.0));
                    }
                } else if self.points.len() >= shape.clicks(&o).max(1) {
                    self.points.clear();
                }
                None
            }
        }
    }

    /// Escape: the shape's points let go, then the tool (Modify).
    pub(crate) fn escape(&mut self) {
        self.offer = None;
        if self.align_from.take().is_some() {
            return;
        }
        if let Some(first) = self.trim {
            self.trim = first.map(|_| None);
            return;
        }
        if !self.points.is_empty() || self.fillet.is_some() {
            self.points.clear();
            self.fillet = None;
        } else {
            self.tool = None;
        }
    }

    /// Delete: the lines picked taken away.
    pub(crate) fn delete_picked(&mut self) {
        let picked = std::mem::take(&mut self.sketch.picked);
        let mut i = 0;
        self.sketch.lines.retain(|_| {
            i += 1;
            !picked.contains(&(i - 1))
        });
        self.sketch.changed();
    }

    /// The shape picked in the Draw panel, or Axis Line's.
    pub(crate) fn set_tool(&mut self, tool: Option<Shape>, axis: bool) {
        self.tool = tool;
        self.axis_tool = axis;
        self.points.clear();
        self.fillet = None;
        self.trim = None;
    }

    /// Trim/Extend to Corner (TR) on: the next two lines picked meet at a corner.
    pub(crate) fn trim_tool(&mut self) {
        self.set_tool(None, false);
        self.trim = Some(None);
    }

    /// The lines over the view: the profile's in sketch magenta (picked, wrong as the project's are), the Axis Line green and
    /// dashed, the shape being drawn to the cursor; `at` puts a point of the plane on screen.
    fn paint(&self, p: &egui::Painter, at: &dyn Fn(P2) -> Option<Pos2>, hovered: Option<usize>) {
        let shown = |q: P2| at(q).unwrap_or(Pos2::new(f32::NAN, f32::NAN));
        let cursor = match self.sketch.drag {
            Some(Drag::Grip(_)) => self.hover,
            _ => None,
        };
        let lit: Vec<usize> = self.fillet.into_iter().chain(self.trim.flatten().map(|(i, _)| i)).collect();
        self.sketch.paint(p, shown, hovered, cursor, &lit);
        for l in self.sketch.lines.iter().filter(|l| is_axis(l)) {
            let pts: Vec<Pos2> = l.points().into_iter().filter_map(at).collect();
            if let [a, b] = pts.as_slice() {
                p.extend(egui::Shape::dashed_line(&[*a, *b], Stroke::new(2.5, AXIS_COLOUR), 10.0, 5.0));
            }
        }
        if let Some(c) = self.hover {
            for curve in self.preview(c) {
                let pts: Vec<Pos2> = curve.points().into_iter().filter_map(at).collect();
                let colour = if self.axis_tool { AXIS_COLOUR } else { crate::style::canvas_pal().accent };
                p.add(egui::Shape::line(pts, Stroke::new(1.5, colour)));
            }
        }
    }
}

/// What Pick Lines says of a line picked that runs square to the work plane.
fn square_to_plane() -> String {
    t("That runs square to the work plane: it has no line on it to pick.").to_string()
}

/// Where Chain is kept for the session: on, unless the Options Bar turned it off.
const CHAIN: &str = "family-sketch-chain";

/// Chain, as the Options Bar last left it this session.
pub(crate) fn chain(ctx: &Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(UiId::new(CHAIN))).unwrap_or(true)
}

/// Where Pick Lines' Lock is kept for the session: off, unless the Options Bar turned it on (as Revit's starts).
const LOCK: &str = "family-sketch-lock";

/// Pick Lines' Lock, as the Options Bar last left it this session.
pub(crate) fn lock(ctx: &Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(UiId::new(LOCK))).unwrap_or(false)
}

/// Whether a line drawn is the Axis Line.
fn is_axis(l: &SketchLine) -> bool {
    l.style.as_deref() == Some(AXIS)
}

/// The way view `which` looks.
pub(crate) fn looking(which: Which) -> [f64; 3] {
    match which {
        Which::Plan => [0.0, 0.0, -1.0],
        Which::Ceiling => [0.0, 0.0, 1.0],
        Which::Elevation(side) => {
            let l = side.looking();
            [l.0, l.1, 0.0]
        }
    }
}

fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Whether view `which` sees plane `f` flat: its normal along the way the view looks (`Which::faces`, which Go To View's
/// list is made by too).
pub(crate) fn flat(which: Which, f: &Frame) -> bool {
    which.faces(f.z)
}

/// Whether view `which` sees plane `f` edge-on: its normal square to the way the view looks.
pub(crate) fn edge_on(which: Which, f: &Frame) -> bool {
    dot3(looking(which), f.z).abs() < 1e-6
}

/// The point of plane `f`, in its own axes, that point `q` of view `which` shows: the view's point taken along the way it
/// looks onto the plane. None where the view sees the plane edge-on.
pub(crate) fn on_plane(st: &Staged, which: Which, f: &Frame, q: P2) -> Option<P2> {
    let p = st.from_view(which, q);
    let d = looking(which);
    let facing = dot3(d, f.z);
    if facing.abs() < 1e-9 {
        return None;
    }
    let t = dot3(std::array::from_fn(|i| f.origin[i] - p[i]), f.z) / facing;
    let p: [f64; 3] = std::array::from_fn(|i| p[i] + d[i] * t - f.origin[i]);
    Some(P2(dot3(p, f.x), dot3(p, f.y)))
}

/// Where point `h` of plane `f` (its own axes) shows in view `which`, in the view's millimetres.
pub(crate) fn in_view(st: &Staged, which: Which, f: &Frame, h: P2) -> P2 {
    st.to_view(which, f.point(h, 0.0))
}

/// A typed number or formula's worth for `values`, for the core's evaluation: unread, said.
fn reader(values: &BTreeMap<String, Val>) -> impl FnMut(&Expr) -> Result<f64, String> + '_ {
    move |e: &Expr| crate::adaptive_editor::worth(e, values).ok_or_else(|| tf("{} can't be worked out", &[&expr_text(e)]))
}

impl FamilyEditor {
    /// Whether a form is being sketched.
    pub(crate) fn sketching(&self) -> bool {
        self.sketch.is_some()
    }

    /// What the contextual tab is called while a form is sketched.
    pub(crate) fn sketch_title(&self) -> Option<&'static str> {
        self.sketch.as_ref().map(|s| s.title())
    }

    /// The frame of the plane being sketched on: the host's; a Blend's top's, at its Second End; a sweep's profile plane,
    /// square to its path where it is shown (F6).
    pub(crate) fn sketch_frame(&self) -> Option<Frame> {
        let s = self.sketch.as_ref()?;
        self.part_frame(s, s.part)
    }

    /// The frame part `p` of sketch `s` is drawn on; none where it has no plane yet (a profile of a path that doesn't chain).
    pub(crate) fn part_frame(&self, s: &FormSketch, p: Part) -> Option<Frame> {
        let host = self.plane_set().frame(&s.host).ok()?;
        match p {
            Part::Main => Some(host),
            Part::Top => {
                let to = crate::adaptive_editor::worth(&text_expr(&s.to), &self.values())?;
                Some(Frame { origin: std::array::from_fn(|k| host.origin[k] + host.z[k] * to), ..host })
            }
            Part::Profile | Part::End => {
                let path = s.path().ok()?;
                let n = path.points.len();
                let (at, way) = match (s.kind, p) {
                    (FormKind::SweptBlend, Part::End) => (path.points[n - 1], path.end_way),
                    (FormKind::SweptBlend, _) => (path.points[0], (path.points[1] - path.points[0]).unit()),
                    _ => (path.at, path.way),
                };
                Some(forms::profile_frame(&host, at, way))
            }
        }
    }

    /// The lines of the parts not on screen, each as polylines on its own plane: drawn grey under the one being sketched.
    pub(crate) fn other_parts(&self) -> Vec<(Frame, Vec<Vec<P2>>)> {
        let Some(s) = self.sketch.as_ref() else { return Vec::new() };
        let mut out = Vec::new();
        for p in [Part::Main, Part::Top, Part::Profile, Part::End] {
            if p == s.part || s.parts[p as usize].is_empty() {
                continue;
            }
            if let Some(f) = self.part_frame(s, p) {
                out.push((f, s.parts[p as usize].iter().map(|l| l.curve.points()).collect()));
            }
        }
        out
    }

    /// Create > Extrusion, Blend, Revolve, Sweep or Swept Blend (`kind`), solid or void: sketch mode on the view's work
    /// plane, unless a plan or an elevation shows it edge-on or askew; then Go To View is asked (the Work Plane dialog's,
    /// `editor/work_plane.rs`), offering the views that show it flat and {3D}, and the sketch begins in the one opened.
    pub(crate) fn create_form(&mut self, kind: FormKind, void: bool) {
        if self.adaptive.is_some() {
            return;
        }
        let host = self.views.work_plane();
        if self.plane_set().frame(&host).is_err() {
            return;
        }
        self.on_work_plane(host, super::work_plane::Then::Create { kind, void });
    }

    /// Edit Extrusion, Edit Revolve, Edit Blend, Edit Sweep or Edit Swept Blend on form `i`: its lines worked out for the
    /// type previewed, each keeping its id and GUID, sketched again where it is, in the view shown if it sees the form's plane
    /// flat, else in {3D}.
    pub(crate) fn edit_form_sketch(&mut self, i: usize) -> Result<(), String> {
        let f = self.forms.get(i).ok_or_else(|| t("the form is gone").to_string())?.form()?;
        if !f.mesh.is_empty() {
            return Err(t("A mesh from a file has no sketch to edit.").to_string());
        }
        let values = self.values();
        // As the type previewed flexes it (F7b): where its locks and labels put its lines.
        let flexed = self.family()?.flexed(&values).ok().and_then(|x| x.forms.get(i).cloned()).unwrap_or_else(|| f.clone());
        let none = Swept::default();
        let sweep = flexed.sweep.as_ref().unwrap_or(&none);
        let shown = [flexed.sketch.clone(), flexed.top.clone(), sweep.profile.clone(), sweep.end.clone()];
        let mut num = reader(&values);
        let mut parts: [Vec<SketchLine>; 4] = Default::default();
        for (k, group) in shown.iter().enumerate() {
            for l in group {
                let mut drawn = l.curve(&mut num)?;
                if l.axis {
                    drawn.style = Some(AXIS.into());
                }
                parts[k].push(drawn);
            }
        }
        let frame = self.plane_set().frame(&f.host)?;
        if let Some(which) = self.views.shown
            && !flat(which, &frame)
        {
            self.views.show(None);
        }
        let ft = &self.forms[i];
        let (from, to, material) = (ft.from.clone(), ft.to.clone(), ft.material.clone());
        let main = std::mem::take(&mut parts[0]);
        self.start_form_sketch(Some(i), f.kind(), f.void, f.host, main);
        if let Some(s) = self.sketch.as_mut() {
            (s.from, s.to, s.material) = (from, to, material);
            s.shown = shown;
            s.twist = f.twist;
            // Its sketch is on screen; its top and profiles wait.
            for (slot, lines) in s.parts.iter_mut().zip(parts).skip(1) {
                *slot = lines;
            }
        }
        Ok(())
    }

    /// Sketch mode on: `lines` to begin with (its sketch: a profile, a Blend's base, a sweep's path), drawing with Line (a
    /// revolve with no axis yet, with Axis Line).
    pub(super) fn start_form_sketch(&mut self, index: Option<usize>, kind: FormKind, void: bool, host: PlaneRef, lines: Vec<SketchLine>) {
        let edit = index.is_some();
        let revolve = kind == FormKind::Revolve;
        let (from, to) = match kind {
            FormKind::Revolve => ("0", "360"),
            FormKind::Sweep | FormKind::SweptBlend => ("0", "0"),
            FormKind::Blend => ("0", "1000"),
            FormKind::Extrusion => ("0", "250"),
        };
        let had = index.and_then(|i| self.forms.get(i)).and_then(|ft| ft.form().ok()).map(|f| f.guid).filter(|g| !g.is_nil());
        // A new form's GUID made now: a new one, not the nil one `Default` is.
        let guid = match had {
            Some(g) => g,
            None => Guid::new(),
        };
        self.sketch = Some(Box::new(FormSketch {
            sketch: Sketch::of_form(Of::Form { revolve, void, edit }, lines),
            index,
            revolve,
            kind,
            part: Part::Main,
            parts: Default::default(),
            twist: 0,
            void,
            host,
            from: from.into(),
            to: to.into(),
            material: String::new(),
            tool: Some(Shape::Line),
            axis_tool: false,
            points: Vec::new(),
            last: None,
            fillet: None,
            sides: "6".into(),
            radius_on: false,
            radius: "100".into(),
            offset: "0".into(),
            hover: None,
            found: None,
            chain: true,
            targets: None,
            guid,
            shown: Default::default(),
            align_from: None,
            offer: None,
            locks: Vec::new(),
            unlocked: Vec::new(),
            lock: false,
            trim: None,
        }));
        self.form = None;
        self.views.plane = None;
        self.views.drawing = None;
        self.views.picking = None;
    }

    /// Part `p` of the form being sketched put on screen (Edit Top, Edit Base, Sketch Path, Edit Profile): refused for a
    /// profile while the path doesn't chain, saying why. A view that doesn't see its plane flat gives way to one that does,
    /// else to {3D}.
    pub(crate) fn show_part(&mut self, p: Part) -> Result<(), String> {
        let Some(s) = self.sketch.as_ref() else { return Ok(()) };
        if matches!(p, Part::Profile | Part::End)
            && let Err(e) = s.path()
        {
            return Err(tf("Sketch the path first: {}", &[&e]));
        }
        if let Some(s) = self.sketch.as_mut() {
            s.set_part(p);
        }
        if let Some(frame) = self.sketch_frame()
            && self.views.shown.is_some_and(|w| !flat(w, &frame))
        {
            let to = Which::ALL.into_iter().find(|w| flat(*w, &frame));
            self.views.show(to);
        }
        Ok(())
    }

    /// Cancel Edit Mode: the lines dropped, and the locks added or taken off while sketching; the draft as it was.
    pub(crate) fn cancel_form_sketch(&mut self) {
        if let Some(s) = self.sketch.take() {
            self.form = s.index;
        }
        self.views.aligning = None;
    }

    /// Finish Edit Mode: the lines checked as the core checks a sketch, and the form they make built with the family; then
    /// put in the draft (a new one last, an edited one where it was, keeping its id and GUID) and selected. Refused, saying
    /// what to fix (the lines it is about shown in red, the part they are in put on screen), the draft left as it was.
    pub(crate) fn finish_form_sketch(&mut self, units: bimcraft_core::units::Units) -> Result<(), String> {
        let values = self.values();
        let Some(s) = self.sketch.as_mut() else { return Ok(()) };
        // Every line a GUID of its own (F7b), so a lock or a dimension can name it: one drawn new gets a new one.
        for l in s.sketch.lines.iter_mut().chain(s.parts.iter_mut().flatten()).filter(|l| l.guid.is_nil()) {
            l.guid = Guid::new();
        }
        if s.kind != FormKind::Extrusion && s.kind != FormKind::Revolve {
            return self.finish_f6(units);
        }
        let boundary = s.boundary();
        let axis = s.axis();
        let axis_guid = s.sketch.lines.iter().find(|l| is_axis(l)).map(|l| l.guid);
        if let Err(why) = fs::finish_check(&boundary, axis, s.revolve) {
            let text = match why {
                fs::Finish::Problems(problems) => {
                    // The lines it is about, by their places among all the sketch's lines.
                    let all: Vec<usize> = (0..s.sketch.lines.len()).filter(|i| !is_axis(&s.sketch.lines[*i])).collect();
                    s.sketch.wrong = problems.iter().flat_map(|p| p.lines()).filter_map(|i| all.get(i).copied()).collect();
                    crate::sketch::friendly(&problems, &boundary, units)
                }
                fs::Finish::NoAxis => t("A revolve needs an axis line: draw one with Axis Line.").to_string(),
                fs::Finish::CrossesAxis => t("The profile crosses the axis line: draw it to one side of the axis.").to_string(),
            };
            return Err(text);
        }
        let (index, revolve, void, host) = (s.index, s.revolve, s.void, s.host);
        let (from, to, material) = (s.from.clone(), s.to.clone(), s.material.clone());
        let (guid, shown) = (s.guid, s.shown[0].clone());
        // What the form had, its lines kept where they didn't move.
        let before = match index.and_then(|i| self.forms.get(i)) {
            Some(ft) => ft.form()?,
            None => Form::default(),
        };
        let mut lines = fs::lines_of(&boundary);
        if revolve && let Some([a, b]) = axis {
            let n = |x: f64| Expr::Number(x);
            let was = before.axis_line();
            lines.push(FormLine {
                id: was.map_or(0, |l| l.id),
                guid: was.map(|l| l.guid).or(axis_guid).unwrap_or_default(),
                axis: true,
                ..FormLine::line([n(a.0), n(a.1)], [n(b.0), n(b.1)])
            });
        }
        let lines = fs::kept(&before.sketch, &shown, lines, &mut reader(&values));
        let mut form =
            Form { host, from: text_expr(&from), to: text_expr(&to), void, material: material.trim().to_string(), ..before.clone() };
        form.sketch = lines;
        if form.guid.is_nil() {
            form.guid = guid;
        }
        self.commit_form(index, form, &before)
    }

    /// Finish Edit Mode for a Blend, a Sweep or a Swept Blend (F6): each part checked (a Blend's base and top, a sweep's path
    /// as one chain and its profiles closed), then the form made of them all.
    fn finish_f6(&mut self, units: bimcraft_core::units::Units) -> Result<(), String> {
        let values = self.values();
        let Some(s) = self.sketch.as_mut() else { return Ok(()) };
        let kind = s.kind;
        let parts: Vec<Part> = match kind {
            FormKind::Blend => vec![Part::Main, Part::Top],
            FormKind::Sweep => vec![Part::Main, Part::Profile],
            _ => vec![Part::Main, Part::Profile, Part::End],
        };
        for &p in &parts {
            let lines: Vec<SketchLine> = s.part_lines(p).to_vec();
            let path = p == Part::Main && kind.swept();
            if path {
                let mut problem = s.path().err();
                if problem.is_none() && kind == FormKind::SweptBlend && fs::lines_of(&lines).len() != 1 {
                    problem = Some(t("A swept blend's path is one line or one arc.").to_string());
                }
                if let Some(e) = problem {
                    s.set_part(Part::Main);
                    return Err(tf("The path can't be swept along: {}", &[&e]));
                }
                continue;
            }
            if let Err(fs::Finish::Problems(problems)) = fs::finish_check(&lines, None, false) {
                s.set_part(p);
                s.sketch.wrong = problems.iter().flat_map(|q| q.lines()).collect();
                let which = match p {
                    Part::Main => t("The base"),
                    Part::Top => t("The top"),
                    Part::Profile if kind == FormKind::SweptBlend => t("Profile 1"),
                    Part::Profile => t("The profile"),
                    Part::End => t("Profile 2"),
                };
                return Err(format!("{which}: {}", crate::sketch::friendly(&problems, &lines, units)));
            }
        }
        let (index, void, host, twist) = (s.index, s.void, s.host, s.twist);
        let (from, to, material) = (s.from.clone(), s.to.clone(), s.material.clone());
        let guid = s.guid;
        let shown = s.shown.clone();
        let drawn: Vec<Vec<FormLine>> =
            [Part::Main, Part::Top, Part::Profile, Part::End].iter().map(|p| fs::lines_of(s.part_lines(*p))).collect();
        let before = match index.and_then(|i| self.forms.get(i)) {
            Some(ft) => ft.form()?,
            None => Form::default(),
        };
        let none = Swept::default();
        let was = before.sweep.as_ref().unwrap_or(&none);
        let mut num = reader(&values);
        let mut keep = |old: &[FormLine], k: usize| fs::kept(old, &shown[k], drawn[k].clone(), &mut num);
        let sketch = keep(&before.sketch, 0);
        let top = if kind == FormKind::Blend { keep(&before.top, 1) } else { Vec::new() };
        let sweep = kind.swept().then(|| Swept {
            profile: keep(&was.profile, 2),
            end: if kind == FormKind::SweptBlend { keep(&was.end, 3) } else { Vec::new() },
        });
        let (from, to) = if kind == FormKind::Blend { (text_expr(&from), text_expr(&to)) } else { (Expr::Number(0.0), Expr::Number(0.0)) };
        let mut form = Form {
            host,
            from,
            to,
            void,
            material: material.trim().to_string(),
            sketch,
            top,
            sweep,
            twist: if matches!(kind, FormKind::Blend | FormKind::SweptBlend) { twist } else { 0 },
            ..before.clone()
        };
        if form.guid.is_nil() {
            form.guid = guid;
        }
        self.commit_form(index, form, &before)
    }

    /// Form `form` (made by Finish from `before`, the one at `index`, or a new one) put in the draft with the locks added and
    /// taken off while sketching: built with each type first, a fault of it or a constraint a type can't hold refusing it,
    /// the draft as it was.
    fn commit_form(&mut self, index: Option<usize>, form: Form, before: &Form) -> Result<(), String> {
        let Some(s) = self.sketch.as_ref() else { return Ok(()) };
        let (added, unlocked) = (s.locks.clone(), s.unlocked.clone());
        // Its locks: those added while sketching, less those taken off, and none to a line that is gone (F7b).
        let mut dims = self.dimensions.clone();
        dims.retain(|d| !unlocked.contains(&d.guid));
        dims.extend(added);
        let mut gone = Family { dimensions: dims, forms: vec![form.clone()], ..Family::default() };
        gone.forget_lines_gone(std::slice::from_ref(before));
        let dims = gone.dimensions;
        let made = FormText { tag: index.and_then(|i| self.forms.get(i)).map_or(0, |f| f.tag), ..FormText::of(&form) };
        let made = if made.tag == 0 { FormText::of(&form) } else { made };
        // Built with the family, as the draft would be: refused, the draft untouched.
        let mut forms = self.forms.clone();
        let at = match index {
            Some(i) if i < forms.len() => {
                forms[i] = made;
                i
            }
            _ => {
                forms.push(made);
                forms.len() - 1
            }
        };
        let kept = std::mem::replace(&mut self.forms, forms);
        let kept_dims = std::mem::replace(&mut self.dimensions, dims);
        // Each type built with it: what is wrong with this form refuses it (another form's fault is the draft's, as before);
        // so do constraints a type can't hold with it.
        let built = self.family().and_then(|f| {
            let mine = format!("form {}:", at + 1);
            for name in f.types.keys() {
                let values = f.values(name, &BTreeMap::new())?;
                if let Err(e) = f.solve(&values) {
                    return Err(format!("{name}: {e}"));
                }
                if let Err(e) = f.pieces(&values)
                    && e.contains(&mine)
                {
                    return Err(e);
                }
            }
            Ok(())
        });
        if let Err(e) = built {
            self.forms = kept;
            self.dimensions = kept_dims;
            return Err(e);
        }
        self.views.aligning = None;
        self.sketch = None;
        self.form = Some(at);
        self.symbol = None;
        self.connector = None;
        Ok(())
    }

    /// Ctrl+Z while sketching: the sketch's own last change taken back.
    pub(crate) fn undo_sketch(&mut self) {
        if let Some(s) = self.sketch.as_mut() {
            s.points.clear();
            s.sketch.undo();
        }
    }

    /// A form's sketch in a plan or an elevation: the lines over the picture, and the clicks and drags that draw them.
    /// `pick_view` is what a click there picks of the view's planes and forms' edges (in the view's millimetres).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn sketch_in_view(
        &mut self,
        ui: &Ui,
        response: &egui::Response,
        painter: &egui::Painter,
        st: &Staged,
        which: Which,
        view: &bimcraft_core::render::View,
        pane: Rect,
        pick_view: &dyn Fn(Pos2) -> Option<PickedInView>,
    ) -> Option<String> {
        let frame = self.sketch_frame()?;
        let screen = |q: P2| {
            let (x, y) = view.px(in_view(st, which, &frame, q));
            Some(pane.min + vec2(x, y))
        };
        let seen_flat = flat(which, &frame);
        let at = |p: Pos2| on_plane(st, which, &frame, view.mm((p.x - pane.min.x, p.y - pane.min.y))).filter(|_| seen_flat);
        let reach = f64::from(REACH) / view.scale.max(1e-9);
        for (f, polys) in self.other_parts() {
            for poly in polys {
                let pts: Vec<Pos2> = poly
                    .iter()
                    .map(|q| {
                        let (x, y) = view.px(in_view(st, which, &f, *q));
                        pane.min + vec2(x, y)
                    })
                    .collect();
                painter.add(egui::Shape::line(pts, Stroke::new(1.5, GHOST)));
            }
        }
        let pick = |p: Pos2| -> Result<Picked, String> {
            let missed = || t("Pick a reference plane, a reference line or a form's edge in the view.").to_string();
            let ((a, b), lock) = pick_view(p).ok_or_else(missed)?;
            let (a, b) = (on_plane(st, which, &frame, a).ok_or_else(missed)?, on_plane(st, which, &frame, b).ok_or_else(missed)?);
            if a.dist(b) > 1.0 { Ok((Curve::line(a, b), lock)) } else { Err(square_to_plane()) }
        };
        self.sketch_input(ui, response, painter, &screen, &at, reach, &pick, seen_flat)
    }

    /// A form's sketch in {3D}: the lines on its plane, drawn over the picture taken through `cam` (stretched over `pane`), and
    /// the clicks that draw them on the plane where the pointer shows it.
    pub(crate) fn sketch_in_3d(
        &mut self,
        ui: &Ui,
        response: &egui::Response,
        painter: &egui::Painter,
        cam: &bimcraft_core::view3d::Camera,
        size: [u32; 2],
        pane: Rect,
    ) -> Option<String> {
        let frame = self.sketch_frame()?;
        let (sx, sy) = (pane.width() / size[0].max(1) as f32, pane.height() / size[1].max(1) as f32);
        let screen = |q: P2| {
            let (x, y, _) = cam.project(frame.point(q, 0.0));
            (x.is_finite() && y.is_finite()).then(|| pane.min + vec2(x as f32 * sx, y as f32 * sy))
        };
        let at = |p: Pos2| {
            let px = (f64::from((p.x - pane.min.x) / sx), f64::from((p.y - pane.min.y) / sy));
            let q = cam.on(px, frame.origin, frame.z)?;
            let d: [f64; 3] = std::array::from_fn(|i| q[i] - frame.origin[i]);
            Some(P2(dot3(d, frame.x), dot3(d, frame.y)))
        };
        // How far a few points reach on the plane, about where the pointer is.
        let reach = response
            .hover_pos()
            .and_then(|p| Some(at(p)?.dist(at(p + vec2(REACH, 0.0))?)))
            .filter(|r| r.is_finite() && *r > 0.0)
            .unwrap_or(10.0);
        // Pick Lines: the edge of another form, a reference line or the line a reference plane was drawn as, where it shows
        // nearest the click, seen square on to the plane.
        let values = self.values();
        let edges = self.family().map(|f| f.edges_3d(&values, self.sketch.as_ref().and_then(|s| s.index))).unwrap_or_default();
        let shown = |p: [f64; 3]| {
            let (x, y, _) = cam.project(p);
            (x.is_finite() && y.is_finite())
                .then(|| P2(f64::from(pane.min.x) + x * f64::from(sx), f64::from(pane.min.y) + y * f64::from(sy)))
        };
        for (f, polys) in self.other_parts() {
            for poly in polys {
                let pts: Vec<Pos2> =
                    poly.iter().filter_map(|q| shown(f.point(*q, 0.0))).map(|q| Pos2::new(q.0 as f32, q.1 as f32)).collect();
                painter.add(egui::Shape::line(pts, Stroke::new(1.5, GHOST)));
            }
        }
        // The reference lines and planes, drawn over the picture while sketching, so they can be seen and picked.
        let family = self.family().ok();
        for (a, b, plane) in family.iter().flat_map(|f| {
            f.lines.iter().map(|l| (l.start, l.end, false)).chain(f.planes.iter().filter(|p| !p.datum()).map(|p| (p.start, p.end, true)))
        }) {
            if let (Some(p), Some(q)) = (shown(a), shown(b)) {
                let ends = [Pos2::new(p.0 as f32, p.1 as f32), Pos2::new(q.0 as f32, q.1 as f32)];
                let stroke = Stroke::new(1.5, REFERENCE);
                if plane {
                    painter.extend(egui::Shape::dashed_line(&ends, stroke, 8.0, 4.0));
                } else {
                    painter.line_segment(ends, stroke);
                }
            }
        }
        let pick = |p: Pos2| -> Result<Picked, String> {
            let at = P2(f64::from(p.x), f64::from(p.y));
            let near = edges
                .iter()
                .filter_map(|e| Some((bimcraft_core::geom::dist_to_segment(at, shown(e.0)?, shown(e.1)?), *e)))
                .filter(|(d, _)| *d <= f64::from(REACH))
                .min_by(|a, b| a.0.total_cmp(&b.0));
            let (_, edge) = near.ok_or_else(|| t("Pick a reference plane, a reference line or a form's edge in the view.").to_string())?;
            let (a, b) = fs::on_plane(&frame, edge).map_err(|_| square_to_plane())?;
            Ok((Curve::line(a, b), None))
        };
        let on = response.hover_pos().is_some_and(|p| at(p).is_some());
        self.sketch_input(ui, response, painter, &screen, &at, reach, &pick, on || response.hover_pos().is_none())
    }

    /// What a view does with a form's sketch, however it puts the plane on screen (`screen`) and finds the plane's point under
    /// the pointer (`at`): the lines painted, a click a point of the shape (or a pick, or Modify's), a picked line's end
    /// dragged. `usable`: the view sees the plane, so it can be drawn on.
    #[allow(clippy::too_many_arguments)]
    fn sketch_input(
        &mut self,
        ui: &Ui,
        response: &egui::Response,
        painter: &egui::Painter,
        screen: &dyn Fn(P2) -> Option<Pos2>,
        at: &dyn Fn(Pos2) -> Option<P2>,
        reach: f64,
        pick: &dyn Fn(Pos2) -> Result<Picked, String>,
        usable: bool,
    ) -> Option<String> {
        // What it snaps to, found again when the draft changes.
        let key = {
            let mut h = DefaultHasher::new();
            self.family().ok().map(|f| serde_json::to_string(&f).unwrap_or_default()).hash(&mut h);
            (self.picked, self.sketch.as_ref().map(|s| (format!("{:?}", s.host), s.index))).hash(&mut h);
            h.finish()
        };
        if self.sketch.as_ref().is_some_and(|s| s.targets.as_ref().is_none_or(|t| t.0 != key)) {
            let values = self.values();
            let (host, index) = self.sketch.as_ref().map(|s| (s.host, s.index)).unwrap_or_default();
            let targets = self.family().map(|f| f.sketch_targets(&values, &host, index)).unwrap_or_default();
            if let Some(s) = self.sketch.as_mut() {
                s.targets = Some((key, targets));
            }
        }
        let snaps = self.snaps;
        let s = self.sketch.as_mut()?;
        s.chain = chain(ui.ctx());
        s.lock = lock(ui.ctx());
        // The pointer snapped, as a project's view snaps it, while a shape is drawn or a line's end dragged.
        let snapping = matches!(s.tool, Some(shape) if !matches!(shape, Shape::Fillet | Shape::PickLines)) || s.sketch.drag.is_some();
        let snap = |s: &FormSketch, raw: P2| -> bimcraft_core::snap::Found {
            let own: Vec<Curve> = s.sketch.lines.iter().map(|l| l.curve.clone()).collect();
            let (traces, segments) = s.targets.as_ref().map(|(_, t)| (t.traces.as_slice(), t.segments.as_slice())).unwrap_or_default();
            let reach = bimcraft_core::snap::Reach { point: reach, line: reach };
            bimcraft_core::snap::among(traces, segments, &own, raw, reach, &s.points, false, &snaps)
        };
        let found = response.hover_pos().and_then(at).filter(|_| snapping).map(|raw| snap(s, raw));
        let hover = found.map(|f| f.at).or_else(|| response.hover_pos().and_then(at));
        s.hover = hover;
        s.found = found;
        let hovered = hover.filter(|_| s.tool.is_none()).and_then(|p| s.sketch.line_at(p, reach));
        let mut problem = None;
        // A picked line's end, dragged with Modify: the lines joined there follow it.
        if s.tool.is_none()
            && response.drag_started_by(PointerButton::Primary)
            && let Some(p) = ui.input(|i| i.pointer.press_origin()).and_then(at)
            && let Some(g) = s.sketch.grip_at(p, reach)
        {
            s.sketch.drag = Some(Drag::Grip(g));
        }
        if response.drag_stopped() {
            if let (Some(Drag::Grip(from)), Some(to)) = (s.sketch.drag, hover) {
                s.sketch.move_end(from, to);
            }
            s.sketch.drag = None;
        }
        if response.clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            match at(pos).map(|raw| if snapping { snap(s, raw).at } else { raw }) {
                Some(p) => {
                    let shift = ui.input(|i| i.modifiers.shift);
                    problem = s.click(p, reach, shift, &|| pick(pos));
                }
                None if !usable => {
                    problem = Some(t("The view sees the work plane edge-on: sketch in a view that sees it flat, or in {3D}.").to_string())
                }
                None => {}
            }
        }
        s.sketch.track();
        s.paint(painter, screen, hovered);
        if let Some(f) = s.found
            && let Some(at) = screen(f.at)
        {
            let from = f.guide.and_then(|g| screen(g.from));
            crate::canvas::snap_mark(painter, at, from, f.kind, crate::canvas::Lock::Free, false, t(f.kind.name()).to_string());
        }
        problem
    }

    /// Align while a form is sketched (F7b), a click at `q` (the view's mm) in view `which`: first the reference (a line of
    /// the sketch, else `outside`, what the view shows there: a plane's trace, a reference line, another form's line, in
    /// the view's mm), then a line of the sketch, moved onto it with the lines joined to it, and its padlock offered.
    pub(crate) fn sketch_align(
        &mut self,
        st: &Staged,
        which: Which,
        q: P2,
        reach: f64,
        outside: Option<(DimRef, (P2, P2))>,
    ) -> Option<String> {
        let frame = self.sketch_frame()?;
        let p = on_plane(st, which, &frame, q)?;
        let s = self.sketch.as_mut()?;
        s.offer = None;
        let straight = |l: &SketchLine| matches!(&l.curve, Curve::Poly { points, arcs, closed: false } if points.len() == 2 && arcs.iter().all(|a| *a == 0.0));
        let own = s.sketch.line_at(p, reach).filter(|i| s.sketch.lines.get(*i).is_some_and(|l| straight(l) && !is_axis(l)));
        match s.align_from {
            None => {
                let picked = match (own, outside) {
                    (Some(i), _) => {
                        let l = &mut s.sketch.lines[i];
                        if l.guid.is_nil() {
                            l.guid = Guid::new();
                        }
                        Some((DimRef::Sketch(s.guid, l.guid), (l.start(), l.end()), Some(i)))
                    }
                    (None, Some((r, (a, b)))) => match (on_plane(st, which, &frame, a), on_plane(st, which, &frame, b)) {
                        (Some(a), Some(b)) if a.dist(b) > 1.0 => Some((r, (a, b), None)),
                        _ => None,
                    },
                    (None, None) => None,
                };
                match picked {
                    Some(r) => {
                        s.align_from = Some(r);
                        None
                    }
                    None => Some(t("Pick a reference plane, a reference line or a line, then a line of the sketch.").to_string()),
                }
            }
            Some((reference, line, fixed)) => {
                let Some(target) = own.filter(|i| Some(*i) != fixed) else {
                    return Some(t("Pick a straight line of the sketch: it moves onto the reference.").to_string());
                };
                let was = (s.sketch.lines[target].start(), s.sketch.lines[target].end());
                match bimcraft_core::align::sketch_lines(&s.sketch.lines, target, line, was, fixed) {
                    Ok(lines) => {
                        s.sketch.lines = lines;
                        s.sketch.changed();
                        let l = &mut s.sketch.lines[target];
                        if l.guid.is_nil() {
                            l.guid = Guid::new();
                        }
                        s.offer = Some(FamilyDimension {
                            id: 0,
                            guid: Guid::new(),
                            view: DimView::of(which),
                            refs: vec![reference, DimRef::Sketch(s.guid, l.guid)],
                            at: st.from_view(which, q),
                            label: None,
                            locked: vec![bimcraft_core::model::Lock { segment: 0, length: 0.0 }],
                            equal: false,
                            alignment: true,
                            kind: DimKind::Aligned,
                        });
                        s.align_from = None;
                        None
                    }
                    Err(e) => Some(e),
                }
            }
        }
    }

    /// The padlocks shown while a form is sketched, each where it is on screen (`screen` puts a point of the plane there):
    /// Align's offer, the locks added while sketching and those the form had, each beside the line it locks on its outer
    /// side, clear of the sketch's lines, of `lines` (what the view draws) and of each other (`dimensions::padlock_spot`).
    pub(crate) fn sketch_padlocks(&self, screen: &dyn Fn(P2) -> Option<Pos2>, lines: &[[Pos2; 2]]) -> Vec<(Pos2, Padlock)> {
        let Some(s) = self.sketch.as_ref() else { return Vec::new() };
        // The sketch's lines on screen: what a padlock keeps off, and whose loops are the inside it keeps out of.
        let mut own: Vec<[Pos2; 2]> = Vec::new();
        for l in s.sketch.lines.iter().filter(|l| !is_axis(l)) {
            let pts: Vec<Pos2> = l.curve.points().into_iter().filter_map(screen).collect();
            own.extend(pts.windows(2).map(|w| [w[0], w[1]]));
            if matches!(l.curve, Curve::Circle { .. })
                && let (Some(a), Some(b)) = (pts.last(), pts.first())
            {
                own.push([*a, *b]);
            }
        }
        let inside = |p: Pos2| {
            own.iter().filter(|[a, b]| (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x)).count() % 2 == 1
        };
        let mut avoid = own.clone();
        avoid.extend_from_slice(lines);
        let mut wanted: Vec<(&FamilyDimension, Padlock)> = Vec::new();
        if let Some(d) = &s.offer {
            wanted.push((d, Padlock::Offer));
        }
        wanted.extend(s.locks.iter().enumerate().map(|(k, d)| (d, Padlock::Added(k))));
        wanted.extend(self.dimensions.iter().filter(|d| d.alignment && !s.unlocked.contains(&d.guid)).map(|d| (d, Padlock::Had(d.guid))));
        let mut out: Vec<(Pos2, Padlock)> = Vec::new();
        for (d, what) in wanted {
            let Some(DimRef::Sketch(_, g)) = d.refs.iter().find(|r| matches!(r, DimRef::Sketch(f, _) if *f == s.guid)) else { continue };
            let Some(l) = s.sketch.lines.iter().find(|l| l.guid == *g) else { continue };
            let (Some(a), Some(b)) = (screen(l.start()), screen(l.end())) else { continue };
            let taken: Vec<Pos2> = out.iter().map(|(p, _)| *p).collect();
            out.push((super::dimensions::padlock_spot(a + (b - a) * 0.5, b - a, &inside, &avoid, &taken), what));
        }
        out
    }

    /// A padlock clicked while sketching: Align's offer locks, a lock unlocks.
    pub(crate) fn sketch_padlock(&mut self, what: Padlock) {
        let Some(s) = self.sketch.as_mut() else { return };
        match what {
            Padlock::Offer => s.locks.extend(s.offer.take()),
            Padlock::Added(k) => {
                if k < s.locks.len() {
                    s.locks.remove(k);
                }
            }
            Padlock::Had(g) => s.unlocked.push(g),
        }
    }

    /// The handles of a selected extrusion's Start and End in view `which`, where the view sees its plane edge-on: where each
    /// shows (the view's millimetres), whether it is End, whether it is free to drag (a number, not tied), and the normal it
    /// moves along.
    pub(crate) fn end_handles(&self, st: &Staged, which: Which) -> Vec<(P2, bool, bool, [f64; 3])> {
        let Some(i) = self.selected_form() else { return Vec::new() };
        let Some(ft) = self.forms.get(i) else { return Vec::new() };
        let Ok(f) = ft.form() else { return Vec::new() };
        if !matches!(f.kind(), FormKind::Extrusion | FormKind::Blend) || !f.mesh.is_empty() {
            return Vec::new();
        }
        let Ok(frame) = self.plane_set().frame(&f.host) else { return Vec::new() };
        if !edge_on(which, &frame) {
            return Vec::new();
        }
        let values = self.values();
        let Ok(loops) = f.loops(&mut reader(&values)) else { return Vec::new() };
        let Some(first) = loops.first() else { return Vec::new() };
        let c = bimcraft_core::geom::centroid(&first.outer);
        let free = |s: &str| s.trim().parse::<f64>().is_ok_and(f64::is_finite);
        let mut out = Vec::new();
        for (text, end) in [(&ft.from, false), (&ft.to, true)] {
            if let Some(d) = crate::adaptive_editor::worth(&text_expr(text), &values) {
                out.push((st.to_view(which, frame.point(c, d)), end, free(text), frame.z));
            }
        }
        out
    }

    /// A selected extrusion's Start or End (`end`) dragged `d` mm along its plane's normal. Refused while it is tied to a
    /// parameter or worked out by a formula.
    pub(crate) fn drag_end(&mut self, end: bool, d: f64) -> Result<(), String> {
        let i = self.selected_form().ok_or_else(String::new)?;
        let ft = &mut self.forms[i];
        let text = if end { &mut ft.to } else { &mut ft.from };
        let Some(x) = text.trim().parse::<f64>().ok().filter(|x| x.is_finite()) else {
            return Err(t("This end is tied to a parameter or worked out by a formula: change it in Family Types.").to_string());
        };
        *text = number_text(x + d);
        Ok(())
    }
}

/// What the status bar asks for while a form is sketched.
pub(crate) fn prompt(s: &FormSketch) -> &'static str {
    match s.trim {
        Some(None) => return msg!("Pick the first line, on the part to keep."),
        Some(Some(_)) => return msg!("Pick the second line, on the part to keep."),
        None => {}
    }
    match s.tool {
        Some(Shape::PickLines) if s.kind.swept() && s.part == Part::Main => {
            msg!("Click a reference plane, a reference line or a form's edge: a line of the path along it.")
        }
        Some(_) if s.kind.swept() && s.part == Part::Main => {
            msg!("Draw the path: lines and arcs end to end, open or closed; then Edit Profile.")
        }
        None => msg!("Click a line to pick it, Shift to add; drag a picked line's end; Delete takes the lines picked away."),
        Some(_) if s.axis_tool => msg!("Click two points: the axis the profile turns about."),
        Some(Shape::PickLines) if s.lock => {
            msg!("Click a reference plane, a reference line or a form's edge: a line along it, locked to it.")
        }
        Some(Shape::PickLines) => msg!("Click a reference plane, a reference line or a form's edge: a line along it."),
        Some(Shape::Fillet) => msg!("Click two lines that meet: they are rounded by an arc of the Options Bar's radius."),
        Some(_) => msg!("Click the points of the shape; Finish Edit Mode when the lines close into loops."),
    }
}

/// The Options Bar while a form is sketched: Depth (an extrusion's End less its Start: typed, Start stays and End moves),
/// a polygon's sides, a fixed Radius and Offset, as the project's Draw panel has them.
pub(crate) fn options_bar(ui: &mut Ui, ed: &mut FamilyEditor, units: bimcraft_core::units::Units) {
    let values = ed.values();
    let flat_view = ed.views.shown.is_some();
    let Some(s) = ed.sketch.as_mut() else { return };
    if matches!(s.kind, FormKind::Extrusion | FormKind::Blend) {
        let l = ui.label(t("Depth"));
        let (from, to) =
            (crate::adaptive_editor::worth(&text_expr(&s.from), &values), crate::adaptive_editor::worth(&text_expr(&s.to), &values));
        let shown = from.zip(to).map(|(a, b)| units.length(b - a)).unwrap_or_else(|| "?".into());
        let free = s.to.trim().parse::<f64>().is_ok_and(f64::is_finite);
        let key = UiId::new("family-sketch-depth");
        let mut text = ui.data(|d| d.get_temp::<String>(key)).unwrap_or(shown);
        let r = ui.add_enabled(free, TextEdit::singleline(&mut text).desired_width(90.0)).labelled_by(l.id);
        if r.changed()
            && let (Some(start), Some(d)) = (from, units.parse(&text).filter(|d| d.is_finite() && d.abs() <= 1e7))
        {
            s.to = number_text(start + d);
        }
        if r.has_focus() {
            ui.data_mut(|d| d.insert_temp(key, text));
        } else {
            ui.data_mut(|d| d.remove::<String>(key));
        }
        ui.separator();
    }
    // Chain: a line or an arc starts where the last ended; off, each starts afresh. Kept for the session.
    if s.tool.is_some_and(Shape::chains) && !s.axis_tool {
        let mut on = chain(ui.ctx());
        if ui.checkbox(&mut on, t("Chain")).changed() {
            ui.ctx().data_mut(|d| d.insert_temp(UiId::new(CHAIN), on));
            s.chain = on;
        }
        ui.separator();
    }
    if matches!(s.tool, Some(Shape::Inscribed | Shape::Circumscribed)) {
        let l = ui.label(t("Sides"));
        ui.add(TextEdit::singleline(&mut s.sides).desired_width(40.0)).labelled_by(l.id);
    }
    if matches!(s.tool, Some(Shape::Inscribed | Shape::Circumscribed | Shape::Circle | Shape::Fillet)) {
        ui.checkbox(&mut s.radius_on, t("Radius"));
        ui.add_enabled(s.radius_on, TextEdit::singleline(&mut s.radius).desired_width(70.0));
    }
    if s.tool.is_some_and(|t| t.offsets() || t == Shape::PickLines) && !s.axis_tool {
        let l = ui.label(t("Offset"));
        ui.add(TextEdit::singleline(&mut s.offset).desired_width(70.0)).labelled_by(l.id);
    }
    // Lock (F10a): a line picked from a reference plane, a reference line or a form's line locked to it, as Align's padlock
    // locks it; with no Offset, and in a plan or an elevation, where Align works too. Kept for the session.
    if s.tool == Some(Shape::PickLines) && !s.axis_tool {
        let usable = s.options().offset == 0.0 && flat_view;
        let mut on = lock(ui.ctx());
        let tip = if usable {
            msg!("Lock: each line picked is locked to what it was picked from, as Align's padlock locks it")
        } else {
            msg!("Lock locks a line picked with no Offset, in a plan or an elevation")
        };
        if ui.add_enabled(usable, egui::Checkbox::new(&mut on, t("Lock"))).on_hover_text(t(tip)).on_disabled_hover_text(t(tip)).changed() {
            ui.ctx().data_mut(|d| d.insert_temp(UiId::new(LOCK), on));
            s.lock = on;
        }
    }
}

impl BimApp {
    /// The contextual tab while a form is sketched, as Revit's "Modify | Create Extrusion": Mode (Finish and Cancel Edit
    /// Mode), Draw (Boundary Line and, for a revolve, Axis Line over the gallery and Pick Lines), Modify (Delete) and Work
    /// Plane (Show).
    pub(crate) fn form_sketch_panels(&mut self, ui: &mut Ui, ed: &mut FamilyEditor) {
        let units = self.session.model.units;
        let mut finish = false;
        let mut cancel = false;
        ribbon::panel(ui, msg!("Mode"), |ui| {
            let tip = msg!("Finish Edit Mode: the lines, closed into loops, become the form");
            finish |= crate::big_button(ui, Icon::Check, "Finish Edit Mode", false, tip).clicked();
            cancel |= crate::big_button(ui, Icon::Close, "Cancel Edit Mode", false, "Cancel Edit Mode: drop the lines, keep it as it was")
                .clicked();
        });
        // F6: a Blend's Edit Top and Edit Base, a sweep's path and profiles, and Edit Vertices' twist.
        let mut part = None;
        let mut twist = 0;
        if let Some(s) = ed.sketch.as_ref() {
            let (kind, on) = (s.kind, s.part);
            match kind {
                FormKind::Blend => ribbon::panel(ui, msg!("Blend"), |ui| {
                    let (label, tip, to) = if on == Part::Top {
                        (msg!("Edit Base"), msg!("Edit Base: sketch the blend's base, at its First End"), Part::Main)
                    } else {
                        (msg!("Edit Top"), msg!("Edit Top: sketch the blend's top, at its Second End"), Part::Top)
                    };
                    if crate::big_button(ui, Icon::Line, label, false, tip).clicked() {
                        part = Some(to);
                    }
                }),
                FormKind::Sweep | FormKind::SweptBlend => {
                    ribbon::panel(ui, if kind == FormKind::Sweep { msg!("Sweep") } else { msg!("Swept Blend") }, |ui| {
                        let tip = msg!("Sketch Path: draw the path on the work plane, lines and arcs end to end");
                        if crate::big_button(ui, Icon::Line, msg!("Sketch Path"), on == Part::Main, tip).clicked() {
                            part = Some(Part::Main);
                        }
                        let tip = msg!("Pick Path: pick reference planes, reference lines or forms' edges: lines of the path along them");
                        if crate::big_button(ui, Icon::PickFaces, msg!("Pick Path"), false, tip).clicked() {
                            part = Some(Part::Main);
                            twist = i32::MIN;
                        }
                        crate::stacked(ui, |ui| {
                            let profiles: &[(Part, &str, &str)] = if kind == FormKind::Sweep {
                                &[(
                                    Part::Profile,
                                    msg!("Edit Profile"),
                                    msg!("Edit Profile: sketch the profile on its plane, square to the path at its first line's middle"),
                                )]
                            } else {
                                &[
                                    (
                                        Part::Profile,
                                        msg!("Profile 1"),
                                        msg!("Profile 1: sketch the profile at the path's start, square to it"),
                                    ),
                                    (Part::End, msg!("Profile 2"), msg!("Profile 2: sketch the profile at the path's end, square to it")),
                                ]
                            };
                            for (p, label, tip) in profiles {
                                if crate::stacked_button(ui, Icon::Line, label, on == *p, tip).clicked() {
                                    part = Some(*p);
                                }
                            }
                            // Revit's Select Profile: a sketched profile; profile families are not here yet (F6c).
                            ui.add_enabled_ui(false, |ui| {
                                ComboBox::from_id_salt("sweep-profile").selected_text(t("<By Sketch>")).width(110.0).show_ui(ui, |_| {});
                            });
                        });
                    })
                }
                _ => {}
            }
            if matches!(kind, FormKind::Blend | FormKind::SweptBlend) {
                ribbon::panel(ui, msg!("Edit Vertices"), |ui| {
                    crate::stacked(ui, |ui| {
                        let tip = msg!("Twist Left: pair the top's vertices with the base's one place on, counter-clockwise");
                        if crate::stacked_button(ui, Icon::Rotate, msg!("Twist Left"), false, tip).clicked() {
                            twist = 1;
                        }
                        let tip = msg!("Twist Right: pair the top's vertices with the base's one place back, clockwise");
                        if crate::stacked_button(ui, Icon::Rotate, msg!("Twist Right"), false, tip).clicked() {
                            twist = -1;
                        }
                        ui.label(tf("Twist: {}", &[&s.twist.to_string()]));
                    });
                });
            }
        }
        if let Some(p) = part {
            match ed.show_part(p) {
                Ok(()) => self.status = Status::default(),
                Err(e) => self.status = Status { text: e, error: true },
            }
        }
        if let Some(s) = ed.sketch.as_mut() {
            if twist == i32::MIN {
                s.set_tool(Some(Shape::PickLines), false);
            } else if twist != 0 {
                s.twist = (s.twist + twist).clamp(-forms::MAX_TWIST, forms::MAX_TWIST);
            }
        }
        let Some(s) = ed.sketch.as_mut() else { return };
        ribbon::panel(ui, msg!("Draw"), |ui| {
            crate::stacked(ui, |ui| {
                let tip = msg!("Boundary Line: the lines of the profile");
                if crate::stacked_button(ui, Icon::Line, msg!("Boundary Line"), !s.axis_tool, tip).clicked() {
                    let tool = s.tool.or(Some(Shape::Line));
                    s.set_tool(tool, false);
                }
                if s.revolve && s.part == Part::Main {
                    let tip = msg!("Axis Line: the line the profile turns about, drawn by two clicks or picked");
                    if crate::stacked_button(ui, Icon::Line, msg!("Axis Line"), s.axis_tool, tip).clicked() {
                        s.set_tool(Some(Shape::Line), true);
                    }
                }
            });
            ui.vertical(|ui| {
                let mut shapes: Vec<Shape> = if s.axis_tool { vec![Shape::Line] } else { GALLERY.to_vec() };
                shapes.push(Shape::PickLines);
                for row in shapes.chunks(4) {
                    ui.horizontal(|ui| {
                        for shape in row {
                            if crate::small_button(
                                ui,
                                crate::draw::icon(*shape),
                                shape.label(),
                                s.tool == Some(*shape),
                                crate::draw::tip(*shape),
                            )
                            .clicked()
                            {
                                let axis = s.axis_tool;
                                s.set_tool(Some(*shape), axis);
                            }
                        }
                    });
                }
            });
        });
        let aligning = ed.views.aligning.is_some();
        let (mut align, mut trim) = (false, false);
        let Some(s) = ed.sketch.as_mut() else { return };
        ribbon::panel(ui, msg!("Modify"), |ui| {
            let tip = msg!("Modify (MD): pick lines of the sketch, and drag their ends");
            let trimming = s.trim.is_some();
            if crate::small_button(ui, Icon::Modify, "Modify", s.tool.is_none() && !aligning && !trimming, tip).clicked() {
                s.set_tool(None, false);
            }
            // Trim/Extend to Corner (TR): two lines trimmed or extended to where they meet, each keeping the part clicked.
            let tip = msg!("Trim/Extend to Corner (TR): pick two lines, on the parts to keep");
            if crate::small_button(ui, Icon::Trim, "Trim/Extend to Corner", trimming, tip).clicked() {
                trim = true;
            }
            // Align (AL, F7b): a line of the sketch onto a plane, a reference line or another line; its padlock locks them.
            let tip = msg!("Align (AL): pick a reference, then a line of the sketch that moves onto it; its padlock locks them");
            align = crate::small_button(ui, Icon::Align, "Align", aligning, tip).clicked();
            let tip = msg!("Delete (DE): the lines picked");
            if ui
                .add_enabled_ui(!s.sketch.picked.is_empty(), |ui| crate::small_button(ui, Icon::Delete, "Delete", false, tip))
                .inner
                .clicked()
            {
                s.delete_picked();
            }
        });
        ribbon::panel(ui, msg!("Work Plane"), |ui| {
            let tip =
                msg!("Set: the plane the sketch is drawn on, by name or picked in the view (the Work Plane dialog); its lines go with it");
            let on = ed.views.dialog.is_some() || ed.views.picking.is_some();
            if crate::big_button(ui, Icon::PickFaces, msg!("Set"), on, tip).clicked() {
                ed.open_work_plane_dialog(super::work_plane::Target::Sketch);
            }
            crate::stacked(ui, |ui| {
                let tip = msg!("Show: the view's work plane, tinted");
                if crate::stacked_button(ui, Icon::Visibility, msg!("Show"), ed.views.show_work_plane, tip).clicked() {
                    ed.views.show_work_plane = !ed.views.show_work_plane;
                }
            });
        });
        if align {
            ed.align_tool();
        }
        if trim {
            ed.views.aligning = None;
            if let Some(s) = ed.sketch.as_mut() {
                s.trim_tool();
            }
        }
        if finish {
            match ed.finish_form_sketch(units) {
                Ok(()) => self.status = Status::default(),
                Err(e) => self.status = Status { text: e, error: true },
            }
        }
        if cancel {
            ed.cancel_form_sketch();
            self.status = Status::default();
        }
    }

    /// Revit's Mode panel on a selected form's contextual tab: Edit Extrusion or Edit Revolve, its sketch drawn again.
    pub(crate) fn form_mode_panel(&mut self, ui: &mut Ui, ed: &mut FamilyEditor, i: usize) {
        let Some(f) = ed.forms.get(i) else { return };
        let (mesh, kind) = (f.mesh(), f.form().map(|x| x.kind()).unwrap_or(FormKind::Extrusion));
        let mut edit = false;
        ribbon::panel(ui, msg!("Mode"), |ui| {
            let (label, tip) = match kind {
                FormKind::Revolve => (msg!("Edit Revolve"), msg!("Edit Revolve: sketch its profile and its axis again")),
                FormKind::Blend => (msg!("Edit Blend"), msg!("Edit Blend: sketch its base and its top again")),
                FormKind::Sweep => (msg!("Edit Sweep"), msg!("Edit Sweep: sketch its path and its profile again")),
                FormKind::SweptBlend => (msg!("Edit Swept Blend"), msg!("Edit Swept Blend: sketch its path and its profiles again")),
                FormKind::Extrusion => (msg!("Edit Extrusion"), msg!("Edit Extrusion: sketch its profile again")),
            };
            edit = ui.add_enabled_ui(!mesh, |ui| crate::big_button(ui, Icon::Line, label, false, tip)).inner.clicked();
        });
        if edit && let Err(e) = ed.edit_form_sketch(i) {
            self.status = Status { text: e, error: true };
        }
    }

    /// Properties while a form is sketched: what it will be, Revit's Constraints (Extrusion Start and End, or a revolve's
    /// Start and End Angle, each a number or a formula), its Material and Solid/Void.
    pub(crate) fn form_sketch_properties(&mut self, ui: &mut Ui, ed: &mut FamilyEditor) {
        let mut materials = vec![String::new()];
        materials.extend(ed.params.iter().filter(|p| p.kind == Holds::Material).map(|p| p.name.clone()));
        materials.extend(self.session.model.materials.keys().cloned());
        let Some(s) = ed.sketch.as_mut() else { return };
        let what = match (s.kind, s.void) {
            (FormKind::Extrusion, false) => t("Extrusion"),
            (FormKind::Revolve, false) => t("Revolve"),
            (FormKind::Blend, false) => t("Blend"),
            (FormKind::Sweep, false) => t("Sweep"),
            (FormKind::SweptBlend, false) => t("Swept Blend"),
            (FormKind::Extrusion, true) => t("Void Extrusion"),
            (FormKind::Revolve, true) => t("Void Revolve"),
            (FormKind::Blend, true) => t("Void Blend"),
            (FormKind::Sweep, true) => t("Void Sweep"),
            (FormKind::SweptBlend, true) => t("Void Swept Blend"),
        };
        ui.label(RichText::new(what).strong());
        ui.add_space(6.0);
        if !s.kind.swept() {
            document::group(ui, msg!("Constraints"), |ui| {
                let (end, start) = match s.kind {
                    FormKind::Revolve => (msg!("End Angle"), msg!("Start Angle")),
                    FormKind::Blend => (msg!("Second End"), msg!("First End")),
                    _ => (msg!("Extrusion End"), msg!("Extrusion Start")),
                };
                for (label, text) in [(end, &mut s.to), (start, &mut s.from)] {
                    let l = ui.label(t(label));
                    ui.add(TextEdit::singleline(text).desired_width(120.0)).labelled_by(l.id);
                    ui.end_row();
                }
            });
        }
        document::group(ui, msg!("Materials and Finishes"), |ui| {
            let l = ui.label(t("Material"));
            let shown = if s.material.is_empty() { t("(none)").to_string() } else { s.material.clone() };
            let c = ComboBox::from_id_salt("sketch-material").selected_text(shown).width(120.0).truncate().show_ui(ui, |ui| {
                for m in &materials {
                    if ui.selectable_label(*m == s.material, if m.is_empty() { t("(none)") } else { m }).clicked() {
                        s.material.clone_from(m);
                    }
                }
            });
            c.response.labelled_by(l.id);
            ui.end_row();
        });
        document::group(ui, msg!("Identity Data"), |ui| {
            ui.label(t("Solid/Void"));
            ui.checkbox(&mut s.void, t("Void"));
            ui.end_row();
        });
    }
}
