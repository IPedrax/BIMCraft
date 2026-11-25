//! The Modify panel's tools in a family that isn't adaptive: Move, Copy, Rotate, Mirror - Pick Axis, Mirror - Draw Axis,
//! Scale and Offset on what is selected in a plan or an elevation, and while a form is sketched the same on the lines picked,
//! with Trim/Extend Single and Multiple Elements and Split Element besides Trim/Extend to Corner (`sketch.rs`).
//!
//! Outside sketch mode Move and Copy are the drag's (`drag_move.rs`) by two clicks, so what a constraint, a
//! formula, a datum or a pivot holds refuses them alike; Offset moves or copies the plane or reference line clicked by the
//! Options Bar's distance, square to its trace, toward the side clicked. Rotate, Mirror and Scale are the core's
//! (`Family::transformed`): what is drawn on what they take goes with it; a constraint they would break asks "Constraints
//! are not satisfied", whose Remove Constraints does it again without them. Each is one step of the history; a refusal
//! leaves everything as it was and says why. Only what the view shows is taken (a form Temporary Hide hides stays).
//!
//! In {3D}, Move, Copy, Rotate and Mirror - Draw Axis work on the view's work plane ([`FamilyEditor::modify_3d`]).
//!
//! Each tool says in the status bar what to click next, and where it can't be used (nothing selected, a form for Scale, a
//! sketch's tool outside a sketch, one that picks from the view in {3D}) its button is greyed with the reason in its
//! tooltip, and Find says it.
use super::drag_move::{Kept, Names, Origin};
use super::select_box::{How, Picked};
use super::*;
use bimcraft_core::curves::Curve;
use bimcraft_core::family::Unsat;
use bimcraft_core::family::constraints::{Named, Stays, Transform, TransformRefused, Transforming};
use bimcraft_core::family::views::Which;
use bimcraft_core::geom::P2;
use bimcraft_core::ids::Guid;
use bimcraft_core::sketch::SketchLine;
use egui::Pos2;

/// A Modify tool in hand, with the points clicked so far: in the view's millimetres outside sketch mode, on the sketch's plane
/// in it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Tool {
    /// Move, or Copy (`copy`): the point it moves from.
    Move {
        copy: bool,
        from: Option<P2>,
    },
    /// Rotate: the centre (none, the selection's own, found where it is shown), the ray it turns from, and Place Centre
    /// waiting for a click.
    Rotate {
        centre: Option<P2>,
        start: Option<P2>,
        placing: bool,
    },
    /// Mirror: Pick Axis, or Draw Axis (`draw`) with its first point.
    Mirror {
        draw: bool,
        start: Option<P2>,
    },
    /// Scale: its origin, then (Graphical) the point it is measured from.
    Scale {
        origin: Option<P2>,
        first: Option<P2>,
    },
    Offset,
    /// Trim/Extend Single (or Multiple) Element: the line picked to trim or extend to.
    TrimTo {
        boundary: Option<usize>,
        many: bool,
    },
    Split,
}

/// The tools as Find lists them on a family tab, by their names on the ribbon.
pub(crate) const FIND: [(&str, Tool); 10] = [
    ("Move", Tool::Move { copy: false, from: None }),
    ("Copy", Tool::Move { copy: true, from: None }),
    ("Rotate", Tool::Rotate { centre: None, start: None, placing: false }),
    (msg!("Mirror - Pick Axis"), Tool::Mirror { draw: false, start: None }),
    (msg!("Mirror - Draw Axis"), Tool::Mirror { draw: true, start: None }),
    (msg!("Scale"), Tool::Scale { origin: None, first: None }),
    ("Offset", Tool::Offset),
    (msg!("Trim/Extend Single Element"), Tool::TrimTo { boundary: None, many: false }),
    (msg!("Trim/Extend Multiple Elements"), Tool::TrimTo { boundary: None, many: true }),
    (msg!("Split Element"), Tool::Split),
];

/// The tool Find's `name` names.
pub(crate) fn found(name: &str) -> Option<Tool> {
    FIND.iter().find(|(n, _)| *n == name).map(|(_, t)| *t)
}

impl Tool {
    /// Its name on the ribbon.
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Tool::Move { copy: false, .. } => "Move",
            Tool::Move { copy: true, .. } => "Copy",
            Tool::Rotate { .. } => "Rotate",
            Tool::Mirror { draw: false, .. } => msg!("Mirror - Pick Axis"),
            Tool::Mirror { draw: true, .. } => msg!("Mirror - Draw Axis"),
            Tool::Scale { .. } => msg!("Scale"),
            Tool::Offset => "Offset",
            Tool::TrimTo { many: false, .. } => msg!("Trim/Extend Single Element"),
            Tool::TrimTo { many: true, .. } => msg!("Trim/Extend Multiple Elements"),
            Tool::Split => msg!("Split Element"),
        }
    }

    /// Whether it is the same tool as `other`, whatever has been clicked.
    pub(crate) fn same(&self, other: &Tool) -> bool {
        match (self, other) {
            (Tool::Move { copy: a, .. }, Tool::Move { copy: b, .. }) => a == b,
            (Tool::Mirror { draw: a, .. }, Tool::Mirror { draw: b, .. }) => a == b,
            (Tool::TrimTo { many: a, .. }, Tool::TrimTo { many: b, .. }) => a == b,
            (a, b) => std::mem::discriminant(a) == std::mem::discriminant(b),
        }
    }

    /// The tool as it is picked, nothing clicked, Rotate's centre to be found again: what another view starts from.
    pub(crate) fn fresh(&self) -> Tool {
        match *self {
            Tool::Move { copy, .. } => Tool::Move { copy, from: None },
            Tool::Rotate { .. } => Tool::Rotate { centre: None, start: None, placing: false },
            Tool::Mirror { draw, .. } => Tool::Mirror { draw, start: None },
            Tool::Scale { .. } => Tool::Scale { origin: None, first: None },
            Tool::TrimTo { many, .. } => Tool::TrimTo { boundary: None, many },
            t => t,
        }
    }

    /// Whether it acts on what is selected (picked, in a sketch), rather than on what is clicked.
    fn on_selection(&self) -> bool {
        matches!(self, Tool::Move { .. } | Tool::Rotate { .. } | Tool::Mirror { .. } | Tool::Scale { .. })
    }

    /// Whether it works in {3D}: one whose clicks are points, which land on the view's work plane. One that picks
    /// something of the view (an axis, what to offset) doesn't, until {3D} picks references.
    fn in_3d(&self) -> bool {
        matches!(self, Tool::Move { .. } | Tool::Rotate { .. } | Tool::Mirror { draw: true, .. })
    }

    /// Whether it is a sketch's alone.
    fn sketch_only(&self) -> bool {
        matches!(self, Tool::TrimTo { .. } | Tool::Split)
    }

    /// The tool as it was picked, the points clicked let go (the centre Rotate found kept); none when there were none.
    pub(crate) fn back(&self) -> Option<Tool> {
        let back = match *self {
            Tool::Move { copy, from: Some(_) } => Tool::Move { copy, from: None },
            Tool::Rotate { centre, start, placing } if start.is_some() || placing => Tool::Rotate { centre, start: None, placing: false },
            Tool::Mirror { draw, start: Some(_) } => Tool::Mirror { draw, start: None },
            Tool::Scale { origin: Some(_), .. } => Tool::Scale { origin: None, first: None },
            Tool::TrimTo { boundary: Some(_), many } => Tool::TrimTo { boundary: None, many },
            _ => return None,
        };
        Some(back)
    }
}

/// The Options Bar of the Modify tools, kept for the session: Move's and Copy's Constrain and Multiple, Rotate's Angle and
/// Copy, Mirror's Copy (on at first), Scale's Graphical or Numerical and its factor, Offset's distance and Copy.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Options {
    pub(crate) constrain: bool,
    pub(crate) multiple: bool,
    pub(crate) rotate_copy: bool,
    pub(crate) angle: String,
    pub(crate) mirror_copy: bool,
    pub(crate) numerical: bool,
    pub(crate) factor: String,
    pub(crate) offset: String,
    pub(crate) offset_copy: bool,
    /// The project's units, which Offset's distance is typed in: the window's, given each frame.
    pub(crate) units: bimcraft_core::units::Units,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            constrain: false,
            multiple: false,
            rotate_copy: false,
            angle: String::new(),
            mirror_copy: true,
            numerical: false,
            factor: "2".into(),
            offset: "300".into(),
            offset_copy: true,
            units: Default::default(),
        }
    }
}

/// A Modify tool in a plan or an elevation: the tool, the view its points are in, and an angle typed on the Options Bar
/// waiting for the view to turn the selection by it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Modifying {
    pub(crate) tool: Tool,
    pub(crate) which: Option<Which>,
    pub(crate) typed: Option<f64>,
    /// The selection as the tool last saw it: changed otherwise (a click, Delete), the points clicked are let go.
    pub(crate) selection: Vec<Picked>,
}

/// What a rotation, a mirror or a scale is asked to do again once Remove Constraints has taken off what held it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Again {
    items: Vec<Picked>,
    transform: Transform,
    copy: bool,
}

/// Why a rotation, a mirror or a scale was refused: what to say, or the constraints that would break, to ask about.
pub(super) enum Refused {
    Said(String),
    Unsat(Unsat),
}

impl From<String> for Refused {
    fn from(s: String) -> Refused {
        Refused::Said(s)
    }
}

/// What Offset takes where a click is: a plane or a reference line and its trace in the view, or why not; none, nothing.
pub(crate) type OffsetHit = Option<Result<(Picked, (P2, P2)), String>>;

/// A click with a Modify tool in a plan or an elevation: where (the view's mm), the axis Mirror - Pick Axis takes there, and
/// what Offset takes.
pub(crate) type Click = (P2, Option<(P2, P2)>, OffsetHit);

/// The view's way into the family's space: where its origin is and its two ways, so a point of it (mm) is a point of the
/// family's.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Space {
    pub(crate) origin: [f64; 3],
    pub(crate) across: [f64; 3],
    pub(crate) up: [f64; 3],
}

impl Space {
    pub(crate) fn of(st: &bimcraft_core::family::views::Staged, which: Which) -> Space {
        Space { origin: st.from_view(which, P2(0.0, 0.0)), across: st.vector(which, P2(1.0, 0.0)), up: st.vector(which, P2(0.0, 1.0)) }
    }

