//! What stands where the next click would put it, shown before it is made, as egui's window does: a wall's thickness along the segment to the
//! pointer, a door or a window in the wall under it (centred, hinged and facing as the Options Bar and Space have it), a column of the type
//! picked on the point. A wall is outlines; the others are the element itself, which the core draws as the plan would (`render::sketch`), so
//! the ghost is what the click makes. Pure, with no window: `App::stands` and `App::shape_preview` carry them to the plan.

use bimcraft_core::curves::Curve;
use bimcraft_core::geom::P2;
use bimcraft_core::ids::Id;
use bimcraft_core::model::{Column, Element, Model, TOL};
use serde_json::json;

use crate::options::Settings;

/// The outline of the wall of the type picked (else the project's default) from `a` to `b`: its two faces and its ends, on the side the
/// bar's location line puts them, as the wall will stand. None for a segment of no length or a type the project lacks.
pub fn wall(m: &Model, s: &Settings, a: P2, b: P2) -> Option<Curve> {
    if !a.is_finite() || !b.is_finite() || a.dist(b) <= TOL {
        return None;
    }
    let name = s.pick(m, "wall").map(|p| p.name.as_str()).or_else(|| m.default_type("wall"))?;
    let ty = m.types.wall.get(name)?;
    // The exterior face is that far to the left of the location line, the interior one a thickness short of it.
    let ext = bimcraft_core::walls::depth(ty.thickness, &ty.layers, s.location);
    let (left, side) = ((b - a).unit().perp(), ext - ty.thickness);
    let ring = vec![a + left * ext, b + left * ext, b + left * side, a + left * side];
    ring.iter().all(|p| p.is_finite()).then_some(Curve::Poly { points: ring, arcs: Vec::new(), closed: true })
}

/// The element a click of `tool` at `at` would make, for the core to draw: a door or a window in the wall under the pointer (None over no
/// wall), a column of the type picked. None for any other tool. `near` is how close to the middle of a stretch of wall a click centres.
pub fn element(m: &Model, s: &Settings, tool: &str, level: Id, at: P2, near: f64) -> Option<Element> {
    if !at.is_finite() {
        return None;
    }
    match tool {
        "door" | "window" => {
            let mut params = json!({"at": [at.0, at.1], "level": level});
            s.feed(tool, m, level, &mut params, near).ok()?;
            bimcraft_core::opening_of(m, &params, tool == "door").ok()
        }
        "column" => {
            let type_name = s.pick(m, "column").map(|p| p.name.clone()).or_else(|| m.default_type("column").map(String::from))?;
            Some(Element::Column(Column {
                type_name,
                level: m.guid(level),
                at,
                angle: 0.0,
                top_level: None,
                height: 0.0,
                base_offset: 0.0,
                top_offset: 0.0,
                attach_top: None,
                attach_base: None,
                params: Default::default(),
                room_bounding: true,
            }))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests;
