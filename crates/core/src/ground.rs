//! The ground a toposolid's top makes, worked out once and asked many times: what the fetches and imports stand buildings on
//! (`import_osm`, `place_on_terrain`). The toposolid is upstream's terrain ([site](../../../docs/areas/site.md)); its top is a floor's
//! shape (`Model::floor_faces` of `Model::toposolid_as_floor`), so this only keeps those faces and looks a point up in them.

use crate::geom::{self, P2};
use crate::model::{Model, Toposolid};
use crate::walls::Plane;

/// The top of one toposolid, as planes over pieces of its outline.
pub struct Ground {
    boundary: Vec<P2>,
    faces: Vec<(Vec<P2>, Plane)>,
}

impl Ground {
    /// The ground toposolid `t` of `m` makes.
    pub fn of(m: &Model, t: &Toposolid) -> Ground {
        let floor = m.toposolid_as_floor(t);
        Ground { boundary: floor.boundary.clone(), faces: m.floor_faces(&floor) }
    }

    /// The height of the ground at `q`, mm from the project's zero; none off the toposolid.
    pub fn height_at(&self, q: P2) -> Option<f64> {
        // On its edge, within the model's tolerance, is on it.
        let near = |r: &[P2]| geom::contains(r, q) || geom::ring_edges(r).any(|(a, b)| geom::dist_to_segment(q, a, b) <= crate::model::TOL);
        if !(q.0.is_finite() && q.1.is_finite()) || !near(&self.boundary) {
            return None;
        }
        self.faces.iter().find(|(r, _)| near(r)).map(|(_, plane)| plane.at(q))
    }

    /// The lowest ground under `ring`'s corners and its middle: where a building on it stands, so none of it floats; none if all of
    /// them are off the toposolid.
    pub fn lowest(&self, ring: &[P2]) -> Option<f64> {
        if ring.is_empty() {
            return None;
        }
        let n = ring.len() as f64;
        let middle = P2(ring.iter().map(|q| q.0).sum::<f64>() / n, ring.iter().map(|q| q.1).sum::<f64>() / n);
        ring.iter().chain([&middle]).filter_map(|q| self.height_at(*q)).reduce(f64::min)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Session;
    use serde_json::json;

    /// A 10 m square toposolid on Level 1 (elevation 0) whose top rises 1 m from west to east: 0 at x = 0, 1000 mm at x = 10 m.
    fn sloped() -> (Session, Toposolid) {
        let mut s = Session::new();
        let boundary = json!([[0, 0], [10000, 0], [10000, 10000], [0, 10000]]);
        let points = json!([[0, 0, 0], [10000, 0, 1000], [10000, 10000, 1000], [0, 10000, 0]]);
        let made = s.execute("toposolid", &json!({"boundary": boundary, "points": points})).unwrap();
        let id = made["id"].as_u64().unwrap();
        let Some(crate::model::Element::Toposolid(t)) = s.model.elements.get(id).cloned() else { panic!("no toposolid") };
        (s, t)
    }

    #[test]
    fn the_ground_is_the_toposolids_top_where_it_is_and_nothing_off_it() {
        let (s, t) = sloped();
        let g = Ground::of(&s.model, &t);
        for (x, want) in [(0.0, 0.0), (2500.0, 250.0), (5000.0, 500.0), (10000.0, 1000.0)] {
            let z = g.height_at(P2(x, 5000.0)).unwrap();
            assert!((z - want).abs() < 1e-6, "at x = {x}: {z} not {want}");
        }
        assert_eq!(g.height_at(P2(-5.0, 5000.0)), None);
        assert_eq!(g.height_at(P2(f64::NAN, 5000.0)), None);
        // A building from x = 4 m to 8 m stands at the lowest of its corners and middle: 400 mm.
        let ring = [P2(4000.0, 4000.0), P2(8000.0, 4000.0), P2(8000.0, 6000.0), P2(4000.0, 6000.0)];
        assert!((g.lowest(&ring).unwrap() - 400.0).abs() < 1e-6);
        assert_eq!(g.lowest(&[P2(-5.0, -5.0), P2(-6.0, -5.0), P2(-5.0, -6.0)]), None);
        assert_eq!(g.lowest(&[]), None);
    }
}