    /// {3D}'s way in: its work plane, whose two axes are the tools' as a plan's are.
    pub(crate) fn on_plane(f: &bimcraft_core::family::Frame) -> Space {
        Space { origin: f.origin, across: f.x, up: f.y }
    }

    fn point(&self, q: P2) -> [f64; 3] {
        std::array::from_fn(|k| self.origin[k] + self.across[k] * q.0 + self.up[k] * q.1)
    }

    /// Where a point of the family's stands on the plane, and how far off it, along what the plane faces.
    fn coords(&self, p: [f64; 3]) -> (P2, f64) {
        let d: [f64; 3] = std::array::from_fn(|k| p[k] - self.origin[k]);
        let dot = |a: [f64; 3]| d[0] * a[0] + d[1] * a[1] + d[2] * a[2];
        (P2(dot(self.across), dot(self.up)), dot(self.toward()))
    }

    /// The point of the family's at `q` on the plane and `off` it.
    fn lifted(&self, q: P2, off: f64) -> [f64; 3] {
        let (p, n) = (self.point(q), self.toward());
        std::array::from_fn(|k| p[k] + n[k] * off)
    }

    pub(crate) fn vector(&self, v: P2) -> [f64; 3] {
        std::array::from_fn(|k| self.across[k] * v.0 + self.up[k] * v.1)
    }

    /// Toward whoever looks at the view: what a turn counter-clockwise on screen turns about.
    fn toward(&self) -> [f64; 3] {
        let (a, b) = (self.across, self.up);
        [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
    }

    /// A turn of `degrees` counter-clockwise on screen about `centre`.
    fn rotate(&self, centre: P2, degrees: f64) -> Transform {
        Transform::Rotate { centre: self.point(centre), axis: self.toward(), degrees }
    }

    /// A mirror across the line through `a` and `b`, square to the view.
    fn mirror(&self, (a, b): (P2, P2)) -> Transform {
        let d = self.vector(b - a);
        let n = self.toward();
        let normal = [d[1] * n[2] - d[2] * n[1], d[2] * n[0] - d[0] * n[2], d[0] * n[1] - d[1] * n[0]];
        Transform::Mirror { point: self.point(a), normal }
    }
}

/// What a Modify tool takes in {3D}, as its work plane has it: each form's box (its eight corners, each where it stands on
/// the plane and how far off it), and each plane's and line's two ends.
pub(crate) type Taken3d = (Vec<[(P2, f64); 8]>, Vec<[(P2, f64); 2]>);

/// The faces of a box by its corners `at` (numbered by three bits: across, up and off the plane), as marks the 3D picture draws with its
/// depth: no fill, each face's outline in `ink`, hidden where a solid stands in front of it.
fn box_marks(at: &[[f64; 3]; 8], ink: egui::Color32) -> Vec<bimcraft_core::view3d::PlaneMark> {
    const FACES: [[usize; 3]; 6] = [[0, 1, 2], [4, 5, 6], [0, 1, 4], [2, 3, 6], [0, 2, 4], [1, 3, 5]];
    let face = |[a, b, c]: [usize; 3]| {
        let (o, u, v) =
            (at[a], std::array::from_fn::<f64, 3, _>(|k| at[b][k] - at[a][k]), std::array::from_fn::<f64, 3, _>(|k| at[c][k] - at[a][k]));
        let (lu, lv) = (u.iter().map(|x| x * x).sum::<f64>().sqrt(), v.iter().map(|x| x * x).sum::<f64>().sqrt());
        (lu > 1e-6 && lv > 1e-6).then(|| bimcraft_core::view3d::PlaneMark {
            origin: o,
            axes: (u.map(|x| x / lu), v.map(|x| x / lv)),
            lo: (0.0, 0.0),
            hi: (lu, lv),
            step: 0.0,
            fill: [0; 4],
            grid: [0; 4],
            edge: ink.to_array(),
            edge_width: 1.5,
        })
    };
    FACES.into_iter().filter_map(face).collect()
}

/// The box of `points` (the family's) in `space`'s plane: its eight corners, each where it stands on the plane and how far
/// off it; none of no points.
fn boxed(space: &Space, points: &[[f64; 3]]) -> Option<[(P2, f64); 8]> {
    let at: Vec<(P2, f64)> = points.iter().map(|p| space.coords(*p)).collect();
    let (first, _) = at.split_first()?;
    let (mut lo, mut hi) = ([first.0.0, first.0.1, first.1], [first.0.0, first.0.1, first.1]);
    for (q, h) in &at {
        for (k, v) in [q.0, q.1, *h].into_iter().enumerate() {
            (lo[k], hi[k]) = (lo[k].min(v), hi[k].max(v));
        }
    }
    let pick = |bit: usize, k: usize| if bit == 0 { lo[k] } else { hi[k] };
    Some(std::array::from_fn(|i| (P2(pick(i & 1, 0), pick((i >> 1) & 1, 1)), pick((i >> 2) & 1, 2))))
}

/// The turn from `from` to `to` about `centre`, degrees counter-clockwise, snapped to the Snaps' angle as the project's Rotate.
fn turn(centre: P2, from: P2, to: P2, snaps: &bimcraft_core::snap::Snaps) -> f64 {
    crate::canvas::angle_between(centre, from, to, snaps.angle)
}

/// What a click where the Scale tool is gives as its factor: Numerical, the Options Bar's; Graphical, how far `to` is from
/// `origin` over how far `first` is.
fn factor_of(o: &Options, origin: P2, first: Option<P2>, to: P2) -> Option<f64> {
    if o.numerical {
        return o.factor.trim().parse::<f64>().ok();
    }
    let first = first?;
    let d = origin.dist(first);
    (d > 1e-6).then(|| origin.dist(to) / d)
}

/// The 2D map of view (or plane) points a tool would do with the pointer at `to`: what its preview draws.
fn preview_map(
    tool: &Tool,
    o: &Options,
    to: P2,
    shift: bool,
    snaps: &bimcraft_core::snap::Snaps,
    axis: Option<(P2, P2)>,
) -> Option<Box<dyn Fn(P2) -> P2>> {
    match *tool {
        Tool::Move { from: Some(a), .. } => {
            let d = super::drag_move::delta(a, to, shift || o.constrain);
            Some(Box::new(move |q| q + d))
        }
        Tool::Rotate { centre: Some(c), start: Some(s), placing: false } => {
            let (sn, cs) = turn(c, s, to, snaps).to_radians().sin_cos();
            Some(Box::new(move |q| {
                let v = q - c;
                P2(c.0 + v.0 * cs - v.1 * sn, c.1 + v.0 * sn + v.1 * cs)
            }))
        }
        Tool::Mirror { draw: true, start: Some(a) } => mirror_map(a, to),
        Tool::Mirror { draw: false, .. } => axis.and_then(|(a, b)| mirror_map(a, b)),
        Tool::Scale { origin: Some(c), first } if first.is_some() || o.numerical => {
            let k = factor_of(o, c, first, to).filter(|k| k.is_finite() && *k > 0.0)?;
            Some(Box::new(move |q| c + (q - c) * k))
        }
        _ => None,
    }
}

fn mirror_map(a: P2, b: P2) -> Option<Box<dyn Fn(P2) -> P2>> {
    let d = b - a;
    let l = (d.0 * d.0 + d.1 * d.1).sqrt();
    if l < 1e-9 {
        return None;
    }
    let u = P2(d.0 / l, d.1 / l);
    Some(Box::new(move |q| {
        let v = q - a;
        let along = v.dot(u);
        a + u * (2.0 * along) - v
    }))
}

/// The unit way square to the line `(a, b)` toward `q`; none for a line of no length.
fn side((a, b): (P2, P2), q: P2) -> Option<P2> {
    let way = b - a;
    let len = (way.0 * way.0 + way.1 * way.1).sqrt();
    if len < 1e-9 {
        return None;
    }
    let n = P2(-way.1 / len, way.0 / len);
    Some(if (q - a).dot(n) < 0.0 { P2(-n.0, -n.1) } else { n })
}

/// Where Offset would put the line `line` (the view's mm) with the pointer at `q`: its preview.
pub(crate) fn offset_line(line: (P2, P2), q: P2, o: &Options) -> Option<(P2, P2)> {
    let d = o.units.parse(&o.offset).filter(|d| d.is_finite() && *d > 0.0)?;
    let n = side(line, q)? * d;
    Some((line.0 + n, line.1 + n))
}

/// `q` (the view's mm) snapped to the traces of the planes and lines `traces` within `reach`: where two cross near it, there;
/// else onto the nearest. A point clicked for a Modify tool lands on the references it is meant to, so a
/// half turn about a point on the level line keeps what is drawn on the level on it.
pub(crate) fn snap_to(q: P2, traces: &[(P2, P2)], reach: f64) -> P2 {
    let near: Vec<(f64, (P2, P2))> = traces
        .iter()
        .filter(|(a, b)| a.dist(*b) > 1e-9)
        .map(|s| (bimcraft_core::geom::dist_to_segment(q, s.0, s.1), *s))
        .filter(|(d, _)| *d <= reach)
        .collect();
    let crossing = |(a, b): (P2, P2), (c, d): (P2, P2)| -> Option<P2> {
        let (r, s) = (b - a, d - c);
        let den = r.cross(s);
        if den.abs() < 1e-9 * r.dot(r).sqrt() * s.dot(s).sqrt() {
            return None;
        }
        let t = (c - a).cross(s) / den;
        Some(a + r * t)
    };
    let mut best: Option<(f64, P2)> = None;
    for (i, (_, s)) in near.iter().enumerate() {
        for (_, u) in &near[i + 1..] {
            if let Some(x) = crossing(*s, *u).filter(|x| x.dist(q) <= reach * 1.5)
                && best.is_none_or(|(d, _)| x.dist(q) < d)
            {
                best = Some((x.dist(q), x));
            }
        }
    }
    if let Some((_, x)) = best {
        return x;
    }
    near.iter()
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, (a, b))| {
            let r = *b - *a;
            *a + r * ((q - *a).dot(r) / r.dot(r))
        })
        .unwrap_or(q)
}

