//! Bounded cross-snapshot geometry reuse for isolated columns and straight runs.
//! Other categories retain the full preparation path until their read sets are tracked.
use super::*;
use crate::bounds::Key;
use crate::render::SectionWork;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default)]
pub struct FragmentStats {
    /// All elements visited by completed preparation chunks, including annotations.
    pub visited: u64,
    /// Uncached component-extrema or other solid preparation, by element.
    pub prepared_uncached: u64,
    pub hits: u64,
    /// New isolated column/run fragments; unsupported geometry is counted above.
    pub built: u64,
    pub evicted: u64,
    pub entries: usize,
    /// Cache ownership, including vector capacities and conservative tree/Arc overhead.
    /// A currently displayed/in-flight SectionWork owns its shared geometry separately.
    pub retained_bytes: usize,
}

struct Fragment {
    solids: Vec<Arc<Solid>>,
    points: Option<Vec<P3>>,
    bounds: Option<[P3; 2]>,
}

impl Fragment {
    fn make(m: &Model, id: Id, e: &Element, components: &mut crate::component_geometry::Components) -> Self {
        // These categories read no wall index, host geometry or spatial neighbours.
        let solids = solids_in_with(m, &[(id, e)], None, components);
        let points = (!solids.is_empty()).then(|| extremes(solids.iter().flat_map(Solid::corners)));
        let bounds = points.as_ref().and_then(|p| super::bounds(p.iter().copied()));
        Self { solids: solids.into_iter().map(Arc::new).collect(), points, bounds }
    }

    fn bytes(&self) -> usize {
        let mut bytes = std::mem::size_of::<Self>() + 64;
        bytes = bytes.saturating_add(self.solids.capacity().saturating_mul(std::mem::size_of::<Arc<Solid>>()));
        if let Some(points) = &self.points {
            bytes = bytes.saturating_add(points.capacity().saturating_mul(std::mem::size_of::<P3>()));
        }
        for prepared in &self.solids {
            bytes = bytes.saturating_add(std::mem::size_of::<Solid>() + 64);
            let heap = match &**prepared {
                Solid::Box { .. } => 0,
                Solid::Slab { face, .. } => face.capacity().saturating_mul(std::mem::size_of::<P3>()),
                Solid::Prism { ring, .. } => ring.capacity().saturating_mul(std::mem::size_of::<P2>()),
                Solid::Mesh { tris, .. } => tris.capacity().saturating_mul(std::mem::size_of::<[P3; 3]>()),
            };
            bytes = bytes.saturating_add(heap);
        }
        bytes
    }

    fn placed(&self, m: &Model, e: &Element) -> impl Iterator<Item = Prepared> + '_ {
        // Appearance is deliberately absent from the geometry key. Material ordinals
        // are resolved afresh too, so adding an earlier texture cannot leave stale ids.
        let paint = m.run_type(e).map(|t| {
            let color = m.materials.get(&t.material).map_or(STRUCTURE, |x| Rgb(x.color[0], x.color[1], x.color[2]));
            (color, skin(m, &t.material))
        });
        self.solids.iter().map(move |solid| Prepared::Shared { geometry: Arc::clone(solid), paint })
    }
}

struct Entry {
    key: Key,
    fragment: Arc<Fragment>,
    used: u64,
    bytes: usize,
}

// Reserve both trees' root capacity even when there is only one retained fragment.
const ROOT_BYTES: usize = 12 * (std::mem::size_of::<Entry>() + std::mem::size_of::<Id>() + std::mem::size_of::<(u64, Id)>()) + 256;

/// Optional section preparation cache. Only column/footing and straight-run
/// fragments are retained currently. Preparation and scene assembly still visit the
/// model; hosted/family/spatial dependencies and desktop adoption remain separate work.
/// The default derived-data budget is 128 MiB, independent of model undo history.
/// Eviction releases cache ownership; completed work keeps its shared geometry alive.
pub struct SectionCache {
    budget: usize,
    clock: u64,
    entries: BTreeMap<Id, Entry>,
    order: BTreeSet<(u64, Id)>,
    stats: FragmentStats,
}

