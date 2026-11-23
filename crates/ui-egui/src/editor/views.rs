//! F4: the family document's floor and ceiling plans and its four elevations, as Revit's family editor has them, beside
//! {3D}. Each is the project's own drawing of a scratch model the family stands in (`family::views`: its planes made project
//! reference planes there, mapped back by GUID), drawn on the interface's thread (a one-family model is small) and kept until
//! what it shows changes. In a family that isn't adaptive a click selects a form (its Properties, as T11) or a plane, a
//! selected plane of the family's own moves by a drag, and Reference Plane (RP) draws one by two clicks: upright in plan,
//! facing across an elevation. Work Plane Set and Show work over the planes. An adaptive family's plans and elevations show
//! its test placement and nothing more: its tools are in {3D}. What each view keeps (how close, where, its work plane) is the
//! session's, never the family's. Each view shows the family as the type previewed flexes it, and its dimensions, Aligned
//! Dimension and Align are F7's (`dimensions.rs`).
use super::dimensions::{self as dims, Clicked};
use super::select_box::Picked;
use super::*;
use bimcraft_core::family::annotation::PaperMark;
use bimcraft_core::family::views::{self as fv, Side, Staged, Which};
use bimcraft_core::family::{DimRef, FamilyDimension};
use bimcraft_core::geom::P2;
use bimcraft_core::graphics::Override;
use bimcraft_core::ids::Guid;
use bimcraft_core::model::Element;
use bimcraft_core::render::{Drawing, Rgb, View};

/// A paste's strokes on screen: all of them, and those of what has a size of its own, whose middle the pointer holds.
pub(crate) type PasteGhost = (Vec<[egui::Pos2; 2]>, Vec<[egui::Pos2; 2]>);

/// A key for what `parts` (each written as JSON, or as Rust writes it) were: what a cache is kept by.
pub(super) fn key_of(json: &[&dyn erased::Json], debug: &[&dyn std::fmt::Debug]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for j in json {
        j.json().hash(&mut h);
    }
    for d in debug {
        format!("{d:?}").hash(&mut h);
    }
    h.finish()
}

pub(super) mod erased {
    /// What can be written as JSON, behind a reference of one type.
    pub(crate) trait Json {
        fn json(&self) -> String;
    }

    impl<T: serde::Serialize> Json for T {
        fn json(&self) -> String {
            serde_json::to_string(self).unwrap_or_default()
        }
    }
}

/// A symbol the plan is drawing: a line, an arc or a fill by its clicks, or a label, text or dot where one is clicked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SymbolKind {
    Line,
    Arc,
    Fill,
    Label,
    Text,
    Dot,
}

#[derive(Clone, Debug)]
pub(crate) struct SymbolDraw {
    pub(crate) kind: SymbolKind,
    pub(crate) points: Vec<P2>,
    pub(crate) fill_type: String,
}

impl SymbolDraw {
    pub(crate) fn new(kind: SymbolKind) -> SymbolDraw {
        SymbolDraw { kind, points: Vec::new(), fill_type: "Solid Black".into() }
    }
}

/// A grip a symbol's plan draws and a drag takes: a point of a line, a ring's point, or an arc's start, middle or end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SymbolNode {
    Line(usize),
    Ring { ring: usize, point: usize },
    ArcGrip(usize),
}

/// A symbol coordinate moved by `d`: a number by adding it, a formula by adding it to the formula, so the expression stays
/// readable and any parameters in it keep working.
fn shifted(e: &bimcraft_core::family::Expr, d: f64) -> bimcraft_core::family::Expr {
    use bimcraft_core::family::Expr;
    match e {
        Expr::Number(x) => Expr::Number(x + d),
        Expr::Formula(f) => Expr::Formula(format!("({f}) + {d}")),
    }
}

fn shifted_text(s: &str, d: f64) -> String {
    super::expr_text(&shifted(&super::text_expr(s), d))
}

/// An arc's centre, radius and end angles, when every one of them is a plain number the grips can be worked from.
fn arc_numbers(y: &super::SymbolText) -> Option<(P2, f64, f64, f64)> {
    let x = y.centre[0].trim().parse::<f64>().ok()?;
    let z = y.centre[1].trim().parse::<f64>().ok()?;
    let r = y.radius.trim().parse::<f64>().ok()?;
    let from = y.from.trim().parse::<f64>().ok()?;
    let to = y.to.trim().parse::<f64>().ok()?;
    (r > 0.0 && from != to).then_some((P2(x, z), r, from, to))
}

/// A point `deg` degrees round an arc's centre.
fn on_arc(c: P2, r: f64, deg: f64) -> P2 {
    let (s, co) = deg.to_radians().sin_cos();
    P2(c.0 + r * co, c.1 + r * s)
}

/// An arc's three grips: its start, its middle and its end.
fn arc_grips(y: &super::SymbolText) -> Option<[P2; 3]> {
    let (c, r, from, to) = arc_numbers(y)?;
    Some([on_arc(c, r, from), on_arc(c, r, (from + to) * 0.5), on_arc(c, r, to)])
}

/// The arc through `a`, `b` and `c`: its centre, radius, and the angles it runs from and to, so that it passes `b`.
fn fit_arc(a: P2, b: P2, c: P2) -> Option<(P2, f64, f64, f64)> {
    let d = 2.0 * (a.0 * (b.1 - c.1) + b.0 * (c.1 - a.1) + c.0 * (a.1 - b.1));
    if d.abs() < 1e-9 {
        return None;
    }
    let (a2, b2, c2) = (a.0 * a.0 + a.1 * a.1, b.0 * b.0 + b.1 * b.1, c.0 * c.0 + c.1 * c.1);
    let centre =
        P2((a2 * (b.1 - c.1) + b2 * (c.1 - a.1) + c2 * (a.1 - b.1)) / d, (a2 * (c.0 - b.0) + b2 * (a.0 - c.0) + c2 * (b.0 - a.0)) / d);
    let r = centre.dist(a);
    if !r.is_finite() || r <= 1e-9 {
        return None;
    }
    let angle = |p: P2| (p.1 - centre.1).atan2(p.0 - centre.0).to_degrees();
    let (fa, fb, fc) = (angle(a), angle(b), angle(c));
    let to_c = (fc - fa).rem_euclid(360.0);
    let to_b = (fb - fa).rem_euclid(360.0);
    // Counter-clockwise when the middle point lies between the ends that way; clockwise when it lies the other.
    let to = if to_b <= to_c { fa + to_c } else { fa - (360.0 - to_c) };
    Some((centre, r, fa, to))
}

/// Points along the arc through `a`, `b` and `c`, for a preview or a drag, at most a turn.
fn arc_points(a: P2, b: P2, c: P2) -> Option<Vec<P2>> {
    let (centre, r, from, to) = fit_arc(a, b, c)?;
    let steps = (((to - from).abs() / 5.0).ceil() as usize).clamp(8, 96);
    Some((0..=steps).map(|k| on_arc(centre, r, from + (to - from) * k as f64 / steps as f64)).collect())
}

/// A symbol moved by `d`, or one grip of it moved alone: the same as a drag, for Copy.
fn shift_symbol(y: &mut super::SymbolText, node: Option<SymbolNode>, d: P2) {
    use super::Part;
    match y.part {
        Part::Line => {
            if let Ok(mut pts) = super::text_profile(&y.line) {
                match node {
                    Some(SymbolNode::Line(n)) if n < pts.len() => {
                        pts[n][0] = shifted(&pts[n][0], d.0);
                        pts[n][1] = shifted(&pts[n][1], d.1);
                    }
                    Some(_) => {}
                    None => {
                        for p in &mut pts {
                            p[0] = shifted(&p[0], d.0);
                            p[1] = shifted(&p[1], d.1);
                        }
                    }
                }
                y.line =
                    pts.iter().map(|[a, b]| format!("{}, {}", super::expr_text(a), super::expr_text(b))).collect::<Vec<_>>().join("\n");
            }
        }
        Part::Arc => {
            let Some((centre, r, from, to)) = arc_numbers(y) else { return };
            let moved = match node {
                Some(SymbolNode::ArcGrip(which)) => {
                    let Some([a, m, b]) = arc_grips(y) else { return };
                    // A grip moves and the arc is drawn through the other two, so an end swings and the middle bends it.
                    match which {
                        0 => fit_arc(a + d, m, b),
                        2 => fit_arc(a, m, b + d),
                        _ => fit_arc(a, m + d, b),
                    }
                }
                None => Some((centre + d, r, from, to)),
                _ => None,
            };
            if let Some((centre, r, from, to)) = moved {
                y.centre = [format!("{}", centre.0), format!("{}", centre.1)];
                y.radius = format!("{r}");
                y.from = format!("{from}");
                y.to = format!("{to}");
            }
        }
        Part::Fill => match node {
            Some(SymbolNode::Ring { ring, point }) => {
                if let Ok(mut rings) = serde_json::from_str::<Vec<Vec<[f64; 2]>>>(&y.fill)
                    && let Some(p) = rings.get_mut(ring).and_then(|r| r.get_mut(point))
                {
                    p[0] += d.0;
                    p[1] += d.1;
                    y.fill = serde_json::to_string(&rings).unwrap_or_default();
                }
            }
            Some(_) => {}
            None => {
                if let Ok(mut rings) = serde_json::from_str::<Vec<Vec<[f64; 2]>>>(&y.fill) {
                    for ring in &mut rings {
                        for p in ring {
                            p[0] += d.0;
                            p[1] += d.1;
                        }
                    }
                    y.fill = serde_json::to_string(&rings).unwrap_or_default();
                }
            }
        },
        _ => {
            y.centre[0] = shifted_text(&y.centre[0], d.0);
            y.centre[1] = shifted_text(&y.centre[1], d.1);
        }
    }
}

/// What the family document's views keep for the session.
#[derive(Default)]
pub(crate) struct Views {
    /// The view shown; none, {3D}.
    pub(crate) shown: Option<Which>,
    /// How close each view is, times as fitted, and how far it is moved (mm).
    pub(super) frames: BTreeMap<Which, (f64, (f64, f64))>,
    /// Each view's work plane ({3D}'s under none), once Set; else its own (`default_work_plane`).
    work_planes: BTreeMap<Option<Which>, PlaneRef>,
    /// Work Plane > Show: the work plane tinted.
    pub(crate) show_work_plane: bool,
    /// The Work Plane dialog's Pick a plane or Pick a line (T12): the next click picks.
    pub(crate) picking: Option<super::work_plane::Picking>,
    /// The Work Plane dialog, open.
    pub(crate) dialog: Option<super::work_plane::Dialog>,
    /// Go To View, asked for a plane the view shown doesn't show flat.
    pub(crate) go_to: Option<super::work_plane::GoTo>,
    /// Family Element Visibility Settings, open (F8).
    pub(crate) visibility: Option<super::visibility::Dialog>,
    /// The document's Detail Level, which every one of its views draws at (F8): the session's, Fine when a family opens.
    pub(crate) detail: bimcraft_core::graphics::Detail,
    /// The Reference Plane tool, on, with its first click once made.
    pub(crate) drawing: Option<Option<P2>>,
    /// The Measure tool, on: its first click and the ruler last laid.
    pub(crate) measuring: Option<super::measure::Measuring>,
    /// What `drawing` draws: a reference line on the view's work plane (FE-11), else a reference plane.
    pub(crate) drawing_line: bool,
    /// The plane being dragged: where the pointer was, in the view's millimetres, and the view as the drag began, which the
    /// pointer is read through while it goes on (the view refitting round the plane would otherwise change its scale under
    /// the hand).
    pub(super) drag: Option<(P2, View)>,
    /// Whether the hand is still within a click's few points (`select_box::still`) of where the button went down: a move of
    /// the selection and a plane's drag wait until it leaves them, so a shaky click moves nothing.
    still: bool,
    /// The press origin, retained while Shift constrains a drag across frames.
    drag_origin: Option<P2>,
    /// A symbolic line, an arc, a fill, a text, a label or a dot being drawn in the plan.
    pub(crate) symbol: Option<SymbolDraw>,
    /// A symbol being dragged: its place in the list, the grip taken (none, the whole symbol), and where the pointer was.
    pub(crate) symbol_drag: Option<(usize, Option<SymbolNode>, P2)>,
    /// A selected extrusion's Start or End (true) being dragged by its handle (F5): where the pointer was, in the view's
    /// millimetres, the normal it moves along, and the view as it was when the drag began, which the pointer is read through
    /// while it goes on (a drag that changes what the view frames would otherwise change its scale under the hand).
    pub(super) end_drag: Option<EndDrag>,
    /// A profile edge being dragged by its handle, in a plan, an elevation or {3D}.
    pub(super) edge_drag: Option<super::shape_handles::EdgeDrag>,
    /// What a drag or a paste did that is worth saying, not an error, in a plan, an elevation or {3D}: the parameter a handle
    /// drags at its value now, a side lined up, a part a move leaves where it is, the constraints a paste dropped.
    pub(crate) said: Option<String>,
    /// A handle being dragged in {3D} (`handles3d.rs`): how the pointer is read along it, and an end's number before.
    pub(super) drag3d: Option<super::handles3d::Drag3d>,
    /// A parameter being dragged by a handle tied to it (`handle_params.rs`), in any view.
    pub(super) param_drag: Option<super::handle_params::ParamDrag>,
    /// What the side or end being dragged by its handle can line up with, and what it is lined up with now.
    pub(super) handle_snap: Option<super::shape_handles::HandleSnap>,
    /// How each handle of the selected form drags, worked out once for the draft as it is.
    pub(super) drives: std::cell::RefCell<super::handle_params::Drives>,
    /// The handle the pointer is over, lit, where it was last drawn: a greyed one never is.
    pub(crate) lit_handle: Option<egui::Pos2>,
    /// Where the selected form's handles were last shown in {3D}.
    pub(crate) handles3d_shown: Vec<(egui::Pos2, super::handles3d::Handle3d)>,
    /// The selection being dragged to move it (`drag_move.rs`).
    pub(super) moving: Option<super::drag_move::MoveDrag>,
    /// A Modify tool in hand (`modify.rs`): Move, Copy, Rotate, Mirror, Scale or Offset, with what has been clicked.
    pub(crate) modify: Option<super::modify::Modifying>,
    /// Where a Modify tool in hand in {3D} would put the forms it takes: their boxes, for the picture to draw with
    /// its depth in the next frame.
    pub(crate) ghost_3d: Vec<bimcraft_core::view3d::PlaneMark>,
    /// What a Modify tool in hand in {3D} takes, as its work plane has it (`modify::taken_3d`), and what it was worked out
    /// for: worked out again only when the draft, the selection or the plane change, not as the pointer moves.
    pub(crate) taken_3d: Option<(u64, super::modify::Taken3d)>,
    /// A paste following the pointer: the copies' strokes in the view and those that hold it, and what they were drawn for
    /// (the draft, the clipboard, the view): drawn again only when one of those changes, not as the pointer moves.
    pub(crate) paste_ghost: Option<(u64, PasteGhost)>,
    /// How many times a cache above was worked out: what a test reads to see it kept as the pointer moves.
    #[cfg(test)]
    pub(crate) cache_builds: u32,
    /// The Modify tools' Options Bar, kept for the session.
    pub(crate) modify_options: super::modify::Options,
    /// Whether the pointer was last over something selected that a drag would move, with no tool in hand: what the status
    /// bar says.
    pub(crate) over_selected: bool,
    /// Whether Ctrl was held over the view last frame: over something selected, a drag copies it.
    pub(crate) ctrl: bool,
    /// Where the selected form's profile handles were last shown on screen.
    pub(crate) shapes_shown: Vec<(egui::Pos2, super::shape_handles::ShapeHandle)>,
    /// Where the selected form's Start and End handles were last shown on screen, and whether each can be dragged.
    pub(crate) ends_shown: Vec<(egui::Pos2, bool)>,
    /// Each view's fitted frame, and the size it was fitted for: kept while the family changes, so a drag leaves the frame
    /// where it was, and each view keeps its own while another is shown. Zoom to Fit lets the shown view's go (`refit`).
    fitted: std::collections::HashMap<Which, ([u32; 2], View)>,
    /// The picture shown, and what it was drawn of.
    pub(super) cache: Option<Cached>,
    /// Aligned Dimension (DI): on, with the references picked so far; Radial or Diameter Dimension too (`dim_kind`), its
    /// circle picked once it is.
    pub(crate) dimensioning: Option<Vec<DimRef>>,
    /// What the dimension tool on measures: aligned, or a circle's radius or diameter (F7b).
    pub(crate) dim_kind: bimcraft_core::family::DimKind,
    /// Align (AL): on, with the reference picked first once it is.
    pub(crate) aligning: Option<Option<DimRef>>,
    /// The padlock Align leaves until the next click: the two references, where it shows (the view's mm), in which view.
    pub(crate) align_lock: Option<(DimRef, DimRef, P2, Which)>,
    /// Where each Align padlock was last shown on screen, by its dimension's place (the one Align offers, past the last).
    pub(crate) locks_shown: Vec<(usize, egui::Pos2)>,
    /// Where Align's offered padlock was last shown on screen.
    pub(crate) offer_shown: Option<egui::Pos2>,
    /// An Align "Constraints are not satisfied" asked about, which Remove Constraints does again.
    pub(crate) align_pending: Option<(DimRef, DimRef, P2, Which)>,
    /// "Constraints are not satisfied", asked until Remove Constraints or Cancel.
    pub(crate) unsatisfied: Option<super::dimensions::Unsatisfied>,
    /// Create Parameter…, for a dimension's label.
    pub(crate) creating: Option<super::dimensions::Creating>,
    /// What a tool of the views couldn't do, for the status bar.
    pub(crate) status: Option<String>,
    /// Where on screen the view was last shown.
    pub(crate) pane: Option<Rect>,
    /// Where the pointer was last over {3D} and the camera {3D} was seen through, and the form or nested instance under it
    /// there (T11's hover).
    pub(crate) hover3d: Option<(egui::Pos2, bimcraft_core::view3d::Camera, Option<bimcraft_core::family::views::Part>)>,
    /// Pivot, on: the line or plane the next click pins.
    pub(crate) pivoting: Option<Guid>,
    /// A grip being dragged, and where the pointer was (the view's mm).
    pub(super) grip: Option<(super::pivot::Grip, P2)>,
    /// Where the selected element's grips were last shown on screen.
    pub(crate) grips_shown: Vec<(super::pivot::Grip, egui::Pos2)>,
    /// What is typed in a pivot's rows of Properties: its element, its angle, its minimum and its maximum.
    pub(crate) pivot_text: Option<(Guid, String, String, String)>,
    /// The planes under the pointer, where several lie on one another (a Sill plane on Ref. Level, F10c), and which of them
    /// a click takes: Tab steps through them, as Revit's Tab does.
    pub(crate) under: Vec<Guid>,
    pub(crate) cycle: usize,
    /// Paste following the pointer (`clipboard.rs`).
    pub(crate) pasting: Option<super::clipboard::Pasting>,
    /// Paste Aligned to Selected Views' dialog, open.
    pub(crate) paste_views: Option<super::clipboard::PasteViews>,
}

