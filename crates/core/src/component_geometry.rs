//! Per-operation reuse of family geometry, bound to an immutable model snapshot.
use std::collections::BTreeMap;

use crate::family::Piece;
use crate::geom::P2;
use crate::model::{Component, Id, Model};

/// What a shape is the shape of: its family, its type, whether it is mirrored, and the values its family evaluates.
pub(crate) type Key = (String, String, bool, String);

/// One local family shape and every component that places it. Mirroring and instance parameters
/// are part of the shape key; rotation and translation belong to the placement.
pub(crate) struct SharedComponents {
    pub(crate) shapes: Vec<Vec<Piece>>,
    /// Local triangle bounds for each shape, used to bound placed instances without expanding triangles.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) bounds: Vec<Option<[[f64; 3]; 2]>>,
    pub(crate) instances: Vec<ComponentInstance>,
    /// Each shape's key, and where it is in `shapes`.
    pub(crate) keys: BTreeMap<Key, usize>,
}

/// A copy of a local shape: `owner`'s, at `at`, its shape's axes taken to `axes` (each square to the others, any length, so a
/// copy may be turned, and stretched along an axis as a run's tube is along its line).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ComponentInstance {
    pub(crate) owner: Id,
    pub(crate) shape: usize,
    pub(crate) at: [f64; 3],
    pub(crate) axes: [[f64; 3]; 3],
}

impl ComponentInstance {
    /// A component's: turned about the vertical by the angle whose sine and cosine these are.
    pub(crate) fn turned(owner: Id, shape: usize, at: [f64; 3], sin: f64, cos: f64) -> Self {
        Self { owner, shape, at, axes: [[cos, sin, 0.0], [-sin, cos, 0.0], [0.0, 0.0, 1.0]] }
    }

    /// `p` placed. For a turn this is, to the last bit, `at + (x cos - y sin, x sin + y cos, z)` as a component's pieces are
    /// placed: what the extra terms add is zero.
    pub(crate) fn place(&self, p: [f64; 3]) -> [f64; 3] {
        let [a, b, c] = self.axes;
        std::array::from_fn(|i| self.at[i] + p[0] * a[i] + p[1] * b[i] + p[2] * c[i])
    }

    /// A normal (unit) of the shape, of the copy: square to the copy's surfaces however it is stretched.
    pub(crate) fn normal(&self, n: [f64; 3]) -> [f64; 3] {
        let [a, b, c] = self.axes;
        let w = |v: [f64; 3]| 1.0 / (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]);
        let (wa, wb, wc) = (n[0] * w(a), n[1] * w(b), n[2] * w(c));
        crate::view3d::unit(std::array::from_fn(|i| a[i] * wa + b[i] * wb + c[i] * wc))
    }

    /// The other way: a normal of the copy, of its shape.
    pub(crate) fn local_normal(&self, n: [f64; 3]) -> [f64; 3] {
        let dot = |v: [f64; 3]| v[0] * n[0] + v[1] * n[1] + v[2] * n[2];
        let [a, b, c] = self.axes;
        crate::view3d::unit([dot(a), dot(b), dot(c)])
    }

    /// Whether its axes span space, so a normal can be placed.
    pub(crate) fn solid(&self) -> bool {
        self.axes.iter().all(|v| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).is_normal()) && self.at.iter().all(|v| v.is_finite())
    }

    pub(crate) fn bounds(&self, local: [[f64; 3]; 2]) -> [[f64; 3]; 2] {
        let mut world = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
        for k in 0..8 {
            let corner = std::array::from_fn(|axis| local[(k >> axis) & 1][axis]);
            let p = self.place(corner);
            for axis in 0..3 {
                world[0][axis] = world[0][axis].min(p[axis]);
                world[1][axis] = world[1][axis].max(p[axis]);
            }
        }
        world
    }
}

impl SharedComponents {
    pub(crate) fn key(model: &Model, c: &Component) -> Key {
        // Paint, phase and design-option metadata live in `params` too. Only names that the family
        // evaluates can change its local geometry; appearance is resolved per placement later.
        let params = model.families.get(&c.family).map(|family| {
            c.params.iter().filter(|(name, _)| family.parameters.iter().any(|p| p.name == **name)).collect::<BTreeMap<_, _>>()
        });
        (c.family.clone(), c.type_name.clone(), c.mirrored, serde_json::to_string(&params).unwrap_or_default())
    }