/// What the status bar asks for while `tool` is in hand.
pub(crate) fn prompt(tool: &Tool, o: &Options, sketch: bool) -> &'static str {
    match *tool {
        Tool::Move { copy: false, from: None } => msg!("Move: click the point to move from."),
        Tool::Move { copy: true, from: None } => msg!("Copy: click the point to copy from."),
        Tool::Move { copy: false, .. } => {
            msg!("Move: click the point to move to (Shift or Constrain keeps it square). Esc picks another start.")
        }
        Tool::Move { copy: true, .. } if o.multiple => {
            msg!("Copy: click where each copy goes (Multiple is on: one more copy each click). Esc stops.")
        }
        Tool::Move { copy: true, .. } => msg!("Copy: click the point to copy to (Shift or Constrain keeps it square)."),
        Tool::Rotate { placing: true, .. } => msg!("Rotate: click the centre of rotation."),
        Tool::Rotate { start: None, .. } => {
            msg!("Rotate: click a point on the ray to turn from (Place Centre moves the centre), or type an Angle and press Enter.")
        }
        Tool::Rotate { .. } => msg!("Rotate: click a point on the ray to turn to. Esc picks the first ray again."),
        Tool::Mirror { draw: false, .. } if sketch => {
            msg!("Mirror - Pick Axis: click a line of the sketch, a reference plane or a reference line as the axis.")
        }
        Tool::Mirror { draw: false, .. } => {
            msg!("Mirror - Pick Axis: click a reference plane, a reference line or a form's edge as the axis.")
        }
        Tool::Mirror { start: None, .. } => msg!("Mirror - Draw Axis: click the axis' first point."),
        Tool::Mirror { .. } => msg!("Mirror - Draw Axis: click the axis' second point."),
        Tool::Scale { origin: None, .. } => msg!("Scale: click the origin it scales about."),
        Tool::Scale { .. } if o.numerical => msg!("Scale: click the origin; it scales by the Options Bar's factor."),
        Tool::Scale { first: None, .. } => msg!("Scale: click the point to measure from."),
        Tool::Scale { .. } => msg!("Scale: click where that point goes; the scale is how far it is from the origin over how far it was."),
        Tool::Offset if sketch => {
            msg!("Offset: click a line of the sketch; the line goes the Options Bar's distance toward the side clicked.")
        }
        Tool::Offset => {
            msg!("Offset: click a reference plane or a reference line; it goes the Options Bar's distance toward the side clicked.")
        }
        Tool::TrimTo { boundary: None, .. } => msg!("Pick a line of the sketch as the trim/extend boundary."),
        Tool::TrimTo { many: false, .. } => msg!("Pick the line to trim or extend to it, on the part to keep."),
        Tool::TrimTo { many: true, .. } => msg!("Pick each line to trim or extend to it, on the part to keep. Esc picks another boundary."),
        Tool::Split => msg!("Split Element: click a line of the sketch where it is split in two."),
    }
}

/// The Options Bar while a Modify tool is in hand, in or out of sketch mode.
pub(crate) fn options_bar(ui: &mut Ui, o: &mut Options, tool: &mut Tool, units: bimcraft_core::units::Units) -> Option<f64> {
    let mut typed = None;
    match tool {
        Tool::Move { copy, .. } => {
            ui.checkbox(&mut o.constrain, t("Constrain"));
            if *copy {
                ui.checkbox(&mut o.multiple, t("Multiple"));
            }
        }
        Tool::Rotate { placing, .. } => {
            let l = ui.label(t("Angle:"));
            let r = ui.add(TextEdit::singleline(&mut o.angle).desired_width(60.0).hint_text("°")).labelled_by(l.id);
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                typed = Some(o.angle.trim().parse::<f64>().ok().filter(|a| a.is_finite()).unwrap_or(f64::NAN));
            }
            if ui.button(t("Place Centre")).clicked() {
                *placing = true;
            }
            ui.checkbox(&mut o.rotate_copy, t("Copy"));
        }
        Tool::Mirror { .. } => {
            ui.checkbox(&mut o.mirror_copy, t("Copy"));
        }
        Tool::Scale { .. } => {
            ui.radio_value(&mut o.numerical, false, t("Graphical"));
            ui.radio_value(&mut o.numerical, true, t("Numerical"));
            let l = ui.label(t("Scale:"));
            ui.add_enabled(o.numerical, TextEdit::singleline(&mut o.factor).desired_width(50.0)).labelled_by(l.id);
        }
        Tool::Offset => {
            let l = ui.label(t("Offset:"));
            ui.add(TextEdit::singleline(&mut o.offset).desired_width(70.0).hint_text(units.name())).labelled_by(l.id);
            ui.checkbox(&mut o.offset_copy, t("Copy"));
        }
        Tool::TrimTo { .. } | Tool::Split => {}
    }
    typed
}

/// What a refusal of a rotation, a mirror or a scale says, naming what it is about as `names` does.
/// The datum `n` (a form or a reference line, as `names` names it) is drawn on, by its name, if it is drawn on one.
fn datum_host(names: &Names, n: Named) -> Option<String> {
    let f = names.family;
    let host = match names.origin.named(n) {
        Named::Form(i) => f.forms.get(i)?.host,
        Named::Line(g) => f.lines.iter().find(|l| l.guid == g)?.host,
        Named::Plane(_) => return None,
    };
    match host {
        bimcraft_core::family::PlaneRef::Plane(g) if bimcraft_core::family::DATUMS.contains(&g) => Some(f.said(Named::Plane(g))),
        _ => None,
    }
}

fn refused_said(r: &TransformRefused, t: &Transform, names: &Names) -> String {
    let said = |n: &Named| names.said(*n);
    let done = match t {
        Transform::Rotate { .. } => crate::lang::t(msg!("rotated")),
        Transform::Mirror { .. } => crate::lang::t(msg!("mirrored")),
        Transform::Scale { .. } => crate::lang::t(msg!("scaled")),
    };
    match r {
        TransformRefused::Stays(s) => match s {
            Stays::Datum(n) => tf("{} is a datum: it stays where it is.", &[&said(n)]),
            Stays::Origin(n) => tf("{} defines the origin: it stays where it is.", &[&said(n)]),
            Stays::Pivot(n) => tf("{} turns about its pivot: turn it by its rotate grip.", &[&said(n)]),
            Stays::Formula(n) => tf("A number of {} it would change is worked out by a formula: change it in Family Types.", &[&said(n)]),
            Stays::Hosted(n) => match datum_host(names, *n) {
                // A datum can't be taken along: the view must see it flat.
                Some(d) => tf(
                    "{} is drawn on {}, a datum, which stays: it would leave it. Do it in a view that sees {} flat.",
                    &[&said(n), &d, &d],
                ),
                None => tf(
                    "{} would leave the plane it is drawn on: select that plane too, or do it in a view that sees the plane flat.",
                    &[&said(n)],
                ),
            },
            Stays::Mesh(n) => tf("{} is a file's triangles: it has no sketch to change.", &[&said(n)]),
        },
        TransformRefused::Unsat(u) => names.origin.unsat(u),
        TransformRefused::NotScaled(n) => tf(
            "{} isn't scaled: Scale takes reference planes and reference lines; a form is sized by its dimensions and parameters.",
            &[&said(n)],
        ),
        TransformRefused::NotMirrored(n) => {
            tf("{} is a sweep: its profile would face the wrong way, so it isn't mirrored or turned over.", &[&said(n)])
        }
        TransformRefused::Nested(i) => {
            tf("Nested instance {} is placed on what would be {}: move it off first.", &[&(i + 1).to_string(), done])
        }
        TransformRefused::Bad(e) => e.clone(),
    }
}

impl FamilyEditor {
    /// Why `tool` can't be picked now, if it can't: what its greyed button's tooltip and its key say.
    pub(crate) fn modify_blocked(&self, tool: &Tool) -> Option<String> {
        if self.adaptive.is_some() {
            return Some(t("An adaptive family is modified in {3D}.").to_string());
        }
        if let Some(s) = self.sketch.as_ref() {
            if tool.on_selection() && s.sketch.picked.is_empty() {
                return Some(tf("Pick lines of the sketch first, then {}.", &[t(tool.label())]));
            }
            return None;
        }
        if tool.sketch_only() {
            return Some(tf("{} works on a sketch's lines: edit a form's sketch (double-click the form) first.", &[t(tool.label())]));
        }
        if !self.editing_2d() {
            // {3D}: a tool whose clicks are points works on the view's work plane.
            if !tool.in_3d() {
                return Some(tf("{} picks from the view, which {3D} can't do yet: use it in a plan or an elevation.", &[t(tool.label())]));
            }
            if let Err(e) = self.work_frame_3d() {
                return Some(e);
            }
        }
        if !tool.on_selection() {
            return None;
        }
        let items: Vec<Picked> = self.selection.items().iter().copied().filter(|p| self.holds(*p)).collect();
        if items.is_empty() {
            return Some(tf("Select what {} takes first.", &[t(tool.label())]));
        }
        let turns = matches!(tool, Tool::Rotate { .. } | Tool::Mirror { .. } | Tool::Scale { .. });
        if turns && items.iter().any(|p| matches!(p, Picked::Symbol(_) | Picked::Connector(_) | Picked::Nested(_))) {
            return Some(tf(
                "{} takes forms, reference planes, reference lines and dimensions: take symbols, connectors and nested instances out of the selection, or move them.",
                &[t(tool.label())],
            ));
        }
        if turns && !items.iter().any(|p| matches!(p, Picked::Form(_) | Picked::Plane(_) | Picked::Line(_))) {
            return Some(tf(
                "{} takes forms, reference planes and reference lines: a dimension goes with what it measures.",
                &[t(tool.label())],
            ));
        }
        if matches!(tool, Tool::Scale { .. }) && items.iter().any(|p| matches!(p, Picked::Form(_))) {
            return Some(
                t("Scale takes reference planes and reference lines: a form is sized by its dimensions and parameters.").to_string(),
            );
        }
        None
    }

    /// `tool` picked from the ribbon or by its key: in hand, the other tools put down; or why it can't be.
    pub(crate) fn modify_tool_on(&mut self, tool: Tool) -> Result<(), String> {
        if let Some(why) = self.modify_blocked(&tool) {
            return Err(why);
        }
        self.views.aligning = None;
        self.views.dimensioning = None;
        self.views.pasting = None;
        if let Some(s) = self.sketch.as_mut() {
            s.pasting = None;
            s.set_tool(None, false);
            s.edit = Some(tool);
            s.edit_seen = None;
            return Ok(());
        }
        self.views.drawing = None;
        self.views.measuring = None;
        self.views.picking = None;
        self.views.pivoting = None;
        self.placing = None;
        let selection = self.selection.items().to_vec();
        self.views.modify = Some(Modifying { tool, which: self.views.shown, typed: None, selection });
        Ok(())
    }