/// A selected extrusion's Start or End (`end`) being dragged by its handle: where the pointer was (the view's
/// millimetres), the normal it moves along, the view as the drag began (the pointer is read through it), what the end was
/// typed before and the form's tag (what Escape puts back), and the view it is dragged in with the last pass it was drawn in.
#[derive(Clone)]
pub(crate) struct EndDrag {
    pub(super) end: bool,
    last: P2,
    n: [f64; 3],
    view: View,
    pub(super) before: String,
    pub(super) tag: u64,
    which: Which,
    seen: u64,
}

/// The planes the view draws under pixel `(x, y)` of `drawing` (within reach), each once, nearest first.
fn planes_under(drawing: &Drawing, staged: &Staged, at: (f32, f32)) -> Vec<Guid> {
    let mut out: Vec<Guid> = Vec::new();
    for g in drawing.pick_all(at, 6.0).into_iter().filter_map(|id| staged.plane_of(id)) {
        if !out.contains(&g) {
            out.push(g);
        }
    }
    out
}

/// A view drawn: what it was drawn of, its scratch model, its frame and drawing (to pick in) and its picture.
pub(super) struct Cached {
    key: u64,
    pub(super) which: Which,
    pub(super) staged: Staged,
    pub(super) view: View,
    drawing: Drawing,
    /// An elevation's hidden-line picture, which its drawing is painted over; and the picture itself, kept for a test.
    texture: Option<TextureHandle>,
    #[cfg(test)]
    picture: Option<bimcraft_core::view3d::Image>,
}

#[cfg(test)]
impl FamilyEditor {
    /// The view shown's pixels, as its drawing paints over its picture: what a test reads where the painted vectors are.
    pub(crate) fn view_pixels(&self) -> Option<ColorImage> {
        let c = self.views.cache.as_ref()?;
        let rgba = bimcraft_core::render::rgba(&c.drawing, c.picture.as_ref()).ok()?;
        let pixels = rgba.as_chunks::<4>().0.iter().map(|p| Color32::from_rgb(p[0], p[1], p[2])).collect();
        Some(ColorImage { size: [c.view.width as usize, c.view.height as usize], pixels, source_size: Default::default() })
    }
}

/// The name a family's view goes by, as Revit's browser and title name it.
pub(crate) fn view_title(which: Option<Which>) -> String {
    match which {
        None => tf("3D View: {}", &[document::VIEW]),
        Some(Which::Plan) => tf("Floor Plan: {}", &[t("Ref. Level")]),
        Some(Which::Ceiling) => tf("Ceiling Plan: {}", &[t("Ref. Level")]),
        Some(Which::Elevation(side)) => tf("Elevation: {}", &[t(side.name())]),
    }
}

/// What the view's tab says after the family's name.
pub(crate) fn view_tab(which: Option<Which>) -> String {
    match which {
        None => document::VIEW.to_string(),
        Some(_) => view_title(which),
    }
}

/// The plane a view draws on until another is Set, as Revit's: Ref. Level in plan and in {3D} (none), the Center plane an
/// elevation faces.
fn default_work_plane(which: Option<Which>) -> PlaneRef {
    match which {
        None | Some(Which::Plan | Which::Ceiling) => PlaneRef::Plane(REF_LEVEL),
        Some(Which::Elevation(Side::Front | Side::Back)) => PlaneRef::Plane(CENTER_FRONT_BACK),
        Some(Which::Elevation(Side::Left | Side::Right)) => PlaneRef::Plane(CENTER_LEFT_RIGHT),
    }
}

/// How far beyond what the family takes up {3D} draws its work plane, mm: as far as the plans and elevations frame past it.
pub(crate) const WORK_PLANE_MARGIN: f64 = 300.0;

/// The selection's colour in the family's views.
pub(crate) const PICKED: Color32 = Color32::from_rgb(40, 120, 230);
/// How far a dimension's tick reaches from its point, in points: what a padlock keeps clear of.
const TICK: f32 = 7.0;

/// How near a shape handle, in points, a press takes it.
const HANDLE: f32 = 10.0;

/// The plane a click would take where several lie on one another (F10c): lit, lighter than one selected.
const PRESELECT: Color32 = Color32::from_rgb(110, 170, 240);
/// Work Plane > Show's tint, the project's 3D view's.
pub(crate) const TINT: Color32 = crate::workplane::TINT;

impl Views {
    /// The work plane of the view shown.
    pub(crate) fn work_plane(&self) -> PlaneRef {
        self.work_planes.get(&self.shown).copied().unwrap_or_else(|| default_work_plane(self.shown))
    }

    /// The work plane of the view shown made `plane`.
    pub(crate) fn set_work_plane(&mut self, plane: PlaneRef) {
        self.work_planes.insert(self.shown, plane);
    }

    /// Another view shown: the tool and the drag in hand go, the selection stays.
    pub(crate) fn show(&mut self, which: Option<Which>) {
        if self.shown != which {
            self.shown = which;
            self.drawing = None;
            self.picking = None;
            self.drag = None;
            self.dimensioning = None;
            self.dim_kind = Default::default();
            self.aligning = None;
            self.align_lock = None;
        }
    }
}

#[cfg(test)]
impl FamilyEditor {
    /// The padlocks shown while a form is sketched, where they are on screen in the view shown (F7b).
    pub(crate) fn sketch_padlocks_shown(&self) -> Vec<(egui::Pos2, super::sketch::Padlock)> {
        let (Some(c), Some(pane), Some(frame)) = (self.views.cache.as_ref(), self.views.pane, self.sketch_frame()) else {
            return Vec::new();
        };
        let on_screen = |q: P2| {
            let (x, y) = c.view.px(super::sketch::in_view(&c.staged, c.which, &frame, q));
            Some(pane.min + vec2(x, y))
        };
        self.sketch_padlocks(&on_screen, &drawn_lines(&c.drawing, pane.min))
    }

    /// The view's drawing's lines on screen.
    pub(crate) fn view_lines_shown(&self) -> Vec<[egui::Pos2; 2]> {
        match (self.views.cache.as_ref(), self.views.pane) {
            (Some(c), Some(pane)) => drawn_lines(&c.drawing, pane.min),
            _ => Vec::new(),
        }
    }

    /// The sketch's lines and the view's drawing on screen, while a form is sketched: what its padlocks keep off.
    pub(crate) fn sketch_lines_shown(&self) -> Vec<[egui::Pos2; 2]> {
        let (Some(c), Some(pane), Some(frame), Some(s)) =
            (self.views.cache.as_ref(), self.views.pane, self.sketch_frame(), self.sketch.as_ref())
        else {
            return Vec::new();
        };
        let on_screen = |q: P2| {
            let (x, y) = c.view.px(super::sketch::in_view(&c.staged, c.which, &frame, q));
            pane.min + vec2(x, y)
        };
        let mut out = drawn_lines(&c.drawing, pane.min);
        for l in &s.sketch.lines {
            let pts: Vec<egui::Pos2> = l.curve.points().into_iter().map(on_screen).collect();
            out.extend(pts.windows(2).map(|w| [w[0], w[1]]));
        }
        out
    }
}

#[cfg(test)]
impl Views {
    /// Where point `p` of the family is in the picture shown, in its pixels.
    pub(crate) fn pixel_of(&self, p: [f64; 3]) -> Option<(f32, f32)> {
        let c = self.cache.as_ref()?;
        Some(c.view.px(c.staged.to_view(c.which, p)))
    }

    /// Where point `p` of the family is on screen, in the view shown.
    pub(crate) fn screen_of(&self, p: [f64; 3]) -> Option<egui::Pos2> {
        let (x, y) = self.pixel_of(p)?;
        Some(self.pane?.min + vec2(x, y))
    }
}

#[cfg(test)]
impl FamilyEditor {
    /// Type `name` previewed, as Family Types picks it.
    pub(crate) fn preview_type(&mut self, name: &str) {
        if let Some(i) = self.types.iter().position(|(n, _)| n == name) {
            self.picked = i;
        }
    }
}

impl FamilyEditor {
    /// Whether the family's view shown is a plan or an elevation of a family that isn't adaptive: where planes are drawn.
    pub(crate) fn editing_2d(&self) -> bool {
        self.views.shown.is_some() && self.adaptive.is_none()
    }

    /// The plane selected, while it alone is and the family has it.
    pub(crate) fn selected_plane(&self) -> Option<Guid> {
        match self.only_selected() {
            Some(Picked::Plane(g)) => Some(g),
            _ => None,
        }
    }

    /// Reference Plane (RP): the tool, in a plan or an elevation of a family that isn't adaptive; the selection goes.
    pub(crate) fn reference_plane_tool(&mut self) {
        self.datum_tool(false);
    }

    /// Reference Line (FE-11): the tool, where Reference Plane's is; its line lies on the view's work plane.
    pub(crate) fn reference_line_tool(&mut self) {
        self.datum_tool(true);
    }

    /// Reference Plane's tool, or Reference Line's (`line`), on; the selection goes.
    fn datum_tool(&mut self, line: bool) {
        if self.editing_2d() {
            self.views.drawing = Some(None);
            self.views.drawing_line = line;
            self.views.picking = None;
            self.views.modify = None;
            self.views.pasting = None;
            self.views.measuring = None;
            self.let_everything_go();
        }
    }

    /// Plane `g` of the family's own taken away; refused, saying why, while a form or a line is drawn on it or for a datum.
    pub(crate) fn delete_plane(&mut self, g: Guid) -> Result<(), String> {
        let f = self.family()?;
        if self.planes.iter().any(|p| p.guid == g && p.datum()) {
            return Err(t("a datum stays: it can't be deleted").to_string());
        }
        let on = f.hosted_on(g);
        if !on.is_empty() {
            return Err(tf("the plane can't go while {} is drawn on it", &[&on.join(", ")]));
        }
        self.planes.retain(|p| p.guid != g);
        self.forget_reference(g);
        self.selection.forget(Picked::Plane(g));
        Ok(())
    }

    /// Whether plane `g` is hidden for now (Temporary Hide/Isolate).
    fn plane_hidden(&self, g: Guid) -> bool {
        self.temp_hidden_plane(g)
    }

    /// The scratch model view `which` is drawn of, staged: the family as Preview Visibility and Temporary Hide/Isolate show it;
    /// while Preview Visibility is off, the solid forms the type hides a second component of their own, halftone, so they
    /// can still be seen and picked (not for a door or a window, whose wall holds one opening).
    fn staged(&self, base: &Model, which: Which) -> Result<Staged, String> {
        let f = self.family()?;
        let values = self.values();
        // The form being sketched again is left out: its sketch stands for it.
        let editing = self.sketch.as_ref().and_then(|s| s.index);
        let hidden = |i: usize| self.temp_hidden_form(i) || editing == Some(i);
        let shown = temp_hide::without(&f, hidden).unwrap_or_else(|| f.clone());
        let mut model = self.scratch(base, &shown)?;
        let mut ghost_id = None;
        // F8: what the view's Visibility Settings leave out, at the document's Detail Level, drawn halftone too.
        let cut = model.levels().first().map_or(bimcraft_core::model::CUT_HEIGHT, |(_, l)| l.view_range.cut);
        let by_view = fv::hidden_in(&f, &values, Some(which), self.views.detail, cut);
        if !self.document.preview_visibility
            && let Some(g) = ghosts(&f, &values, hidden, &by_view)
        {
            let mut s = Session::new();
            s.model = Arc::new(model);
            let name = format!("{} {GHOST}", self.name.trim());
            let type_name = self.types.get(self.picked).map(|(n, _)| n.clone()).unwrap_or_default();
            // A door's or a window's ghost stands where its family's axes are in the wall: its opening's frame.
            let mut place = json!({"family": name, "type": type_name, "at": [0, 0], "params": self.preview_params});
            let frame = s.model.elements.values().find_map(|e| match e {
                Element::Door(o) | Element::Window(o) => s.model.opening_frame(o),
                _ => None,
            });
            if let Some((p, _, z)) = frame {
                (place["at"], place["offset"]) = (json!([p.0, p.1]), json!(z));
            }
            super::at_origin(&f, &mut place);
            s.model_mut().families.insert(name.clone(), g);
            let made = s.execute("component", &place)?;
            ghost_id = made["id"].as_u64();
            model = Arc::unwrap_or_clone(s.model);
        }
        let ghost_guid = ghost_id.and_then(|id| model.elements.guid(id));
        let host_hidden = self.temp_hidden_host();
        // Its planes where the type's dimensions put them.
        let flexed = f.flexed(&values).map(std::borrow::Cow::into_owned).unwrap_or_else(|_| f.clone());
        let mut st = fv::stage(model, &flexed, &values, &|g| !self.plane_hidden(g));
        let mut ids = vec![st.level];
        ids.extend(Side::ALL.iter().filter_map(|s| st.elevation(*s)));
        for id in ids {
            let graphics = match st.model.elements.get_mut(id) {
                Some(Element::Level(l)) => &mut l.graphics,
                Some(Element::Elevation(s)) => &mut s.graphics,
                _ => continue,
            };
            graphics.detail = self.views.detail;
            if let Some(g) = ghost_guid {
                graphics.elements.insert(g, Override { halftone: true, ..Default::default() });
            }
            if host_hidden {
                graphics.categories.insert("wall".into(), Override { hidden: true, ..Default::default() });
            }
        }
        Ok(st)
    }

    /// The view shown fitted afresh the next time it is drawn (Zoom to Fit): to the room the family takes up now.
    pub(super) fn refit(&mut self) {
        match self.views.shown {
            Some(which) => {
                self.views.fitted.remove(&which);
            }
            None => self.views.fitted.clear(),
        }
        self.views.cache = None;
    }

