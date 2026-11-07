//! Conservative, independently cached primitive bounds. These boxes are query data;
//! exact framing continues to use the original extreme points. Unknown geometry must
//! be retained by callers, never interpreted as empty or outside a query.
use std::collections::{BTreeMap, BTreeSet};

use crate::ids::{Id, Revision};
use crate::model::Model;
use crate::view3d::P3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Bounds {
    /// The id is absent from this model.
    Empty,
    /// Unsupported geometry, a missing definition, or non-finite arithmetic.
    Unknown,
    Box([P3; 2]),
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub hits: u64,
    pub built: u64,
    pub evicted: u64,
    pub entries: usize,
    /// Inline entries plus a conservative charge for both tree nodes. No model,
    /// strings, element objects, or old definition data are retained.
    pub retained_bytes: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    Other,
    Missing,
    Column { dimensions: [u64; 2], round: bool },
    Run { dimensions: [u64; 2], round: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Key {
    own: Option<Revision>,
    levels: [Option<Revision>; 2],
    shape: Shape,
}

impl Key {
    pub(crate) fn of(m: &Model, id: Id) -> Self {
        let mut key = Self { own: m.elements.revision(id), levels: [None; 2], shape: Shape::Other };
        let Some(e) = m.elements.get(id) else { return key };
        if let Some(c) = e.column() {
            key.levels = [m.elements.revision(c.level), c.top_level.and_then(|id| m.elements.revision(id))];
            key.shape = m
                .column_type(e)
                .map_or(Shape::Missing, |t| Shape::Column { dimensions: [t.width.to_bits(), t.depth.to_bits()], round: t.round });
        } else if let Some(r) = e.run() {
            key.levels[0] = m.elements.revision(r.level);
            key.shape =
                m.run_type(e).map_or(Shape::Missing, |t| Shape::Run { dimensions: [t.width.to_bits(), t.depth.to_bits()], round: t.round });
        } else if let crate::model::Element::Component(c) = e {
            key.levels[0] = m.elements.revision(c.level);
        }
        key
    }
}

struct Entry {
    key: Key,
    value: Bounds,
    used: u64,
    family: Option<crate::shared::Identity<BTreeMap<String, crate::family::Family>>>,
}

/// Lazy per-id bounds with explicit byte-budgeted least-recently-used eviction.
/// A query validates only its own element, up to two levels, and its used type's
/// dimensions. Material colours do not invalidate boxes. Covers columns/footings,
/// straight runs and supported component envelopes. Component bounds conservatively
/// invalidate on any family-map edit without retaining its data. Unsupported
/// categories/forms return [`Bounds::Unknown`].
pub struct BoundsCache {
    budget: usize,
    clock: u64,
    entries: BTreeMap<Id, Entry>,
    order: BTreeSet<(u64, Id)>,
    stats: Stats,
}

// Charge the partially occupied roots once, as well as per-entry node slack. A
// single-entry tree still reserves slots for multiple inline keys/values.
const ENTRY_BYTES: usize = std::mem::size_of::<Entry>() + 512;
const ROOT_BYTES: usize = 12 * (std::mem::size_of::<Entry>() + std::mem::size_of::<Id>() + std::mem::size_of::<(u64, Id)>()) + 256;

impl Default for BoundsCache {
    fn default() -> Self {
        Self::new(16 * 1024 * 1024)
    }
}

impl BoundsCache {
    pub fn new(budget: usize) -> Self {
        Self { budget, clock: 0, entries: BTreeMap::new(), order: BTreeSet::new(), stats: Stats::default() }
    }

    pub fn stats(&self) -> Stats {
        self.stats
    }

    pub fn set_budget(&mut self, budget: usize) {
        self.budget = budget;
        self.trim();
    }

    pub fn clear(&mut self) {
        self.entries = BTreeMap::new();
        self.order = BTreeSet::new();
        self.stats.entries = 0;
        self.stats.retained_bytes = 0;
    }

    fn trim(&mut self) {
        while self.stats.retained_bytes > self.budget {
            let Some((_, id)) = self.order.pop_first() else { break };
            self.entries.remove(&id);
            self.stats.evicted += 1;
            self.stats.entries -= 1;
            self.stats.retained_bytes -= ENTRY_BYTES;
            if self.stats.entries == 0 {
                self.stats.retained_bytes -= ROOT_BYTES;
            }
        }
        if self.entries.is_empty() {
            self.entries = BTreeMap::new();
            self.order = BTreeSet::new();
        }
    }

    pub fn get(&mut self, m: &Model, id: Id) -> Bounds {
        let key = Key::of(m, id);
        let component = matches!(m.elements.get(id), Some(crate::model::Element::Component(_)));
        // Avoid generation aliasing even on an exhausted usage counter.
        if self.clock == u64::MAX {
            self.clear();
            self.clock = 0;
        }
        self.clock += 1;
        if let Some(entry) = self.entries.get_mut(&id) {
            self.order.remove(&(entry.used, id));
            let family_matches = match &entry.family {
                Some(identity) => component && identity.is_of(&m.families),
                None => !component,
            };
            if entry.key == key && family_matches {
                self.stats.hits += 1;
                entry.used = self.clock;
                self.order.insert((entry.used, id));
                return entry.value;
            }
        }
        self.stats.built += 1;
        let value = primitive(m, id);
        if self.budget >= ROOT_BYTES + ENTRY_BYTES {
            let family = component.then(|| m.families.identity());
            if self.entries.insert(id, Entry { key, value, used: self.clock, family }).is_none() {
                if self.stats.entries == 0 {
                    self.stats.retained_bytes += ROOT_BYTES;
                }
                self.stats.entries += 1;
                self.stats.retained_bytes += ENTRY_BYTES;
            }
            self.order.insert((self.clock, id));
            self.trim();
        }
        value
    }
}

pub(crate) fn primitive(m: &Model, id: Id) -> Bounds {
    let Some(e) = m.elements.get(id) else { return Bounds::Empty };
    if let Some(c) = e.column() {
        let Some(t) = m.column_type(e) else { return Bounds::Unknown };
        if t.width <= 0.0 || (!t.round && t.depth <= 0.0) {
            return Bounds::Unknown;
        }
        // Attached, it depends on what it's attached to, which the key doesn't hold.
        if c.attach_top.is_some() || c.attach_base.is_some() {
            return Bounds::Unknown;
        }
        let (lo, hi) = m.column_span(e);
        // Match the slab's sums, including cancellation and overflow in its rise.
        let top = lo + (hi - lo);
        return padded(m.column_outline(e).into_iter().flat_map(|p| [[p.0, p.1, lo], [p.0, p.1, top]]));
    }
    if e.run().is_some() {
        let Some(g) = m.run_geom(e) else { return Bounds::Unknown };
        if g.width <= 0.0 || g.depth <= 0.0 {
            return Bounds::Unknown;
        }
        if g.round {
            // The exact same basis as tube(), but bound all angles without triangles.
            let d = crate::view3d::unit(crate::view3d::sub(g.b, g.a));
            let u = if g.vertical() { [1.0, 0.0, 0.0] } else { crate::view3d::unit([-d[1], d[0], 0.0]) };
            let v = crate::view3d::cross(d, u);
            let reach: P3 = std::array::from_fn(|i| (u[i].abs() + v[i].abs()) * (g.width / 2.0));
            return padded(
                [g.a, g.b].into_iter().flat_map(|p| [std::array::from_fn(|i| p[i] - reach[i]), std::array::from_fn(|i| p[i] + reach[i])]),
            );
        }
        let (ring, bottom, top) = g.prism();
        return padded(ring.into_iter().flat_map(|p| [[p.0, p.1, bottom.at(p)], [p.0, p.1, top.at(p)]]));
    }
    if let crate::model::Element::Component(c) = e {
        let Some(family) = m.families.get(&c.family) else { return Bounds::Unknown };
        let Some(bounds) = family.values(&c.type_name, &c.params).ok().and_then(|values| family.conservative_bounds(&values)) else {
            return Bounds::Unknown;
        };
        return place_component(m, c, bounds);
    }
    Bounds::Unknown
}

pub(crate) fn place_component(m: &Model, c: &crate::model::Component, bounds: [P3; 2]) -> Bounds {
    let (sin, cos) = c.angle.to_radians().sin_cos();
    let base = m.elevation(c.level) + c.offset;
    padded((0..8).map(|k| {
        let [x, y, z] = [bounds[k & 1][0], bounds[(k >> 1) & 1][1], bounds[(k >> 2) & 1][2]];
        let y = if c.mirrored { -y } else { y };
        [c.at.0 + x * cos - y * sin, c.at.1 + x * sin + y * cos, base + z]
    }))
}

pub(crate) fn padded(points: impl Iterator<Item = P3>) -> Bounds {
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for p in points {
        if !p.iter().all(|v| v.is_finite()) {
            return Bounds::Unknown;
        }
        for i in 0..3 {
            lo[i] = lo[i].min(p[i]);
            hi[i] = hi[i].max(p[i]);
        }
    }
    for i in 0..3 {
        let margin = 32.0 * f64::EPSILON * lo[i].abs().max(hi[i].abs()) + 1e-9;
        lo[i] -= margin;
        hi[i] += margin;
    }
    if lo.iter().chain(&hi).all(|v| v.is_finite()) { Bounds::Box([lo, hi]) } else { Bounds::Unknown }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::P2;
    use crate::model::{Column, ColumnType, Element, Justify, Run, RunType};

    fn encloses_solids(m: &Model, id: Id) {
        let Bounds::Box([lo, hi]) = BoundsCache::default().get(m, id) else { panic!("finite primitive bounds") };
        let corners: Vec<_> = crate::view3d::solids(m).iter().filter(|s| s.owner() == id).flat_map(|s| s.corners()).collect();
        assert!(!corners.is_empty());
        for p in corners {
            for i in 0..3 {
                assert!(lo[i] <= p[i] && p[i] <= hi[i], "{p:?} outside {lo:?}..{hi:?}");
            }
        }
    }

    #[test]
    fn primitive_boxes_enclose_fresh_solid_corners_without_tessellating() {
        let mut m = Model::new("Bounds oracle");
        let level = m.guid(100000);
        let id = 200000;
        for round in [false, true] {
            m.types.column.insert("Probe".into(), ColumnType { width: 600.0, depth: 250.0, round });
            for angle in [0.0, 17.0, 90.0, -225.0] {
                for at in [P2(0.0, 0.0), P2(1e9, -1e9)] {
                    m.elements.insert(
                        id,
                        Element::Column(Column {
                            type_name: "Probe".into(),
                            level,
                            at,
                            angle,
                            top_level: None,
                            height: 3200.0,
                            base_offset: -300.0,
                            top_offset: 0.0,
                            attach_top: None,
                            attach_base: None,
                            params: Default::default(),
                            room_bounding: true,
                        }),
                    );
                    encloses_solids(&m, id);
                }
            }
            m.types
                .runs
                .entry("pipe".into())
                .or_default()
                .insert("Probe".into(), RunType { width: 400.0, depth: 180.0, round, material: String::new() });
            for end in [P2(1000.0, 0.0), P2(-1000.0, 370.0), P2(0.0, 0.0), P2(0.5, 0.0), P2(1.01, 0.0)] {
                for end_offset in [Some(10000.0), Some(-400.0), None] {
                    for justify in [Justify::Centre, Justify::Top, Justify::Bottom] {
                        m.elements.insert(
                            id,
                            Element::Pipe(Run {
                                type_name: "Probe".into(),
                                level,
                                start: P2(0.0, 0.0),
                                end,
                                offset: 100.0,
                                end_offset,
                                justify,
                                system: String::new(),
                                params: Default::default(),
                            }),
                        );
                        encloses_solids(&m, id);
                    }
                }
            }
        }
    }

    #[test]
    fn nonfinite_bounds_and_counter_exhaustion_remain_conservative() {
        assert_eq!(padded([[f64::NAN, 0.0, 0.0]].into_iter()), Bounds::Unknown);
        assert_eq!(padded([[f64::MAX; 3]].into_iter()), Bounds::Unknown);
        assert_eq!(padded([].into_iter()), Bounds::Unknown);
        let m = Model::new("Counter wrap");
        let mut cache = BoundsCache::default();
        assert_eq!(cache.get(&m, 42), Bounds::Empty);
        cache.clock = u64::MAX;
        assert_eq!(cache.get(&m, 42), Bounds::Empty);
        assert_eq!(cache.stats().entries, 1);
        assert_eq!(cache.stats().hits, 0);
        assert_eq!(cache.stats().built, 2);
    }
}