    /// {3D}'s work plane: where a Modify tool's clicks land there.
    fn work_frame_3d(&self) -> Result<bimcraft_core::family::Frame, String> {
        self.plane_set().frame(&self.views.work_plane())
    }

    /// A Modify tool in hand in {3D}: its clicks land on the view's work plane, whose frame is the tool's space as
    /// a plan's is, so the edit is the one a plan makes; a drag still turns the view. `click`, where one landed. It takes
    /// of the selection what a plan would, but a form hidden for now. Over the picture go the tool's marks and its
    /// rubber band on the plane, and where the click would put them the planes and lines taken; the box of each form taken
    /// is left for the picture to draw with its depth (`ghost_3d`). A tool taken in another view starts again, or is put
    /// down where {3D} can't work with it. What went wrong, if anything.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn modify_3d(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        painter: &egui::Painter,
        cam: &bimcraft_core::view3d::Camera,
        size: [u32; 2],
        pane: egui::Rect,
        click: Option<Pos2>,
    ) -> Option<String> {
        self.views.ghost_3d.clear();
        let m = self.views.modify.as_ref()?;
        // Its points were another view's: here it starts again, where it can work at all.
        if m.which.is_some() {
            let tool = m.tool.fresh();
            if let Some(why) = self.modify_blocked(&tool) {
                self.views.modify = None;
                return Some(why);
            }
            if let Some(m) = self.views.modify.as_mut() {
                (m.which, m.tool) = (None, tool);
            }
        }
        // Nor do they hold once the selection changed otherwise: the tool starts again on what is selected.
        if let Some(m) = self.views.modify.as_mut()
            && m.selection.as_slice() != self.selection.items()
        {
            (m.selection, m.tool) = (self.selection.items().to_vec(), m.tool.fresh());
        }
        let frame = match self.work_frame_3d() {
            Ok(f) => f,
            Err(e) => {
                self.views.modify = None;
                return Some(e);
            }
        };
        let space = Space::on_plane(&frame);
        let (sx, sy) = (pane.width() / size[0].max(1) as f32, pane.height() / size[1].max(1) as f32);
        let project = |p: [f64; 3]| {
            let (x, y, _) = cam.project(p);
            (x.is_finite() && y.is_finite()).then(|| pane.min + egui::vec2(x as f32 * sx, y as f32 * sy))
        };
        // How squarely the view looks at the plane: within two degrees of edge on, a click would land far off along it.
        let facing = {
            let (a, e) = (cam.azimuth.to_radians(), cam.elevation.to_radians());
            let back = [e.cos() * a.cos(), e.cos() * a.sin(), e.sin()];
            (back[0] * frame.z[0] + back[1] * frame.z[1] + back[2] * frame.z[2]).abs()
        };
        let edge_on = cam.lens.is_none() && facing < 2.0_f64.to_radians().sin();
        // The point of the plane under a point of the screen, or why there is none.
        let at = |p: Pos2| -> Result<P2, &'static str> {
            if edge_on {
                return Err(msg!("The work plane is seen edge on from here: turn the view, or set another work plane."));
            }
            let px = (f64::from((p.x - pane.min.x) / sx), f64::from((p.y - pane.min.y) / sy));
            let Some(q) = cam.on(px, frame.origin, frame.z) else {
                // In perspective, over the plane's horizon: the plane isn't there.
                return Err(match cam.lens {
                    Some(_) => msg!("The work plane doesn't reach there: click where the plane shows."),
                    None => msg!("The work plane is seen edge on from here: turn the view, or set another work plane."),
                });
            };
            if !q.iter().all(|v| v.is_finite() && v.abs() <= bimcraft_core::model::MAX_COORD) {
                return Err(msg!("That point of the work plane is too far off: click nearer the family, or turn the view."));
            }
            Ok(space.coords(q).0)
        };
        // What of the selection the tool takes: what a plan would, but a form Temporary Hide/Isolate hides, which stays where
        // it is.
        let shown: Vec<Picked> =
            self.selection.items().iter().copied().filter(|p| !matches!(p, Picked::Form(i) if self.temp_hidden_form(*i))).collect();
        // What the tool takes, as the plane has it: each form's box, and each plane's and line's two ends. Worked out once
        // for the draft, the selection and the plane, not as the pointer moves.
        let key = self.family().ok().map(|f| {
            let plane = (space.origin, space.across, space.up);
            super::views::key_of(&[&f], &[&self.values(), &shown, &plane])
        });
        let (boxes, segments) = match self.views.taken_3d.take().filter(|(k, _)| Some(*k) == key) {
            Some((k, taken)) => {
                let out = taken.clone();
                self.views.taken_3d = Some((k, taken));
                out
            }
            None => {
                let taken = self.taken_3d(&space, &shown);
                #[cfg(test)]
                {
                    self.views.cache_builds += 1;
                }
                self.views.taken_3d = key.map(|k| (k, taken.clone()));
                taken
            }
        };
        // Rotate's centre where it is the selection's own: the middle of what it takes.
        if let Some(m) = self.views.modify.as_mut()
            && let Tool::Rotate { centre: centre @ None, .. } = &mut m.tool
        {
            let pts: Vec<P2> = boxes.iter().flatten().chain(segments.iter().flatten()).map(|(q, _)| *q).collect();
            if !pts.is_empty() {
                let lo = pts.iter().fold(P2(f64::INFINITY, f64::INFINITY), |a, p| P2(a.0.min(p.0), a.1.min(p.1)));
                let hi = pts.iter().fold(P2(f64::NEG_INFINITY, f64::NEG_INFINITY), |a, p| P2(a.0.max(p.0), a.1.max(p.1)));
                *centre = Some(P2((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0));
            }
        }
        let shift = ui.input(|i| i.modifiers.shift);
        let mut said = None;
        // An angle typed on the Options Bar, as in a plan.
        if self.views.modify.as_ref().is_some_and(|m| m.typed.is_some()) {
            said = self.modify_typed(space, &shown);
        }
        if let Some(p) = click {
            match at(p) {
                Ok(q) => {
                    // The view a drag would be read through isn't read by a move of two clicks.
                    let free = bimcraft_core::render::View { scale: 1.0, cx: 0.0, cy: 0.0, width: 1, height: 1, angle: 0.0 };
                    said = self.modify_click(q, (Which::Plan, free, 0), space, &shown, None, None, shift);
                }
                Err(why) => said = Some(t(why).to_string()),
            }
        }
        // What the tool selected itself (the copies) is what it goes on with.
        let now: Vec<Picked> = self.selection.items().to_vec();
        if let Some(m) = self.views.modify.as_mut() {
            m.selection = now;
        }
        // The marks, the rubber band and what is taken where the click would put it.
        let Some(m) = self.views.modify.as_ref() else { return said };
        let (tool, o, snaps) = (m.tool, self.views.modify_options.clone(), self.snaps);
        let hover = response.hover_pos().filter(|p| pane.contains(*p)).and_then(|p| at(p).ok());
        // Drawn only while every point of it is on screen: one behind the eye would draw a line across the view.
        let seen = |q: P2| project(space.point(q)).filter(|p| pane.expand(2000.0).contains(*p));
        let points: Vec<P2> = match tool {
            Tool::Move { from, .. } => from.into_iter().collect(),
            Tool::Rotate { centre, start, .. } => centre.into_iter().chain(start).collect(),
            Tool::Mirror { start, .. } => start.into_iter().collect(),
            Tool::Scale { origin, first } => origin.into_iter().chain(first).collect(),
            _ => Vec::new(),
        };
        if points.iter().chain(hover.iter()).all(|q| seen(*q).is_some()) {
            let off = Pos2::new(-1.0e6, -1.0e6);
            let screen = |q: P2| seen(q).unwrap_or(off);
            let back = |p: Pos2| at(p).unwrap_or(P2(f64::NAN, f64::NAN));
            paint_tool(painter, &tool, &o, hover, &[], &screen, &back, None, shift, &snaps);
        }
        if let Some(map) = hover.and_then(|h| preview_map(&tool, &o, h, shift, &snaps, None)) {
            let accent = crate::style::canvas_pal().accent.gamma_multiply(0.8);
            // The forms' boxes are the picture's to draw, behind what stands in front of them.
            let moved = |(q, h): &(P2, f64)| space.lifted(map(*q), *h);
            self.views.ghost_3d = boxes.iter().flat_map(|corners| box_marks(&corners.each_ref().map(moved), accent)).collect();
            for ends in &segments {
                if let [Some(a), Some(b)] = ends.each_ref().map(|end| project(moved(end)).filter(|p| pane.expand(2000.0).contains(*p))) {
                    painter.extend(egui::Shape::dashed_line(&[a, b], egui::Stroke::new(1.5, accent), 8.0, 4.0));
                }
            }
        }
        said
    }

    /// What a Modify tool takes in {3D}, as `space`'s plane has it: the box of each form of `shown`, and the two ends of
    /// each reference plane and reference line of it. A nested instance, a symbol and a connector have none.
    fn taken_3d(&self, space: &Space, shown: &[Picked]) -> Taken3d {
        let Ok(f) = self.family() else { return (Vec::new(), Vec::new()) };
        let values = self.values();
        let flexed = f.flexed(&values).map(std::borrow::Cow::into_owned).unwrap_or_else(|_| f.clone());
        let mut boxes = Vec::new();
        let mut segments = Vec::new();
        for p in shown {
            let ends = match p {
                Picked::Form(i) => {
                    boxes.extend(boxed(space, &bimcraft_core::family::views::form_corners(&f, &values, *i)));
                    None
                }
                Picked::Plane(g) => flexed.planes.iter().find(|x| x.guid == *g).map(|x| (x.start, x.end)),
                Picked::Line(g) => flexed.lines.iter().find(|x| x.guid == *g).map(|x| (x.start, x.end)),
                _ => None,
            };
            if let Some((a, b)) = ends {
                segments.push([space.coords(a), space.coords(b)]);
            }
        }
        (boxes, segments)
    }

    /// The Modify tool in hand, in or out of sketch mode.
    pub(crate) fn modify_in_hand(&self) -> Option<Tool> {
        match self.sketch.as_ref() {
            Some(s) => s.edit,
            None => self.views.modify.as_ref().map(|m| m.tool),
        }
    }

    /// The Modify tool in hand back to its first step, what was clicked let go (Rotate's centre found again): what Undo and
    /// Redo leave it at, as they give a drag up, since what was clicked may be gone or elsewhere.
    pub(crate) fn modify_fresh(&mut self) {
        if let Some(m) = self.views.modify.as_mut() {
            m.tool = m.tool.fresh();
            m.typed = None;
        }
        if let Some(s) = self.sketch.as_mut() {
            s.edit = s.edit.map(|t| t.fresh());
            s.edit_seen = None;
        }
    }

    /// Escape with a Modify tool in hand outside sketch mode: the points clicked let go, then the tool. Whether there was one.
    pub(crate) fn modify_escape(&mut self) -> bool {
        let Some(m) = self.views.modify.as_mut() else { return false };
        match m.tool.back() {
            Some(back) => m.tool = back,
            None => self.views.modify = None,
        }
        true
    }

    /// The items of the selection a tool takes in the view: those the draft has and the view shows (`shown`).
    fn taken(&self, shown: &[Picked]) -> Vec<Picked> {
        self.selection.items().iter().copied().filter(|p| self.holds(*p) && shown.contains(p)).collect()
    }

    /// Move or Copy by two clicks (`d`, the family's axes), as the drag moves or copies (`drag_move.rs`); `seen`, the pass the
    /// view was drawn in.
    fn shift_taken(
        &mut self,
        shown: &[Picked],
        d: [f64; 3],
        copy: bool,
        (which, view, seen): (Which, bimcraft_core::render::View, u64),
    ) -> Result<(), String> {
        let drag = self.move_drag((P2(0.0, 0.0), view, which, seen), shown, copy, None);
        self.views.moving = Some(drag);
        let done = self.drag_move_to(d, copy);
        self.views.moving = None;
        done
    }

    /// `items` rotated, mirrored or scaled by `t` (copies of them, with `copy`): one step; else everything put back and why.
    fn transform_items(&mut self, items: &[Picked], t: Transform, copy: bool) -> Result<(), Refused> {
        let kept = Kept::of(self);
        let selected = self.selection.items().to_vec();
        let broken = self.broken_types();
        let sound = self.sound();
        let as_it_was = self.family()?;
        let done = self.transform_inner(items, t, copy, broken, sound, &as_it_was);
        if done.is_err() {
            kept.put_back(self);
            self.selection.apply(&selected, How::Replace);
        }
        done
    }

    fn transform_inner(
        &mut self,
        items: &[Picked],
        t: Transform,
        copy: bool,
        broken: Vec<String>,
        sound: bool,
        as_it_was: &Family,
    ) -> Result<(), Refused> {
        let (moved, chosen, origin) = if copy {
            let (moved, selected, origin) = self.copy_picked(items)?;
            (moved, Some(selected), origin)
        } else {
            (items.to_vec(), None, Origin::default())
        };
        let names = Names { family: as_it_was, origin: &origin };
        let mut taking = Transforming::default();
        for p in &moved {
            match *p {
                Picked::Plane(g) => taking.planes.push(g),
                Picked::Line(g) => taking.lines.push(g),
                Picked::Form(i) => taking.forms.push(i),
                Picked::Dimension(_) => {}
                Picked::Symbol(_) | Picked::Connector(_) | Picked::Nested(_) => {
                    return Err(Refused::Said(tf(
                        "{} takes forms, reference planes, reference lines and dimensions: take symbols, connectors and nested instances out of the selection, or move them.",
                        &[t_label(&t)],
                    )));
                }
            }
        }
        if taking.planes.is_empty() && taking.lines.is_empty() && taking.forms.is_empty() {
            return Err(Refused::Said(tf(
                "{} takes forms, reference planes and reference lines, and none selected is shown in this view.",
                &[t_label(&t)],
            )));
        }
        let f = self.family()?;
        let done = match f.transformed(&self.values(), &taking, &t) {
            Ok(done) => done,
            Err(TransformRefused::Unsat(u)) if !copy => return Err(Refused::Unsat(u)),
            Err(r) => return Err(Refused::Said(refused_said(&r, &t, &names))),
        };
        for g in &done.planes {
            if let (Some(p), Some(q)) = (self.planes.iter_mut().find(|p| p.guid == *g), done.family.planes.iter().find(|p| p.guid == *g)) {
                *p = q.clone();
            }
        }
        for g in &done.lines {
            if let (Some(l), Some(m)) = (self.lines.iter_mut().find(|l| l.guid == *g), done.family.lines.iter().find(|l| l.guid == *g)) {
                *l = m.clone();
            }
        }
        for &i in &done.forms {
            if let (Some(ft), Some(form)) = (self.forms.get(i), done.family.forms.get(i)) {
                let tag = ft.tag;
                self.forms[i] = FormText::again(form, tag);
            }
        }
        for g in &done.dimensions {
            if let (Some(d), Some(e)) =
                (self.dimensions.iter_mut().find(|d| d.guid == *g), done.family.dimensions.iter().find(|d| d.guid == *g))
            {
                d.at = e.at;
            }
        }
        // A dimension that no longer measures (its references turned apart) deleted.
        self.dimensions.retain(|d| !done.dropped.contains(&d.guid));
        self.selection.retain(|p| !matches!(p, Picked::Dimension(g) if done.dropped.contains(g)));
        self.builds(&broken, &names)?;
        if sound && let Err(e) = self.family().and_then(|f| f.check(&|_| true)) {
            return Err(Refused::Said(e));
        }
        if let Some(copies) = chosen {
            self.selection.apply(&copies, How::Replace);
        }
        Ok(())
    }

    /// A rotation, a mirror or a scale of what the view shows of the selection (`shown`): done, or why not, or the constraints
    /// asked about ("Constraints are not satisfied", whose Remove Constraints does it again).
    fn transform_taken(&mut self, shown: &[Picked], t: Transform, copy: bool) -> Option<String> {
        let items = self.taken(shown);
        if items.is_empty() {
            return Some(crate::lang::t("Nothing selected is shown in this view.").to_string());
        }
        match self.transform_items(&items, t, copy) {
            Ok(()) => None,
            Err(Refused::Said(s)) => Some(s),
            Err(Refused::Unsat(u)) => {
                self.unsatisfied(u, super::dimensions::Retry::Modify(Again { items, transform: t, copy }));
                None
            }
        }
    }

    /// Remove Constraints said yes to: what was refused done again.
    /// Whether it was done: refused, the caller puts back the constraints it took off, so nothing is left of either.
    pub(super) fn modify_again(&mut self, a: Again) -> bool {
        match self.transform_items(&a.items, a.transform, a.copy) {
            Ok(()) => {
                self.views.modify = None;
                true
            }
            Err(Refused::Said(s)) => {
                self.views.status = Some(s);
                false
            }
            Err(Refused::Unsat(u)) => {
                self.views.status = Some(u.message);
                false
            }
        }
    }

    /// A click at `q` (the view's mm) with a Modify tool in hand in view `which` (`space` its way into the family), `shown` what
    /// of the selection the view shows, `axis` the line a click there picks as a mirror's axis, `offset` what Offset takes
    /// there (a plane or a reference line, and its trace), `shift` Shift held. What went wrong, if anything.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn modify_click(
        &mut self,
        q: P2,
        (which, view, seen): (Which, bimcraft_core::render::View, u64),
        space: Space,
        shown: &[Picked],
        axis: Option<(P2, P2)>,
        offset: OffsetHit,
        shift: bool,
    ) -> Option<String> {
        let m = self.views.modify.clone()?;
        let o = self.views.modify_options.clone();
        let snaps = self.snaps;
        let set = |ed: &mut FamilyEditor, tool: Tool| {
            if let Some(m) = ed.views.modify.as_mut() {
                m.tool = tool;
            }
        };
        match m.tool {
            Tool::Move { copy, from: None } => set(self, Tool::Move { copy, from: Some(q) }),
            Tool::Move { copy, from: Some(a) } => {
                if self.taken(shown).is_empty() {
                    return Some(t("Nothing selected is shown in this view.").to_string());
                }
                let d = space.vector(super::drag_move::delta(a, q, shift || o.constrain));
                let done = self.shift_taken(shown, d, copy, (which, view, seen));
                match done {
                    Ok(()) if copy && o.multiple => {
                        // The next copy from here, of the copies just made (as copies of the originals, a step further).
                        let to = a + super::drag_move::delta(a, q, shift || o.constrain);
                        set(self, Tool::Move { copy, from: Some(to) });
                    }
                    Ok(()) => self.views.modify = None,
                    Err(e) => return Some(e),
                }
            }
            Tool::Rotate { placing: true, start, .. } => set(self, Tool::Rotate { centre: Some(q), start, placing: false }),
            Tool::Rotate { centre: Some(c), start: None, .. } => {
                set(self, Tool::Rotate { centre: Some(c), start: Some(q), placing: false })
            }
            Tool::Rotate { centre: Some(c), start: Some(s), .. } => {
                if c.dist(q) < 1e-6 || c.dist(s) < 1e-6 {
                    return Some(t("Click away from the centre of rotation.").to_string());
                }
                let angle = turn(c, s, q, &snaps);
                return self.rotate_by(space, c, angle, shown, o.rotate_copy);
            }
            Tool::Rotate { centre: None, .. } => return Some(t("Rotate: nothing selected is shown in this view to turn.").to_string()),
            Tool::Mirror { draw: false, .. } => match axis {
                Some(line) => return self.mirror_across(space, line, shown, o.mirror_copy),
                None => return Some(t("Pick a reference plane, a reference line or a form's edge as the axis.").to_string()),
            },
            Tool::Mirror { draw: true, start: None } => set(self, Tool::Mirror { draw: true, start: Some(q) }),
            Tool::Mirror { draw: true, start: Some(a) } => {
                let b = if shift { bimcraft_core::snap::orthogonal_step(a, q).0 } else { q };
                if a.dist(b) < 1e-6 {
                    return Some(t("Click the axis' second point away from its first.").to_string());
                }
                return self.mirror_across(space, (a, b), shown, o.mirror_copy);
            }
            Tool::Scale { origin: None, .. } if o.numerical => return self.scale_by(space, q, None, q, shown, &o),
            Tool::Scale { origin: None, .. } => set(self, Tool::Scale { origin: Some(q), first: None }),
            Tool::Scale { origin: Some(c), first: None } if !o.numerical => {
                if c.dist(q) < 1e-6 {
                    return Some(t("Click away from the origin.").to_string());
                }
                set(self, Tool::Scale { origin: Some(c), first: Some(q) });
            }
            Tool::Scale { origin: Some(c), first } => return self.scale_by(space, c, first, q, shown, &o),
            Tool::Offset => {
                let (hit, (a, b)) = match offset {
                    Some(Ok(hit)) => hit,
                    Some(Err(e)) => return Some(e),
                    None => return Some(t("Pick a reference plane or a reference line to offset.").to_string()),
                };
                let Some(distance) = o.units.parse(&o.offset).filter(|d| d.is_finite() && *d > 0.0) else {
                    return Some(tf("Offset: `{}` isn't a distance above 0.", &[&o.offset]));
                };
                let Some(n) = side((a, b), q) else {
                    return Some(t("The view sees it end-on: offset it in a view that sees its trace.").to_string());
                };
                let d = space.vector(n * distance);
                let selected = self.selection.items().to_vec();
                self.selection.apply(&[hit], How::Replace);
                if let Err(e) = self.shift_taken(&[hit], d, o.offset_copy, (which, view, seen)) {
                    self.selection.apply(&selected, How::Replace);
                    return Some(e);
                }
            }
            Tool::TrimTo { .. } | Tool::Split => {}
        }
        None
    }

    fn rotate_by(&mut self, space: Space, centre: P2, angle: f64, shown: &[Picked], copy: bool) -> Option<String> {
        let said = self.transform_taken(shown, space.rotate(centre, angle), copy);
        if said.is_none() && self.views.unsatisfied.is_none() {
            self.views.modify = None;
        }
        said
    }

    fn mirror_across(&mut self, space: Space, line: (P2, P2), shown: &[Picked], copy: bool) -> Option<String> {
        let said = self.transform_taken(shown, space.mirror(line), copy);
        if said.is_none() && self.views.unsatisfied.is_none() {
            self.views.modify = None;
        }
        said
    }

    fn scale_by(&mut self, space: Space, origin: P2, first: Option<P2>, to: P2, shown: &[Picked], o: &Options) -> Option<String> {
        let Some(k) = factor_of(o, origin, first, to).filter(|k| k.is_finite() && *k > 0.0) else {
            return Some(tf("Scale: `{}` isn't a factor above 0.", &[&o.factor]));
        };
        let said = self.transform_taken(shown, Transform::Scale { centre: space.point(origin), factor: k, normal: space.toward() }, false);
        if said.is_none() && self.views.unsatisfied.is_none() {
            self.views.modify = None;
        }
        said
    }

    /// Rotate's Angle typed on the Options Bar and Enter pressed: the selection turned by it about the centre, in the view
    /// shown. What went wrong, if anything.
    pub(super) fn modify_typed(&mut self, space: Space, shown: &[Picked]) -> Option<String> {
        let m = self.views.modify.as_mut()?;
        let typed = m.typed.take()?;
        let Tool::Rotate { centre: Some(c), .. } = m.tool else {
            return Some(t("Rotate: nothing selected is shown in this view to turn.").to_string());
        };
        if !typed.is_finite() {
            return Some(tf("Rotate: `{}` isn't an angle in degrees.", &[&self.views.modify_options.angle]));
        }
        let copy = self.views.modify_options.rotate_copy;
        self.rotate_by(space, c, typed, shown, copy)
    }
}