    /// The view shown, drawn again when what it shows or how it's looked at changes.
    fn refresh_view(&mut self, ctx: &Context, base: &Model, which: Which, size: [u32; 2]) {
        let (zoom, pan) = self.views.frames.get(&which).copied().unwrap_or((1.0, (0.0, 0.0)));
        let key = {
            let mut h = DefaultHasher::new();
            (
                self.family().ok().map(|f| serde_json::to_string(&f).unwrap_or_default()),
                self.picked,
                self.name.trim(),
                which,
                size,
                zoom.to_bits(),
                pan.0.to_bits(),
                pan.1.to_bits(),
                self.document.preview_visibility,
                self.temporary(),
                self.views.detail,
                crate::style::canvas_dark(),
            )
                .hash(&mut h);
            (base.materials.len(), base.images.len()).hash(&mut h);
            // The form being sketched again, which the view leaves out.
            self.sketch.as_ref().and_then(|s| s.index).hash(&mut h);
            h.finish()
        };
        if self.views.cache.as_ref().is_some_and(|c| c.key == key) {
            return;
        }
        let kept = self.views.fitted.get(&which).filter(|(at, _)| *at == size).map(|(_, v)| *v);
        let dragging = self.views.moving.is_some()
            || self.views.edge_drag.is_some()
            || self.views.end_drag.is_some()
            || self.views.param_drag.is_some()
            || self.views.drag.is_some()
            || self.views.grip.is_some();
        let drawn = self.staged(base, which).and_then(|mut st| {
            // A view is fitted to the room the family takes up when it is first shown at this size and when Zoom to Fit asks
            // (`refit`), and keeps that frame while the family changes: a drag that takes a form past the room would
            // otherwise zoom the view out under the hand, frame after frame. Once nothing is dragged, a family grown past the
            // frame (a form sketched far off in a family that began empty, a file reloaded) is fitted again.
            let room = st.fitted(which, size[0], size[1]);
            let fitted = match kept {
                Some(v) if dragging || holds(&v, &room) => v,
                _ => room,
            };
            let shown = st.draw_lines_at(which, fitted, zoom, P2(pan.0, pan.1))?;
            Ok((st, shown, fitted))
        });
        match drawn {
            Ok((staged, shown, fitted)) => {
                self.views.fitted.insert(which, (size, fitted));
                // The drawing is painted as vectors, as a project's plan is; an elevation's hidden-line picture goes under it, as a
                // section's does, dark on a dark canvas as the drawing over it is.
                let texture = shown.picture.as_ref().map(|image| {
                    let pixels = image.rgba.as_chunks::<4>().0.iter().map(|p| crate::canvas::color(Rgb(p[0], p[1], p[2]))).collect();
                    let picture =
                        ColorImage { size: [image.width as usize, image.height as usize], pixels, source_size: Default::default() };
                    ctx.load_texture("family-view", picture, TextureOptions::NEAREST)
                });
                self.views.cache = Some(Cached {
                    key,
                    which,
                    staged,
                    view: shown.view,
                    drawing: shown.drawing,
                    texture,
                    #[cfg(test)]
                    picture: shown.picture,
                });
                self.problem = None;
            }
            Err(e) => {
                // Keep the last picture on screen: a mistyped label says what is wrong under the tab instead of leaving a blank
                // view, and the next good edit draws again.
                self.problem = Some(e);
            }
        }
    }

