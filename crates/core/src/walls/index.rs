//! Read-only acceleration for one model snapshot. Height buckets never decide geometry:
//! the existing overlap and join rules still test every candidate.
use std::collections::BTreeMap;

use crate::model::{Element, Guid, Id, Model};

const BAND: f64 = 6000.0;
const MAX_BANDS: i64 = 128;
const MAX_REFERENCES: usize = 1_000_000;

pub(crate) struct Index {
    spans: BTreeMap<Id, (f64, f64)>,
    bands: BTreeMap<i64, Vec<Id>>,
    broad: Vec<Id>,
    hosts: BTreeMap<Guid, Vec<Id>>,
}

impl Index {
    pub(crate) fn new(m: &Model) -> Self {
        let mut index = Self { spans: BTreeMap::new(), bands: BTreeMap::new(), broad: Vec::new(), hosts: BTreeMap::new() };
        let mut references = 0;
        for (&id, e) in &m.elements {
            match e {
                Element::Wall(w) => {
                    let (lo, hi) = (m.base_z(w), m.top_z(w));
                    index.spans.insert(id, (lo, hi));
                    let (a, b) = (band(lo), band(hi));
                    let count = b.saturating_sub(a).saturating_add(1);
                    if !(1..=MAX_BANDS).contains(&count) || references + count as usize > MAX_REFERENCES {
                        index.broad.push(id);
                    } else {
                        references += count as usize;
                        for b in a..=b {
                            index.bands.entry(b).or_default().push(id);
                        }
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

    pub(crate) fn neighbours(&self, id: Id) -> Vec<Id> {
        let Some(&(lo, hi)) = self.spans.get(&id) else { return Vec::new() };
        let (a, b) = (band(lo), band(hi));
        if a > b || b.saturating_sub(a) >= MAX_BANDS {
            return self.spans.keys().copied().collect();
        }
        let mut ids = self.broad.clone();
        for (_, found) in self.bands.range(a..=b) {
            ids.extend(found);
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

fn band(z: f64) -> i64 {
    (z / BAND).floor() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{engine::Session, walls};
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
        let index = Index::new(&s.model);
        for (&id, e) in &s.model.elements {
            if !matches!(e, Element::Wall(_)) {
                continue;
            }
            for end in [false, true] {
                assert_eq!(walls::cap(&s.model, id, end), walls::cap_in(&s.model, id, end, Some(&index)));
            }
            assert_eq!(walls::outline(&s.model, id), walls::outline_in(&s.model, id, Some(&index)));
            assert_eq!(walls::layer_lines(&s.model, id), walls::layer_lines_in(&s.model, id, Some(&index)));
            for openings in [false, true] {
                let a = walls::prisms(&s.model, id, openings);
                let b = walls::prisms_in(&s.model, id, openings, Some(&index));
                assert_eq!(a, b);
            }
        }
    }
}