/// Over the view while a Modify tool is in hand (`hover`, the pointer in the view's mm; `pieces`, what of the selection it
/// takes on screen; `screen` and `at_view` between the view's mm and the screen): the centre, the axis, the rubber band and
/// the selection where the click would put it. Rotate's centre is found here where it is the selection's own: the middle of
/// what it takes.
#[allow(clippy::too_many_arguments)]
pub(super) fn paint_modify(
    (modify, o): (&mut Option<Modifying>, &Options),
    snaps: &bimcraft_core::snap::Snaps,
    painter: &egui::Painter,
    hover: Option<P2>,
    (pieces, centred): (&[[Pos2; 2]], &[[Pos2; 2]]),
    screen: &dyn Fn(P2) -> Pos2,
    at_view: &dyn Fn(Pos2) -> P2,
    axis: Option<(P2, P2)>,
    shift: bool,
) {
    let Some(m) = modify.as_mut() else { return };
    // Rotate's centre: the middle of the forms, planes and lines it takes (a dimension goes with them, and alone turns nothing).
    if let Tool::Rotate { centre: centre @ None, .. } = &mut m.tool
        && !centred.is_empty()
    {
        let pts: Vec<P2> = centred.iter().flatten().map(|p| at_view(*p)).collect();
        let lo = pts.iter().fold(P2(f64::INFINITY, f64::INFINITY), |a, p| P2(a.0.min(p.0), a.1.min(p.1)));
        let hi = pts.iter().fold(P2(f64::NEG_INFINITY, f64::NEG_INFINITY), |a, p| P2(a.0.max(p.0), a.1.max(p.1)));
        *centre = Some(P2((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0));
    }
    paint_tool(painter, &m.tool, o, hover, pieces, screen, at_view, axis, shift, snaps);
}

/// `t`'s tool's name, for a refusal.
fn t_label(t: &Transform) -> &'static str {
    crate::lang::t(match t {
        Transform::Rotate { .. } => "Rotate",
        Transform::Mirror { .. } => msg!("Mirror"),
        Transform::Scale { .. } => msg!("Scale"),
    })
}