    /// A symbolic line, a fill or a label being drawn in the plan: its clicks, its preview, and the symbol it makes when it
    /// closes. Click the line's points and press Enter or double-click; click a fill's ring, its first point again or Enter;
    /// click where a label goes.
    #[allow(clippy::too_many_arguments)]
    fn symbol_in_view(
        &mut self,
        ui: &mut Ui,
        response: &egui::Response,
        painter: &egui::Painter,
        view: View,
        pane: Rect,
        which: Which,
        staged: &Staged,
    ) -> Option<String> {
        if which != Which::Plan {
            self.views.symbol = None;
            return Some(t("annotation symbols are drawn in the plan").to_string());
        }
        let mut draw = self.views.symbol.take()?;
        let screen = |q: P2| {
            let (x, y) = view.px(q);
            pane.min + vec2(x, y)
        };
        let at_view = |p: egui::Pos2| view.mm((p.x - pane.min.x, p.y - pane.min.y));
        // The same snaps as the project: the family's symbols, forms, reference planes and lines.
        let family = self.family().ok();
        let values = self.values();
        let mut traces: Vec<(P2, P2)> = Vec::new();
        let mut segments: Vec<(P2, P2)> = Vec::new();
        if let Some(f) = &family {
            for g in staged.planes.values() {
                if let Some(p) = f.planes.iter().find(|p| p.guid == *g)
                    && let Some((a, b)) = plane_trace(staged, which, &p.frame())
                {
                    traces.push((a, b));
                }
            }
            for l in &f.lines {
                segments.push((staged.to_view(which, l.start), staged.to_view(which, l.end)));
            }
            if let Ok(marks) = f.marks(&values) {
                for mark in marks {
                    if let bimcraft_core::family::Mark::Line(pts) = mark {
                        for w in pts.windows(2) {
                            segments.push((w[0], w[1]));
                        }
                    }
                }
            }
            for i in 0..f.forms.len() {
                let ring = fv::outline(f, &values, staged, which, i);
                for k in 0..ring.len() {
                    segments.push((ring[k], ring[(k + 1) % ring.len()]));
                }
            }
        }
        let reach =
            bimcraft_core::snap::Reach { point: crate::canvas::SNAP / view.scale, line: f64::from(crate::canvas::PICK) / view.scale };
        let square = ui.input(|i| i.modifiers.shift);
        let (snaps, snap_to_coordinates) = (self.snaps, self.snap_to_coordinates);
        let snap_step = if self.snap_step.is_finite() && self.snap_step > 0.0 { self.snap_step } else { 1.0 };
        let snap_at = |q: P2, from: &[P2]| {
            let found = bimcraft_core::snap::among(&traces, &segments, &[], q, reach, from, square, &snaps);
            // The engine's last resort rounds to 10 mm: a paper millimetre grid in a family, too coarse to draw with. Off,
            // a point with nothing to snap to stays where the cursor is; on, it rounds to the family's own step.
            if found.kind == bimcraft_core::snap::Kind::Free {
                let at =
                    if snap_to_coordinates { P2((q.0 / snap_step).round() * snap_step, (q.1 / snap_step).round() * snap_step) } else { q };
                bimcraft_core::snap::Found { at, ..found }
            } else {
                found
            }
        };
        let hover = response.hover_pos().filter(|p| pane.contains(*p)).map(at_view).map(|q| snap_at(q, &draw.points).at);
        let close_reach = 8.0 / view.scale.max(1e-9);
        if let Some(f) = response.hover_pos().filter(|p| pane.contains(*p)).map(at_view) {
            let found = snap_at(f, &draw.points);
            crate::canvas::snap_mark(
                painter,
                screen(found.at),
                found.guide.map(|g| screen(g.from)),
                found.kind,
                crate::canvas::Lock::Free,
                crate::canvas::GuideLook::of(painter, false),
                found.kind.name().to_string(),
            );
        }
        // The preview: the line so far and to the pointer, the fill's ring, where the label would go, or the arc through the
        // start, the pointer and the end.
        if draw.kind == SymbolKind::Arc {
            match (draw.points.as_slice(), hover) {
                ([a], Some(b)) => {
                    painter.line_segment([screen(*a), screen(b)], Stroke::new(1.5, PICKED));
                }
                ([a, b], Some(t)) => match arc_points(*a, t, *b) {
                    Some(pts) => {
                        painter.add(egui::Shape::line(pts.iter().map(|p| screen(*p)).collect(), Stroke::new(1.5, PICKED)));
                    }
                    None => {
                        painter.line_segment([screen(*a), screen(*b)], Stroke::new(1.0, pal().muted));
                    }
                },
                _ => {}
            }
        } else {
            let preview = draw.points.iter().copied().chain(hover.filter(|_| !draw.points.is_empty())).collect::<Vec<_>>();
            if preview.len() >= 2 {
                painter.add(egui::Shape::line(preview.iter().map(|p| screen(*p)).collect(), Stroke::new(1.5, PICKED)));
            }
        }
        for p in &draw.points {
            painter.circle_filled(screen(*p), 3.0, PICKED);
        }
        if matches!(draw.kind, SymbolKind::Label | SymbolKind::Text)
            && let Some(p) = hover
        {
            let at = screen(p);
            painter.line_segment([at + vec2(-6.0, 0.0), at + vec2(6.0, 0.0)], Stroke::new(1.0, PICKED));
            painter.line_segment([at + vec2(0.0, -6.0), at + vec2(0.0, 6.0)], Stroke::new(1.0, PICKED));
        }
        if draw.kind == SymbolKind::Dot
            && let Some(p) = hover
        {
            painter.circle_stroke(screen(p), view.scale as f32, Stroke::new(1.0, PICKED));
        }
        let mut close = false;
        // A click that only puts the pointer in the view — just after a ribbon button — may arrive as a release without egui's
        // `clicked`; take it too, so the first point after switching tools lands.
        let released = ui.input(|i| i.pointer.primary_released()) && ui.input(|i| i.pointer.latest_pos()).is_some_and(|p| pane.contains(p));
        let clicked = response.clicked() || released;
        let pointer = response.interact_pointer_pos().or_else(|| ui.input(|i| i.pointer.latest_pos()));
        if response.double_clicked()
            && let Some(p) = response.interact_pointer_pos().map(at_view).map(|q| snap_at(q, &draw.points).at)
        {
            if draw.kind == SymbolKind::Line {
                draw.points.push(p);
                close = true;
            }
        } else if clicked && let Some(p) = pointer.filter(|p| pane.contains(*p)).map(at_view).map(|q| snap_at(q, &draw.points).at) {
            match draw.kind {
                SymbolKind::Label => {
                    self.place_label(p);
                    return None;
                }
                SymbolKind::Text => {
                    self.place_text(p);
                    return None;
                }
                SymbolKind::Dot => {
                    self.place_dot(p);
                    return None;
                }
                SymbolKind::Arc => match draw.points.as_slice() {
                    // Start, then end, then a point the arc passes through.
                    [] | [_] => draw.points.push(p),
                    [a, b] => {
                        if fit_arc(*a, p, *b).is_some() {
                            self.place_arc(*a, *b, p);
                            return None;
                        }
                        self.views.status = Some(t("Arc: click the third point off the line through the first two").to_string());
                    }
                    _ => {}
                },
                SymbolKind::Line => draw.points.push(p),
                SymbolKind::Fill => {
                    let first = draw.points.first().copied();
                    if draw.points.len() >= 3 && first.is_some_and(|q| q.dist(p) <= close_reach) {
                        close = true;
                    } else {
                        draw.points.push(p);
                    }
                }
            }
        }
        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            close = true;
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.views.symbol = None;
            return None;
        }
        if close {
            match draw.kind {
                SymbolKind::Line if draw.points.len() >= 2 => self.place_line(&draw.points),
                SymbolKind::Fill if draw.points.len() >= 3 => self.place_fill(&draw.points, &draw.fill_type),
                SymbolKind::Label | SymbolKind::Text | SymbolKind::Dot | SymbolKind::Arc => {}
                _ => self.views.symbol = Some(draw),
            }
            self.views.symbol = None;
        } else {
            self.views.symbol = Some(draw);
        }
        None
    }

    /// The symbols as the plan draws them, each with its place in the family's list.
    fn symbol_marks(&self) -> Vec<(usize, PaperMark)> {
        let Some(f) = self.family().ok() else { return Vec::new() };
        let values = self.values();
        bimcraft_core::family::annotation::marks_indexed(&f, &values).unwrap_or_default()
    }

    /// The symbol under `q`, and the grip of it when the pointer is on one: what a click selects and a drag takes.
    fn symbol_at(&self, q: P2, reach: f64) -> Option<(usize, Option<SymbolNode>)> {
        for (i, mark) in self.symbol_marks() {
            match mark {
                PaperMark::Line { pts, .. } => {
                    // A grip first, so an end can be moved without moving the whole symbol.
                    if let Some(arc) = self.symbols.get(i).filter(|y| y.part == super::Part::Arc) {
                        if let Some(node) = arc_grips(arc).and_then(|g| g.iter().position(|p| p.dist(q) <= reach)) {
                            return Some((i, Some(SymbolNode::ArcGrip(node))));
                        }
                    } else if let Some(node) = pts.iter().position(|p| p.dist(q) <= reach) {
                        return Some((i, Some(SymbolNode::Line(node))));
                    }
                    if pts.windows(2).any(|w| bimcraft_core::geom::dist_to_segment(q, w[0], w[1]) <= reach) {
                        return Some((i, None));
                    }
                }
                PaperMark::Fill { rings, .. } => {
                    // A ring's point first, so a corner can be dragged without moving the whole region.
                    for (ring, points) in rings.iter().enumerate() {
                        if let Some(point) = points.iter().position(|p| p.dist(q) <= reach) {
                            return Some((i, Some(SymbolNode::Ring { ring, point })));
                        }
                    }
                    if rings.iter().any(|r| bimcraft_core::geom::contains(r, q)) {
                        return Some((i, None));
                    }
                }
                PaperMark::Dot { centre, radius } => {
                    if centre.dist(q) <= radius + reach {
                        return Some((i, None));
                    }
                }
                PaperMark::Label { at, height, text, .. } => {
                    let wide = text.chars().count() as f64 * height * 0.6;
                    if q.0 >= at.0 - reach && q.0 <= at.0 + wide + reach && (q.1 - at.1).abs() <= height.max(reach) {
                        return Some((i, None));
                    }
                }
            }
        }
        None
    }

    /// The plan's next tool takes its clicks: any other drawing, dimension or Modify tool lets go.
    fn begin_plan_tool(&mut self) {
        self.views.symbol = None;
        self.views.drawing = None;
        self.views.dimensioning = None;
        self.views.aligning = None;
    }

    pub(crate) fn start_symbol_draw(&mut self, kind: SymbolKind, fill_type: &str) {
        let mut draw = SymbolDraw::new(kind);
        if !fill_type.is_empty() {
            draw.fill_type = fill_type.to_string();
        }
        self.begin_plan_tool();
        self.views.symbol = Some(draw);
    }

    fn drag_symbol(&mut self, i: usize, node: Option<SymbolNode>, d: P2) {
        let Some(y) = self.symbols.get_mut(i) else { return };
        shift_symbol(y, node, d);
    }

    /// A symbol over the picture: selected, its grips drawn.
    fn paint_symbol(&self, painter: &egui::Painter, view: View, pane: Rect, mark: &PaperMark) {
        let screen = |q: P2| {
            let (x, y) = view.px(q);
            pane.min + vec2(x, y)
        };
        let grip = |p: P2| {
            painter.circle_filled(screen(p), 4.0, PICKED);
            painter.circle_stroke(screen(p), 4.0, Stroke::new(1.0, Color32::WHITE));
        };
        let selected = self.selected_symbol().and_then(|i| self.symbols.get(i));
        // The selection's own highlight is `select_box`'s, for every element; here are the grips a drag takes.
        match mark {
            PaperMark::Line { pts, .. } => match selected.map(|y| y.part) {
                // A line's every point is a grip; an arc's start, middle and end are.
                Some(super::Part::Arc) => {
                    if let Some(grips) = selected.and_then(arc_grips) {
                        grips.into_iter().for_each(grip);
                    }
                }
                Some(super::Part::Line) => pts.iter().copied().for_each(grip),
                _ => {}
            },
            PaperMark::Fill { rings, .. } => {
                // Both regions — Filled and Masking — take a drag of any of their corners.
                for ring in rings {
                    ring.iter().copied().for_each(grip);
                }
            }
            PaperMark::Dot { .. } | PaperMark::Label { .. } => {}
        }
    }

    /// A line through `points` as a symbolic line, selected.
    fn place_line(&mut self, points: &[P2]) {
        use super::Part;
        let mut t = super::SymbolText::blank(Part::Line);
        t.line = points.iter().map(|p| format!("{}, {}", p.0, p.1)).collect::<Vec<_>>().join("\n");
        self.symbols.push(t);
        let i = self.symbols.len() - 1;
        self.select_symbol(i);
    }

    /// The arc from `start` to `end` through `through`, as a symbolic arc, selected.
    fn place_arc(&mut self, start: P2, end: P2, through: P2) {
        use super::Part;
        let Some((centre, radius, from, to)) = fit_arc(start, through, end) else { return };
        let mut t = super::SymbolText::blank(Part::Arc);
        t.centre = [format!("{}", centre.0), format!("{}", centre.1)];
        t.radius = format!("{radius}");
        t.from = format!("{from}");
        t.to = format!("{to}");
        self.symbols.push(t);
        let i = self.symbols.len() - 1;
        self.select_symbol(i);
    }

    /// The ring `points` as a filled region of `type_name`, selected.
    fn place_fill(&mut self, points: &[P2], type_name: &str) {
        use super::Part;
        let mut t = super::SymbolText::blank(Part::Fill);
        let ring = points.iter().map(|p| [p.0, p.1]).collect::<Vec<_>>();
        t.fill = serde_json::to_string(&vec![ring]).unwrap_or_default();
        t.fill_type = type_name.to_string();
        self.symbols.push(t);
        let i = self.symbols.len() - 1;
        self.select_symbol(i);
    }

    /// A label at `at`, reading the family's first parameter until its Properties say another.
    fn place_label(&mut self, at: P2) {
        use super::Part;
        let mut t = super::SymbolText::blank(Part::Text);
        t.centre = [format!("{}", at.0), format!("{}", at.1)];
        t.radius = "2.5".into();
        t.value = self.parameter_kinds().first().map_or_else(String::new, |(name, _)| format!("{{{name}}}"));
        self.symbols.push(t);
        let i = self.symbols.len() - 1;
        self.select_symbol(i);
    }

    /// Plain text at `at`, selected: its Properties say what it reads.
    fn place_text(&mut self, at: P2) {
        use super::Part;
        let mut t = super::SymbolText::blank(Part::Text);
        t.centre = [format!("{}", at.0), format!("{}", at.1)];
        t.radius = "2.5".into();
        t.value = "Text".into();
        self.symbols.push(t);
        let i = self.symbols.len() - 1;
        self.select_symbol(i);
    }

    /// A filled dot at `at`, 1 mm across in the family's units, selected; its Properties change the radius.
    fn place_dot(&mut self, at: P2) {
        use super::Part;
        let mut t = super::SymbolText::blank(Part::Dot);
        t.centre = [format!("{}", at.0), format!("{}", at.1)];
        t.radius = "1".into();
        self.symbols.push(t);
        let i = self.symbols.len() - 1;
        self.select_symbol(i);
    }

    /// The plan or elevation shown, filling `pane`: its picture, what is selected over it, the work plane tinted while Show is
    /// on, the plane being drawn; and the clicks and drags in it.
    pub(crate) fn view_2d(&mut self, ui: &mut Ui, base: &Model, which: Which, pane: Rect) -> Option<String> {
        let size = [pane.width().clamp(64.0, 2400.0) as u32, pane.height().clamp(64.0, 1600.0) as u32];
        // A handle's drag begun in another view, or in this one before something else was shown, is given up.
        let pass = ui.ctx().cumulative_pass_nr();
        let stale = |w: Which, seen: u64| w != which || seen + 1 < pass;
        if self.views.edge_drag.as_ref().is_some_and(|d| d.which.is_none_or(|w| stale(w, d.seen)))
            || self.views.drag3d.is_some()
            || self.views.end_drag.as_ref().is_some_and(|d| stale(d.which, d.seen))
            || self.views.moving.as_ref().is_some_and(|d| stale(d.which, d.seen))
        {
            self.cancel_drags();
        }
        if let Some(d) = self.views.edge_drag.as_mut() {
            d.seen = pass;
        }
        if let Some(d) = self.views.end_drag.as_mut() {
            d.seen = pass;
        }
        if let Some(d) = self.views.moving.as_mut() {
            d.seen = pass;
        }
        // A Modify tool's points are the view's they were clicked in: in another, it starts again.
        self.views.modify_options.units = base.units;
        if let Some(m) = self.views.modify.as_mut()
            && m.which != Some(which)
        {
            (m.which, m.tool) = (Some(which), m.tool.fresh());
        }
        // Nor do they hold once the selection changed otherwise (a click, Delete): the tool starts again on what is selected.
        if let Some(m) = self.views.modify.as_mut()
            && m.selection.as_slice() != self.selection.items()
        {
            (m.selection, m.tool) = (self.selection.items().to_vec(), m.tool.fresh());
        }
        self.refresh_view(ui.ctx(), base, which, size);
        let response = ui.interact(pane, UiId::new("family-view-2d"), Sense::click_and_drag());
        self.views.pane = Some(pane);
        // The first click after a ribbon button may arrive as a release without egui's `clicked`: take it as a click too.
        let released = ui.input(|i| i.pointer.primary_released()) && ui.input(|i| i.pointer.latest_pos()).is_some_and(|p| pane.contains(p));
        // A symbolic line, a fill or a label takes the plan's clicks until it is done; the family is drawn under it.
        if self.views.symbol.is_some() {
            let cache = self.views.cache.take().filter(|c| c.which == which)?;
            let view = cache.view;
            let painter = ui.painter_at(pane);
            painter.rect_filled(pane, 0.0, crate::canvas::color(bimcraft_core::render::PAPER));
            crate::canvas::paint(&painter, crate::canvas::Place::at(pane.min), &cache.drawing, &[], None, false, &[]);
            let problem = self.symbol_in_view(ui, &response, &painter, view, pane, which, &cache.staged);
            self.views.cache = Some(cache);
            self.zoom_and_pan(ui, &response, which, view.scale);
            return problem;
        }
        let c = self.views.cache.as_ref().filter(|c| c.which == which)?;
        let view = c.view;
        let painter = ui.painter_at(pane);
        painter.rect_filled(pane, 0.0, crate::canvas::color(bimcraft_core::render::PAPER));
        if let Some(t) = &c.texture {
            let at = Rect::from_min_size(pane.min, vec2(view.width as f32, view.height as f32));
            painter.image(t.id(), at, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        crate::canvas::paint(&painter, crate::canvas::Place::at(pane.min), &c.drawing, &[], None, false, &[]);
        let screen = |q: P2| {
            let (x, y) = view.px(q);
            pane.min + vec2(x, y)
        };
        let at_view = |p: egui::Pos2| view.mm((p.x - pane.min.x, p.y - pane.min.y));
        let editable = self.adaptive.is_none();
        // Annotation symbols have paper marks and their own grips; a component family keeps the selection #598 gives it.
        let annotation = self.category.is_annotation();
        // A filled region's boundary is invisible in the project; here a dashed hint shows where its contour is. It is the
        // editor's alone, not a symbol line: it never reaches the family's drawing or a sheet.
        if editable {
            for (_, mark) in self.symbol_marks() {
                if let PaperMark::Fill { rings, .. } = mark {
                    for ring in rings {
                        if ring.len() < 2 {
                            continue;
                        }
                        let mut pts: Vec<egui::Pos2> = ring.iter().map(|p| screen(*p)).collect();
                        pts.push(pts[0]);
                        painter.add(egui::Shape::dashed_line(&pts, Stroke::new(1.0, pal().muted), 5.0, 3.0));
                    }
                }
            }
        }
        // The planes under the pointer, and Tab stepping through them where several lie on one another (F10c).
        let under =
            response.hover_pos().map(|p| planes_under(&c.drawing, &c.staged, (p.x - pane.min.x, p.y - pane.min.y))).unwrap_or_default();
        if under != self.views.under {
            (self.views.under, self.views.cycle) = (under, 0);
        }
        if response.hovered() && ui.input(|i| i.key_pressed(egui::Key::Tab)) {
            ui.ctx().memory_mut(|m| {
                if let Some(f) = m.focused() {
                    m.surrender_focus(f);
                }
            });
            if self.views.under.len() > 1 {
                self.views.cycle = (self.views.cycle + 1) % self.views.under.len();
            }
        }
        let (under, cycle) = (self.views.under.clone(), self.views.cycle);
        // The plane a click at `p` takes: the one Tab has come to, where the pointer is over several; else the nearest.
        let plane_at = |drawing: &Drawing, staged: &Staged, p: egui::Pos2| -> Option<Guid> {
            let here = planes_under(drawing, staged, (p.x - pane.min.x, p.y - pane.min.y));
            if here.len() > 1 && here == under { here.get(cycle % here.len()).copied() } else { here.first().copied() }
        };
        // Where several planes lie on one another, the one a click would take, lit.
        if editable && under.len() > 1 {
            for s in plane_strokes(&c.drawing, &c.staged, under[cycle % under.len()], pane.min) {
                painter.line_segment(s, Stroke::new(2.0, PRESELECT));
            }
        }
        // A form being sketched (F5): the view draws and picks for the sketch alone.
        if editable && self.sketch.is_some() {
            let cache = self.views.cache.take()?;
            let family = self.family().ok();
            let values = self.values();
            let editing = self.sketch.as_ref().and_then(|s| s.index);
            let planes = self.planes.clone();
            let reach = 6.0 / view.scale.max(1e-9);
            // The Work Plane dialog's Pick a plane or Pick a line, from sketch mode's Set (T12): the click is its.
            if self.views.picking.is_some() {
                let mut problem = None;
                if response.clicked()
                    && let Some(p) = response.interact_pointer_pos()
                {
                    let plane = plane_at(&cache.drawing, &cache.staged, p);
                    let line = cache.staged.line_at(which, at_view(p), reach);
                    problem = self.picked(&Arc::new(base.clone()), plane, line);
                }
                self.views.cache = Some(cache);
                self.zoom_and_pan(ui, &response, which, view.scale);
                return problem;
            }
            // Pick Lines: a reference plane's or a reference line's trace, else the nearest edge of another form's outline. With
            // Lock on (F10a), what Align would take there (a plane, a reference line, another form's line), as its lock waits.
            let locking = self.sketch.as_ref().is_some_and(|s| s.lock && s.options().offset == 0.0);
            let own = self.sketch.as_ref().map(|s| s.guid).unwrap_or_default();
            let pick = |p: egui::Pos2| -> Option<super::sketch::PickedInView> {
                let picked = plane_at(&cache.drawing, &cache.staged, p);
                let lock = |r: DimRef| FamilyDimension {
                    id: 0,
                    guid: Guid::new(),
                    view: bimcraft_core::family::DimView::of(which),
                    refs: vec![r],
                    at: cache.staged.from_view(which, at_view(p)),
                    label: None,
                    locked: vec![bimcraft_core::model::Lock { segment: 0, length: 0.0 }],
                    equal: false,
                    alignment: true,
                    kind: Default::default(),
                };
                // The planes as the type previewed puts them, as the view draws them.
                let flexed = family.as_ref().map(|f| f.flexed(&values).map(std::borrow::Cow::into_owned).unwrap_or_else(|_| f.clone()));
                let trace = |g: Guid| {
                    let plane = flexed
                        .as_ref()
                        .and_then(|f| f.planes.iter().find(|q| q.guid == g))
                        .or_else(|| planes.iter().find(|q| q.guid == g))?;
                    plane_trace(&cache.staged, which, &plane.frame())
                };
                if locking && let Some(flexed) = flexed.as_ref() {
                    let screen_of = dims::screen_of(view, pane.min);
                    if let Some(r) =
                        dims::reference_at(flexed, &values, &cache.staged, which, picked, p, &screen_of).filter(|r| !r.names(own))
                        && let Some((a, b)) = dims::drawn_as(flexed, &values, &cache.staged, which, &r).filter(|(a, b)| a != b)
                    {
                        // A plane's whole trace, as Pick Lines takes it unlocked; else the line the reference is drawn as.
                        let line = match r {
                            DimRef::Plane(g) => trace(g),
                            _ => None,
                        };
                        return Some((line.unwrap_or((a, b)), Some(lock(r))));
                    }
                }
                if let Some(t) = picked.and_then(trace) {
                    return Some((t, None));
                }
                let q = at_view(p);
                let f = family.as_ref()?;
                let mut best: Option<(f64, (P2, P2))> = None;
                // An edge the view draws the form with: an L's inner edges, never its hull's chord across the corner.
                for i in (0..f.forms.len()).filter(|i| Some(*i) != editing) {
                    for (a, b) in fv::outline_edges(f, &values, &cache.staged, which, i) {
                        let d = bimcraft_core::geom::dist_to_segment(q, a, b);
                        if d <= reach && a.dist(b) > 1e-6 && best.is_none_or(|(e, _)| d < e) {
                            best = Some((d, (a, b)));
                        }
                    }
                }
                best.map(|(_, s)| (s, None))
            };
            // Align on the sketch's lines (F7b), and the padlocks of its locks: clicked before the sketch takes the click.
            let frame = self.sketch_frame();
            let on_screen = |q: P2| -> Option<egui::Pos2> {
                let f = frame.as_ref()?;
                let (x, y) = view.px(super::sketch::in_view(&cache.staged, which, f, q));
                Some(pane.min + vec2(x, y))
            };
            let mut lines = drawn_lines(&cache.drawing, pane.min);
            // The sketch's dimensions: those made while sketching and the draft's that measure the form, and the one
            // being placed. Align's padlocks keep off them too.
            self.prune_selected_dim();
            let mm_screen = dims::screen_of(view, pane.min);
            let plane_here = |p: egui::Pos2| plane_at(&cache.drawing, &cache.staged, p);
            let sketch_dims = self.sketch_dims(&cache.staged, which, &mm_screen, response.hover_pos(), &at_view, &plane_here);
            if let Some(sd) = sketch_dims.as_ref() {
                lines.extend(sd.lines(&mm_screen));
            }
            let locks = self.sketch_padlocks(&on_screen, &lines);
            let free = self.sketch.as_ref().is_some_and(|s| s.tool.is_none());
            // A shape of the Draw panel picked puts Align and the dimension tools down, and lets a dimension go.
            if !free {
                self.views.aligning = None;
                self.views.dimensioning = None;
                if let Some(s) = self.sketch.as_mut() {
                    s.selected_dim = None;
                }
            }
            let aligning = self.views.aligning.is_some();
            let mut aligned = None;
            let mut dims_took = false;
            if response.clicked()
                && let Some(p) = response.interact_pointer_pos()
            {
                let near = locks.iter().find(|(at, _)| dims::lock_rect(*at).contains(p)).map(|(_, w)| *w);
                match near {
                    Some(what) if (aligning || free) && self.views.dimensioning.is_none() => self.sketch_padlock(what),
                    _ if !aligning && let Some(sd) = sketch_dims.as_ref() => {
                        let (took, e) = self.sketch_dims_click(sd, &cache.staged, which, p, at_view(p), &mm_screen, &plane_here);
                        dims_took = took;
                        if e.is_some() {
                            aligned = Some(e);
                        }
                    }
                    _ if aligning => {
                        // What the view shows there besides the sketch: a plane, a reference line, another form's line.
                        let own = self.sketch.as_ref().map(|s| s.guid).unwrap_or_default();
                        let flexed =
                            family.as_ref().map(|f| f.flexed(&values).map(std::borrow::Cow::into_owned).unwrap_or_else(|_| f.clone()));
                        let screen_of = dims::screen_of(view, pane.min);
                        let plane = plane_at(&cache.drawing, &cache.staged, p);
                        let outside = flexed.as_ref().and_then(|f| {
                            let r = dims::reference_at(f, &values, &cache.staged, which, plane, p, &screen_of).filter(|r| !r.names(own))?;
                            let (a, b) = dims::drawn_as(f, &values, &cache.staged, which, &r)?;
                            (a != b).then_some((r, (a, b)))
                        });
                        aligned = Some(self.sketch_align(&cache.staged, which, at_view(p), reach, outside));
                    }
                    _ => {}
                }
            }
            let problem = self.sketch_in_view(ui, &response, &painter, &cache.staged, which, &view, pane, &pick);
            // A click the dimensions took picks no line of the sketch.
            if (dims_took || self.views.dimensioning.is_some())
                && let Some(s) = self.sketch.as_mut()
            {
                s.sketch.picked.clear();
            }
            if let Some(sd) = sketch_dims.as_ref() {
                self.paint_sketch_dims(&painter, sd, &cache.staged, which, &mm_screen, base.units);
            }
            // The reference Align picked, and the padlocks, over the lines.
            if let Some((_, (a, b), _)) = self.sketch.as_ref().and_then(|s| s.align_from.as_ref())
                && let (Some(a), Some(b)) = (on_screen(*a), on_screen(*b))
            {
                painter.line_segment([a, b], Stroke::new(2.5, PICKED));
            }
            for (at, what) in self.sketch_padlocks(&on_screen, &lines) {
                let open = what == super::sketch::Padlock::Offer;
                dims::padlock(&painter, at, !open, pal().accent);
            }
            self.views.cache = Some(cache);
            if let Some(Some(e)) = aligned {
                return Some(e);
            }
            self.zoom_and_pan(ui, &response, which, view.scale);
            return problem;
        }
        let values = self.values();
        let drawn = self.family().ok();
        // The family as the type previewed flexes it: where its dimensions put its planes and what is drawn on them.
        let family: Option<Family> =
            drawn.as_ref().map(|f| f.flexed(&values).map(std::borrow::Cow::into_owned).unwrap_or_else(|_| f.clone()));
        let on_screen = dims::screen_of(view, pane.min);
        let pick_plane = |p: egui::Pos2| -> Option<Guid> { plane_at(&c.drawing, &c.staged, p) };
        let kind = self.views.dim_kind;
        let radial = kind.is_round();
        let angular = kind == bimcraft_core::family::DimKind::Angular;
        let circles = radial && self.views.dimensioning.is_some();
        let crossing = angular && self.views.dimensioning.is_some();
        // Radial and Diameter Dimension pick a circle or an arc by its edge, Angular Dimension lines, the others references.
        let tool_kind = if circles || crossing { kind } else { bimcraft_core::family::DimKind::Aligned };
        let reference_at = |p: egui::Pos2| -> Option<DimRef> {
            let f = family.as_ref()?;
            dims::pick_reference(f, &values, &c.staged, which, tool_kind, pick_plane(p), p, &on_screen)
        };
        // Its dimensions, the one being placed with them, over the picture: the selected one, and those that can't hold, picked.
        let placing = match (&self.views.dimensioning, response.hover_pos(), family.as_ref()) {
            (Some(refs), Some(h), Some(f)) => dims::placing(f, &values, &c.staged, which, kind, refs, reference_at(h), at_view(h)),
            _ => None,
        };
        let marks = match (drawn.as_ref(), family.as_ref()) {
            (Some(d), Some(f)) if editable => dims::marks_of(d, f, &values, &c.staged, which, placing),
            _ => Vec::new(),
        };
        // Pivots (F7d): their marks, and the selected element's grips.
        let pivots = match family.as_ref() {
            Some(f) if editable => bimcraft_core::family::constraints::pivot_marks(f, &values, &c.staged, which),
            _ => Vec::new(),
        };
        let grips = match family.as_ref() {
            Some(f) if editable => self.grips(&pivots, f, &c.staged, which, &on_screen),
            _ => Vec::new(),
        };
        let selected_dim = self.selected_dimension();
        let unsatisfied: Vec<Guid> = self.views.unsatisfied.as_ref().map(|u| u.dims.clone()).unwrap_or_default();
        // Where Align's padlocks go: beside their lines, clear of every line drawn and of each other (`padlock_spot`), the
        // one Align offers too (the last).
        let mut lines = drawn_lines(&c.drawing, pane.min);
        for m in marks.iter().filter(|m| !m.alignment) {
            if m.kind == bimcraft_core::family::DimKind::Angular {
                // Where its lines cross out to its arc, along each.
                if let Some(v) = m.points.first() {
                    lines.extend(m.points.iter().skip(1).map(|p| [on_screen(*v), on_screen(*p)]));
                }
            } else if let (Some(a), Some(b)) = (m.points.first(), m.points.last()) {
                lines.push([on_screen(*a), on_screen(*b)]);
                // Its ticks: a small cross at each point, so a padlock keeps off them too.
                for p in &m.points {
                    let c = on_screen(*p);
                    lines.push([c - egui::vec2(TICK, 0.0), c + egui::vec2(TICK, 0.0)]);
                    lines.push([c - egui::vec2(0.0, TICK), c + egui::vec2(0.0, TICK)]);
                }
            }
        }
        let offer = self.views.align_lock.filter(|(.., w)| *w == which).and_then(|(reference, target, at, _)| {
            let (d, f) = (drawn.as_ref()?, family.as_ref()?);
            let lock = FamilyDimension {
                refs: vec![reference, target],
                alignment: true,
                ..dims::new_dimension(f, &values, &c.staged, which, &[], at, Default::default())
            };
            dims::marks_of(d, f, &values, &c.staged, which, Some(lock)).pop()
        });
        let mut pending: Vec<(&dims::DimMark, Vec<DimRef>)> = marks
            .iter()
            .filter(|m| m.alignment)
            .map(|m| (m, self.dimensions.get(m.index).map(|d| d.refs.clone()).unwrap_or_default()))
            .collect();
        if let (Some(o), Some((a, b, ..))) = (offer.as_ref(), self.views.align_lock) {
            pending.push((o, vec![a, b]));
        }
        let mut locks: Vec<(usize, egui::Pos2)> = Vec::new();
        for (m, refs) in pending {
            let (Some(base), Some(f)) = (m.points.first(), family.as_ref()) else { continue };
            let base_at = on_screen(*base);
            let dir = on_screen(*base + m.across) - base_at;
            // A form one of its references belongs to: its outline is the inside the padlock keeps out of. Planes and lines
            // alone: every solid form's, so a plane's padlock isn't left inside a form it runs through.
            let ring = |i: usize| -> Vec<egui::Pos2> { fv::outline(f, &values, &c.staged, which, i).into_iter().map(&on_screen).collect() };
            let own = refs.iter().find_map(|r| match r {
                DimRef::Form(g, _) | DimRef::Sketch(g, _) | DimRef::Point(g, ..) => f.forms.iter().position(|x| x.guid == *g),
                _ => None,
            });
            let rings: Vec<Vec<egui::Pos2>> = match own {
                Some(i) => vec![ring(i)],
                None => (0..f.forms.len()).filter(|i| !f.forms[*i].void).map(ring).collect(),
            };
            let taken: Vec<egui::Pos2> = locks.iter().map(|(_, p)| *p).collect();
            let inside = |p: egui::Pos2| rings.iter().any(|r| r.len() > 2 && dims::within(r, p));
            let at = dims::padlock_spot(base_at, dir, &inside, &lines, &taken);
            locks.push((m.index, at));
        }
        let offered = offer.as_ref().and_then(|o| locks.iter().find(|(i, _)| *i == o.index).map(|(_, p)| *p));
        self.views.locks_shown = locks.clone();
        self.views.offer_shown = offered;
        for m in &marks {
            let g = self.dimensions.get(m.index).map(|d| d.guid);
            let picked = g.is_some_and(|g| self.selection.contains(Picked::Dimension(g)) || unsatisfied.contains(&g));
            let lock = locks.iter().find(|(i, _)| *i == m.index).map(|(_, p)| *p);
            dims::paint(&painter, m, &on_screen, picked, base.units, lock);
        }
        // The references Aligned Dimension and Align have picked.
        let picked_refs: Vec<DimRef> = match (&self.views.dimensioning, &self.views.aligning) {
            (Some(refs), _) => refs.clone(),
            (_, Some(Some(r))) => vec![*r],
            _ => Vec::new(),
        };
        if let Some(f) = family.as_ref() {
            for r in &picked_refs {
                // A plane by the strokes its trace is drawn with, short of its name.
                let strokes = match r {
                    DimRef::Plane(g) => plane_strokes(&c.drawing, &c.staged, *g, pane.min),
                    _ => Vec::new(),
                };
                if !strokes.is_empty() {
                    for s in strokes {
                        painter.line_segment(s, Stroke::new(2.5, PICKED));
                    }
                    continue;
                }
                match dims::drawn_as(f, &values, &c.staged, which, r) {
                    // A point (a circle's centre): a ring round it.
                    Some((a, b)) if a == b => {
                        painter.circle_stroke(screen(a), 5.0, Stroke::new(2.0, PICKED));
                    }
                    Some((a, b)) => {
                        painter.line_segment([screen(a), screen(b)], Stroke::new(2.5, PICKED));
                    }
                    None => {}
                }
                // A circle or an arc picked by Radial or Diameter Dimension: the circle or the arc itself.
                if radial
                    && let Some(round) =
                        bimcraft_core::family::constraints::sketch_circles(f, &values, &c.staged, which).into_iter().find(|x| x.r == *r)
                {
                    let (centre, radius) = (round.centre, round.radius);
                    match round.arc {
                        None => {
                            let rim = screen(P2(centre.0 + radius, centre.1));
                            painter.circle_stroke(screen(centre), (rim - screen(centre)).length(), Stroke::new(2.5, PICKED));
                        }
                        Some([a, m, b]) => {
                            let angle = |q: P2| (q.1 - centre.1).atan2(q.0 - centre.0);
                            let tau = std::f64::consts::TAU;
                            let (to_m, to_b) = ((angle(m) - angle(a)).rem_euclid(tau), (angle(b) - angle(a)).rem_euclid(tau));
                            let sweep = if to_m <= to_b { to_b } else { to_b - tau };
                            let pts: Vec<egui::Pos2> = (0..=48)
                                .map(|k| {
                                    let t = angle(a) + sweep * f64::from(k) / 48.0;
                                    screen(P2(centre.0 + radius * t.cos(), centre.1 + radius * t.sin()))
                                })
                                .collect();
                            painter.add(egui::Shape::line(pts, Stroke::new(2.5, PICKED)));
                        }
                    }
                }
            }
        }
        // Align's padlock, open, until the next click.
        let align_lock = offered;
        if let Some(at) = align_lock {
            dims::padlock(&painter, at, false, pal().accent);
        }
        // The connectors (#269), at their places in the family's axes as the view sees them; the one under the pointer lit, as a
        // click would pick it.
        let connector_marks = if editable { self.connector_marks() } else { Vec::new() };
        let project = |q: [f64; 3]| Some(screen(c.staged.to_view(which, q)));
        self.connector_hover = response.hover_pos().and_then(|p| super::connector_marks::pick(&connector_marks, &project, p));
        let lit = (PICKED, crate::style::canvas_pal().hover);
        let chosen_connectors: Vec<usize> =
            self.selection.items().iter().filter_map(|p| if let Picked::Connector(i) = p { Some(*i) } else { None }).collect();
        super::connector_marks::paint(&painter, &connector_marks, &project, &chosen_connectors, self.connector_hover, lit);
        // The insertion point, where the planes that define the origin cross (O1).
        if let Some(at) = self.origin_mark() {
            super::origin_mark::paint(&painter, &project, at);
        }
        // What is selected, over the picture: each of it as one selected alone is drawn (FE-1, `select_box.rs`); the
        // reference line selected, its pivots' marks and its grips (F7d).
        // The forms the view leaves out: Temporary Hide/Isolate's, and while Preview Visibility shows the family as placed,
        // those its Visibility Settings hide here (F8), which it otherwise draws halftone (`staged`).
        let by_view = match drawn.as_ref() {
            Some(f) if self.document.preview_visibility => fv::hidden_in(f, &values, Some(which), self.views.detail, c.staged.cut()),
            _ => Vec::new(),
        };
        let hidden: Vec<bool> =
            (0..self.forms.len()).map(|i| self.temp_hidden_form(i) || by_view.get(i).copied().unwrap_or(false)).collect();
        let scene = family.as_ref().map(|f| super::select_box::Scene {
            family: f,
            values: &values,
            staged: &c.staged,
            drawing: &c.drawing,
            which,
            view,
            min: pane.min,
            dims: &marks,
            dimensions: &self.dimensions,
            connectors: &connector_marks,
            detail: self.views.detail,
            hidden: &hidden,
            by_face: self.selecting.by_face,
            drag_faces: self.selecting.drag_faces,
        });
        if editable && let Some(scene) = scene.as_ref() {
            scene.paint_selected(&painter, self.selection.items());
        }
        // An annotation symbol selected: its grips, which the plan's clicks drag.
        if editable
            && annotation
            && let Some(i) = self.selected_symbol()
            && let Some((_, mark)) = self.symbol_marks().into_iter().find(|(j, _)| *j == i)
        {
            self.paint_symbol(&painter, view, pane, &mark);
        }
        // A Modify tool: what of the selection the view shows and takes, the view's way into the family, the axis Mirror
        // - Pick Axis would take under the pointer, and what Offset takes where a click is.
        let space = super::modify::Space::of(&c.staged, which);
        let shown_selection: Vec<Picked> = match scene.as_ref() {
            Some(s) if self.views.modify.is_some() => self
                .selection
                .items()
                .iter()
                .copied()
                .filter(|p| !matches!(p, Picked::Form(i) if hidden.get(*i).copied().unwrap_or(false)) && !s.pieces(*p).is_empty())
                .collect(),
            _ => Vec::new(),
        };
        let reach = 6.0 / view.scale.max(1e-9);
        let axis_at = |p: egui::Pos2| -> Option<(P2, P2)> {
            let f = family.as_ref()?;
            if let Some(g) = pick_plane(p) {
                let plane = f
                    .planes
                    .iter()
                    .find(|q| q.guid == g)
                    .cloned()
                    .or_else(|| bimcraft_core::family::datums().into_iter().find(|d| d.guid == g))?;
                return plane_trace(&c.staged, which, &plane.frame());
            }
            let q = at_view(p);
            if let Some(i) = c.staged.line_at(which, q, reach)
                && let Some(l) = f.lines.get(i)
            {
                let (a, b) = (c.staged.to_view(which, l.start), c.staged.to_view(which, l.end));
                return (a.dist(b) > 1e-6).then_some((a, b));
            }
            let mut best: Option<(f64, (P2, P2))> = None;
            // An edge the view draws a form with, as Pick Lines takes: never its hull's chord.
            for i in 0..f.forms.len() {
                for (a, b) in fv::outline_edges(f, &values, &c.staged, which, i) {
                    let d = bimcraft_core::geom::dist_to_segment(q, a, b);
                    if d <= reach && a.dist(b) > 1e-6 && best.is_none_or(|(e, _)| d < e) {
                        best = Some((d, (a, b)));
                    }
                }
            }
            best.map(|(_, s)| s)
        };
        let offset_at = |p: egui::Pos2| -> super::modify::OffsetHit {
            let f = family.as_ref()?;
            let q = at_view(p);
            if let Some(i) = c.staged.line_at(which, q, reach)
                && let Some(l) = f.lines.get(i)
            {
                return Some(Ok((Picked::Line(l.guid), (c.staged.to_view(which, l.start), c.staged.to_view(which, l.end)))));
            }
            if let Some(g) = pick_plane(p) {
                let Some(plane) = f.planes.iter().find(|q| q.guid == g && !q.datum()) else {
                    return Some(Err(t("A datum stays where it is: offset a reference plane of the family's own.").to_string()));
                };
                return plane_trace(&c.staged, which, &plane.frame()).map(|t| Ok((Picked::Plane(g), t)));
            }
            fv::form_at(f, &values, &c.staged, which, q)
                .map(|_| Err(t("Offset takes reference planes and reference lines: edit a form's sketch to offset its lines.").to_string()))
        };
        // The planes' and lines' traces a Modify tool's points and Measure's snap to.
        let traces: Vec<(P2, P2)> = match family.as_ref() {
            Some(f) if self.views.modify.is_some() || self.views.measuring.is_some() => {
                let planes = f
                    .planes
                    .iter()
                    .cloned()
                    .chain(bimcraft_core::family::datums().into_iter().filter(|d| !f.planes.iter().any(|p| p.guid == d.guid)));
                planes
                    .filter_map(|p| plane_trace(&c.staged, which, &p.frame()))
                    .chain(f.lines.iter().map(|l| (c.staged.to_view(which, l.start), c.staged.to_view(which, l.end))))
                    .collect()
            }
            _ => Vec::new(),
        };
        let snapped = |p: egui::Pos2| super::modify::snap_to(at_view(p), &traces, reach);
        if self.views.modify.is_some() {
            let pieces: Vec<[egui::Pos2; 2]> =
                scene.as_ref().map(|s| shown_selection.iter().flat_map(|p| s.pieces(*p)).collect()).unwrap_or_default();
            let centred: Vec<[egui::Pos2; 2]> = scene
                .as_ref()
                .map(|s| shown_selection.iter().filter(|p| !matches!(p, Picked::Dimension(_))).flat_map(|p| s.pieces(*p)).collect())
                .unwrap_or_default();
            let hover = response.hover_pos();
            let axis = hover.and_then(axis_at);
            let shift = ui.input(|i| i.modifiers.shift);
            super::modify::paint_modify(
                (&mut self.views.modify, &self.views.modify_options),
                &self.snaps,
                &painter,
                hover.map(snapped),
                (&pieces, &centred),
                &screen,
                &at_view,
                axis,
                shift,
            );
            // Offset: where the plane or the line under the pointer would go.
            if let (Some(h), Some(super::modify::Tool::Offset)) = (hover, self.views.modify.as_ref().map(|m| m.tool))
                && let Some(Ok((_, line))) = offset_at(h)
                && let Some((a, b)) = super::modify::offset_line(line, at_view(h), &self.views.modify_options)
            {
                let stroke = Stroke::new(1.5, crate::style::canvas_pal().accent);
                painter.extend(egui::Shape::dashed_line(&[screen(a), screen(b)], stroke, 8.0, 4.0));
            }
        }
        // Paste following the pointer: the copies where they were copied, as the view draws them, their middle held by
        // the pointer; that middle is what a click puts where it is.
        if self.views.pasting.is_some() && editable {
            // Worked out once for the draft, the clipboard and the view; the pointer only moves it.
            let key = self.views.pasting.as_ref().map(|p| {
                let view_now = (which, view.scale, view.cx, view.cy, view.width, view.height, view.angle, pane.min, self.views.detail);
                key_of(&[&family, &p.clip], &[&values, &view_now, &hidden])
            });
            let kept = self.views.paste_ghost.take().filter(|(k, _)| Some(*k) == key);
            let (ghost, held) = match kept {
                Some((_, g)) => {
                    let out = g.clone();
                    self.views.paste_ghost = Some((key.unwrap_or_default(), g));
                    out
                }
                None => {
                    let mut ghost: Vec<[egui::Pos2; 2]> = Vec::new();
                    let mut held: Vec<[egui::Pos2; 2]> = Vec::new();
                    let preview = self.paste_preview();
                    if let Some((trial, items)) = preview.as_ref() {
                        let flexed = trial.flexed(&values).map(std::borrow::Cow::into_owned).unwrap_or_else(|_| trial.clone());
                        let s = super::select_box::Scene {
                            family: &flexed,
                            values: &values,
                            staged: &c.staged,
                            drawing: &c.drawing,
                            which,
                            view,
                            min: pane.min,
                            dims: &marks,
                            dimensions: &self.dimensions,
                            connectors: &connector_marks,
                            detail: self.views.detail,
                            hidden: &hidden,
                            by_face: false,
                            drag_faces: false,
                        };
                        ghost = items.iter().flat_map(|p| s.pieces(*p)).collect();
                        // Held by the middle of what has a size of its own: a plane's trace runs across the view.
                        held = items.iter().filter(|p| !matches!(p, Picked::Plane(_))).flat_map(|p| s.pieces(*p)).collect();
                    }
                    #[cfg(test)]
                    {
                        self.views.cache_builds += 1;
                    }
                    self.views.paste_ghost = key.map(|k| (k, (ghost.clone(), held.clone())));
                    (ghost, held)
                }
            };
            let by = if held.is_empty() { &ghost } else { &held };
            let anchor = (!by.is_empty()).then(|| {
                let r = egui::Rect::from_points(&by.iter().flatten().copied().collect::<Vec<_>>());
                at_view(r.center())
            });
            if let Some(p) = self.views.pasting.as_mut() {
                p.anchor = anchor.map(|a| (which, a));
            }
            if let (Some(a), Some(h)) = (anchor, response.hover_pos()) {
                let d = at_view(h) - a;
                let stroke = Stroke::new(1.5, crate::style::canvas_pal().accent.gamma_multiply(0.8));
                for [p, q] in &ghost {
                    painter.line_segment([screen(at_view(*p) + d), screen(at_view(*q) + d)], stroke);
                }
            }
        }
        let chosen = self.selected_line().or_else(|| self.selected_plane());
        let turning = match self.views.grip {
            Some((super::pivot::Grip::Rotate(g), _)) => Some(g),
            _ => None,
        };
        super::pivot::paint(&painter, &pivots, &on_screen, chosen, turning);
        let ends: Vec<(super::pivot::Grip, egui::Pos2)> =
            grips.iter().filter(|(g, _)| matches!(g, super::pivot::Grip::End(..))).copied().collect();
        super::pivot::paint_ends(&painter, &ends);
        self.views.grips_shown = grips.clone();
        // Pivot, on: the point the click would pin it to, ringed.
        if self.views.pivoting.is_some()
            && let (Some(h), Some(f)) = (response.hover_pos(), family.as_ref())
        {
            let targets = bimcraft_core::family::constraints::pivot_targets(f, &values, &c.staged, which);
            let (_, p) = super::pivot::target(&c.staged, which, &targets, at_view(h), &on_screen);
            painter.circle_stroke(screen(c.staged.to_view(which, p)), 7.0, Stroke::new(2.0, PICKED));
        }
        // Work Plane > Show: the plane tinted, all over where the view looks straight at it, along its trace where across it.
        if editable
            && self.views.show_work_plane
            && let Ok(f) = self.plane_set().frame(&self.views.work_plane())
        {
            match plane_trace(&c.staged, which, &f) {
                Some((a, b)) => {
                    painter.line_segment([screen(a), screen(b)], Stroke::new(10.0, TINT));
                }
                None => {
                    painter.rect_filled(pane, 0.0, TINT);
                }
            }
        }
        // The plane being drawn, from its first click to the pointer.
        if let Some(Some(first)) = self.views.drawing
            && let Some(hover) = response.hover_pos()
        {
            let hover =
                if ui.input(|i| i.modifiers.shift) { screen(bimcraft_core::snap::orthogonal_step(first, at_view(hover)).0) } else { hover };
            painter.line_segment([screen(first), hover], Stroke::new(1.5, PICKED));
        }
        // Measure's ruler: from its first click to the pointer, and the one laid in this view, read beside it.
        if let Some(m) = self.views.measuring.as_ref() {
            if let (Some(first), Some(hover)) = (m.first, response.hover_pos()) {
                painter.line_segment([screen(first), hover], Stroke::new(1.5, PICKED));
            }
            if let Some((a, b, _)) = m.laid.filter(|l| l.2 == which) {
                let (pa, pb) = (screen(a), screen(b));
                painter.line_segment([pa, pb], Stroke::new(2.0, PICKED));
                for q in [pa, pb] {
                    let mark = egui::Rect::from_center_size(q, egui::vec2(8.0, 8.0));
                    painter.rect_stroke(mark, 0.0, Stroke::new(2.0, PICKED), egui::StrokeKind::Middle);
                }
                // Beside the ruler, on its upper side, clear of it whatever its slope.
                let n = (pb - pa).normalized().rot90();
                let n = if n.y > 0.0 { -n } else { n };
                let text = super::measure::read(&bimcraft_core::snap::ruler(a, b), base.units);
                // On a backing of the canvas's colour, so it reads over what the view draws.
                let galley = painter.layout_no_wrap(text, egui::FontId::proportional(12.0), PICKED);
                let size = galley.size();
                let clear = n.x.abs() * size.x / 2.0 + n.y.abs() * size.y / 2.0 + 8.0;
                let at = egui::Rect::from_center_size(pa + (pb - pa) / 2.0 + n * clear, size);
                painter.rect_filled(at.expand(3.0), 3.0, crate::style::pal().surface);
                painter.galley(at.min, galley, PICKED);
            }
        }
        // A selected extrusion's Start and End, where the view sees its plane edge-on: shape handles to drag (F5); one tied to
        // a parameter, a formula or a dimension, greyed. Its profile's edges' handles too, drawn alike.
        let handles = if editable { self.end_handles(&c.staged, which) } else { Vec::new() };
        let shapes = match (drawn.as_ref(), family.as_ref()) {
            (Some(d), Some(f)) if editable => self.shape_handles(d, f, &c.staged, which),
            _ => Vec::new(),
        };
        // The handle under the pointer that a drag takes, lit; a greyed one never is.
        let hover = response.hover_pos();
        let lit = hover.and_then(|p| {
            let free = handles.iter().filter(|h| h.2.is_none()).map(|h| screen(h.0));
            let free = free.chain(shapes.iter().filter(|h| h.blocked.is_none()).map(|h| screen(h.at)));
            free.filter(|at| at.distance(p) <= HANDLE).min_by(|a, b| a.distance(p).total_cmp(&b.distance(p)))
        });
        let lit = lit.filter(|_| self.views.moving.is_none() && self.views.grip.is_none());
        self.views.lit_handle = lit;
        let arrow = |at: P2, way: [f64; 3], free: bool| {
            let centre = screen(at);
            let towards = c.staged.to_view(which, way) - c.staged.to_view(which, [0.0; 3]);
            let way = egui::vec2(towards.0 as f32, -towards.1 as f32).normalized();
            super::handles3d::paint_arrow(&painter, centre, way, free, lit == Some(centre));
        };
        // The side or end held, lined up with something: a dashed line through it, along what it lines up with.
        if self.lined_up() {
            let (side, end) = (self.views.edge_drag.as_ref().map(|d| d.line()), self.views.end_drag.as_ref().map(|d| d.end));
            let held = shapes.iter().find(|h| Some(h.line) == side).map(|h| (h.at, h.way));
            let held = held.or_else(|| handles.iter().find(|h| Some(h.1) == end).map(|h| (h.0, h.3)));
            if let Some((at, way)) = held {
                let centre = screen(at);
                let towards = c.staged.to_view(which, way) - c.staged.to_view(which, [0.0; 3]);
                let along = egui::vec2(towards.0 as f32, -towards.1 as f32).normalized().rot90() * pane.size().length();
                let stroke = Stroke::new(1.0, crate::style::canvas_pal().hover);
                painter.extend(egui::Shape::dashed_line(&[centre - along, centre + along], stroke, 6.0, 4.0));
            }
        }
        for (at, end, blocked, n, _) in &handles {
            let k = if *end { 1.0 } else { -1.0 };
            arrow(*at, [n[0] * k, n[1] * k, n[2] * k], blocked.is_none());
        }
        for h in &shapes {
            arrow(h.at, h.way, h.blocked.is_none());
        }
        self.views.ends_shown = handles.iter().map(|h| (screen(h.0), h.2.is_none())).collect();
        self.views.shapes_shown = shapes.iter().map(|h| (screen(h.at), h.clone())).collect();
        let on_handle = |p: egui::Pos2| {
            handles.iter().any(|h| screen(h.0).distance(p) <= HANDLE) || shapes.iter().any(|h| screen(h.at).distance(p) <= HANDLE)
        };
        let mut problem = None;
        let mut end_moved: Option<(bool, f64)> = None;
        let mut edge_moved: Option<[f64; 3]> = None;
        let mut begin_param: Option<(usize, super::handle_params::Target, Vec<String>)> = None;
        let mut begin_snap: Option<(super::handle_params::Target, [f64; 3], bool)> = None;
        if response.drag_started_by(PointerButton::Primary)
            && let Some(p) = ui.input(|i| i.pointer.press_origin())
        {
            if let Some((_, end, blocked, n, param)) = handles.iter().find(|h| screen(h.0).distance(p) <= HANDLE) {
                match blocked {
                    None => {
                        // Through a parameter: its drag begun below, once the view is drawn.
                        if let (Some(names), Some(i)) = (param, self.selected_form()) {
                            begin_param = Some((i, super::handle_params::Target::End(*end), names.clone()));
                        }
                        if let Some(ft) = self.selected_form().and_then(|i| self.forms.get(i)) {
                            let before = if *end { ft.to.clone() } else { ft.from.clone() };
                            let (last, n, tag, end) = (at_view(p), *n, ft.tag, *end);
                            self.views.end_drag = Some(EndDrag { end, last, n, view, before, tag, which, seen: pass });
                            begin_snap = Some((super::handle_params::Target::End(end), n, param.is_none()));
                        }
                    }
                    Some(why) => problem = Some(why.clone()),
                }
            } else if let Some(h) = shapes
                .iter()
                .filter(|h| screen(h.at).distance(p) <= HANDLE)
                .min_by(|a, b| screen(a.at).distance(p).total_cmp(&screen(b.at).distance(p)))
                && let Some(f) = family.as_ref()
            {
                match self.edge_drag(h, (Some((at_view(p), view)), Some(which), pass), f) {
                    Ok(d) => {
                        self.views.edge_drag = Some(d);
                        begin_snap = Some((super::handle_params::Target::Edge(h.line, h.out), h.way, h.param.is_none()));
                        if let (Some(names), Some(i)) = (&h.param, self.selected_form()) {
                            begin_param = Some((i, super::handle_params::Target::Edge(h.line, h.out), names.clone()));
                        }
                    }
                    Err(e) => problem = Some(e),
                }
            }
        }
        let held_at = |v: View, p: egui::Pos2| v.mm((p.x - pane.min.x, p.y - pane.min.y));
        if let Some(EndDrag { end, last: from, n, view: then, .. }) = self.views.end_drag.clone()
            && let Some(to) = response.interact_pointer_pos().map(|p| held_at(then, p))
            && (response.dragged_by(PointerButton::Primary) || response.drag_stopped_by(PointerButton::Primary))
        {
            let d = c.staged.vector(which, P2(to.0 - from.0, to.1 - from.1));
            end_moved = Some((end, d[0] * n[0] + d[1] * n[1] + d[2] * n[2]));
            if let Some(e) = self.views.end_drag.as_mut() {
                e.last = to;
            }
        }
        if let Some((from, then)) = self.views.edge_drag.as_ref().and_then(|d| d.on)
            && let Some(to) = response.interact_pointer_pos().map(|p| held_at(then, p))
            && (response.dragged_by(PointerButton::Primary) || response.drag_stopped_by(PointerButton::Primary))
        {
            edge_moved = Some(c.staged.vector(which, P2(to.0 - from.0, to.1 - from.1)));
        }
        // What a click selects (FE-1): an element, or (inner none) nothing.
        let mut select: Option<Option<Picked>> = None;
        let mut symbol_move: Option<(usize, Option<SymbolNode>, P2)> = None;
        let mut pin: Option<(Guid, (bimcraft_core::family::PivotPoint, bimcraft_core::family::PivotAt))> = None;
        let mut grip_moved: Option<super::pivot::GripMove> = None;
        // A grip pressed (F7d): the rotate grip, or a line's end; the drag is the grip's.
        if editable
            && response.drag_started_by(PointerButton::Primary)
            && let Some(p) = ui.input(|i| i.pointer.press_origin())
            && let Some((grip, _)) = grips.iter().find(|(_, at)| at.distance(p) <= super::pivot::REACH)
        {
            self.views.grip = Some((*grip, at_view(p)));
        }
        if let Some((grip, last)) = self.views.grip
            && response.dragged_by(PointerButton::Primary)
            && let Some(now) = response.interact_pointer_pos().map(at_view)
            && now != last
        {
            grip_moved = Some(match grip {
                super::pivot::Grip::Rotate(g) => super::pivot::GripMove::Rotate(g, c.staged.from_view(which, now)),
                super::pivot::Grip::End(g, spot) => {
                    super::pivot::GripMove::End(g, spot, c.staged.vector(which, P2(now.0 - last.0, now.1 - last.1)))
                }
            });
            self.views.grip = Some((grip, now));
        }
        let mut place_at: Option<(PlaneRef, (f64, f64), f64)> = None;
        let mut moved: Option<(Guid, [f64; 3])> = None;
        let mut new_plane = None;
        let mut new_line = None;
        let mut picked_work_plane = None;
        let mut new_dim = None;
        let mut align = None;
        let mut on_dims: Option<(Clicked, Vec<f64>)> = None;
        let mut modify_at: Option<super::modify::Click> = None;
        let mut measure_at: Option<P2> = None;
        let mut paste_at: Option<P2> = None;
        let mut clicked_any = false;
        // A double click on a form sketches it again, as Edit Extrusion or Edit Revolve does (F5).
        let edit = (editable
            && response.double_clicked()
            && self.views.measuring.is_none()
            && self.views.drawing.is_none()
            && self.views.picking.is_none()
            && self.views.modify.is_none()
            && self.views.pasting.is_none())
        .then(|| response.interact_pointer_pos())
        .flatten()
        .and_then(|p| scene.as_ref().and_then(|s| s.form_hit(at_view(p))));
        // A box let go where it began (a slow or a shaky click, which egui calls a drag) is the click there (FE-20).
        let box_click = self.selection.box_click(ui, &response);
        // An adaptive family's views only show it: its tools are in {3D}.
        if editable
            && (response.clicked() || box_click.is_some() || released)
            && let Some(p) = box_click.or_else(|| response.interact_pointer_pos()).or_else(|| ui.input(|i| i.pointer.latest_pos()))
        {
            let q = at_view(p);
            clicked_any = true;
            if self.views.pasting.is_some() {
                // Paste following the pointer: the copies put where the click is, once the view is drawn.
                paste_at = Some(q);
            } else if self.views.picking.is_some() {
                // Pick a line: the reference line within the reach a plane's trace has.
                let line = c.staged.line_at(which, q, 6.0 / view.scale.max(1e-9));
                picked_work_plane = Some((pick_plane(p), line));
            } else if let Some(g) = self.views.pivoting {
                // Pivot: the point the element is pinned to.
                self.views.pivoting = None;
                pin = family
                    .as_ref()
                    .and_then(|f| super::pivot::picked(f, &values, &c.staged, which, g, q, &on_screen, view.scale))
                    .map(|x| (g, x));
            } else if self.views.modify.is_some() {
                // A Modify tool: the point, the axis or the line clicked, done once the view is drawn.
                // Offset reads the side clicked, so its point isn't snapped onto what it offsets.
                let offsetting = self.views.modify.as_ref().is_some_and(|m| m.tool == super::modify::Tool::Offset);
                modify_at = Some((if offsetting { q } else { snapped(p) }, axis_at(p), offset_at(p)));
            } else if self.views.measuring.is_some() {
                // Measure: a ruler by two clicks, snapped as a Modify tool's points are; Shift keeps it square.
                let first = self.views.measuring.as_ref().and_then(|m| m.first);
                let at = match first {
                    Some(a) if ui.input(|i| i.modifiers.shift) => bimcraft_core::snap::orthogonal_step(a, snapped(p)).0,
                    _ => snapped(p),
                };
                measure_at = Some(at);
            } else if let Some(first) = self.views.drawing {
                match first {
                    None => self.views.drawing = Some(Some(q)),
                    Some(a) => {
                        let q = if ui.input(|i| i.modifiers.shift) { bimcraft_core::snap::orthogonal_step(a, q).0 } else { q };
                        if self.views.drawing_line {
                            // Reference Line (FE-11): on the view's work plane, where the view looks onto it.
                            let host = self.views.work_plane();
                            let edge_on =
                                || t("The view sees its work plane edge on: set a work plane it sees flat (Work Plane > Set).").to_string();
                            match self
                                .plane_set()
                                .frame(&host)
                                .map_err(|e| e.to_string())
                                .and_then(|f| fv::new_line(&c.staged, which, &f, a, q).map_err(|_| edge_on()))
                            {
                                Ok(line) => new_line = line.map(|(start, end)| (start, end, host)),
                                Err(e) => self.views.status = Some(e),
                            }
                        } else {
                            new_plane = fv::new_plane(&c.staged, which, a, q);
                        }
                        self.views.drawing = Some(None);
                    }
                }
            } else if self.placing.is_some() {
                // Component (T6a): an instance on the view's work plane, where the click is on it.
                let host = self.views.work_plane();
                if let Some(f) = family.as_ref()
                    && let Some((at, offset)) = bimcraft_core::family::nested::on_plane(f, &host, c.staged.from_view(which, q))
                {
                    place_at = Some((host, (at.0, at.1), offset));
                }
            } else if align_lock.is_some_and(|at| dims::lock_rect(at).contains(p)) {
                // Align's padlock, waiting: it locks what was aligned, whatever tool is on.
                on_dims = Some((Clicked::AlignLock, Vec::new()));
            } else if let Some(refs) = self.views.dimensioning.as_mut() {
                // A dimension tool: references picked one after another, then a click where it goes (`dims::dimension_click`).
                let under = reference_at(p);
                match family.as_ref().map(|f| dims::dimension_click(f, &values, &c.staged, which, kind, refs, under, q)) {
                    Some(Ok(made)) => new_dim = made,
                    Some(Err(e)) => problem = Some(e),
                    None => problem = Some(t("Pick two references or more, then click where the dimension goes.").to_string()),
                }
            } else if let Some(first) = self.views.aligning {
                // Align: the reference to align to, then what moves onto it.
                match (reference_at(p), first) {
                    (Some(r), None) => self.views.aligning = Some(Some(r)),
                    (Some(r), Some(a)) if r != a => {
                        align = Some((a, r, q));
                        self.views.aligning = Some(None);
                    }
                    (Some(_), Some(_)) => {
                        problem = Some(t("That is the reference: pick what moves onto it.").to_string());
                    }
                    (None, _) => {
                        // Over a form but on none of its faces: say what of a form Align takes.
                        let form = family.as_ref().and_then(|f| fv::form_at(f, &values, &c.staged, which, q).and_then(|i| f.forms.get(i)));
                        problem = Some(match form {
                            Some(f) if !f.mesh.is_empty() => {
                                t("A form made of a mesh has no faces Align can take: pick a reference plane, an extrusion's end or a line of a form.")
                            }
                            Some(_) => t("Align takes a form's straight edge, a side the view sees edge on, or its end: click on its outline."),
                            None => t("Pick a reference plane, an extrusion's end or a line of a form."),
                        }
                        .to_string());
                    }
                }
            } else if on_handle(p) {
                // A click on a handle, with no drag: the selection stays.
            } else if let Some(hit) = dims::clicked(&marks, &self.dimensions, selected_dim, align_lock, &locks, p, &on_screen) {
                let lengths = match &hit {
                    Clicked::Lock(g, _) => marks
                        .iter()
                        .find(|m| self.dimensions.get(m.index).is_some_and(|d| d.guid == *g))
                        .map(|m| m.lengths.clone())
                        .unwrap_or_default(),
                    _ => Vec::new(),
                };
                on_dims = Some((hit, lengths));
            } else if annotation && let Some((i, _)) = self.symbol_at(q, 6.0 / view.scale.max(1e-9)) {
                select = Some(Some(Picked::Symbol(i)));
            } else if let Some(i) = super::connector_marks::pick(&connector_marks, &project, p) {
                select = Some(Some(Picked::Connector(i)));
            } else if let Some(i) = scene.as_ref().and_then(|s| s.nested_face(q)) {
                // A nested instance by its face, while Select Elements by Face is on (FE-20).
                select = Some(Some(Picked::Nested(i)));
            } else {
                // A reference line where the view draws it (F7d): over the planes' traces and the forms, as it is drawn.
                let line = c
                    .staged
                    .line_at(which, q, 6.0 / view.scale.max(1e-9))
                    .and_then(|i| family.as_ref().and_then(|f| f.lines.get(i)).map(|l| l.guid));
                let plane = if line.is_some() { None } else { pick_plane(p) };
                // Then a form by its face, or a nested instance or a form by an edge of its outline: after the plane and the
                // line, so an edge lying on a plane leaves the plane to be clicked (FE-20).
                let body = scene.as_ref().and_then(|s| s.body_hit(q));
                select = Some(match (plane, line, body) {
                    (Some(g), _, _) => Some(Picked::Plane(g)),
                    (None, Some(g), _) => Some(Picked::Line(g)),
                    (None, None, body) => body,
                });
            }
        }
        // What is selected as the frame began: a drag from the face of one of it is its own, and draws no box (`Scene::under`).
        let chosen: Vec<Picked> = self.selection.items().to_vec();
        // What a press at `p` is on that a drag moves: a dimension's line, else what a click there selects (and the box
        // doesn't begin on, `Scene::under`), or a selected symbol's strokes; never a handle, a grip or a padlock.
        let tools_free = editable
            && self.views.measuring.is_none()
            && self.views.drawing.is_none()
            && self.views.picking.is_none()
            && self.views.pivoting.is_none()
            && self.views.dimensioning.is_none()
            && self.views.aligning.is_none()
            && self.placing.is_none()
            && self.views.end_drag.is_none()
            && self.views.edge_drag.is_none()
            && self.views.grip.is_none()
            && self.views.moving.is_none()
            && self.views.modify.is_none()
            && self.views.pasting.is_none();
        // Where the annotation symbols' grips show: a press on one drags that corner, not the selection.
        let symbol_grips: Vec<egui::Pos2> = if annotation {
            self.symbol_marks()
                .into_iter()
                .flat_map(|(i, mark)| {
                    let part = self.symbols.get(i).map(|y| y.part);
                    let pts: Vec<P2> = match mark {
                        PaperMark::Line { .. } if part == Some(super::Part::Arc) => {
                            self.symbols.get(i).and_then(arc_grips).map(|g| g.to_vec()).unwrap_or_default()
                        }
                        PaperMark::Line { pts, .. } => pts,
                        PaperMark::Fill { rings, .. } => rings.into_iter().flatten().collect(),
                        _ => Vec::new(),
                    };
                    pts.into_iter().map(&on_screen).collect::<Vec<_>>()
                })
                .collect()
        } else {
            Vec::new()
        };
        let press_hit = |p: egui::Pos2| -> Option<Picked> {
            if !tools_free
                || on_handle(p)
                || grips.iter().any(|(_, at)| at.distance(p) <= super::pivot::REACH)
                || symbol_grips.iter().any(|g| g.distance(p) <= 6.0)
            {
                return None;
            }
            match dims::clicked(&marks, &self.dimensions, selected_dim, align_lock, &locks, p, &on_screen) {
                Some(Clicked::Select(g)) => return Some(Picked::Dimension(g)),
                Some(_) => return None,
                None => {}
            }
            let s = scene.as_ref()?;
            let hit = s.hit(p, pick_plane(p), &chosen)?;
            (matches!(hit, Picked::Symbol(_)) || s.under(p, &chosen)).then_some(hit)
        };
        self.views.over_selected =
            !self.selection.is_empty() && response.hover_pos().and_then(press_hit).is_some_and(|h| self.selection.contains(h));
        // Ctrl held: a drag of the selection copies it, and the status bar says so.
        let ctrl = ui.input(|i| i.modifiers.ctrl || i.modifiers.command);
        self.views.ctrl = ctrl;
        let pressed =
            response.drag_started_by(PointerButton::Primary).then(|| ui.input(|i| i.pointer.press_origin())).flatten().and_then(press_hit);
        // What of the selection (and the element pressed) the view shows: a move takes that alone, leaving a
        // plan's symbol or dimension where it is when dragged in an elevation.
        let in_view: Vec<Picked> = match (pressed, scene.as_ref()) {
            (Some(hit), Some(s)) => self
                .selection
                .items()
                .iter()
                .copied()
                .chain([hit])
                .filter(|p| !matches!(p, Picked::Form(i) if hidden.get(*i).copied().unwrap_or(false)) && !s.pieces(*p).is_empty())
                .collect(),
            _ => Vec::new(),
        };
        // A drag from nothing a click would select, nor a handle, a grip or a padlock, with no tool in hand: a selection box,
        // a window left to right, a crossing right to left; Ctrl adds what it takes, Shift takes it away (FE-1).
        let free = editable
            && pressed.is_none()
            && self.views.measuring.is_none()
            && self.views.drawing.is_none()
            && self.views.picking.is_none()
            && self.views.pivoting.is_none()
            && self.views.dimensioning.is_none()
            && self.views.aligning.is_none()
            && self.placing.is_none()
            && self.views.end_drag.is_none()
            && self.views.edge_drag.is_none()
            && self.views.grip.is_none()
            && self.views.moving.is_none()
            && self.views.modify.is_none()
            && self.views.pasting.is_none();
        let start =
            response.drag_started_by(PointerButton::Primary).then(|| ui.input(|i| i.pointer.press_origin())).flatten().filter(|p| {
                free && scene.as_ref().is_some_and(|s| !s.under(*p, &chosen))
                    && !on_handle(*p)
                    && grips.iter().all(|(_, at)| at.distance(*p) > super::pivot::REACH)
                    && (!annotation || self.symbol_at(at_view(*p), reach).is_none_or(|(_, n)| n.is_none()))
                    && dims::clicked(&marks, &self.dimensions, selected_dim, align_lock, &locks, *p, &on_screen).is_none()
            });
        let boxed = super::select_box::track(&mut self.selection, ui, &response, &painter, pane, start);
        if let (Some((from, to)), Some(scene)) = (boxed, scene.as_ref()) {
            let taken = scene.in_box(from, to);
            self.selection.apply(&taken, super::select_box::How::of(crate::canvas::click_modifiers(ui)));
        }
        // The view's ways in the family's axes, for the move below (the view's drawing isn't read past here).
        let (across, up) = (c.staged.vector(which, P2(1.0, 0.0)), c.staged.vector(which, P2(0.0, 1.0)));
        // A press on an element and a drag: one not selected is selected first, alone or with Ctrl added; then the
        // selection moves, but a lone plane of the family's own, which the plane's own drag below moves through its constraints.
        if let Some(hit) = pressed
            && let Some(p) = ui.input(|i| i.pointer.press_origin())
        {
            if !self.selection.contains(hit) {
                match super::select_box::How::of(crate::canvas::click_modifiers(ui)) {
                    super::select_box::How::Add => self.selection.apply(&[hit], super::select_box::How::Add),
                    super::select_box::How::Remove => {}
                    super::select_box::How::Replace => self.selection.apply(&[hit], super::select_box::How::Replace),
                }
            }
            let lone_plane =
                self.selected_plane().is_some_and(|g| self.planes.iter().any(|q| q.guid == g && !q.datum() && q.pivot.is_none()));
            // With Ctrl, a lone plane is copied too: a copy goes as the selection's move does.
            if self.selection.contains(hit) && (!lone_plane || ctrl) {
                let lone = self.selected_plane().filter(|_| lone_plane);
                self.views.moving = Some(self.move_drag((at_view(p), view, which, pass), &in_view, ctrl, lone));
                if self.unplaced_connector_selected() {
                    self.views.said = Some(
                        t("A connector with no place of its own stays where it is: give it one in Properties to move it.").to_string(),
                    );
                }
            }
        }
        // An annotation symbol's grip pressed: that corner, arc end or line point drags alone; the whole symbol is the
        // selection's drag above.
        if editable
            && annotation
            && self.views.symbol_drag.is_none()
            && self.views.moving.is_none()
            && self.views.modify.is_none()
            && self.views.measuring.is_none()
            && response.drag_started_by(PointerButton::Primary)
            && let Some(p) = ui.input(|i| i.pointer.press_origin())
        {
            let q = at_view(p);
            if let Some((i, Some(node))) = self.symbol_at(q, 6.0 / view.scale.max(1e-9)) {
                self.selection.apply(&[Picked::Symbol(i)], super::select_box::How::Replace);
                self.views.symbol_drag = Some((i, Some(node), q));
            }
        }
        if let Some((i, node, last)) = self.views.symbol_drag
            && response.dragged_by(PointerButton::Primary)
            && let Some(now) = response.interact_pointer_pos().map(at_view)
        {
            let d = P2(now.0 - last.0, now.1 - last.1);
            if d.0 != 0.0 || d.1 != 0.0 {
                symbol_move = Some((i, node, d));
                self.views.symbol_drag = Some((i, node, now));
            }
        }
        // Within a click's few points of the press, a drag moves nothing yet; once past them, it does to the end.
        if response.drag_started_by(PointerButton::Primary) {
            self.views.still = true;
        }
        if let (Some(o), Some(p)) = (ui.input(|i| i.pointer.press_origin()), response.interact_pointer_pos())
            && !super::select_box::still(o, p)
        {
            self.views.still = false;
        }
        let shaking = self.views.still;
        let mut move_by: Option<[f64; 3]> = None;
        if let Some((from, then)) = self.views.moving.as_ref().map(|d| (d.from, d.view))
            && let Some(to) = response.interact_pointer_pos().map(|p| held_at(then, p))
            && response.dragged_by(PointerButton::Primary)
            && !shaking
        {
            let P2(dx, dy) = super::drag_move::delta(from, to, ui.input(|i| i.modifiers.shift));
            move_by = Some(std::array::from_fn(|k| across[k] * dx + up[k] * dy));
        }
        // A plane of the family's own, selected, dragged square to itself.
        if editable
            && self.views.moving.is_none()
            && self.views.symbol_drag.is_none()
            && self.views.drawing.is_none()
            && self.views.picking.is_none()
            && self.views.end_drag.is_none()
            && self.views.edge_drag.is_none()
            && self.views.grip.is_none()
            && response.dragged_by(PointerButton::Primary)
        {
            // From where the button went down: the drag begins only once the hand has moved on.
            if response.drag_started()
                && let Some(p) = ui.input(|i| i.pointer.press_origin())
            {
                // A plane its pivot holds turns by its grip, never slides.
                let own = self.selected_plane().filter(|g| self.planes.iter().any(|q| q.guid == *g && !q.datum() && q.pivot.is_none()));
                self.views.drag = own.filter(|g| pick_plane(p) == Some(*g)).map(|_| (at_view(p), view));
                self.views.drag_origin = self.views.drag.map(|(q, _)| q);
            }
            if let (Some((from, then)), Some(g)) = (self.views.drag, self.selected_plane())
                && let Some(to) = response.interact_pointer_pos().map(|p| held_at(then, p))
                && !shaking
            {
                let to = if ui.input(|i| i.modifiers.shift) {
                    self.views.drag_origin.map_or(to, |origin| bimcraft_core::snap::orthogonal_step(origin, to).0)
                } else {
                    to
                };
                moved = Some((g, c.staged.vector(which, P2(to.0 - from.0, to.1 - from.1))));
                self.views.drag = Some((to, then));
            }
        }
        if response.drag_stopped() {
            self.views.drag = None;
            self.views.drag_origin = None;
            self.views.end_drag = None;
            self.views.grip = None;
            self.views.symbol_drag = None;
        }
        let param_let_go = response.drag_stopped() || !ui.input(|i| i.pointer.primary_down());
        self.zoom_and_pan(ui, &response, which, view.scale);
        // A handle's drag through a parameter begun; none can move it: the drag given up, saying why.
        if let Some((i, target, names)) = begin_param
            && let Err(e) = self.start_param_drag(i, target, &names, true)
        {
            self.views.edge_drag = None;
            self.views.end_drag = None;
            (end_moved, edge_moved) = (None, None);
            problem = Some(e);
        }
        // What the side or end held can line up with, within a few points as the view is scaled.
        if let Some((target, way, free)) = begin_snap
            && (self.views.edge_drag.is_some() || self.views.end_drag.is_some())
        {
            self.begin_snap(target, way, f64::from(super::shape_handles::SNAP) / view.scale.max(1e-9), free);
        }
        // An End's drag comes a step at a time: the step that puts it where the whole drag, lined up, has it.
        let end_moved = end_moved.map(|(end, d)| (end, self.snap_step(d)));
        if let Some((end, d)) = end_moved
            && d != 0.0
            && let Err(e) = if self.dragging_param() { self.param_by(if end { d } else { -d }) } else { self.drag_end(end, d) }
        {
            self.snap_refused();
            problem = Some(e);
        }
        if let Some(moved) = edge_moved
            && let Err(e) = self.drag_edge_to(moved)
        {
            problem = Some(e);
        }
        if let Some(d) = move_by
            && let Err(e) = self.drag_move_to(d, ctrl)
        {
            problem = Some(e);
        }
        // Lined up with something a padlock can lock it to: said, while it is held there.
        let lock = self.snap_lock();
        if lock.is_some() {
            self.views.said = Some(t("Aligned: let go, then click the padlock to lock it there.").to_string());
        }
        // The edge or the selection let go: they stay as the drag left them, a step of the history once the button is up.
        if response.drag_stopped() || !ui.input(|i| i.pointer.primary_down()) {
            // Let go lined up with something: Align's padlock, open, where the hand left it; the next click locks the two
            // or lets it go.
            if let Some((on, own)) = lock.filter(|_| self.views.handle_snap.take().is_some())
                && problem.is_none()
                && let Some(p) = response.interact_pointer_pos().or(hover)
            {
                self.views.align_lock = Some((on, own, at_view(p), which));
            }
            // Ctrl as the button comes up decides between a copy and a move.
            let m = crate::canvas::click_modifiers(ui);
            if self.moving()
                && let Err(e) = self.settle_drag(m.ctrl || m.command)
            {
                problem = Some(e);
            }
            self.views.edge_drag = None;
            self.views.moving = None;
        }
        // A parameter dragged by a handle: written as the drag left it.
        if param_let_go && self.views.end_drag.is_none() {
            self.let_param_go();
        }
        if let Some(i) = edit
            && let Err(e) = self.edit_form_sketch(i)
        {
            problem = Some(e);
        }
        if let Some((plane, line)) = picked_work_plane {
            let base = Arc::new(base.clone());
            problem = self.picked(&base, plane, line);
        }
        // The next click lets Align's padlock go, unless it locks it.
        if clicked_any && !matches!(on_dims, Some((Clicked::AlignLock, _))) && align.is_none() {
            self.views.align_lock = None;
        }
        if let Some(d) = new_dim {
            self.dimensions.push(d);
        }
        if let Some((a, b, q)) = align {
            self.align_through(a, b, q, which);
        }
        if let Some((hit, lengths)) = on_dims {
            let done = match hit {
                Clicked::Select(g) => {
                    self.click_select(Some(Picked::Dimension(g)), crate::canvas::click_modifiers(ui));
                    Ok(())
                }
                Clicked::Lock(g, i) => self.toggle_lock(g, i, lengths.get(i).copied().unwrap_or(0.0)),
                Clicked::Equal(g) => self.toggle_equal(g),
                Clicked::Unlock(g) => {
                    self.delete_dimension(g);
                    Ok(())
                }
                Clicked::AlignLock => {
                    self.lock_alignment();
                    Ok(())
                }
            };
            if let Err(e) = done {
                problem = Some(e);
            }
        }
        if let Some((host, at, offset)) = place_at {
            self.place_nested(host, at, offset);
        }
        if let Some(m) = grip_moved
            && let Some(e) = self.drag_grip(m)
        {
            problem = Some(e);
        }
        if let Some((g, (to, at))) = pin
            && let Err(e) = self.set_pivot(g, to, at)
        {
            problem = Some(e);
        }
        if let Some(hit) = select {
            self.click_select(hit, crate::canvas::click_modifiers(ui));
        }
        // A paste's click.
        if let Some(q) = paste_at {
            match self.paste_click(q, which, space) {
                Ok(said) if !said.is_empty() => self.views.said = Some(said),
                Ok(_) => {}
                Err(e) => problem = Some(e),
            }
        }
        // Measure's click: the second lays the ruler, which the status bar reads.
        if let Some(at) = measure_at
            && let Some(text) = self.measure_click(at, which, base.units)
        {
            self.views.said = Some(text);
        }
        // A Modify tool's click, and Rotate's Angle typed.
        if let Some((q, axis, offset)) = modify_at
            && let Some(e) =
                self.modify_click(q, (which, view, pass), space, &shown_selection, axis, offset, ui.input(|i| i.modifiers.shift))
        {
            problem = Some(e);
        }
        if self.views.modify.as_ref().is_some_and(|m| m.typed.is_some())
            && let Some(e) = self.modify_typed(space, &shown_selection)
        {
            problem = Some(e);
        }
        // What the tool selected itself (the copies) is what it goes on with.
        let now: Vec<Picked> = self.selection.items().to_vec();
        if let Some(m) = self.views.modify.as_mut() {
            m.selection = now;
        }
        // A plane its dimensions hold is dragged through them: a label it changes takes the new length, what is locked to it
        // follows, and what can't hold says so.
        // Refused where it leaves the family failing its checks it passed: put back, saying why.
        let kept = moved.map(|_| (self.draft(), self.sound()));
        let held = family.as_ref().is_some_and(|f| {
            f.constrained() && f.dimensions.iter().any(|d| d.refs.contains(&DimRef::Plane(moved.map_or(Guid::default(), |m| m.0))))
        });
        if let Some((g, d)) = moved
            && held
            && let Some(p) = family.as_ref().and_then(|f| f.planes.iter().find(|p| p.guid == g))
        {
            let n = p.frame().z;
            let at = p.start[0] * n[0] + p.start[1] * n[1] + p.start[2] * n[2] + d[0] * n[0] + d[1] * n[1] + d[2] * n[2];
            self.drag_through(g, at);
        } else if let Some((g, d)) = moved
            && let Some(p) = self.planes.iter_mut().find(|p| p.guid == g)
        {
            let n = p.frame().z;
            let along = d[0] * n[0] + d[1] * n[1] + d[2] * n[2];
            for (i, k) in n.iter().enumerate() {
                p.start[i] += k * along;
                p.end[i] += k * along;
            }
            // The reference lines drawn on it go with it.
            super::drag_move::carry_lines(&mut self.lines, g, std::array::from_fn(|i| n[i] * along));
        }
        if let Some((draft, true)) = kept
            && let Err(e) = self.family().and_then(|f| f.check(&|_| true))
        {
            self.put_back(draft);
            problem = Some(e);
        }
        if let Some((start, end, up)) = new_plane {
            let guid = Guid::new();
            self.planes.push(RefPlane {
                id: 0,
                guid,
                name: String::new(),
                start,
                end,
                up,
                reference: Default::default(),
                defines_origin: false,
                subcategory: None,
                level: false,
                pivot: None,
                turned: None,
            });
            self.select_only(Picked::Plane(guid));
        }
        if let Some((start, end, host)) = new_line {
            let guid = Guid::new();
            self.lines.push(RefLine { guid, start, end, host, ..Default::default() });
            self.select_only(Picked::Line(guid));
        }
        if let Some((i, node, d)) = symbol_move {
            self.drag_symbol(i, node, d);
        }
        problem.or_else(|| self.views.status.take())
    }
}

impl FamilyEditor {
    /// How close view `which` is, by the wheel over it, and where, moved by the middle button (its `scale`, pixels a mm).
    pub(super) fn zoom_and_pan(&mut self, ui: &Ui, response: &egui::Response, which: Which, scale: f64) {
        let mut frame = self.views.frames.get(&which).copied().unwrap_or((1.0, (0.0, 0.0)));
        let before = frame;
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                frame.0 = (frame.0 * f64::from(scroll * 0.003).exp()).clamp(0.05, 50.0);
            }
        }
        if response.dragged_by(PointerButton::Middle) {
            let d = response.drag_delta();
            frame.1 = (frame.1.0 - f64::from(d.x) / scale, frame.1.1 + f64::from(d.y) / scale);
        }
        if frame != before {
            self.views.frames.insert(which, frame);
        }
    }

    /// The work plane {3D} shows, while Work Plane > Show is on in a family that isn't adaptive, or a Modify tool is in hand
    /// there, whose clicks land on it: its frame.
    pub(crate) fn shown_work_plane_3d(&self) -> Option<bimcraft_core::family::Frame> {
        let tooling = self.views.shown.is_none() && self.views.modify.is_some() && !self.sketching();
        if self.adaptive.is_some() || !(self.views.show_work_plane || tooling) {
            return None;
        }
        self.plane_set().frame(&self.views.work_plane()).ok()
    }
}

