//! A scan's index opened for views ([`Indexed`]): which of its nodes a view shows ([`Indexed::select`]), each at the detail its
//! points show at on the screen, and the nodes read, held in a cache as big as the machine's free memory allows
//! ([`memory`]). A project holds the levels from the root down ([`IndexRef::levels`]); a view reads the ones below.

use std::collections::{BinaryHeap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use super::index::Index;

/// A node is read below one whose points lie farther apart than this on the screen, px.
pub const SPREAD: f64 = 1.5;
/// The most points of the index a plan or an export draws of one scan.
pub const DRAWN: usize = 5_000_000;
/// The least and most the cache takes when it's sized from the memory that's free, bytes.
const LEAST: u64 = 256 << 20;
const MOST: u64 = 8 << 30;
/// What the cache takes where nothing says what's free (the CLI, MCP, tests), bytes.
const FIXED: u64 = 1 << 30;

/// What a project holds of a scan's index: where it is, which index it is, and the levels the project holds itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexRef {
    /// Its file, relative to the project's folder where it's in it.
    pub path: String,
    /// Its file where it was built, in full: where it's looked for when the project has moved from beside it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full: Option<String>,
    /// The index's id, as 32 hex digits: its header holds the same.
    pub id: String,
    /// The points it holds.
    pub count: u64,
    /// The levels from the root a project holds whole: 0 when it holds a thinned root.
    pub levels: u8,
}

impl IndexRef {
    pub fn id_of(id: u128) -> String {
        format!("{id:032x}")
    }

    /// What's wrong with it, if anything.
    pub fn check(&self) -> Result<(), String> {
        if self.path.trim().is_empty() || self.path.len() > 4096 || self.full.as_ref().is_some_and(|f| f.len() > 4096) {
            return Err("a scan's index has no path, or one too long".into());
        }
        if self.id.len() != 32 || u128::from_str_radix(&self.id, 16).is_err() {
            return Err(format!("a scan's index id {:?} isn't 32 hex digits", self.id));
        }
        if self.count > super::index::MAX_COUNT || self.levels > super::index::MAX_DEPTH + 1 {
            return Err("a scan's index says more than an index holds".into());
        }
        Ok(())
    }
}

/// What the app says the cache may take, bytes: 0 for what's free.
static MEMORY: AtomicU64 = AtomicU64::new(0);

/// Sets what the cache of every index may take, bytes; none for a share of what's free.
pub fn set_memory(bytes: Option<u64>) {
    MEMORY.store(bytes.unwrap_or(0), Ordering::Relaxed);
}

/// What the cache of every index may take, bytes: what the app set, or a quarter of the memory that's free (between
/// [`LEAST`] and [`MOST`]), halved while the machine swaps.
pub fn memory() -> u64 {
    let set = MEMORY.load(Ordering::Relaxed);
    if set > 0 {
        return set;
    }
    let Some(free) = crate::memory::free() else { return FIXED };
    let share = (free / 4).clamp(LEAST, MOST);
    match crate::memory::pressure() {
        Some(p) if p.total > 0 && p.available < p.total / 10 => (share / 2).max(LEAST / 2),
        _ => share,
    }
}

/// A node's points as views share them.
#[derive(Debug)]
pub struct Points {
    pub points: Arc<Vec<[f32; 3]>>,
    pub colours: Option<Arc<Vec<[u8; 3]>>>,
    pub intensity: Option<Arc<Vec<u8>>>,
    bytes: u64,
}

/// The nodes read, by when they were last asked for.
#[derive(Default)]
struct Cache {
    nodes: HashMap<usize, (Arc<Points>, u64)>,
    bytes: u64,
    tick: u64,
    /// Nodes that couldn't be read: not tried again.
    failed: HashSet<usize>,
}

/// Every index open, for their caches to share what they may take.
static HELD: AtomicU64 = AtomicU64::new(0);

/// A scan's index, open: its nodes, those read, and how many have been read, which a plan drawn from them notes.
pub struct Indexed {
    pub index: Index,
    path: PathBuf,
    levels: u8,
    cache: Mutex<Cache>,
    loads: AtomicU64,
}

impl std::fmt::Debug for Indexed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Indexed({}, {:?})", self.path.display(), self.index)
    }
}

impl Drop for Indexed {
    fn drop(&mut self) {
        if let Ok(c) = self.cache.lock() {
            HELD.fetch_sub(c.bytes.min(HELD.load(Ordering::Relaxed)), Ordering::Relaxed);
        }
    }
}

