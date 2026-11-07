//! A strip footing under a wall, stuck to it as Revit glues a footing to the wall it carries: the footing's line
//! is the wall's — the same direction and place — and the top of the footing's section meets the wall's base.
//! Nothing of the glue is stored: the model keeps no reference from a run to a wall, so a wall's deletion leaves
//! the footing where it is and no dangling reference can outlive one. The condition is read from the geometry
//! whenever a command changes a wall or a footing, and while it holds the footing rides its wall in plan and keeps
//! its top on the wall's base.

use crate::geom::P2;
use crate::model::{Id, Justify, Model, Run, TOL, Wall};

/// Whether `r` lies flat: not upright, and with no `end_offset` to slope it — a footing sloping end to end has no
/// flat top to meet a wall with.
pub(crate) fn level(r: &Run) -> bool {
    r.start.dist(r.end) > TOL && r.end_offset.is_none_or(|e| (e - r.offset).abs() <= TOL)
}

/// The height of the top of run `r`'s section, of type `type_name` in `category` of `m`.
pub(crate) fn top(m: &Model, r: &Run, category: &str) -> Option<f64> {
    let t = m.types.runs.get(category)?.get(&r.type_name)?;
    let base = m.elevation(r.level) + r.offset;
    Some(match r.justify {
        Justify::Top => base,
        Justify::Centre => base + t.depth / 2.0,
        Justify::Bottom => base + t.depth,
    })
}

/// How far off square two runs of lines `a` and `b` long may be and still count for one line, at the model's tolerance:
/// a millimetre of drift end to end.
fn squareness(a: f64, b: f64) -> f64 {
    TOL / a.max(b).max(TOL)
}

/// How far `p` lies from the line through `a` and `b`.
fn off_line(p: P2, a: P2, b: P2) -> f64 {
    let d = b - a;
    if d.length() <= TOL {
        return p.dist(a);
    }
    (d.cross(p - a) / d.length()).abs()
}

/// Whether footing `f` of `category` lies under straight wall `w` and touches it: their lines run the same way and
/// bear on one another, their stretches in plan meet (the footing really lies under the wall), and the footing's
/// top is the wall's base — each within the model's tolerance.
pub(crate) fn holds(m: &Model, w: &Wall, f: &Run, category: &str) -> bool {
    if !level(f) || w.arc != 0.0 {
        return false;
    }
    let (u, v) = (w.end - w.start, f.end - f.start);
    let (lw, lf) = (u.length(), v.length());
    if lw <= TOL || lf <= TOL || u.cross(v).abs() > squareness(lw, lf) * lw * lf {
        return false;
    }
    if off_line(f.start, w.start, w.end) > TOL || off_line(f.end, w.start, w.end) > TOL {
        return false;
    }
    // Their stretches along the wall meet, with more than a touch to them.
    let d = u * (1.0 / lw);
    let at = |p: P2| (p - w.start).dot(d);
    let (w0, w1): (f64, f64) = (0.0, lw);
    let (f0, f1) = (at(f.start).min(at(f.end)), at(f.start).max(at(f.end)));
    if w1.min(f1) - w0.max(f0) <= TOL {
        return false;
    }
    top(m, f, category).is_some_and(|t| (t - m.base_z(w)).abs() <= TOL)
}

/// `r` with each of its ends put through `f`, and the heights of its ends kept.
pub(crate) fn mapped(r: &Run, f: impl Fn(P2) -> P2) -> Run {
    Run { start: f(r.start), end: f(r.end), ..r.clone() }
}

/// `r` moved so that the top of its section sits on `base`, of a type in `category` of `m`: its level kept and its
/// `offset` set for its `justify`, so that the top is exactly there.
pub(crate) fn on_base(m: &Model, r: &Run, category: &str, base: f64) -> Option<Run> {
    let t = m.types.runs.get(category)?.get(&r.type_name)?;
    let shift = match r.justify {
        Justify::Top => 0.0,
        Justify::Centre => t.depth / 2.0,
        Justify::Bottom => t.depth,
    };
    let offset = base - m.elevation(r.level) - shift;
    Some(Run { offset, end_offset: r.end_offset.map(|_| offset), ..r.clone() })
}

/// The wall footing `id` is glued to, if it is glued to one: the nearest wall, where several stacked walls lie under
/// the same footing, whose line is the footing's and whose base the footing's top meets.
pub(crate) fn wall_of(m: &Model, id: Id) -> Option<Id> {
    let e = m.elements.get(id)?;
    let (Some(f), category) = (e.run(), e.category()) else { return None };
    if category != "stripfooting" || !level(f) {
        return None;
    }
    let top = top(m, f, category)?;
    let reach = [f.start.0.min(f.end.0), f.start.1.min(f.end.1), f.start.0.max(f.end.0), f.start.1.max(f.end.1)];
    let mut best: Option<(f64, Id)> = None;
    for w in m.lookup().walls_between(top, top) {
        let Some(wall) = m.wall(w) else { continue };
        let d = (m.base_z(wall) - top).abs();
        if best.is_none_or(|(bd, _)| d < bd) && holds(m, wall, f, category) {
            best = Some((d, w));
        }
    }
    let _ = reach;
    best.map(|(_, w)| w)
}

/// Every strip footing glued to one of the walls `walls`, by its id and the wall's: the footings a command that
/// changes those walls must take along.
pub(crate) fn footings_of(m: &Model, walls: &[Id]) -> Vec<(Id, Id)> {
    let mut out = Vec::new();
    for id in m.elements.keys().copied().collect::<Vec<_>>() {
        if m.elements.get(id).is_some_and(|e| e.category() == "stripfooting")
            && let Some(w) = wall_of(m, id)
            && walls.contains(&w)
        {
            out.push((id, w));
        }
    }
    out
}