/// Work Plane > Show in the family's {3D}: plane `f` as a mark over its picture, as the project's 3D view lays out its work plane
/// (`workplane::patch`) over what the family takes up (`extents`) and [`WORK_PLANE_MARGIN`] round, tinted as the plans tint it,
/// with a grid through the family's origin about 5 to 20 lines across (`workplane::grid_step`) and its outline; depth tested,
/// so a form in front of it hides it.
pub(crate) fn work_plane_mark(f: &bimcraft_core::family::Frame, extents: ([f64; 3], [f64; 3])) -> Option<bimcraft_core::view3d::PlaneMark> {
    let plane = bimcraft_core::workplane::Frame { origin: f.origin, normal: f.z };
    let p = crate::workplane::patch(plane, extents, WORK_PLANE_MARGIN, [0.0; 3])?;
    let step = bimcraft_core::workplane::grid_step((p.hi.0 - p.lo.0).max(p.hi.1 - p.lo.1));
    Some(crate::workplane::plane_mark(p, step))
}

/// The solid forms the type previewed (`values`) hides, and those the view's Visibility Settings leave out (`by_view`, F8),
/// alone: what a view draws halftone while Preview Visibility is off. Every other form, and each `hidden` for now, is left
/// out. A door's or a window's is a generic family of its own, its wall's thickness a parameter, so it stands as a component
/// beside the opening rather than a second opening in the wall. None when nothing is hidden.
fn ghosts(
    f: &Family,
    values: &BTreeMap<String, bimcraft_core::family::Val>,
    hidden: impl Fn(usize) -> bool,
    by_view: &[bool],
) -> Option<Family> {
    let mut g = f.clone();
    let mut any = false;
    for (i, form) in g.forms.iter_mut().enumerate() {
        let in_type = form.visible.as_ref().is_some_and(|v| crate::adaptive_editor::worth(v, values) == Some(0.0));
        let off = !form.void && !hidden(i) && (in_type || by_view.get(i).copied().unwrap_or(false));
        form.visible = if off { None } else { Some(Expr::Number(0.0)) };
        form.visibility = None;
        any |= off;
    }
    if !any {
        return None;
    }
    if g.category.hosted() {
        let wall = match values.get(bimcraft_core::family::WALL) {
            Some(bimcraft_core::family::Val::Number(t)) => *t,
            _ => bimcraft_core::family::DEFAULT_WALL,
        };
        let p = serde_json::from_value(json!({"name": bimcraft_core::family::WALL, "value": wall})).ok()?;
        g.parameters.push(p);
        g.category = Category::Generic;
        g.operation = None;
    }
    // Its symbols and nested instances are the family's, drawn as it draws them.
    g.symbols.clear();
    g.nested_instances.clear();
    Some(g)
}