    /// Build each local shape once. No triangles are placed or copied for the instances here.
    pub(crate) fn new(model: &Model, cancelled: &(dyn Fn() -> bool + Sync)) -> Option<Self> {
        let mut shapes = Vec::new();
        let mut bounds = Vec::new();
        let mut indices = BTreeMap::new();
        let mut instances = Vec::new();
        for (i, (owner, element)) in model.elements.iter().enumerate() {
            if i % 256 == 0 && cancelled() {
                return None;
            }
            let crate::model::Element::Component(c) = element else { continue };
            let key = Self::key(model, c);
            let shape = match indices.get(&key) {
                Some(&n) => n,
                None => {
                    let n = shapes.len();
                    let pieces = model.component_local_pieces(c);
                    let local = pieces.iter().flat_map(|piece| piece.tris.iter().flatten());
                    let extent = local.fold(None, |state: Option<[[f64; 3]; 2]>, p| {
                        let [mut lo, mut hi] = state.unwrap_or([*p, *p]);
                        for axis in 0..3 {
                            lo[axis] = lo[axis].min(p[axis]);
                            hi[axis] = hi[axis].max(p[axis]);
                        }
                        Some([lo, hi])
                    });
                    bounds.push(extent);
                    shapes.push(pieces);
                    indices.insert(key, n);
                    n
                }
            };
            let (sin, cos) = c.angle.to_radians().sin_cos();
            instances.push(ComponentInstance::turned(*owner, shape, [c.at.0, c.at.1, model.elevation(c.level) + c.offset], sin, cos));
        }
        (!cancelled()).then_some(Self { shapes, bounds, instances, keys: indices })
    }
}

/// The local form envelope of each family and type that components with no values of their own use, found once for a whole
/// pass over the model: most of a project's, each found without a key made for it, and no chunk of the pass finding it again.
pub(crate) type PlainEnvelopes<'a> = std::collections::HashMap<(&'a str, &'a str), Option<[[f64; 3]; 2]>>;

/// [`PlainEnvelopes`] of `components`. None when `cancelled`.
pub(crate) fn plain_envelopes<'a>(
    m: &Model,
    components: impl Iterator<Item = &'a Component>,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Option<PlainEnvelopes<'a>> {
    let mut plain = PlainEnvelopes::new();
    for (i, c) in components.filter(|c| c.params.is_empty()).enumerate() {
        if i % 256 == 0 && cancelled() {
            return None;
        }
        plain.entry((c.family.as_str(), c.type_name.as_str())).or_insert_with(|| local_envelope(m, c));
    }
    Some(plain)
}

fn local_envelope(m: &Model, c: &Component) -> Option<[[f64; 3]; 2]> {
    let family = m.families.get(&c.family)?;
    let values = family.values(&c.type_name, &c.params).ok()?;
    family.conservative_bounds(&values)
}

pub(crate) struct Components<'a> {
    model: &'a Model,
    local: BTreeMap<Key, Vec<Piece>>,
    /// The heights of the model's components with no values of their own, over their base, by family, type and mirroring:
    /// most of a project's, found without a key made for each.
    plain: std::collections::HashMap<(&'a str, &'a str, bool), (f64, f64)>,
    /// The points of each geometry's triangles that may be its farthest any way across or up ([`Components::extremes`]).
    extremes: BTreeMap<Key, Vec<[f64; 3]>>,
    envelopes: BTreeMap<Key, Option<[[f64; 3]; 2]>>,
}

impl<'a> Components<'a> {
    pub(crate) fn new(model: &'a Model) -> Self {
        Self { model, local: BTreeMap::new(), plain: Default::default(), extremes: BTreeMap::new(), envelopes: BTreeMap::new() }
    }

    /// Supported local form envelopes, placed without building any triangles.
    pub(crate) fn envelope(&mut self, c: &Component, plain: &PlainEnvelopes) -> Option<[[f64; 3]; 2]> {
        let model = self.model;
        let local = match plain.get(&(c.family.as_str(), c.type_name.as_str())).filter(|_| c.params.is_empty()) {
            Some(local) => *local,
            None => *self.envelopes.entry(Self::key(c)).or_insert_with(|| local_envelope(model, c)),
        };
        match crate::bounds::place_component(model, c, local?) {
            crate::bounds::Bounds::Box(bounds) => Some(bounds),
            _ => None,
        }
    }

