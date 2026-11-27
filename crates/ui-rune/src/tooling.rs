//! The plan tools as data, and what a click does in each. `tools::TOOLS` is what the palette shows; here every one of those tools has
//! a [`Spec`]: the core's command it ends in and a [`Kind`] that says how the points picked become that command's params and when
//! it is done. [`advance`] is the whole state machine, a pure function of the points picked so far (the chain `App::picked` holds,
//! the click just made last), so it is tested with no window and no widget: it answers with a [`Step`] that `App` carries out through
//! `Session::execute`.
//!
//! How a tool finishes: a click that completes it (a column, a door, a pair of ends), the last point clicked again (a double click
//! is two clicks at one place, so it does too), or Enter (`finish`). Backspace gives the last point back (`App::back_point`).
//! The Modify tools work on the selection through the same machine: Move and Copy (a base point and a destination), Rotate (a centre and two
//! rays), Mirror (an axis), Split, Trim to Corner, Align and Offset (elements picked). The params are what the command needs and no more: the type, height and location line an Options Bar would set are the
//! command's defaults here, as that bar is not in this window yet (docs/areas/rune-ui-tools.md).

use bimcraft_core::align;
use bimcraft_core::draw::{self, Options, Shape};
use bimcraft_core::family::Category;
use bimcraft_core::geom::P2;
use bimcraft_core::ids::Id;
use bimcraft_core::model::{Element, Line, MAX_POINTS, Model, TOL};
use serde_json::{Value, json};

use crate::lang::{t, tf};

/// What a chain of clicks leaves of itself once a click has made something.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chain {
    /// Nothing: the next click starts again.
    No,
    /// The last point, which the next element starts from (a beam run on from the one before).
    Last,
    /// Every point, as a wall's chain is: the guides run from all of them and the first closes it.
    Whole,
}