/// What the ghost family is called beside the family in a view's scratch model.
const GHOST: &str = "(hidden in type)";

/// The lines of drawing `d` (shown from `min` on screen) as segments on screen: what a padlock keeps off.
pub(super) fn drawn_lines(d: &Drawing, min: egui::Pos2) -> Vec<[egui::Pos2; 2]> {
    let at = |p: &(f32, f32)| min + vec2(p.0, p.1);
    let mut out = Vec::new();
    for prim in &d.prims {
        if let bimcraft_core::render::Prim::Line { pts, closed, .. } = prim {
            out.extend(pts.windows(2).map(|w| [at(&w[0]), at(&w[1])]));
            if *closed && let (Some(a), Some(b)) = (pts.last(), pts.first()) {
                out.push([at(a), at(b)]);
            }
        }
    }
    out
}

/// The strokes the view's drawing draws plane `g`'s trace with, on screen: what highlights it, short of its name as the trace
/// is (none where the drawing hasn't it).
pub(super) fn plane_strokes(d: &Drawing, st: &Staged, g: Guid, min: egui::Pos2) -> Vec<[egui::Pos2; 2]> {
    let at = |p: &(f32, f32)| min + vec2(p.0, p.1);
    let mut out = Vec::new();
    for (prim, owner) in d.prims.iter().zip(&d.owners) {
        if owner.and_then(|o| st.plane_of(o)) == Some(g)
            && let bimcraft_core::render::Prim::Line { pts, .. } = prim
        {
            out.extend(pts.windows(2).map(|w| [at(&w[0]), at(&w[1])]));
        }
    }
    out
}