/// A tool's marks and preview, in the view's or the sketch's points (`screen` puts one on screen, `at_view` back).
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_tool(
    painter: &egui::Painter,
    tool: &Tool,
    o: &Options,
    hover: Option<P2>,
    pieces: &[[Pos2; 2]],
    screen: &dyn Fn(P2) -> Pos2,
    at_view: &dyn Fn(Pos2) -> P2,
    axis: Option<(P2, P2)>,
    shift: bool,
    snaps: &bimcraft_core::snap::Snaps,
) {
    let accent = crate::style::canvas_pal().accent;
    let mark = Stroke::new(1.5, accent);
    let dot = |p: P2| painter.circle_stroke(screen(p), 5.0, Stroke::new(2.0, accent));
    match *tool {
        Tool::Move { from: Some(a), .. } => {
            dot(a);
            if let Some(h) = hover {
                let to = a + super::drag_move::delta(a, h, shift || o.constrain);
                painter.line_segment([screen(a), screen(to)], mark);
            }
        }
        Tool::Rotate { centre: Some(c), start, placing: false } => {
            dot(c);
            painter.circle_filled(screen(c), 2.5, accent);
            if let Some(s) = start {
                painter.line_segment([screen(c), screen(s)], mark);
            }
            if let Some(h) = hover {
                painter.extend(egui::Shape::dashed_line(&[screen(c), screen(h)], mark, 6.0, 4.0));
            }
        }
        Tool::Mirror { draw: true, start: Some(a) } => {
            if let Some(h) = hover {
                let b = if shift { bimcraft_core::snap::orthogonal_step(a, h).0 } else { h };
                painter.extend(egui::Shape::dashed_line(&[screen(a), screen(b)], Stroke::new(2.0, accent), 10.0, 5.0));
            }
        }
        Tool::Mirror { draw: false, .. } => {
            if let Some((a, b)) = axis {
                painter.extend(egui::Shape::dashed_line(&[screen(a), screen(b)], Stroke::new(2.0, accent), 10.0, 5.0));
            }
        }
        Tool::Scale { origin: Some(c), first } => {
            dot(c);
            if let Some(f) = first {
                painter.line_segment([screen(c), screen(f)], mark);
            }
        }
        _ => {}
    }
    // The selection where the click would put it.
    if let Some(map) = hover.and_then(|h| preview_map(tool, o, h, shift, snaps, axis)) {
        let ghost = Stroke::new(1.5, accent.gamma_multiply(0.8));
        for [a, b] in pieces {
            let (a, b) = (screen(map(at_view(*a))), screen(map(at_view(*b))));
            painter.line_segment([a, b], ghost);
        }
    }
}

// --- Sketch mode -------------------------------------------------------------------------------------------------------------

/// Whether `l` is a revolve's Axis Line, which no tool copies.
fn axis_line(l: &SketchLine) -> bool {
    l.style.as_deref() == Some(super::sketch::AXIS)
}