/// How a tool's clicks become its command.
#[derive(Clone, Copy, Debug)]
pub enum Kind {
    /// One click makes it, at the point: `{<key>: [x, y], level}` (a column, a door, a room).
    Point { key: &'static str },
    /// A click anywhere makes it, from what the level has: a floor under the walls, a roof over them. No point is sent.
    Anywhere,
    /// Two clicks, its ends: `{<from>: [x, y], <to>: [x, y], level}` (a stair or a ramp).
    Span { from: &'static str, to: &'static str },
    /// A line from click to click: `{points: [a, b], level}`, each click after the first making the next one. `shapes` lets the Draw
    /// panel's shapes (a rectangle, a circle, an arc) stand in for the line, as Revit's Wall does; `curtain` picks a curtain wall's type.
    Run { chain: Chain, shapes: bool, curtain: bool },
    /// Points until Enter or the last one clicked again: `{<key>: [[x, y], ...], level}`. With `host`, a click on a stair or a ramp
    /// as the first one makes it for that element instead (a railing).
    Path { key: &'static str, min: usize, host: bool },
    /// A family placed at the point: `component {family, type, at, level}`, of the first family that is one of these kinds (any
    /// when there are none named). The Type Selector is not in this window yet, so the first type of the first family it is.
    Family { kinds: &'static [Category] },
    /// Move and Copy, on the selection: from a base point to a destination, `{ids, by: [dx, dy]}`.
    Shift,
    /// Rotate, on the selection: about a centre, from one ray to the next, `{ids, about, angle}`.
    Turn,
    /// Mirror, on the selection: across the line of two points, `{ids, axis, copy}`.
    Reflect,
    /// Split: the element under the pointer cut at the point, `{id, at}`.
    Cut,
    /// Trim/Extend to Corner: two elements, each picked on the part that stays, `corner {ids, keep}`.
    Corner,
    /// Align: an element's line picked, then the element to bring to it, `{id, reference, line}`.
    Align,
    /// Offset: an element picked, then the point that says how far and to which side.
    Offset,
}

/// A plan tool: its id in `tools::TOOLS`, the core's command it makes, and how its clicks make it.
#[derive(Clone, Copy, Debug)]
pub struct Spec {
    pub id: &'static str,
    pub command: &'static str,
    pub kind: Kind,
}

impl Spec {
    /// Whether the Draw panel's shapes stand in for its line (the Wall tool).
    pub fn takes_shape(&self) -> bool {
        matches!(self.kind, Kind::Run { shapes: true, .. })
    }

    /// Whether the tool is put down once it has done its work (Move, Copy, Rotate and Mirror, as in the egui window, which goes back to
    /// Modify); the others, Split, Trim, Align and Offset, stay for the next pick, as they do there.
    pub fn ends_tool(&self) -> bool {
        matches!(self.kind, Kind::Shift | Kind::Turn | Kind::Reflect)
    }
}

const fn spec(id: &'static str, command: &'static str, kind: Kind) -> Spec {
    Spec { id, command, kind }
}

const RUN: Kind = Kind::Run { chain: Chain::Last, shapes: false, curtain: false };
const AT: Kind = Kind::Point { key: "at" };

/// Every tool of `tools::TOOLS` that draws on the plan with the points it picks, and the tools of `tools::MODIFY` that act on what is
/// selected or picked (not Array, which the core has no command for, and Scale, which only scales annotation and needs a typed factor).
/// A test keeps the tables the same.
pub const SPECS: &[Spec] = &[
    spec("wall", "wall", Kind::Run { chain: Chain::Whole, shapes: true, curtain: false }),
    spec("curtain_wall", "wall", Kind::Run { chain: Chain::Whole, shapes: false, curtain: true }),
    spec("door", "door", AT),
    spec("window", "window", AT),
    spec("column", "column", AT),
    spec("component", "component", Kind::Family { kinds: &[] }),
    spec("roof", "roof", Kind::Anywhere),
    spec("floor", "floor", Kind::Anywhere),
    spec("ceiling", "ceiling", Kind::Point { key: "point" }),
    spec("stair", "stair", Kind::Span { from: "start", to: "end" }),
    spec("ramp", "ramp", Kind::Span { from: "start", to: "end" }),
    spec("railing", "railing", Kind::Path { key: "path", min: 2, host: true }),
    spec("room", "room", Kind::Point { key: "point" }),
    spec("beam", "beam", RUN),
    spec("structural_column", "structural_column", AT),
    spec("footing", "footing", AT),
    spec("strip_footing", "strip_footing", RUN),
    spec("pipe", "pipe", RUN),
    spec("plumbing_fixture", "component", Kind::Family { kinds: &[Category::Plumbing] }),
    spec("drain", "component", Kind::Family { kinds: &[Category::Drain] }),
    spec("conduit", "conduit", RUN),
    spec("outlet", "component", Kind::Family { kinds: &[Category::Outlet] }),
    spec("switch", "component", Kind::Family { kinds: &[Category::Switch] }),
    spec("lighting", "component", Kind::Family { kinds: &[Category::Lighting] }),
    spec("cable_tray", "cable_tray", RUN),
    spec("move", "move", Kind::Shift),
    spec("copy", "copy", Kind::Shift),
    spec("rotate", "rotate", Kind::Turn),
    spec("mirror", "mirror", Kind::Reflect),
    spec("trim", "corner", Kind::Corner),
    spec("split", "split", Kind::Cut),
    spec("align", "align", Kind::Align),
    spec("offset", "offset", Kind::Offset),
];

/// The tool with this id, if it draws with picked points.
pub fn spec_of(id: &str) -> Option<&'static Spec> {
    SPECS.iter().find(|s| s.id == id)
}

/// What a Modify tool keeps between its clicks besides the points: the element or the line its first pick named.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Held {
    #[default]
    Nothing,
    Element(Id),
    Line(Line),
}

/// What the Draw panel has set, which the tools that take a shape read, what a Modify tool holds from its first pick, and where the
/// plan's right-click menu is open.
#[derive(Clone, Copy, Debug, Default)]
pub struct Tooling {
    pub shape: Shape,
    pub options: Options,
    pub held: Held,
    /// Where the right-click menu is open over the plan, in window points.
    pub menu: Option<(f32, f32)>,
    /// The end of the selected element being dragged by its grip (`grips.rs`).
    pub grip: Option<crate::grips::Grip>,
}

/// What the machine reads of the window and the model besides the points.
pub struct Ctx<'a> {
    pub model: &'a Model,
    pub level: Id,
    /// The element nearest the pointer, which a click on a stair or a ramp takes as a railing's host.
    pub hovered: Option<Id>,
    /// The Draw panel's shape and its settings, for the tools that take one.
    pub shape: Shape,
    pub options: Options,
    /// What is selected, which the Modify tools act on.
    pub selection: &'a [Id],
    /// What the tool's first pick named (a Modify tool's); read only on its second click, as the first one overwrites it.
    pub held: Held,
    /// How near an element's line the pointer must be to pick it, millimetres.
    pub reach: f64,
}