    /// [`Components::heights`] of a component of the model's.
    pub(crate) fn heights_of(&mut self, c: &'a Component) -> (f64, f64) {
        if !c.params.is_empty() {
            return self.heights(c);
        }
        let base = self.model.elevation(c.level) + c.offset;
        let key = (c.family.as_str(), c.type_name.as_str(), c.mirrored);
        let (lo, hi) = match self.plain.get(&key) {
            Some(span) => *span,
            None => {
                let span = self.local(c).iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p.bottom), hi.max(p.top)));
                self.plain.insert(key, span);
                span
            }
        };
        (base + lo, base + hi)
    }

    fn key(c: &Component) -> Key {
        (c.family.clone(), c.type_name.clone(), c.mirrored, serde_json::to_string(&c.params).unwrap_or_default())
    }

    fn local(&mut self, c: &Component) -> &[Piece] {
        self.local.entry(Self::key(c)).or_insert_with(|| self.model.component_local_pieces(c))
    }

    /// The points of its pieces' triangles, where it is placed, that may be the farthest any way: across (along any line
    /// in plan) or up. Those within a micron of the hull of all of them in plan, and the lowest and highest: the same
    /// points [`Components::pieces`] places, by the same sums, so the farthest of them any way is the farthest of every
    /// point of its pieces, to the last bit.
    pub(crate) fn extremes(&mut self, c: &Component) -> Vec<[f64; 3]> {
        let key = Self::key(c);
        if !self.extremes.contains_key(&key) {
            let points: Vec<[f64; 3]> = self.local(c).iter().flat_map(|pc| pc.tris.iter().flatten().copied()).collect();
            let finite = |p: &[f64; 3]| p.iter().all(|v| v.is_finite());
            let hull = crate::geom::hull(points.iter().filter(|p| finite(p)).map(|p| P2(p[0], p[1])).collect());
            let near_hull = |q: P2| match hull.len() {
                0 => true,
                1 => hull.first().is_some_and(|h| h.dist(q) <= 1e-6),
                n => (0..n).any(|i| crate::geom::dist_to_segment(q, hull[i], hull[(i + 1) % n]) <= 1e-6),
            };
            // The lowest and highest, of those that aren't NaN, as `f64::min` and `f64::max` leave NaN out.
            let (low, high) =
                points.iter().filter(|p| !p[2].is_nan()).fold((None, None), |(lo, hi): (Option<[f64; 3]>, Option<[f64; 3]>), p| {
                    (Some(lo.filter(|l| l[2] <= p[2]).unwrap_or(*p)), Some(hi.filter(|h| h[2] >= p[2]).unwrap_or(*p)))
                });
            // And any point not of finite numbers: more points never change the farthest. Each place in plan once.
            let mut kept = crate::view3d::extremes(points.iter().copied().filter(|p| !finite(p) || near_hull(P2(p[0], p[1]))));
            kept.extend(low);
            kept.extend(high);
            self.extremes.insert(key.clone(), kept);
        }
        let base = self.model.elevation(c.level) + c.offset;
        let (sin, cos) = c.angle.to_radians().sin_cos();
        let place = |p: [f64; 3]| [c.at.0 + p[0] * cos - p[1] * sin, c.at.1 + p[0] * sin + p[1] * cos, base + p[2]];
        self.extremes.get(&key).map(|v| v.iter().map(|p| place(*p)).collect()).unwrap_or_default()
    }

    /// Its geometry's heights over its base, lowest and highest: none (infinity, minus infinity) when it has no pieces.
    pub(crate) fn local_heights(&mut self, c: &Component) -> (f64, f64) {
        self.local(c).iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p.bottom), hi.max(p.top)))
    }

    pub(crate) fn heights(&mut self, c: &Component) -> (f64, f64) {
        let base = self.model.elevation(c.level) + c.offset;
        self.local(c).iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(base + p.bottom), hi.max(base + p.top)))
    }

    /// Its pieces' outlines in plan, as [`Components::pieces`] places them, without their triangles.
    pub(crate) fn outlines(&mut self, c: &Component) -> Vec<P2> {
        let base = self.model.elevation(c.level) + c.offset;
        let (sin, cos) = c.angle.to_radians().sin_cos();
        let place = |p: [f64; 3]| [c.at.0 + p[0] * cos - p[1] * sin, c.at.1 + p[0] * sin + p[1] * cos, base + p[2]];
        let outlines: Vec<P2> =
            self.local(c).iter().flat_map(|pc| pc.outline.iter().map(|q| place([q.0, q.1, 0.0])).map(|p| P2(p[0], p[1]))).collect();
        #[cfg(test)]
        assert_eq!(outlines, self.model.component_pieces(c).into_iter().flat_map(|p| p.outline).collect::<Vec<_>>());
        outlines
    }

    pub(crate) fn pieces(&mut self, c: &Component) -> Vec<Piece> {
        let base = self.model.elevation(c.level) + c.offset;
        let (sin, cos) = c.angle.to_radians().sin_cos();
        let place = |p: [f64; 3]| [c.at.0 + p[0] * cos - p[1] * sin, c.at.1 + p[0] * sin + p[1] * cos, base + p[2]];
        self.local(c)
            .iter()
            .map(|pc| Piece {
                tris: pc.tris.iter().map(|t| t.map(place)).collect(),
                outline: pc.outline.iter().map(|q| place([q.0, q.1, 0.0])).map(|p| P2(p[0], p[1])).collect(),
                bottom: base + pc.bottom,
                top: base + pc.top,
                material: pc.material.clone(),
                caps: pc.caps,
                round: pc.round,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Session;
    use serde_json::json;

    #[test]
    fn repeated_components_keep_their_placement_mirroring_and_instance_parameters() {
        let mut s = Session::new();
        s.execute(
            "family",
            &json!({"name": "Probe", "parameters": [{"name": "Height", "instance": true, "value": 750}],
            "types": {"A": {}}, "forms": [{"profile": [[0,0],[800,0],[800,400],[0,400]], "to": "Height"}]}),
        )
        .unwrap();
        for (angle, mirrored, params) in
            [(0, false, json!({})), (45, false, json!({})), (90, true, json!({})), (180, false, json!({"Height": 1200}))]
        {
            s.execute(
                "component",
                &json!({"family":"Probe", "at":[angle*10,1000], "angle": angle,
                "offset": 100, "mirrored":mirrored, "params": params}),
            )
            .unwrap();
        }
        let first = s.model.elements.iter().find(|(_, e)| matches!(e, crate::model::Element::Component(_))).map(|(id, _)| *id).unwrap();
        s.execute("material", &json!({"name": "Test paint", "color": [90, 120, 150]})).unwrap();
        s.execute("paint", &json!({"id": first, "face": "all", "material": "Test paint"})).unwrap();
        let mut cache = Components::new(&s.model);
        for _ in 0..2 {
            for e in s.model.elements.values() {
                if let crate::model::Element::Component(c) = e {
                    let a = s.model.component_pieces(c);
                    let b = cache.pieces(c);
                    assert_eq!(a, b);
                    let heights = a.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p.bottom), hi.max(p.top)));
                    assert_eq!(cache.heights(c), heights);
                }
            }
        }
        let shared = SharedComponents::new(&s.model, &|| false).unwrap();
        assert_eq!(shared.shapes.len(), 3);
        assert_eq!(shared.instances.len(), 4);
        for instance in &shared.instances {
            let Some(crate::model::Element::Component(c)) = s.model.elements.get(instance.owner) else { panic!("component") };
            let placed = s.model.component_pieces(c);
            let local = &shared.shapes[instance.shape];
            let bounds = instance.bounds(shared.bounds[instance.shape].unwrap());
            assert_eq!(placed.len(), local.len());
            for (p, shape) in placed.iter().zip(local) {
                assert_eq!(p.tris, shape.tris.iter().map(|t| t.map(|q| instance.place(q))).collect::<Vec<_>>());
                for (world, local) in p.tris.iter().zip(&shape.tris) {
                    for point in world {
                        for axis in 0..3 {
                            assert!(point[axis] >= bounds[0][axis] - 1e-9 && point[axis] <= bounds[1][axis] + 1e-9);
                        }
                    }
                    let normal = |[a, b, c]: &[[f64; 3]; 3]| {
                        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
                        [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]
                    };
                    let expected = instance.normal(normal(local));
                    let actual = crate::view3d::unit(normal(world));
                    for axis in 0..3 {
                        assert!((actual[axis] - expected[axis]).abs() <= expected[axis].abs().max(1.0) * 1e-9);
                    }
                }
            }
        }
    }

    #[test]
    #[ignore = "measures the large tower fixture"]
    fn shared_component_shape_count_in_the_ten_storey_tower() {
        let model = crate::sample::tower::annotated(crate::sample::tower::Plate::REFERENCE, 10).unwrap();
        let shared = SharedComponents::new(&model, &|| false).unwrap();
        let tris = |shape: &Vec<Piece>| shape.iter().map(|p| p.tris.len()).sum::<usize>();
        let unique = shared.shapes.iter().map(&tris).sum::<usize>();
        let placed = shared.instances.iter().map(|i| tris(&shared.shapes[i.shape])).sum::<usize>();
        eprintln!(
            "{} instances, {} shapes, {} placed triangles, {} unique triangles",
            shared.instances.len(),
            shared.shapes.len(),
            placed,
            unique
        );
        assert!(placed > unique * 10);
    }
}