impl Indexed {
    /// The index at `path`, if it's the one `r` names.
    pub fn open(path: &Path, r: &IndexRef) -> Result<Indexed, String> {
        let index = Index::open(path)?;
        if IndexRef::id_of(index.header.id) != r.id {
            return Err(format!("{} is another scan's index, or one built again since", path.display()));
        }
        Ok(Indexed { index, path: path.to_path_buf(), levels: r.levels, cache: Mutex::default(), loads: AtomicU64::new(0) })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The levels the project holds itself: a view reads from the one below.
    pub fn levels(&self) -> u8 {
        self.levels
    }

    /// How many nodes have been read: what a picture made from the cache was made at.
    pub fn loads(&self) -> u64 {
        self.loads.load(Ordering::Relaxed)
    }

    /// Node `i`'s points if they've been read.
    pub fn cached(&self, i: usize) -> Option<Arc<Points>> {
        let mut c = self.cache.lock().ok()?;
        c.tick += 1;
        let tick = c.tick;
        let (p, at) = c.nodes.get_mut(&i)?;
        *at = tick;
        Some(Arc::clone(p))
    }

    /// Whether node `i` is one that couldn't be read, or has been: nothing to read.
    pub fn settled(&self, i: usize) -> bool {
        self.cache.lock().is_ok_and(|c| c.nodes.contains_key(&i) || c.failed.contains(&i))
    }

    /// Node `i`'s points, read now if they haven't been; none when they can't be.
    pub fn fetch(&self, i: usize) -> Option<Arc<Points>> {
        if let Some(p) = self.cached(i) {
            return Some(p);
        }
        if self.cache.lock().ok()?.failed.contains(&i) {
            return None;
        }
        let read = self.index.load(i);
        let mut c = self.cache.lock().ok()?;
        let Ok(p) = read else {
            c.failed.insert(i);
            return None;
        };
        let bytes = p.bytes() as u64;
        let p =
            Arc::new(Points { points: Arc::new(p.points), colours: p.colours.map(Arc::new), intensity: p.intensity.map(Arc::new), bytes });
        c.tick += 1;
        let tick = c.tick;
        if let Some((old, _)) = c.nodes.insert(i, (Arc::clone(&p), tick)) {
            c.bytes = c.bytes.saturating_sub(old.bytes);
            HELD.fetch_sub(old.bytes.min(HELD.load(Ordering::Relaxed)), Ordering::Relaxed);
        }
        c.bytes += bytes;
        HELD.fetch_add(bytes, Ordering::Relaxed);
        self.loads.fetch_add(1, Ordering::Relaxed);
        // Over what every index may take: what was asked for longest ago goes, of this one's.
        let most = memory();
        if HELD.load(Ordering::Relaxed) > most {
            let mut by: Vec<(u64, usize, u64)> = c.nodes.iter().map(|(&k, (p, t))| (*t, k, p.bytes)).collect();
            by.sort_unstable();
            for (_, k, b) in by {
                if HELD.load(Ordering::Relaxed) <= most - most / 8 || k == i {
                    break;
                }
                c.nodes.remove(&k);
                c.bytes = c.bytes.saturating_sub(b);
                HELD.fetch_sub(b.min(HELD.load(Ordering::Relaxed)), Ordering::Relaxed);
            }
        }
        Some(p)
    }

    /// What the cache holds, bytes.
    pub fn held(&self) -> u64 {
        self.cache.lock().map_or(0, |c| c.bytes)
    }

    /// The nodes below the project's levels a view shows, most needed first, until they hold `most` points. `seen` says of a
    /// node's box (its lowest and highest corner from the scan's middle, mm) how many pixels a millimetre takes on the screen
    /// there, or none where the view doesn't show it. A node is read below one whose points are more than [`SPREAD`] px apart.
    pub fn select(&self, seen: &dyn Fn([f64; 3], [f64; 3]) -> Option<f64>, most: usize) -> Vec<usize> {
        #[derive(PartialEq)]
        struct Next(f64, usize);
        impl Eq for Next {}
        impl PartialOrd for Next {
            fn partial_cmp(&self, o: &Next) -> Option<std::cmp::Ordering> {
                Some(self.cmp(o))
            }
        }
        impl Ord for Next {
            fn cmp(&self, o: &Next) -> std::cmp::Ordering {
                self.0.total_cmp(&o.0).then(o.1.cmp(&self.1))
            }
        }
        let mut out = Vec::new();
        let mut heap = BinaryHeap::from([Next(f64::INFINITY, self.index.root())]);
        let mut held = 0_usize;
        while let Some(Next(_, i)) = heap.pop() {
            let Some(n) = self.index.nodes.get(i) else { continue };
            let (lo, edge) = self.index.cube(i);
            let Some(px) = seen(lo, lo.map(|v| v + edge)).filter(|px| px.is_finite() && *px > 0.0) else { continue };
            if n.level >= self.levels && n.points > 0 {
                if held + n.points as usize > most && !out.is_empty() {
                    break;
                }
                held += n.points as usize;
                out.push(i);
            }
            let spread = self.index.spacing(n.level) * px;
            if spread > SPREAD {
                for c in self.index.children(i) {
                    heap.push(Next(spread / 2.0, c));
                }
            }
        }
        out
    }
}

/// Where a scan's index is: `path` as the project holds it, from the project's folder when it's relative.
pub fn resolve(path: &str, project: Option<&Path>) -> PathBuf {
    let p = PathBuf::from(path);
    match project.and_then(Path::parent) {
        Some(dir) if p.is_relative() => dir.join(p),
        _ => p,
    }
}

/// `index` as a project at `project` holds it: from the project's folder when it's in it.
pub fn relative(index: &Path, project: Option<&Path>) -> String {
    project
        .and_then(Path::parent)
        .and_then(|dir| index.strip_prefix(dir).ok())
        .map_or_else(|| index.to_string_lossy().into_owned(), |p| p.to_string_lossy().replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::Format;
    use crate::cloud::index::{Input, Tuning, build_tuned};

    pub(crate) fn built(name: &str, n: usize) -> (PathBuf, IndexRef) {
        let dir = std::env::temp_dir().join(format!("bimcraft-lod-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("scan.bimpc");
        let mut s = String::new();
        for i in 0..n {
            let (x, y) = ((i % 300) as f64 * 0.05, (i / 300) as f64 * 0.05);
            s += &format!("{x:.3} {y:.3} 0\n");
        }
        let input = Input { name: "plane.xyz".into(), src: Box::new(std::io::Cursor::new(s.into_bytes())), format: Format::Xyz };
        let b = build_tuned(vec![input], 1000.0, &out, 2_000, None, None, Tuning { bucket: 1_000_000, leaf: 500 }).unwrap();
        let r =
            IndexRef { path: out.to_string_lossy().into_owned(), full: None, id: IndexRef::id_of(b.id), count: b.count, levels: b.levels };
        (out, r)
    }

    #[test]
    fn nearer_reads_deeper_and_within_the_points_asked() {
        let (path, r) = built("select", 60_000);
        let ix = Indexed::open(&path, &r).unwrap();
        let deepest = |px: f64, most: usize| {
            let s = ix.select(&|_, _| Some(px), most);
            let held: usize = s.iter().map(|&i| ix.index.nodes[i].points as usize).sum();
            (s.iter().map(|&i| ix.index.nodes[i].level).max().unwrap_or(0), held, s)
        };
        let (far, ..) = deepest(0.01, usize::MAX);
        let (near, held, s) = deepest(10.0, usize::MAX);
        assert!(near > far, "{near} {far}");
        assert_eq!(held as u64 + ix.index.nodes.iter().filter(|n| n.level < r.levels).map(|n| u64::from(n.points)).sum::<u64>(), r.count);
        assert!(s.iter().all(|&i| ix.index.nodes[i].level >= r.levels));
        let (_, capped, _) = deepest(10.0, 5_000);
        assert!(capped <= 5_000 + crate::cloud::index::MAX_NODE_POINTS as usize && capped < held);
        // Nothing the view doesn't show.
        assert!(ix.select(&|_, _| None, usize::MAX).is_empty());
        // Only what's in a corner of it.
        let s = ix.select(&|lo, _| (lo[0] < -6000.0 && lo[1] < -3000.0).then_some(10.0), usize::MAX);
        assert!(!s.is_empty());
        assert!(s.iter().all(|&i| ix.index.cube(i).0[0] < -6000.0));
    }

    #[test]
    fn the_cache_reads_a_node_once_and_holds_no_more_than_it_may() {
        let (path, r) = built("cache", 60_000);
        let ix = Indexed::open(&path, &r).unwrap();
        let some = ix.select(&|_, _| Some(100.0), usize::MAX);
        let a = ix.fetch(some[0]).unwrap();
        assert_eq!(ix.loads(), 1);
        let b = ix.fetch(some[0]).unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(ix.loads(), 1);
        set_memory(Some(ix.held() * 3));
        for &i in &some {
            ix.fetch(i);
        }
        assert!(ix.held() <= memory(), "{} > {}", ix.held(), memory());
        set_memory(None);
        // Another scan's index isn't taken for it.
        let other = IndexRef { id: IndexRef::id_of(1), ..r };
        assert!(Indexed::open(&path, &other).unwrap_err().contains("another scan's index"));
    }

    #[test]
    fn a_path_is_kept_from_the_project_folder() {
        let project = Path::new("/work/house.bimc");
        assert_eq!(relative(Path::new("/work/house_clouds/a.bimpc"), Some(project)), "house_clouds/a.bimpc");
        assert_eq!(resolve("house_clouds/a.bimpc", Some(project)), Path::new("/work/house_clouds/a.bimpc"));
        assert_eq!(relative(Path::new("/else/a.bimpc"), Some(project)), "/else/a.bimpc");
        assert_eq!(resolve("/else/a.bimpc", Some(project)), Path::new("/else/a.bimpc"));
    }
}