/// A command and its params.
pub type Command = (&'static str, Value);

/// What `App` does after a click or Enter.
#[derive(Debug, PartialEq)]
pub enum Step {
    /// Nothing yet: it wants more points.
    More,
    /// The click makes nothing and is not kept: the same place twice where that means nothing, or a point a tool cannot use.
    Ignore,
    /// The tool is finished with its points and makes nothing (a chain ended by its last point clicked again): the points go.
    Done,
    /// The click is the first of a pick that names an element or a line: the point stays and the tool holds that until the next.
    Hold(Held),
    /// Run these commands, as one step when there are several. Then the points become `keep`; if the core refuses, the click is given back.
    Run { commands: Vec<Command>, keep: Chain },
    /// The tool cannot go on, and says why.
    Refuse(String),
}

fn xy(p: P2) -> [f64; 2] {
    [p.0, p.1]
}

fn run(spec: &Spec, params: Value, keep: Chain) -> Step {
    Step::Run { commands: vec![(spec.command, params)], keep }
}

fn label(spec: &Spec) -> &'static str {
    t(crate::tools::label(spec.id).unwrap_or(spec.id))
}

/// The first family of one of `kinds` (any when none are named) with a type, and its first type.
fn family_of(m: &Model, kinds: &[Category]) -> Option<(String, String)> {
    m.families
        .iter()
        .filter(|(_, f)| kinds.is_empty() || kinds.contains(&f.category))
        .find_map(|(name, f)| f.types.keys().next().map(|ty| (name.clone(), ty.clone())))
}

/// The first wall type that is a curtain wall.
fn curtain_type(m: &Model) -> Option<String> {
    m.types.wall.iter().find(|(_, w)| w.curtain.is_some()).map(|(name, _)| name.clone())
}

/// The stair or ramp under the pointer, which a railing goes along.
fn host_of(ctx: &Ctx) -> Option<Id> {
    let id = ctx.hovered?;
    matches!(ctx.model.elements.get(id), Some(Element::Stair(_) | Element::Ramp(_))).then_some(id)
}

/// The points, a place clicked twice in a row taken once.
fn distinct(points: &[P2]) -> Vec<P2> {
    let mut out: Vec<P2> = Vec::with_capacity(points.len());
    for p in points {
        if out.last().is_none_or(|l| l.dist(*p) > TOL) {
            out.push(*p);
        }
    }
    out
}

/// A path of points made, once it has enough of them.
fn path(spec: &Spec, ctx: &Ctx, points: &[P2], key: &'static str, min: usize) -> Step {
    let pts = distinct(points);
    if pts.len() < min {
        return Step::Refuse(tf("{}: click at least {} points.", &[label(spec), &min.to_string()]));
    }
    let list: Vec<[f64; 2]> = pts.iter().copied().map(xy).collect();
    run(spec, json!({ key: list, "level": ctx.level }), Chain::No)
}

