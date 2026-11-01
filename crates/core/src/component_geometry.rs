//! Per-operation reuse of family geometry, bound to an immutable model snapshot.
use std::collections::BTreeMap;

use crate::family::Piece;
use crate::geom::P2;
use crate::model::{Component, Model};

type Key = (String, String, bool, String);

pub(crate) struct Components<'a> {
    model: &'a Model,
    local: BTreeMap<Key, Vec<Piece>>,
    /// The heights of the model's components with no values of their own, over their base, by family, type and mirroring:
    /// most of a project's, found without a key made for each.
    plain: std::collections::HashMap<(&'a str, &'a str, bool), (f64, f64)>,
    /// The points of each geometry's triangles that may be its farthest any way across or up ([`Components::extremes`]).
    extremes: BTreeMap<Key, Vec<[f64; 3]>>,
}

impl<'a> Components<'a> {
    pub(crate) fn new(model: &'a Model) -> Self {
        Self { model, local: BTreeMap::new(), plain: Default::default(), extremes: BTreeMap::new() }
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
    }
}
