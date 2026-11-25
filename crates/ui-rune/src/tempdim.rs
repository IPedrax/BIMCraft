//! Temporary dimensions: what the element selected alone measures to, as data. A wall shows its length and how far its centre line stands
//! from the nearest parallel wall or grid on each side, a door or window the two stretches of its wall either side of its middle, a
//! column, footing or component how far it stands from the nearest wall or grid in each of the four directions; a typed length moves
//! the element so it measures that (`apply`). Pure, tested with no window; `App` (`app/tempdim.rs`) holds the typed numbers and
//! `paint/dims.rs` and `ui/tempdim.rs` draw them. After the egui window's `temporary_dimension`, `wall_dimensions`, `opening_dimensions`
//! and `standing_dimensions` in `canvas.rs`, which are the reference and stay as they are: **a duplicate to fold into the core when egui
//! adopts it**. Not here from there: a curved wall's arc length, Make Dimension Permanent, which side of a wall the line is drawn on
//! (away from the middle of the view there; here the wall's left), and a dimension's constraints (locks and equality).

use bimcraft_core::geom::P2;
use bimcraft_core::ids::Id;
use bimcraft_core::model::{Element, Model, Opening, TOL, Wall};
use bimcraft_core::walls::Axis;
use serde_json::json;

use crate::lang::{t, tf};
use crate::tooling::Command;

/// What typing a length into a dimension does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Edit {
    /// A wall's own length: `wall_length {id, value}`.
    WallLength(Id),
    /// The distance to a line, `toward` it: the element moves along that way until it stands the length typed from it.
    Clearance { id: Id, toward: P2, distance: f64 },
    /// An opening's distance from the start of its span, else from its end: it slides along its wall.
    Opening { id: Id, start: f64, end: f64, from_start: bool, offset: f64 },
}

/// How far a dimension's line stands from what it measures: `mm` millimetres on the plan, which the zoom scales, and `px` points more.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Off {
    pub mm: f64,
    pub px: f32,
}

/// A dimension: the segment it measures, the side its line is drawn on (a unit vector of the plan), how far from the segment, and
/// what typing a length into it does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dim {
    pub a: P2,
    pub b: P2,
    pub side: P2,
    pub off: Off,
    pub length: f64,
    pub edit: Edit,
}

/// The dimensions of element `id`, none when it has none.
pub fn dims(m: &Model, id: Id) -> Vec<Dim> {
    match m.elements.get(id) {
        Some(Element::Wall(w)) => wall_dims(m, id, w),
        Some(Element::Door(o) | Element::Window(o)) => opening_dims(m, id, o),
        Some(e @ (Element::Column(_) | Element::StructuralColumn(_) | Element::Footing(_) | Element::Component(_))) => {
            standing_dims(m, id, e)
        }
        _ => Vec::new(),
    }
}

/// A straight wall's length, and its clearances to the nearest parallel wall or grid on its left and its right.
fn wall_dims(m: &Model, id: Id, w: &Wall) -> Vec<Dim> {
    let length = w.start.dist(w.end);
    if w.arc != 0.0 || !length.is_finite() || length <= TOL {
        return Vec::new();
    }
    let u = (w.end - w.start).unit();
    let mut out = vec![Dim {
        a: w.start,
        b: w.end,
        side: u.perp(),
        off: Off { mm: m.thickness(w) / 2.0, px: 18.0 },
        length,
        edit: Edit::WallLength(id),
    }];
    // Each clearance is measured from the wall's middle, square to it, its line a little along the wall.
    for c in m.wall_clearances(id).into_iter().flatten() {
        let edit = Edit::Clearance { id, toward: c.toward, distance: c.distance };
        out.push(Dim { a: c.from, b: c.from + c.toward * c.distance, side: u, off: Off { mm: 0.0, px: 36.0 }, length: c.distance, edit });
    }
    out
}

/// A door's or window's two stretches: from the start of its span to its middle, and from there to the end of the span.
fn opening_dims(m: &Model, id: Id, o: &Opening) -> Vec<Dim> {
    let (Some(w), Some((start, end))) = (m.wall(o.host).filter(|w| w.arc == 0.0), m.opening_span(o)) else { return Vec::new() };
    let axis = Axis::of(w);
    let (from, middle, to) = (axis.at(start, 0.0), axis.at(o.offset, 0.0), axis.at(end, 0.0));
    let side = (w.end - w.start).unit().perp();
    let off = Off { mm: m.thickness(w) / 2.0, px: 18.0 };
    [(from, middle, true), (middle, to, false)]
        .into_iter()
        .filter(|(a, b, _)| a.dist(*b) > TOL)
        .map(|(a, b, from_start)| Dim {
            a,
            b,
            side,
            off,
            length: a.dist(b),
            edit: Edit::Opening { id, start, end, from_start, offset: o.offset },
        })
        .collect()
}

/// A column's, footing's or component's distance to the nearest wall centre line or grid east, north, west and south of it.
fn standing_dims(m: &Model, id: Id, e: &Element) -> Vec<Dim> {
    let (level, at) = match e {
        Element::Column(c) | Element::StructuralColumn(c) | Element::Footing(c) => (c.level, c.at),
        Element::Component(c) => (c.level, c.at),
        _ => return Vec::new(),
    };
    m.clearances(level, at)
        .into_iter()
        .enumerate()
        .filter_map(|(n, c)| Some((n, c?)))
        .map(|(n, c)| {
            // East and west dimensions lie north of the element, north and south ones east of it.
            let side = if n % 2 == 0 { P2(0.0, 1.0) } else { P2(1.0, 0.0) };
            let edit = Edit::Clearance { id, toward: c.toward, distance: c.distance };
            Dim { a: at, b: at + c.toward * c.distance, side, off: Off { mm: 0.0, px: 30.0 }, length: c.distance, edit }
        })
        .collect()
}

/// What a length typed into `dim` does: the command that makes the element measure it, none when it already does, or what is wrong
/// with what was typed.
pub fn apply(m: &Model, dim: &Dim, text: &str) -> Result<Option<Command>, String> {
    let typed = text.trim();
    let Some(len) = m.units.parse(typed).filter(|l| l.is_finite()) else { return Err(tf("`{}` is not a length", &[typed])) };
    match dim.edit {
        Edit::WallLength(id) if len > 0.0 => {
            Ok(((len - dim.length).abs() > 1e-6).then(|| ("wall_length", json!({"id": id, "value": len}))))
        }
        Edit::Clearance { id, toward, distance } if len >= 0.0 => {
            let by = toward * (distance - len);
            Ok((by.length() > 1e-6).then(|| ("move", json!({"ids": [id], "by": [by.0, by.1]}))))
        }
        Edit::Opening { id, start, end, from_start, offset } if (0.0..=end - start).contains(&len) => {
            let to = if from_start { start + len } else { end - len };
            Ok(((to - offset).abs() > 1e-6).then(|| ("modify", json!({"id": id, "offset": to}))))
        }
        Edit::Opening { .. } => Err(t("the opening has to stay on its wall").to_string()),
        _ => Err(tf("`{}` is not a length", &[typed])),
    }
}

#[cfg(test)]
mod tests;
