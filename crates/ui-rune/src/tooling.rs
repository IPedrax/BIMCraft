//! The plan tools as data, and what a click does in each. `tools::TOOLS` is what the palette shows; here every one of those tools has
//! a [`Spec`]: the core's command it ends in and a [`Kind`] that says how the points picked become that command's params and when
//! it is done. [`advance`] is the whole state machine, a pure function of the points picked so far (the chain `App::picked` holds,
//! the click just made last), so it is tested with no window and no widget: it answers with a [`Step`] that `App` carries out through
//! `Session::execute`.
//!
//! How a tool finishes: a click that completes it (a column, a door, a pair of ends), the last point clicked again (a double click
//! is two clicks at one place, so it does too), or Enter (`finish`). Backspace gives the last point back (`App::back_point`).
//! The params are what the command needs and no more: the type, height and location line an Options Bar would set are the
//! command's defaults here, as that bar is not in this window yet (docs/areas/rune-ui-tools.md).

use bimcraft_core::draw::{self, Options, Shape};
use bimcraft_core::family::Category;
use bimcraft_core::geom::P2;
use bimcraft_core::ids::Id;
use bimcraft_core::model::{Element, MAX_POINTS, Model, TOL};
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
}

const fn spec(id: &'static str, command: &'static str, kind: Kind) -> Spec {
    Spec { id, command, kind }
}

const RUN: Kind = Kind::Run { chain: Chain::Last, shapes: false, curtain: false };
const AT: Kind = Kind::Point { key: "at" };

/// Every tool of `tools::TOOLS` that draws on the plan with the points it picks. A test keeps the two tables the same.
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
];

/// The tool with this id, if it draws with picked points.
pub fn spec_of(id: &str) -> Option<&'static Spec> {
    SPECS.iter().find(|s| s.id == id)
}

/// What the Draw panel has set, which the tools that take a shape read.
#[derive(Clone, Copy, Debug, Default)]
pub struct Tooling {
    pub shape: Shape,
    pub options: Options,
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