/// The middle of the lines picked: Rotate's centre in a sketch.
pub(crate) fn picked_centre(s: &super::sketch::FormSketch) -> Option<P2> {
    let pts: Vec<P2> = s.sketch.picked.iter().filter_map(|i| s.sketch.lines.get(*i)).flat_map(|l| l.curve.points()).collect();
    if pts.is_empty() {
        return None;
    }
    let lo = pts.iter().fold(P2(f64::INFINITY, f64::INFINITY), |a, p| P2(a.0.min(p.0), a.1.min(p.1)));
    let hi = pts.iter().fold(P2(f64::NEG_INFINITY, f64::NEG_INFINITY), |a, p| P2(a.0.max(p.0), a.1.max(p.1)));
    Some(P2((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0))
}

/// What a sketch tool did to its lines: what to say, and the GUIDs of the lines it moved (not copied), whose locks to what
/// stays are let go.
#[derive(Default)]
pub(crate) struct SketchDone {
    pub(crate) said: Option<String>,
    pub(crate) moved: Vec<Guid>,
    /// A line split: its GUID, kept by the half from its start, and the new half's, which has its end.
    pub(crate) split: Option<(Guid, Guid)>,
}

/// The lines picked mapped by `f` (copies of them, with `copy`, picked in their place): what moved.
fn map_picked(s: &mut super::sketch::FormSketch, f: &dyn Fn(P2) -> P2, copy: bool) -> SketchDone {
    each_picked(s, &|c| c.map(f), copy)
}

/// A curve scaled by `k` about `o`: its points, and a circle's radius and an ellipse's radii, which a map of its points
/// alone (a move, a turn or a mirror) leaves as they are.
pub(crate) fn scaled(c: &Curve, o: P2, k: f64) -> Curve {
    match c.map(|q| o + (q - o) * k) {
        Curve::Circle { centre, radius } => Curve::Circle { centre, radius: radius * k },
        Curve::Ellipse { centre, radii, angle, span } => Curve::Ellipse { centre, radii: [radii[0] * k, radii[1] * k], angle, span },
        other => other,
    }
}

/// The lines picked made again by `f` (copies of them, with `copy`, picked in their place): what moved.
fn each_picked(s: &mut super::sketch::FormSketch, f: &dyn Fn(&Curve) -> Curve, copy: bool) -> SketchDone {
    let picked: Vec<usize> = s.sketch.picked.iter().copied().collect();
    let mut done = SketchDone::default();
    if copy {
        let mut made = Vec::new();
        for i in picked {
            let Some(l) = s.sketch.lines.get(i).filter(|l| !axis_line(l)).cloned() else { continue };
            s.sketch.lines.push(SketchLine { id: 0, guid: Guid::new(), curve: f(&l.curve), ..l });
            made.push(s.sketch.lines.len() - 1);
        }
        if made.is_empty() {
            done.said = Some(t("The Axis Line isn't copied: pick lines of the profile.").to_string());
            return done;
        }
        s.sketch.picked = made.into_iter().collect();
    } else {
        for i in picked {
            if let Some(l) = s.sketch.lines.get_mut(i) {
                l.curve = f(&l.curve);
                done.moved.push(l.guid);
            }
        }
    }
    s.sketch.changed();
    done
}

/// A click at `p` on the sketch's plane with `tool` in hand, `o` the Options Bar, `reach` how near a line a click picks it,
/// `pick` what Pick Lines would find there (a reference's line on the plane), `shift` Shift held.
#[allow(clippy::too_many_arguments)]
pub(crate) fn sketch_click(
    s: &mut super::sketch::FormSketch,
    tool: Tool,
    p: P2,
    reach: f64,
    o: &Options,
    snaps: &bimcraft_core::snap::Snaps,
    pick: &dyn Fn() -> Result<super::sketch::Picked, String>,
    shift: bool,
) -> SketchDone {
    let said = |text: &str| SketchDone { said: Some(t(text).to_string()), moved: Vec::new(), split: None };
    let set = |s: &mut super::sketch::FormSketch, tool: Tool| s.edit = Some(tool);
    let needs = tool.on_selection() && s.sketch.picked.is_empty();
    if needs {
        s.edit = None;
        return said(msg!("Pick lines of the sketch first."));
    }
    match tool {
        Tool::Move { copy, from: None } => set(s, Tool::Move { copy, from: Some(p) }),
        Tool::Move { copy, from: Some(a) } => {
            let d = super::drag_move::delta(a, p, shift || o.constrain);
            let done = map_picked(s, &|q| q + d, copy);
            if copy && o.multiple {
                set(s, Tool::Move { copy, from: Some(a + d) });
            } else {
                s.edit = None;
            }
            return done;
        }
        Tool::Rotate { placing: true, start, .. } => set(s, Tool::Rotate { centre: Some(p), start, placing: false }),
        Tool::Rotate { centre, start: None, .. } => {
            let c = centre.or_else(|| picked_centre(s));
            set(s, Tool::Rotate { centre: c, start: Some(p), placing: false });
        }
        Tool::Rotate { centre, start: Some(st), .. } => {
            let Some(c) = centre.or_else(|| picked_centre(s)) else { return said(msg!("Pick lines of the sketch first.")) };
            if c.dist(p) < 1e-6 || c.dist(st) < 1e-6 {
                return said(msg!("Click away from the centre of rotation."));
            }
            let angle = turn(c, st, p, snaps);
            s.edit = None;
            return rotate_picked(s, c, angle, o.rotate_copy);
        }
        Tool::Mirror { draw: false, .. } => {
            let line = s
                .sketch
                .line_at(p, reach)
                .and_then(|i| s.sketch.lines.get(i))
                .filter(|l| l.curve.kind() == "line")
                .map(|l| (l.curve.start(), l.curve.end()))
                .or_else(|| pick().ok().filter(|(c, _)| c.kind() == "line").map(|(c, _)| (c.start(), c.end())));
            let Some((a, b)) = line else {
                return said(msg!("Pick a straight line of the sketch, a reference plane or a reference line as the axis."));
            };
            let Some(m) = mirror_map(a, b) else {
                return said(msg!("Pick a straight line of the sketch, a reference plane or a reference line as the axis."));
            };
            s.edit = None;
            return map_picked(s, &*m, o.mirror_copy);
        }
        Tool::Mirror { draw: true, start: None } => set(s, Tool::Mirror { draw: true, start: Some(p) }),
        Tool::Mirror { draw: true, start: Some(a) } => {
            let b = if shift { bimcraft_core::snap::orthogonal_step(a, p).0 } else { p };
            let Some(m) = mirror_map(a, b) else { return said(msg!("Click the axis' second point away from its first.")) };
            s.edit = None;
            return map_picked(s, &*m, o.mirror_copy);
        }
        Tool::Scale { origin: None, .. } if !o.numerical => set(s, Tool::Scale { origin: Some(p), first: None }),
        Tool::Scale { origin: Some(c), first: None } if !o.numerical => {
            if c.dist(p) < 1e-6 {
                return said(msg!("Click away from the origin."));
            }
            set(s, Tool::Scale { origin: Some(c), first: Some(p) });
        }
        Tool::Scale { origin, first } => {
            let c = origin.unwrap_or(p);
            let Some(k) = factor_of(o, c, first, p).filter(|k| k.is_finite() && *k > 0.0 && (1e-3..=1e3).contains(k)) else {
                return SketchDone { said: Some(tf("Scale: `{}` isn't a factor above 0.", &[&o.factor])), moved: Vec::new(), split: None };
            };
            s.edit = None;
            return each_picked(s, &|curve| scaled(curve, c, k), false);
        }
        Tool::Offset => {
            let Some(i) = s.sketch.line_at(p, reach) else { return said(msg!("Pick a line of the sketch to offset.")) };
            let Some(d) = o.units.parse(&o.offset).filter(|d| d.is_finite() && *d > 0.0) else {
                return SketchDone {
                    said: Some(tf("Offset: `{}` isn't a distance above 0.", &[&o.offset])),
                    moved: Vec::new(),
                    split: None,
                };
            };
            let Some(l) = s.sketch.lines.get(i).cloned() else { return SketchDone::default() };
            if axis_line(&l) {
                return said(msg!("The Axis Line isn't offset: draw it again where it goes."));
            }
            let Some(c) = bimcraft_core::draw::pick(&l.curve, p, d) else {
                return said(msg!("It can't be offset by so much: an arc or a circle would close up."));
            };
            let mut done = SketchDone::default();
            if o.offset_copy {
                s.sketch.lines.push(SketchLine { id: 0, guid: Guid::new(), curve: c, ..l });
            } else {
                s.sketch.lines[i].curve = c;
                done.moved.push(l.guid);
            }
            s.sketch.changed();
            return done;
        }
        Tool::TrimTo { boundary: None, many } => match s.sketch.line_at(p, reach) {
            Some(b) => set(s, Tool::TrimTo { boundary: Some(b), many }),
            None => return said(msg!("Pick a line of the sketch as the trim/extend boundary.")),
        },
        Tool::TrimTo { boundary: Some(b), many } => {
            let Some(i) = s.sketch.line_at(p, reach).filter(|i| *i != b) else {
                return said(msg!("Pick another line of the sketch, on the part to keep."));
            };
            let (Some(edge), Some(l)) = (s.sketch.lines.get(b).cloned(), s.sketch.lines.get(i).cloned()) else {
                return SketchDone::default();
            };
            let made = bimcraft_core::draw::meet(&l.curve, &edge.curve, p).and_then(|x| bimcraft_core::draw::trim_to(&l.curve, p, x));
            let Some(c) = made else { return said(msg!("The lines don't meet: they're parallel, or an arc's circle misses the other.")) };
            s.sketch.lines[i].curve = c;
            s.sketch.changed();
            if !many {
                set(s, Tool::TrimTo { boundary: None, many });
            }
            return SketchDone { said: None, moved: vec![l.guid], split: None };
        }
        Tool::Split => {
            let Some(i) = s.sketch.line_at(p, reach) else { return said(msg!("Click a line of the sketch where it is split.")) };
            let Some(l) = s.sketch.lines.get(i).cloned() else { return SketchDone::default() };
            if axis_line(&l) {
                return said(msg!("The Axis Line is one line: it isn't split."));
            }
            let Some((a, b)) = l.curve.split_at(p) else { return said(msg!("Split between the line's ends.")) };
            s.sketch.lines[i].curve = a;
            let mut split = None;
            if let Some(b) = b {
                let guid = Guid::new();
                s.sketch.lines.push(SketchLine { id: 0, guid, curve: b, ..l.clone() });
                split = Some((l.guid, guid));
            }
            s.sketch.changed();
            return SketchDone { said: None, moved: vec![l.guid], split };
        }
    }
    SketchDone::default()
}

/// The lines picked turned `degrees` counter-clockwise about `c` on the plane (copies of them, with `copy`).
fn rotate_picked(s: &mut super::sketch::FormSketch, c: P2, degrees: f64, copy: bool) -> SketchDone {
    let (sn, cs) = degrees.to_radians().sin_cos();
    map_picked(
        s,
        &|q| {
            let v = q - c;
            P2(c.0 + v.0 * cs - v.1 * sn, c.1 + v.0 * sn + v.1 * cs)
        },
        copy,
    )
}

/// Rotate's Angle typed in a sketch: the lines picked turned by it about the centre.
pub(crate) fn sketch_typed(s: &mut super::sketch::FormSketch, angle: f64, copy: bool) -> SketchDone {
    let centre = match s.edit {
        Some(Tool::Rotate { centre, .. }) => centre.or_else(|| picked_centre(s)),
        _ => None,
    };
    let Some(c) = centre else {
        return SketchDone { said: Some(t("Pick lines of the sketch first.").to_string()), moved: Vec::new(), split: None };
    };
    if !angle.is_finite() {
        return SketchDone { said: Some(t("Rotate: that isn't an angle in degrees.").to_string()), moved: Vec::new(), split: None };
    }
    s.edit = None;
    rotate_picked(s, c, angle, copy)
}

/// A sketch tool's marks and preview: the lines picked where the click would put them.
pub(crate) fn paint_sketch_tool(
    s: &super::sketch::FormSketch,
    painter: &egui::Painter,
    at: &dyn Fn(P2) -> Option<Pos2>,
    o: &Options,
    shift: bool,
    snaps: &bimcraft_core::snap::Snaps,
) {
    let Some(tool) = s.edit else { return };
    let mut tool = tool;
    if let Tool::Rotate { centre: centre @ None, .. } = &mut tool {
        *centre = picked_centre(s);
    }
    let screen = |q: P2| at(q).unwrap_or(Pos2::new(f32::NAN, f32::NAN));
    // Lines are drawn as their points; the preview maps the points and draws them again.
    let lines: Vec<Vec<P2>> = s.sketch.picked.iter().filter_map(|i| s.sketch.lines.get(*i)).map(|l| l.curve.points()).collect();
    let accent = crate::style::canvas_pal().accent;
    let mark = Stroke::new(1.5, accent);
    match tool {
        Tool::Move { from: Some(a), .. } => {
            painter.circle_stroke(screen(a), 5.0, Stroke::new(2.0, accent));
            if let Some(h) = s.hover {
                painter.line_segment([screen(a), screen(a + super::drag_move::delta(a, h, shift || o.constrain))], mark);
            }
        }
        Tool::Rotate { centre: Some(c), start, placing: false } => {
            painter.circle_stroke(screen(c), 5.0, Stroke::new(2.0, accent));
            if let Some(st) = start {
                painter.line_segment([screen(c), screen(st)], mark);
            }
            if let Some(h) = s.hover {
                painter.extend(egui::Shape::dashed_line(&[screen(c), screen(h)], mark, 6.0, 4.0));
            }
        }
        Tool::Mirror { draw: true, start: Some(a) } => {
            if let Some(h) = s.hover {
                painter.extend(egui::Shape::dashed_line(&[screen(a), screen(h)], Stroke::new(2.0, accent), 10.0, 5.0));
            }
        }
        Tool::Scale { origin: Some(c), first } => {
            painter.circle_stroke(screen(c), 5.0, Stroke::new(2.0, accent));
            if let Some(f) = first {
                painter.line_segment([screen(c), screen(f)], mark);
            }
        }
        Tool::Offset => {
            // Where the line under the pointer would go.
            let d = o.units.parse(&o.offset).filter(|d| d.is_finite() && *d > 0.0);
            if let (Some(h), Some(d)) = (s.hover, d)
                && let Some(l) = s.sketch.line_at(h, 1e9).and_then(|i| s.sketch.lines.get(i)).filter(|l| l.curve.dist(h) <= 3.0 * d)
                && let Some(c) = bimcraft_core::draw::pick(&l.curve, h, d)
            {
                let pts: Vec<Pos2> = c.points().into_iter().map(screen).collect();
                painter.extend(egui::Shape::dashed_line(&pts, mark, 8.0, 4.0));
            }
        }
        Tool::TrimTo { boundary: Some(b), .. } => {
            if let Some(l) = s.sketch.lines.get(b) {
                let pts: Vec<Pos2> = l.curve.points().into_iter().map(screen).collect();
                painter.add(egui::Shape::line(pts, Stroke::new(3.0, super::views::PICKED)));
            }
        }
        _ => {}
    }
    if let Some(map) = s.hover.and_then(|h| preview_map(&tool, o, h, shift, snaps, None)) {
        let ghost = Stroke::new(1.5, accent.gamma_multiply(0.8));
        for pts in &lines {
            let shown: Vec<Pos2> = pts.iter().map(|q| screen(map(*q))).collect();
            painter.add(egui::Shape::line(shown, ghost));
        }
    }
}

impl FamilyEditor {
    /// A line of the sketch split (`old`, kept by the half from its start; `new`, the half with its end): what measured to its
    /// end measures to the new half's, so a dimension or a lock there stays on the same point. The sketch's own dimensions
    /// and locks change at once; the draft's are held as changed while sketching (written at Finish, dropped by Cancel).
    pub(crate) fn rebind_split(&mut self, old: Guid, new: Guid) {
        use bimcraft_core::family::{DimRef, Spot};
        let Some(s) = self.sketch.as_mut() else { return };
        let own = s.guid;
        let at_end = |r: &DimRef| matches!(r, DimRef::Point(f, l, Spot::End) if *f == own && *l == old);
        let rebind = |d: &mut bimcraft_core::family::FamilyDimension| {
            for r in &mut d.refs {
                if at_end(r) {
                    *r = DimRef::Point(own, new, Spot::End);
                }
            }
        };
        s.dims.iter_mut().chain(s.locks.iter_mut()).for_each(rebind);
        for d in self.dimensions.iter().filter(|d| d.refs.iter().any(at_end)) {
            let mut c = s.changed.iter().find(|c| c.guid == d.guid).cloned().unwrap_or_else(|| d.clone());
            rebind(&mut c);
            s.changed.retain(|x| x.guid != d.guid);
            s.changed.push(c);
        }
    }

    /// The locks a sketch tool broke let go, since they no longer hold: a lock or an Align lock added while
    /// sketching, or one the form had, between a line `moved` and what didn't move with it. What to say, if any were.
    pub(crate) fn release_sketch_locks(&mut self, moved: &[Guid]) -> Option<String> {
        let s = self.sketch.as_mut()?;
        let own = s.guid;
        let moves = |r: &bimcraft_core::family::DimRef| match *r {
            bimcraft_core::family::DimRef::Sketch(f, l) | bimcraft_core::family::DimRef::Point(f, l, _) => f == own && moved.contains(&l),
            _ => false,
        };
        let breaks = |d: &bimcraft_core::family::FamilyDimension| {
            (d.alignment || !d.locked.is_empty()) && d.refs.iter().any(moves) && !d.refs.iter().all(moves)
        };
        let before = s.locks.len();
        s.locks.retain(|d| !breaks(d));
        let mut released = before - s.locks.len();
        for d in self.dimensions.iter().filter(|d| breaks(d)) {
            if !s.unlocked.contains(&d.guid) {
                s.unlocked.push(d.guid);
                released += 1;
            }
        }
        (released > 0).then(|| tf("{} lock(s) to what didn't move with the lines were released.", &[&released.to_string()]))
    }
}

/// Where the next click lands for a Modify tool in a sketch: snapped as a shape's points are.
pub(crate) fn snaps_points(tool: Option<Tool>) -> bool {
    matches!(tool, Some(Tool::Move { .. } | Tool::Rotate { .. } | Tool::Mirror { draw: true, .. } | Tool::Scale { .. }))
}

/// A tool's ribbon icon.
fn icon(tool: &Tool) -> crate::icons::Icon {
    use crate::icons::Icon;
    match tool {
        Tool::Move { copy: false, .. } => Icon::Move,
        Tool::Move { copy: true, .. } => Icon::Copy,
        Tool::Rotate { .. } => Icon::Rotate,
        Tool::Mirror { draw: false, .. } => Icon::MirrorPick,
        Tool::Mirror { draw: true, .. } => Icon::MirrorDraw,
        Tool::Scale { .. } => Icon::Scale,
        Tool::Offset => Icon::Offset,
        Tool::TrimTo { many: false, .. } => Icon::TrimSingle,
        Tool::TrimTo { many: true, .. } => Icon::TrimMultiple,
        Tool::Split => Icon::Split,
    }
}

/// A tool's tooltip.
fn tip(tool: &Tool) -> &'static str {
    match tool {
        Tool::Move { copy: false, .. } => msg!("Move: what is selected, from a point clicked to another"),
        Tool::Move { copy: true, .. } => msg!("Copy: copies of what is selected, from a point clicked to another"),
        Tool::Rotate { .. } => {
            msg!("Rotate: what is selected, about its centre or one placed, by the angle between two clicks or one typed")
        }
        Tool::Mirror { draw: false, .. } => {
            msg!("Mirror - Pick Axis: what is selected, mirrored across a line or a plane clicked; Copy keeps the original")
        }
        Tool::Mirror { draw: true, .. } => {
            msg!("Mirror - Draw Axis: what is selected, mirrored across a line drawn by two clicks; Copy keeps the original")
        }
        Tool::Scale { .. } => msg!("Scale: the reference planes and lines selected, or a sketch's lines, about an origin"),
        Tool::Offset => msg!("Offset: the line or plane clicked, moved or copied the Options Bar's distance toward the side clicked"),
        Tool::TrimTo { many: false, .. } => {
            msg!("Trim/Extend Single Element: pick the boundary line, then the line to trim or extend, on the part to keep")
        }
        Tool::TrimTo { many: true, .. } => {
            msg!("Trim/Extend Multiple Elements: pick the boundary line, then each line to trim or extend, on the part to keep")
        }
        Tool::Split => msg!("Split Element: click a line of the sketch where to split it"),
    }
}

