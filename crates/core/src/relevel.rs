//! Change Base Level: an element that stands on a level put on another, where it is. Its base offset (or its offset, for what
//! has only one) takes up the difference between the two levels, so nothing moves in the model; a top tied to a level stays
//! tied to it, and an unconnected height stays as tall. The `change_level` command, and what the right-click menu asks of an
//! element before it offers it ([`can_change_level`]).

use serde_json::{Value, json};

use crate::commands::{bad, ids_param, level_param, only};
use crate::engine::Session;
use crate::ids::Id;
use crate::model::{Element, Model};

/// Why element `e` can't be put on another level where it stands, or none when it can: it stands on a level, and has an
/// offset from it to take up the difference.
pub fn why_not(m: &Model, id: Id, e: &Element) -> Option<String> {
    let kind = e.category();
    if crate::groups::group_of(m, id).is_some() {
        return Some(format!("{kind} {id} is in a group: its level is its group's"));
    }
    match e {
        Element::Wall(w) if w.panel_of.is_some() => Some(format!("wall {id} stands in a curtain wall's panel: it goes with it")),
        Element::Wall(_)
        | Element::Floor(_)
        | Element::Ceiling(_)
        | Element::Shaft(_)
        | Element::Mass(_)
        | Element::Toposolid(_)
        | Element::Room(_)
        | Element::Column(_)
        | Element::StructuralColumn(_)
        | Element::Footing(_)
        | Element::Stair(_)
        | Element::Roof(_)
        | Element::PointCloud(_) => None,
        e if e.run().is_some() => None,
        Element::Component(c) if c.host.is_none() => None,
        Element::Component(_) => Some(format!("component {id} is hosted: it goes where its host goes")),
        Element::Railing(r) if r.host.is_none() => None,
        Element::Railing(_) => Some(format!("railing {id} follows its stair or ramp")),
        Element::Door(_) | Element::Window(_) => Some(format!("{kind} {id} stands on its wall's level")),
        _ if m.level_of(e).is_some() => Some(format!("{kind} {id} has no offset from its level to keep it where it is")),
        _ => Some(format!("{kind} {id} stands on no level")),
    }
}

/// Whether every one of `ids` can be put on another level where it stands (and there is at least one).
pub fn can_change_level(m: &Model, ids: &[Id]) -> bool {
    !ids.is_empty() && ids.iter().all(|id| m.elements.get(*id).is_some_and(|e| why_not(m, *id, e).is_none()))
}

/// `e`, on level `to` (`by` mm lower than its own: the old level's elevation less the new one's), standing where it stood.
fn moved(e: &Element, to: crate::ids::Guid, by: f64) -> Element {
    let mut e = e.clone();
    match &mut e {
        Element::Wall(w) => (w.level, w.base_offset) = (to, w.base_offset + by),
        Element::Shaft(sh) => (sh.level, sh.base_offset) = (to, sh.base_offset + by),
        Element::Mass(ms) => (ms.level, ms.base_offset) = (to, ms.base_offset + by),
        Element::Room(r) => (r.level, r.base_offset) = (to, r.base_offset + by),
        Element::Column(c) | Element::StructuralColumn(c) | Element::Footing(c) => (c.level, c.base_offset) = (to, c.base_offset + by),
        Element::Floor(f) => (f.level, f.offset) = (to, f.offset + by),
        Element::Ceiling(c) => (c.level, c.offset) = (to, c.offset + by),
        Element::Toposolid(t) => (t.level, t.offset) = (to, t.offset + by),
        Element::Roof(r) => (r.level, r.offset) = (to, r.offset + by),
        Element::Component(c) => (c.level, c.offset) = (to, c.offset + by),
        Element::PointCloud(pc) => (pc.level, pc.offset) = (to, pc.offset + by),
        Element::Railing(r) => (r.level, r.base_offset) = (to, r.base_offset + by),
        // A stair's offset is how far its foot stands below its level.
        Element::Stair(st) => (st.level, st.base_offset) = (to, st.base_offset - by),
        other => {
            if let Some(r) = other.run_mut() {
                r.level = to;
                r.offset += by;
                r.end_offset = r.end_offset.map(|o| o + by);
            }
        }
    }
    e
}

/// `change_level {ids, level}`: each element put on `level` where it stands.
pub(crate) fn change_level(s: &mut Session, p: &Value) -> Result<Value, String> {
    let c = "change_level";
    only(c, p, &["ids", "id", "level"])?;
    let m = &s.model;
    let ids = ids_param(c, m, p)?;
    if p.get("level").is_none_or(Value::is_null) {
        return Err(bad(c, "`level` is the level they go on, by name or id"));
    }
    let to = level_param(c, m, p, "level")?;
    let to_z = m.elevation(to);
    let mut changed = Vec::new();
    for &id in &ids {
        let e = m.elements.get(id).ok_or_else(|| bad(c, format!("no element {id}")))?;
        if let Some(why) = why_not(m, id, e) {
            return Err(bad(c, why));
        }
        if m.is_pinned(id) {
            return Err(bad(c, format!("{} {id} is pinned: `unpin` it first to change its level", e.category())));
        }
        let Some(from) = m.level_of(e) else { continue };
        if from == to {
            continue;
        }
        let by = m.elevation(from) - to_z;
        changed.push((id, moved(e, to, by)));
    }
    let ids: Vec<Id> = changed.iter().map(|(id, _)| *id).collect();
    let model = s.model_mut();
    for (id, e) in changed {
        if let Some(slot) = model.elements.get_mut(id) {
            *slot = e;
        }
    }
    Ok(json!({"changed": ids}))
}

/// `e` raised by `dz` mm (lowered, for less than none) on its own level: its offset from it taking the rise, a door's or a
/// window's sill, a rebar's points; none for what has no height of its own to change (annotation, grids, rooms' lines), and
/// for a level, whose elevation `move` changes itself.
pub(crate) fn lifted(m: &Model, e: &Element, dz: f64) -> Option<Element> {
    match e {
        Element::Door(o) | Element::Window(o) => {
            let mut o = o.clone();
            o.sill += dz;
            Some(if matches!(e, Element::Door(_)) { Element::Door(o) } else { Element::Window(o) })
        }
        Element::Rebar(r) => {
            let mut r = r.clone();
            for p in &mut r.path {
                p[2] += dz;
            }
            Some(Element::Rebar(r))
        }
        // A component on a host goes where its host goes.
        Element::Component(c) if c.host.is_some() => None,
        Element::Railing(r) if r.host.is_some() => None,
        Element::Wall(w) if w.panel_of.is_some() => None,
        Element::Wall(_)
        | Element::Floor(_)
        | Element::Ceiling(_)
        | Element::Shaft(_)
        | Element::Mass(_)
        | Element::Toposolid(_)
        | Element::Room(_)
        | Element::Column(_)
        | Element::StructuralColumn(_)
        | Element::Footing(_)
        | Element::Stair(_)
        | Element::Roof(_)
        | Element::PointCloud(_)
        | Element::Component(_)
        | Element::Railing(_) => m.level_of(e).map(|l| moved(e, l, dz)),
        e if e.run().is_some() => m.level_of(e).map(|l| moved(e, l, dz)),
        _ => None,
    }
}