impl Default for SectionCache {
    fn default() -> Self {
        Self::new(128 * 1024 * 1024)
    }
}

impl SectionCache {
    pub fn new(budget: usize) -> Self {
        Self { budget, clock: 0, entries: BTreeMap::new(), order: BTreeSet::new(), stats: FragmentStats::default() }
    }

    pub fn stats(&self) -> FragmentStats {
        self.stats
    }

    pub fn clear(&mut self) {
        self.entries = BTreeMap::new();
        self.order = BTreeSet::new();
        self.stats.entries = 0;
        self.stats.retained_bytes = 0;
    }

    pub fn set_budget(&mut self, budget: usize) {
        self.budget = budget;
        self.trim();
    }

    fn trim(&mut self) {
        while self.stats.retained_bytes > self.budget {
            let Some((_, id)) = self.order.pop_first() else { break };
            if let Some(entry) = self.entries.remove(&id) {
                self.stats.retained_bytes -= entry.bytes;
                self.stats.entries -= 1;
                self.stats.evicted += 1;
                if self.stats.entries == 0 {
                    self.stats.retained_bytes -= ROOT_BYTES;
                }
            }
        }
        if self.entries.is_empty() {
            // Drop even the final empty root nodes; zero budget retains no tree data.
            self.entries = BTreeMap::new();
            self.order = BTreeSet::new();
        }
    }

    fn retain(&mut self, id: Id, key: Key, fragment: Arc<Fragment>) {
        if self.clock == u64::MAX {
            self.clear();
            self.clock = 0;
        }
        self.clock += 1;
        if let Some(old) = self.entries.remove(&id) {
            self.order.remove(&(old.used, id));
            self.stats.retained_bytes -= old.bytes;
            self.stats.entries -= 1;
            if self.stats.entries == 0 {
                self.stats.retained_bytes -= ROOT_BYTES;
            }
        }
        let bytes = fragment.bytes().saturating_add(std::mem::size_of::<Entry>() + 512);
        if bytes.saturating_add(ROOT_BYTES) > self.budget {
            self.trim();
            return;
        }
        self.order.insert((self.clock, id));
        self.entries.insert(id, Entry { key, fragment, used: self.clock, bytes });
        if self.stats.entries == 0 {
            self.stats.retained_bytes += ROOT_BYTES;
        }
        self.stats.entries += 1;
        self.stats.retained_bytes = self.stats.retained_bytes.saturating_add(bytes);
        self.trim();
    }

    pub fn work(&mut self, m: &Model) -> SectionWork {
        self.work_cancellable(m, &|| false).unwrap_or_else(|| SectionWork::new(m))
    }