/// The walls along `curves` (lines, arcs and circles; the rest make none): a run of segments joined end to end that turn alike is one
/// `wall` command, as Revit's Wall does for a rectangle or a polygon; a circle is two half circles, which join into one closed command.
fn walls_along(spec: &Spec, ctx: &Ctx, curves: &[bimcraft_core::curves::Curve]) -> Step {
    let mut runs: Vec<(Vec<P2>, f64)> = Vec::new();
    for (a, b, arc) in draw::wall_segments(curves) {
        match runs.last_mut() {
            Some((pts, turn)) if *turn == arc && pts.last().is_some_and(|q| q.dist(a) <= TOL) => pts.push(b),
            _ => runs.push((vec![a, b], arc)),
        }
    }
    let commands: Vec<Command> = runs
        .into_iter()
        .map(|(mut pts, arc)| {
            let closed = pts.len() > 2 && pts.first().zip(pts.last()).is_some_and(|(f, l)| f.dist(*l) <= TOL);
            if closed {
                pts.pop();
            }
            let list: Vec<[f64; 2]> = pts.iter().copied().map(xy).collect();
            (spec.command, json!({"points": list, "level": ctx.level, "closed": closed, "arc": arc}))
        })
        .collect();
    if commands.is_empty() {
        return Step::Refuse(t("That shape makes no wall.").to_string());
    }
    Step::Run { commands, keep: Chain::No }
}

/// Pick Lines: the line of the element clicked (a wall's location line, a detail line's curve, a grid, a beam), beside it by the Options Bar's
/// offset on the side clicked, as the walls along it. The click is the pointer's place on the plan, which `Ctx::hovered` is the nearest
/// element to.
fn pick_lines(spec: &Spec, ctx: &Ctx, at: P2) -> Step {
    let line = ctx.hovered.and_then(|id| draw::element_curve(ctx.model, id)).and_then(|c| draw::pick(&c, at, ctx.options.offset));
    match line {
        Some(curve) => walls_along(spec, ctx, &[curve]),
        None => Step::Refuse(tf("{}: click a wall or a line.", &[label(spec)])),
    }
}

/// Pick Faces: the walls of the face of a mass clicked, which the core's `wall_by_face` makes.
fn pick_faces(spec: &Spec, ctx: &Ctx, at: P2) -> Step {
    match bimcraft_core::massing::face_at(ctx.model, at, ctx.reach) {
        Some((mass, face)) => Step::Run { commands: vec![("wall_by_face", json!({"mass": mass, "faces": [face]}))], keep: Chain::No },
        None => Step::Refuse(tf("{}: click a face of a mass.", &[label(spec)])),
    }
}

/// A shape other than a line, its points clicked: when it has them all, the walls along what they make.
fn shaped(spec: &Spec, ctx: &Ctx, points: &[P2]) -> Step {
    let n = ctx.shape.clicks(&ctx.options);
    if n == 0 || !ctx.shape.for_walls() {
        return Step::Refuse(tf("{}: {} is not drawn with clicks here.", &[label(spec), t(ctx.shape.label())]));
    }
    if points.len() < n {
        return Step::More;
    }
    let pts = distinct(points);
    if pts.len() < n {
        return Step::Ignore;
    }
    match draw::make(ctx.shape, &pts, &ctx.options, None) {
        Some(curves) => walls_along(spec, ctx, &curves),
        None => Step::Refuse(t("Those points make no shape.").to_string()),
    }
}