/// Where a plane of frame `f` crosses view `which`: its trace across the view's room, ends in the view's millimetres. None
/// for a plane the view looks straight at (or askew), which has none.
pub(super) fn plane_trace(st: &Staged, which: Which, f: &bimcraft_core::family::Frame) -> Option<(P2, P2)> {
    let eye = match which {
        Which::Plan | Which::Ceiling => [0.0, 0.0, 1.0],
        Which::Elevation(side) => {
            let l = side.looking();
            [l.0, l.1, 0.0]
        }
    };
    let n = f.z;
    if (n[0] * eye[0] + n[1] * eye[1] + n[2] * eye[2]).abs() > 1e-6 {
        return None;
    }
    // Along it, square to the way the view looks.
    let d = [n[1] * eye[2] - n[2] * eye[1], n[2] * eye[0] - n[0] * eye[2], n[0] * eye[1] - n[1] * eye[0]];
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if len < 1e-9 {
        return None;
    }
    let reach = (0..3).map(|i| (st.room.1[i] - st.room.0[i]).abs()).fold(0.0, f64::max);
    let at = |k: f64| std::array::from_fn(|i| f.origin[i] + d[i] / len * k * reach);
    Some((st.to_view(which, at(-1.0)), st.to_view(which, at(1.0))))
}

/// Whether frame `kept` takes in the room `room` frames (the middle three quarters of its picture, as `Staged::fitted`
/// leaves a margin round it): what lets a view keep its frame while the family changes.
fn holds(kept: &View, room: &View) -> bool {
    let half = |v: &View, k: f64| (k * f64::from(v.width) / (2.0 * v.scale), k * f64::from(v.height) / (2.0 * v.scale));
    let (rw, rh) = half(room, 0.75);
    let (kw, kh) = half(kept, 1.0);
    kept.scale > 0.0
        && room.scale > 0.0
        && room.cx - rw >= kept.cx - kw - 1e-6
        && room.cx + rw <= kept.cx + kw + 1e-6
        && room.cy - rh >= kept.cy - kh - 1e-6
        && room.cy + rh <= kept.cy + kh + 1e-6
}