    pub fn work_cancellable(&mut self, m: &Model, cancelled: &(dyn Fn() -> bool + Sync)) -> Option<SectionWork> {
        if cancelled() {
            return None;
        }
        let elements: Vec<_> = m.elements.iter().map(|(&id, e)| (id, e)).collect();
        let index = walls::Index::new(m);
        if cancelled() {
            return None;
        }
        struct Part {
            made: PreparedMade,
            used: Vec<(Id, Key, Arc<Fragment>, bool)>,
            visited: usize,
            uncached: u64,
        }
        let entries = &self.entries;
        let part = |chunk: &[(Id, &Element)]| -> Option<Part> {
            let mut components = crate::component_geometry::Components::new(m);
            let (mut solids, mut placed, mut boxes, mut used) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
            let mut uncached = 0;
            for &(id, e) in chunk {
                if cancelled() {
                    return None;
                }
                if e.column().is_some() || e.run().is_some() {
                    let key = Key::of(m, id);
                    let (fragment, hit) = match entries.get(&id).filter(|entry| entry.key == key) {
                        Some(entry) => (Arc::clone(&entry.fragment), true),
                        None => (Arc::new(Fragment::make(m, id, e, &mut components)), false),
                    };
                    solids.extend(fragment.placed(m, e));
                    if let Some(points) = &fragment.points {
                        placed.push((id, points.clone()));
                        boxes.push(fragment.bounds);
                    }
                    used.push((id, key, fragment, hit));
                } else if let Element::Component(c) = e {
                    uncached += 1;
                    let points = components.extremes(c);
                    boxes.push(super::bounds(points.iter().copied()));
                    placed.push((id, points));
                } else {
                    let own = solids_in_with(m, &[(id, e)], Some(&index), &mut components);
                    if !own.is_empty() {
                        uncached += 1;
                        let points = extremes(own.iter().flat_map(Solid::corners));
                        boxes.push(super::bounds(points.iter().copied()));
                        placed.push((id, points));
                    }
                    solids.extend(own.into_iter().map(Prepared::from));
                }
            }
            Some(Part { made: (solids, placed, boxes), used, visited: chunk.len(), uncached })
        };
        let parts = match (elements.len() >= 1000).then(render_pool).flatten() {
            Some(pool) => {
                use rayon::prelude::*;
                pool.install(|| elements.par_chunks(256).map(part).collect::<Option<Vec<_>>>())?
            }
            None => vec![part(&elements)?],
        };
        if cancelled() {
            return None;
        }
        let (mut solids, mut placed, mut boxes) = (Vec::new(), Vec::new(), Vec::new());
        for Part { made: (s, p, b), used, visited, uncached } in parts {
            if cancelled() {
                return None;
            }
            solids.extend(s);
            placed.extend(p);
            boxes.extend(b);
            self.stats.visited += visited as u64;
            self.stats.prepared_uncached += uncached;
            for (id, key, fragment, hit) in used {
                if hit {
                    self.stats.hits += 1
                } else {
                    self.stats.built += 1
                }
                self.retain(id, key, fragment);
            }
        }
        SectionWork::from_prepared_cancellable(m, (solids, placed, boxes), cancelled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Session;
    use serde_json::json;

    #[test]
    fn paint_and_texture_ordinals_refresh_without_rebuilding_retained_run_geometry() {
        let mut s = Session::new();
        s.execute("material", &json!({"name":"Probe", "texture":{"kind":"brick"}})).unwrap();
        let id = s.execute("pipe", &json!({"points":[[0,0],[3000,1000]]})).unwrap()["ids"][0].as_u64().unwrap();
        let name = s.model.elements.get(id).unwrap().run().unwrap().type_name.clone();
        s.model_mut().types.runs.get_mut("pipe").unwrap().get_mut(&name).unwrap().material = "Probe".into();
        let mut cache = SectionCache::default();
        cache.work(&s.model);
        let before = Arc::clone(&cache.entries[&id].fragment);
        let mut material = s.model.materials["Probe"].clone();
        material.color = [12, 34, 56];
        material.transparency = 40;
        s.model_mut().materials.insert("Probe".into(), material.clone());
        s.model_mut().materials.insert("AAA earlier texture".into(), material);
        cache.work(&s.model);
        assert_eq!(cache.stats().built, 1);
        assert!(Arc::ptr_eq(&before, &cache.entries[&id].fragment));
        let m = &s.model;
        let e = m.elements.get(id).unwrap();
        let mut retained = Vec::new();
        for solid in cache.entries[&id].fragment.placed(m, e) {
            solid.painted().tris(&mut retained);
        }
        let mut fresh = Vec::new();
        for solid in solids_in_with(m, &[(id, e)], None, &mut crate::component_geometry::Components::new(m)) {
            solid.tris(&mut fresh);
        }
        assert_eq!(retained, fresh, "geometry, colour, transparency and shifted texture ids");
    }

    #[test]
    fn eviction_releases_cache_geometry_after_the_last_completed_work_is_dropped() {
        let mut s = Session::new();
        let id = s.execute("column", &json!({"at":[0,0]})).unwrap()["id"].as_u64().unwrap();
        let mut cache = SectionCache::default();
        let work = cache.work(&s.model);
        let geometry = Arc::downgrade(&cache.entries[&id].fragment.solids[0]);
        cache.set_budget(0);
        assert_eq!(cache.stats().entries, 0);
        assert!(geometry.upgrade().is_some(), "completed work keeps its geometry valid");
        drop(work);
        assert!(geometry.upgrade().is_none(), "evicted geometry must not survive the last work owner");
    }
}
