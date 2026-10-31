//! Read-only acceleration for one model snapshot. Plan tiles and heights never decide geometry: the existing overlap
//! and join rules still test every candidate, in the same order (by id). A wall's candidates at one of its ends are the
//! walls whose bodies reach that end's tile and overlap it in height, not every wall of the project.
use std::collections::{BTreeMap, HashMap};

use crate::geom::P2;
use crate::model::{Element, Guid, Id, Model, TOL};

const MAX_REFERENCES: usize = 1_000_000;
/// Plan tiles, mm on a side, and the most tiles one wall goes in (a longer one is a candidate everywhere).
const TILE: f64 = 6000.0;
const MAX_TILES: i64 = 4096;

pub(crate) struct Index {
    spans: BTreeMap<Id, (f64, f64)>,
    hosts: BTreeMap<Guid, Vec<Id>>,
    /// Each tile's walls: those whose body, grown by a tolerance, reaches into it.
    tiles: HashMap<(i64, i64), Vec<Id>>,
    /// Walls in no tile: too long for them, or of no finite size.
    everywhere: Vec<Id>,
}

impl Index {
    pub(crate) fn new(m: &Model) -> Self {
        let mut index = Self {
            spans: BTreeMap::new(),
            hosts: BTreeMap::new(),
            tiles: HashMap::new(),
            everywhere: Vec::new(),
        };
        let mut tiled = 0;
        for (&id, e) in &m.elements {
            match e {
                Element::Wall(w) => {
                    let (lo, hi) = (m.base_z(w), m.top_z(w));
                    index.spans.insert(id, (lo, hi));
                    match reach(m, w).map(|[x0, y0, x1, y1]| ([tile(x0), tile(y0)], [tile(x1), tile(y1)])) {
                        Some(([a, b], [c, d]))
                            if (c - a + 1).saturating_mul(d - b + 1) <= MAX_TILES && tiled < MAX_REFERENCES =>
                        {
                            for x in a..=c {
                                for y in b..=d {
                                    index.tiles.entry((x, y)).or_default().push(id);
                                    tiled += 1;
                                }
                            }
                        }
                        _ => index.everywhere.push(id),
                    }
                }
                Element::Door(o) | Element::Window(o) => {
                    index.hosts.entry(o.host).or_default().push(id);
                }
                _ => {}
            }
        }
        index
    }

    /// The walls that may meet wall `id` at `node`, one of its ends: those that overlap it in height and whose bodies reach
    /// the node's tile, by id. Every wall that ends there or that the node lies in is among them.
    pub(crate) fn neighbours_at(&self, id: Id, node: P2) -> Vec<Id> {
        let Some(&(lo, hi)) = self.spans.get(&id) else { return Vec::new() };
        let mut ids = self.everywhere.clone();
        if node.0.is_finite() && node.1.is_finite() {
            ids.extend(self.tiles.get(&(tile(node.0), tile(node.1))).map_or(&[][..], Vec::as_slice));
        } else {
            ids = self.spans.keys().copied().collect();
        }
        ids.sort_unstable();
        ids.dedup();
        ids.retain(|id| self.spans.get(id).is_some_and(|&(bottom, top)| lo < top && bottom < hi));
        ids
    }

    pub(crate) fn openings(&self, host: Guid) -> &[Id] {
        self.hosts.get(&host).map_or(&[], Vec::as_slice)
    }
}

/// The plan box a wall's body lies in, grown by twice the tolerance: [x0, y0, x1, y1]. An arc's whole circle's.
fn reach(m: &Model, w: &crate::model::Wall) -> Option<[f64; 4]> {
    let axis = super::Axis::of(w);
    let (ext, int) = super::faces(m, w);
    let grow = ext.abs().max(int.abs()) + 2.0 * TOL;
    let [x0, y0, x1, y1] = match axis.arc() {
        Some((c, r, _, _)) => [c.0 - r, c.1 - r, c.0 + r, c.1 + r],
        None => [w.start.0.min(w.end.0), w.start.1.min(w.end.1), w.start.0.max(w.end.0), w.start.1.max(w.end.1)],
    };
    let b = [x0 - grow, y0 - grow, x1 + grow, y1 + grow];
    b.iter().all(|v| v.is_finite() && v.abs() < 1e12).then_some(b)
}

fn tile(v: f64) -> i64 {
    (v / TILE).floor() as i64
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::{engine::Session, model::Model, walls};
    use serde_json::json;

    #[test]
    fn indexed_joins_match_brute_force_for_storeys_arcs_tall_and_attached_walls() {
        let mut s = Session::new();
        s.script(crate::sample::SAMPLE).unwrap();
        s.execute("wall", &json!({"points":[[14000,0],[18000,0]],"arc":90,"height":9000})).unwrap();
        s.execute("wall", &json!({"points":[[0,0],[0,-5000]],"base_offset":-10000,"height":1000000})).unwrap();
        for level in ["Level 1", "Level 2"] {
            s.execute("wall", &json!({"points":[[0,0],[6000,0],[6000,4000]],"level":level})).unwrap();
        }
        agree(&s.model);
        // Walls meeting on the tiles' edges (every 6 m), crossing, and ending on each other, storeys apart.
        agree(&crate::sample::tower::tower_on(crate::sample::tower::Plate::SMALL, 2).unwrap());
    }

    /// Each wall's geometry found through the index is what it is found among every wall.
    fn agree(m: &Model) {
        let index = Index::new(m);
        for (&id, e) in &m.elements {
            if !matches!(e, Element::Wall(_)) {
                continue;
            }
            assert_eq!(walls::centre_line(m, id), walls::centre_line_in(m, id, Some(&index)), "wall {id}");
            for end in [false, true] {
                assert_eq!(walls::cap(m, id, end), walls::cap_in(m, id, end, Some(&index)));
            }
            assert_eq!(walls::outline(m, id), walls::outline_in(m, id, Some(&index)));
            assert_eq!(walls::layer_lines(m, id), walls::layer_lines_in(m, id, Some(&index)));
            for openings in [false, true] {
                let a = walls::prisms(m, id, openings);
                let b = walls::prisms_in(m, id, openings, Some(&index));
                assert_eq!(a, b);
            }
        }
    }
}
