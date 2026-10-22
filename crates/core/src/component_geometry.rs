//! Per-operation reuse of family geometry, bound to an immutable model snapshot.
use std::collections::BTreeMap;

use crate::family::Piece;
use crate::geom::P2;
use crate::model::{Component, Model};

type Key = (String, String, bool, String);

pub(crate) struct Components<'a> {
    model: &'a Model,
    local: BTreeMap<Key, Vec<Piece>>,
}

impl<'a> Components<'a> {
    pub(crate) fn new(model: &'a Model) -> Self {
        Self { model, local: BTreeMap::new() }
    }

    fn local(&mut self, c: &Component) -> &[Piece] {
        let key = (c.family.clone(), c.type_name.clone(), c.mirrored, serde_json::to_string(&c.params).unwrap_or_default());
        self.local.entry(key).or_insert_with(|| self.model.component_local_pieces(c))
    }

    pub(crate) fn heights(&mut self, c: &Component) -> (f64, f64) {
        let base = self.model.elevation(c.level) + c.offset;
        self.local(c).iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(base + p.bottom), hi.max(base + p.top)))
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