/// What the click that has just added the last of `points` does in `spec`.
pub fn advance(spec: &Spec, ctx: &Ctx, points: &[P2]) -> Step {
    let Some(&last) = points.last() else { return Step::More };
    if points.iter().any(|p| !p.is_finite()) {
        return Step::Ignore;
    }
    let before = points.len().checked_sub(2).and_then(|i| points.get(i)).copied();
    let again = before.is_some_and(|p| p.dist(last) <= TOL);
    match spec.kind {
        Kind::Point { key } => run(spec, json!({ key: xy(last), "level": ctx.level }), Chain::No),
        Kind::Anywhere => {
            let m = ctx.model;
            let params = match spec.command {
                // A roof stands on the level above the walls it covers, as the Roof tool of the egui window puts it.
                "roof" => json!({"level": m.level_above(ctx.level).unwrap_or_else(|| m.guid(ctx.level))}),
                _ => json!({"level": ctx.level}),
            };
            run(spec, params, Chain::No)
        }
        Kind::Span { from, to } => match before {
            None => Step::More,
            Some(_) if again => Step::Ignore,
            Some(a) => run(spec, json!({ from: xy(a), to: xy(last), "level": ctx.level }), Chain::No),
        },
        Kind::Run { chain, shapes, curtain } => {
            // The shapes that pick what is there rather than draw: one click makes the walls.
            if shapes && ctx.shape == Shape::PickLines {
                return pick_lines(spec, ctx, last);
            }
            if shapes && ctx.shape == Shape::PickFaces {
                return pick_faces(spec, ctx, last);
            }
            if shapes && ctx.shape != Shape::Line {
                return shaped(spec, ctx, points);
            }
            let Some(prev) = before else { return Step::More };
            // The last point clicked again ends the chain, as Escape does: what was made stays.
            if again {
                return Step::Done;
            }
            let mut params = json!({"points": [xy(prev), xy(last)], "level": ctx.level});
            if curtain {
                match curtain_type(ctx.model) {
                    Some(name) => params["type"] = json!(name),
                    None => return Step::Refuse(t("Curtain Wall: the project has no curtain wall type.").to_string()),
                }
            }
            // Back on its first point a chain of walls closes; and a chain that reaches the cap ends, so no click is lost.
            let closes = points.len() > 2 && points.first().is_some_and(|f| f.dist(last) <= TOL);
            let keep = match chain {
                Chain::Whole if closes || points.len() >= MAX_POINTS => Chain::No,
                other => other,
            };
            run(spec, params, keep)
        }
        Kind::Path { key, min, host } => {
            if host
                && points.len() == 1
                && let Some(id) = host_of(ctx)
            {
                return run(spec, json!({"host": id}), Chain::No);
            }
            if again {
                // The last point clicked again finishes the path, as a double click does.
                return path(spec, ctx, points, key, min);
            }
            if points.len() >= MAX_POINTS { path(spec, ctx, points, key, min) } else { Step::More }
        }
        Kind::Family { kinds } => match family_of(ctx.model, kinds) {
            Some((family, ty)) => run(spec, json!({"family": family, "type": ty, "at": xy(last), "level": ctx.level}), Chain::No),
            None => Step::Refuse(tf("{}: the project has no family to place; load one first.", &[label(spec)])),
        },
        Kind::Shift | Kind::Turn | Kind::Reflect if ctx.selection.is_empty() => {
            Step::Refuse(tf("{}: select the elements first, then pick the tool.", &[label(spec)]))
        }
        Kind::Shift => match before {
            None => Step::More,
            Some(_) if again => Step::Ignore,
            Some(from) => run(spec, json!({"ids": ctx.selection, "by": xy(last - from)}), Chain::No),
        },
        Kind::Turn => match points {
            [_] => Step::More,
            [centre, from] if from.dist(*centre) <= TOL => Step::Ignore,
            [_, _] => Step::More,
            [centre, from, to] if to.dist(*centre) > TOL && from.dist(*centre) > TOL => {
                let angle = angle_between(*centre, *from, *to);
                run(spec, json!({"ids": ctx.selection, "about": xy(*centre), "angle": angle, "copy": false}), Chain::No)
            }
            _ => Step::Ignore,
        },
        Kind::Reflect => match before {
            None => Step::More,
            Some(_) if again => Step::Ignore,
            Some(a) => run(spec, json!({"ids": ctx.selection, "axis": [xy(a), xy(last)], "copy": true}), Chain::No),
        },
        Kind::Cut => match ctx.hovered {
            Some(id) => run(spec, json!({"id": id, "at": xy(last)}), Chain::No),
            None => Step::Refuse(tf("{}: click a wall or a detail line.", &[label(spec)])),
        },
        Kind::Corner => match (points.len(), ctx.hovered, ctx.held) {
            (1, Some(a), _) => Step::Hold(Held::Element(a)),
            (2, Some(b), Held::Element(a)) if a != b => {
                let keep = [xy(points[0]), xy(last)];
                run(spec, json!({"ids": [a, b], "keep": keep}), Chain::No)
            }
            (1 | 2, None, _) => Step::Refuse(tf("{}: click a wall or a line on the part that stays.", &[label(spec)])),
            _ => Step::Ignore,
        },
        Kind::Align => {
            let line = ctx.hovered.and_then(|id| align::nearest(ctx.model, id, last).map(|(line, d)| (id, line, d)));
            match (points.len(), line, ctx.held) {
                (1, Some((_, line, d)), _) if d <= ctx.reach => Step::Hold(Held::Line(line)),
                (2, Some((id, line, d)), Held::Line(reference)) if d <= ctx.reach => run(
                    spec,
                    json!({"id": id, "reference": [xy(reference.0), xy(reference.1)], "line": [xy(line.0), xy(line.1)]}),
                    Chain::No,
                ),
                (1 | 2, ..) => Step::Refuse(tf("{}: click a wall, a line or an edge.", &[label(spec)])),
                _ => Step::Ignore,
            }
        }
        Kind::Offset => match (points.len(), ctx.hovered, ctx.held) {
            (1, Some(id), _) if offsets(ctx.model, id) => Step::Hold(Held::Element(id)),
            (1, ..) => Step::Refuse(tf("{}: click a straight wall or a detail line.", &[label(spec)])),
            (2, _, Held::Element(id)) => offset_to(spec, ctx, id, last),
            _ => Step::Ignore,
        },
    }
}

