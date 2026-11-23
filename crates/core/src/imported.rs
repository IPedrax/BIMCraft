//! What an element's imported shape ([`ImportedShape`]: another program's own geometry of a wall, a floor, a roof, a ceiling or a
//! column) is held to: the element as it was when the shape was drawn for it. The shape holds a key of that element; while the
//! element's key is still the shape's, the 3D view draws the shape in place of the element's own geometry, and once an edit changes
//! the key, the element's own geometry again.
//!
//! The key is FNV-1a over what decides the element's geometry, written as JSON: the element itself (its imported shape and its
//! parameters' values left out: a value of its own is no edit of its shape), its type, each element it points at (its levels, the
//! roof or floor it is attached to: their own geometry, so a level moved up changes it) and each door and window set in it, with
//! their types. What only says something of it (a tag, a dimension) points at it and does not change it.

use crate::model::{Element, Id, ImportedShape, Model};

/// FNV-1a, 64 bits: the same in every build and on every machine.
struct Fnv(u64);

impl Fnv {
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 ^= u64::from(*b);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
        // A separator, so that two pieces are not read as one.
        self.0 ^= 0xff;
        self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
    }
}

/// `e` as JSON, without its imported shape and, where its parameters do not shape it, without their values.
fn geometry(e: &Element) -> Vec<u8> {
    let mut e = e.clone();
    if let Some(slot) = e.imported_mut() {
        *slot = None;
    }
    // A door's, a window's or a component's values may be its family's sizes; anything else's are values said of it.
    if !matches!(e, Element::Door(_) | Element::Window(_) | Element::Component(_))
        && let Some(params) = e.params_mut()
    {
        params.clear();
    }
    serde_json::to_vec(&e).unwrap_or_default()
}

/// The type `e` is of, as JSON: a system type's, or its family type's values. Empty for what has none.
fn type_of(m: &Model, e: &Element) -> Vec<u8> {
    let json = |v: Option<serde_json::Result<Vec<u8>>>| v.and_then(Result::ok).unwrap_or_default();
    if let Some((family, ty)) = e.family() {
        return json(m.families.get(family).and_then(|f| f.types.get(ty)).map(serde_json::to_vec));
    }
    let t = &m.types;
    match e {
        Element::Wall(w) => json(t.wall.get(&w.type_name).map(serde_json::to_vec)),
        Element::Floor(f) => json(t.floor.get(&f.type_name).map(serde_json::to_vec)),
        Element::Roof(r) => json(t.roof.get(&r.type_name).map(serde_json::to_vec)),
        Element::Ceiling(c) => json(t.ceiling.get(&c.type_name).map(serde_json::to_vec)),
        Element::Column(c) => json(t.column.get(&c.type_name).map(serde_json::to_vec)),
        Element::StructuralColumn(c) => json(t.structural_column.get(&c.type_name).map(serde_json::to_vec)),
        Element::Footing(c) => json(t.footing.get(&c.type_name).map(serde_json::to_vec)),
        Element::Door(o) => json(t.door.get(&o.type_name).map(serde_json::to_vec)),
        Element::Window(o) => json(t.window.get(&o.type_name).map(serde_json::to_vec)),
        _ => Vec::new(),
    }
}

/// Element `id`'s key, which an imported shape drawn for it holds ([`ImportedShape::key`]); none where it is of no kind that may
/// carry one.
pub fn key(m: &Model, id: Id) -> Option<u64> {
    let e = m.elements.get(id)?;
    e.clone().imported_mut()?;
    let mut h = Fnv(0xcbf2_9ce4_8422_2325);
    h.write(&geometry(e));
    h.write(&type_of(m, e));
    let mut refs = e.refs();
    refs.sort();
    refs.dedup();
    for g in refs {
        h.write(g.to_string().as_bytes());
        if let Some(x) = m.elements.id(g).and_then(|k| m.elements.get(k)) {
            h.write(&geometry(x));
        }
    }
    let guid = m.guid(id);
    let mut cut: Vec<Vec<u8>> = m
        .lookup()
        .referrers(&guid)
        .filter_map(|k| m.elements.get(k))
        .filter(|x| matches!(x, Element::Door(o) | Element::Window(o) if o.host == guid))
        .map(|x| [geometry(x), type_of(m, x)].concat())
        .collect();
    cut.sort();
    for c in cut {
        h.write(&c);
    }
    Some(h.0)
}

/// Element `id`'s imported shape, where it has one and it is still the element it was drawn for ([`key`]).
pub fn live(m: &Model, id: Id) -> Option<&ImportedShape> {
    let shape = m.elements.get(id)?.imported()?;
    (key(m, id) == Some(shape.key)).then_some(shape)
}
