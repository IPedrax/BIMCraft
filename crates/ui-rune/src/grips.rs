//! Grips on the element selected alone: the ends of a wall, a run (a beam, a pipe) or a grid, which a drag moves. Pure, and tested with
//! no window; `App` (`app/grips.rs`) holds the drag and `paint/grips.rs` draws it. After the egui window's `gripped`, `grip_ends`,
//! `grip_target` and `grip_drag` in `canvas.rs`, which are the reference and stay as they are: a duplicate to fold into the core when
//! egui adopts it. Not here from there: the angle to click and type beside a wall's end, a Fixed Grid wall's stay in its pattern's
//! lengths, and the grips of a stair, a section, a callout or a shape's points.

use bimcraft_core::geom::P2;
use bimcraft_core::ids::Id;
use bimcraft_core::model::{Element, Model, TOL};
use serde_json::json;

use crate::tooling::Command;

/// An end being dragged: the element, which end (`start`, else its end), and where it would go.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grip {
    pub id: Id,
    pub start: bool,
    pub to: P2,
}

/// The ends of an element that has grips, `(start, end)`: a wall's, a grid's or a run's.
pub fn ends(m: &Model, id: Id) -> Option<(P2, P2)> {
    match m.elements.get(id)? {
        Element::Wall(w) => Some((w.start, w.end)),
        Element::Grid(g) => Some((g.start, g.end)),
        e => e.run().map(|r| (r.start, r.end)),
    }
}

/// The element whose ends are grips: one selected alone, with no tool in hand.
pub fn gripped(m: &Model, selection: &[Id], tool: bool) -> Option<Id> {
    match (selection, tool) {
        (&[id], false) if ends(m, id).is_some() => Some(id),
        _ => None,
    }
}

/// Which end a press at `p` takes, `true` for the start: the nearer of the two, if it is within `reach` mm.
pub fn end_at(ends: (P2, P2), p: P2, reach: f64) -> Option<bool> {
    let (a, b) = (ends.0.dist(p), ends.1.dist(p));
    let (start, d) = if a <= b { (true, a) } else { (false, b) };
    (d <= reach).then_some(start)
}

/// Where the dragged end goes for a pointer that landed at `found`: there, or, with `keep_length` (Ctrl), along the line from the end
/// that stays through `found` at the line's own length, so the element turns about the fixed end.
pub fn target(ends: (P2, P2), start: bool, found: P2, keep_length: bool) -> P2 {
    let (mine, fixed) = if start { (ends.0, ends.1) } else { (ends.1, ends.0) };
    if keep_length && fixed.dist(found) > TOL {
        let p = fixed + (found - fixed).unit() * mine.dist(fixed);
        // To a micron, as the core keeps a point.
        P2((p.0 * 1e3).round() / 1e3 + 0.0, (p.1 * 1e3).round() / 1e3 + 0.0)
    } else {
        found
    }
}

/// The `move` that drags an end of the element to `grip.to`, and the walls joined there follow; none when it goes nowhere.
pub fn command(m: &Model, grip: Grip) -> Option<Command> {
    let (a, b) = ends(m, grip.id)?;
    let by = grip.to - if grip.start { a } else { b };
    (by.is_finite() && by.length() > 1e-6)
        .then(|| ("move", json!({"ids": [grip.id], "by": [by.0, by.1], "end": if grip.start { "start" } else { "end" }})))
}

#[cfg(test)]
mod tests;