/// The Modify tools outside sketch mode, in the Modify panel's order.
pub(crate) const VIEW_TOOLS: [Tool; 7] = [
    Tool::Offset,
    Tool::Mirror { draw: false, start: None },
    Tool::Mirror { draw: true, start: None },
    Tool::Move { copy: false, from: None },
    Tool::Move { copy: true, from: None },
    Tool::Rotate { centre: None, start: None, placing: false },
    Tool::Scale { origin: None, first: None },
];

/// The sketch's own: Trim/Extend Single and Multiple Elements and Split Element.
pub(crate) const SKETCH_TOOLS: [Tool; 3] =
    [Tool::TrimTo { boundary: None, many: false }, Tool::TrimTo { boundary: None, many: true }, Tool::Split];

/// `tools` as the Modify panel's small buttons: the one in hand lit, one that can't be used now greyed with why in its
/// tooltip. The one clicked.
pub(crate) fn tool_buttons(ui: &mut Ui, ed: &FamilyEditor, tools: &[Tool]) -> Option<Tool> {
    buttons(ui, &states(ed, tools))
}

/// Each of `tools`, whether it is in hand, and why it can't be used now, if it can't: what [`buttons`] draws.
pub(crate) fn states(ed: &FamilyEditor, tools: &[Tool]) -> Vec<(Tool, bool, Option<String>)> {
    let in_hand = ed.modify_in_hand();
    tools.iter().map(|tool| (*tool, in_hand.is_some_and(|h| h.same(tool)), ed.modify_blocked(tool))).collect()
}

/// The small buttons of `states` ([`states`]): the one clicked.
pub(crate) fn buttons(ui: &mut Ui, states: &[(Tool, bool, Option<String>)]) -> Option<Tool> {
    let mut clicked = None;
    for (tool, active, blocked) in states {
        let (active, blocked) = (*active, blocked.clone());
        let r = ui.add_enabled_ui(blocked.is_none(), |ui| crate::small_button(ui, icon(tool), tool.label(), active, tip(tool))).inner;
        let r = match blocked {
            Some(why) => r.on_disabled_hover_text(why),
            None => r,
        };
        if r.clicked() {
            clicked = Some(*tool);
        }
    }
    clicked
}