/// Degrees from the ray `centre` to `from` to the ray `centre` to `to`, counter-clockwise, in (-180, 180].
pub fn angle_between(centre: P2, from: P2, to: P2) -> f64 {
    let (u, v) = (from - centre, to - centre);
    let d = (v.1.atan2(v.0) - u.1.atan2(u.0)).to_degrees();
    if d > 180.0 {
        d - 360.0
    } else if d <= -180.0 {
        d + 360.0
    } else {
        d
    }
}

/// Whether Offset takes the element: a straight wall or a detail line.
fn offsets(m: &Model, id: Id) -> bool {
    m.wall(id).is_some_and(|w| w.arc == 0.0) || matches!(m.elements.get(id), Some(Element::DetailLine(_)))
}

/// Offset's second pick: the element `id` drawn again as far from itself as the point `at` is, on the side it is on. A detail line
/// is offset by the core's `offset`; a wall is copied by the distance across it, as the egui window does.
fn offset_to(spec: &Spec, ctx: &Ctx, id: Id, at: P2) -> Step {
    let m = ctx.model;
    if let Some(w) = m.wall(id).filter(|w| w.arc == 0.0) {
        let n = (w.end - w.start).unit().perp();
        let across = (at - w.start).dot(n);
        if across.abs() <= TOL {
            return Step::Ignore;
        }
        return Step::Run { commands: vec![("copy", json!({"ids": [id], "by": xy(n * across)}))], keep: Chain::No };
    }
    match draw::element_curve(m, id).map(|c| c.dist(at)) {
        Some(d) if d > TOL => run(spec, json!({"id": id, "distance": d, "toward": xy(at), "copy": true}), Chain::No),
        Some(_) => Step::Ignore,
        None => Step::Refuse(tf("{}: click a straight wall or a detail line.", &[label(spec)])),
    }
}

/// Enter: what a tool does with the points it has, which only a path makes something of. The step is `More` with no point picked,
/// so the key goes on to whatever else wants it.
pub fn finish(spec: &Spec, ctx: &Ctx, points: &[P2]) -> Step {
    if points.is_empty() {
        return Step::More;
    }
    match spec.kind {
        Kind::Path { key, min, .. } => path(spec, ctx, points, key, min),
        _ => Step::Done,
    }
}

#[cfg(test)]
mod tests;
