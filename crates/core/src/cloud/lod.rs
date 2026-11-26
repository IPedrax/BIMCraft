//! A scan's index opened for views ([`Indexed`]): which of its nodes a view shows ([`Indexed::select`]), each at the detail its
//! points show at on the screen, and the nodes read, held in a cache as big as the machine's free memory allows
//! ([`memory`]). A project holds the levels from the root down ([`IndexRef::levels`]); a view reads the ones below.

use std::collections::{BinaryHeap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use super::index::Index;
use super::source::NodeSource;

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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
    /// The files it was built from, in full: what Rebuild Index reads again.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<String>,
    /// Millimetres to their unit, as they were read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
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
        if self.sources.len() > super::MAX_STATIONS || self.sources.iter().any(|s| s.len() > 4096) {
            return Err("a scan's index names too many files, or one too long".into());
        }
        if self.scale.is_some_and(|k| !(k.is_finite() && k > 0.0 && k <= 1e6)) {
            return Err("a scan's index's unit isn't one".into());
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

/// A node of an index open: the index's number ([`Indexed::uid`]) and the node's.
type Key = (u64, usize);

/// The nodes every open index has read, by when they were last asked for, held together to [`memory`]: past it, the node
/// asked for longest ago goes, whichever index it's of. And the nodes being read, so a node two threads ask for is read once.
#[derive(Default)]
struct Cache {
    nodes: HashMap<Key, (Arc<Points>, u64)>,
    bytes: u64,
    tick: u64,
    reading: HashMap<Key, Arc<OnceLock<Option<Arc<Points>>>>>,
}

static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(Mutex::default);
/// The number the next index opened gets.
static NEXT: AtomicU64 = AtomicU64::new(1);

impl Cache {
    fn touch(&mut self, k: Key) -> Option<Arc<Points>> {
        self.tick += 1;
        let tick = self.tick;
        let (p, at) = self.nodes.get_mut(&k)?;
        *at = tick;
        Some(Arc::clone(p))
    }

    /// Node `k` read, kept, and what was asked for longest ago let go while they all take more than [`memory`].
    fn keep(&mut self, k: Key, p: &Arc<Points>) {
        self.tick += 1;
        let tick = self.tick;
        if let Some((old, _)) = self.nodes.insert(k, (Arc::clone(p), tick)) {
            self.bytes = self.bytes.saturating_sub(old.bytes);
        }
        self.bytes += p.bytes;
        let most = memory();
        if self.bytes > most {
            let mut by: Vec<(u64, Key, u64)> = self.nodes.iter().map(|(&k, (p, t))| (*t, k, p.bytes)).collect();
            by.sort_unstable();
            for (_, old, b) in by {
                if self.bytes <= most - most / 8 {
                    break;
                }
                if old != k {
                    self.nodes.remove(&old);
                    self.bytes = self.bytes.saturating_sub(b);
                }
            }
        }
    }
}

/// A scan's index, open: where its nodes come from ([`NodeSource`]), how many have been read, which a plan drawn from them
/// notes, and which couldn't be.
pub struct Indexed {
    source: Box<dyn NodeSource>,
    uid: u64,
    path: PathBuf,
    levels: u8,
    failed: Mutex<HashSet<usize>>,
    loads: AtomicU64,
}

impl std::fmt::Debug for Indexed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Indexed({}, {:?})", self.path.display(), self.source)
    }
}

impl Drop for Indexed {
    fn drop(&mut self) {
        if let Ok(mut c) = CACHE.lock() {
            let uid = self.uid;
            let gone: u64 = c.nodes.iter().filter(|((u, _), _)| *u == uid).map(|(_, (p, _))| p.bytes).sum();
            c.nodes.retain(|(u, _), _| *u != uid);
            c.bytes = c.bytes.saturating_sub(gone);
        }
    }
}

impl Indexed {
    /// The index at `path`, if it's the one `r` names.
    pub fn open(path: &Path, r: &IndexRef) -> Result<Indexed, String> {
        Indexed::of(Box::new(Index::open(path)?), path, r)
    }

    /// The nodes `source` keeps, found at `path`, if they're the index `r` names.
    pub fn of(source: Box<dyn NodeSource>, path: &Path, r: &IndexRef) -> Result<Indexed, String> {
        if IndexRef::id_of(source.id()) != r.id {
            return Err(format!("{} is another scan's index, or one built again since", path.display()));
        }
        Ok(Indexed {
            source,
            uid: NEXT.fetch_add(1, Ordering::Relaxed),
            path: path.to_path_buf(),
            levels: r.levels,
            failed: Mutex::default(),
            loads: AtomicU64::new(0),
        })
    }

    /// Where its nodes come from.
    pub fn source(&self) -> &dyn NodeSource {
        self.source.as_ref()
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
        CACHE.lock().ok()?.touch((self.uid, i))
    }

    /// Whether node `i` is one that couldn't be read, or has been: nothing to read.
    pub fn settled(&self, i: usize) -> bool {
        CACHE.lock().is_ok_and(|c| c.nodes.contains_key(&(self.uid, i))) || self.failed.lock().is_ok_and(|f| f.contains(&i))
    }

    /// Node `i`'s points, read now if they haven't been; none when they can't be. A node another thread is reading is waited
    /// for, not read again.
    pub fn fetch(&self, i: usize) -> Option<Arc<Points>> {
        let k = (self.uid, i);
        let slot = {
            let mut c = CACHE.lock().ok()?;
            if let Some(p) = c.touch(k) {
                return Some(p);
            }
            if self.failed.lock().ok()?.contains(&i) {
                return None;
            }
            Arc::clone(c.reading.entry(k).or_default())
        };
        let read = slot
            .get_or_init(|| {
                let p = self.source.load(i).ok()?;
                let bytes = p.bytes() as u64;
                self.loads.fetch_add(1, Ordering::Relaxed);
                Some(Arc::new(Points {
                    points: Arc::new(p.points),
                    colours: p.colours.map(Arc::new),
                    intensity: p.intensity.map(Arc::new),
                    bytes,
                }))
            })
            .clone();
        let mut c = CACHE.lock().ok()?;
        if c.reading.get(&k).is_some_and(|s| Arc::ptr_eq(s, &slot)) {
            c.reading.remove(&k);
            match &read {
                Some(p) => c.keep(k, p),
                None => {
                    if let Ok(mut f) = self.failed.lock() {
                        f.insert(i);
                    }
                }
            }
        }
        read
    }

    /// What the cache holds of it, bytes.
    pub fn held(&self) -> u64 {
        CACHE.lock().map_or(0, |c| c.nodes.iter().filter(|((u, _), _)| *u == self.uid).map(|(_, (p, _))| p.bytes).sum())
    }

    /// [`select`] of this index alone: its nodes a view shows, most needed first.
    pub fn select(&self, seen: &dyn Fn([f64; 3], [f64; 3]) -> Option<f64>, most: usize) -> Vec<usize> {
        select(&[(self, seen)], most).into_iter().map(|(_, i)| i).collect()
    }
}

/// What the cache of every index holds together, bytes.
pub fn held() -> u64 {
    CACHE.lock().map_or(0, |c| c.bytes)
}

/// Of the indexes `scans`, the nodes below each one's project levels a view shows, most needed first across all of them,
/// until they hold `most` points: as `(scan, node)`, `scan` its place in `scans`. Each scan's `seen` says of a node's box (its
/// lowest and highest corner from the scan's middle, mm) how many pixels a millimetre takes on the screen there, or none
/// where the view doesn't show it. A node is read below one whose points are more than [`SPREAD`] px apart, and the nodes
/// are taken by how far apart their own points are on the screen, roots too, so the scan the view is near gets the detail.
pub fn select(scans: &[(&Indexed, &dyn Fn([f64; 3], [f64; 3]) -> Option<f64>)], most: usize) -> Vec<(usize, usize)> {
    #[derive(PartialEq)]
    struct Next(f64, usize, usize);
    impl Eq for Next {}
    impl PartialOrd for Next {
        fn partial_cmp(&self, o: &Next) -> Option<std::cmp::Ordering> {
            Some(self.cmp(o))
        }
    }
    impl Ord for Next {
        fn cmp(&self, o: &Next) -> std::cmp::Ordering {
            self.0.total_cmp(&o.0).then(o.1.cmp(&self.1)).then(o.2.cmp(&self.2))
        }
    }
    // A node in the queue by how far apart its points are on the screen: none where the view doesn't show it.
    let spread = |s: usize, i: usize| -> Option<f64> {
        let (ix, seen) = scans[s];
        let (level, _) = ix.source.node(i)?;
        let (lo, edge) = ix.source.cube(i);
        let px = seen(lo, lo.map(|v| v + edge)).filter(|px| px.is_finite() && *px > 0.0)?;
        Some(ix.source.spacing(level) * px)
    };
    let mut out = Vec::new();
    let mut heap: BinaryHeap<Next> =
        (0..scans.len()).filter(|&s| !scans[s].0.source.is_empty()).filter_map(|s| spread(s, 0).map(|p| Next(p, s, 0))).collect();
    let mut held = 0_usize;
    while let Some(Next(apart, s, i)) = heap.pop() {
        let ix = scans[s].0;
        let Some((level, points)) = ix.source.node(i) else { continue };
        if level >= ix.levels && points > 0 {
            if held + points as usize > most && !out.is_empty() {
                break;
            }
            held += points as usize;
            out.push((s, i));
        }
        if apart > SPREAD {
            for c in ix.source.children(i) {
                if let Some(p) = spread(s, c) {
                    heap.push(Next(p, s, c));
                }
            }
        }
    }
    out
}

/// Where a scan's index is: `path` as the project holds it, from the project's folder when it's relative.
pub fn resolve(path: &str, project: Option<&Path>) -> PathBuf {
    let p = PathBuf::from(path);
    match project.and_then(Path::parent) {
        Some(dir) if p.is_relative() => dir.join(p),
        _ => p,
    }
}

/// Where to look for the index `r` names, in order: where the project holds it (from the folder of the project at `project`),
/// where it was built, and in the folder `cache` by its file's name.
pub fn places(r: &IndexRef, project: Option<&Path>, cache: Option<&Path>) -> Vec<PathBuf> {
    let mut out = vec![resolve(&r.path, project)];
    out.extend(r.full.as_deref().map(PathBuf::from));
    if let (Some(dir), Some(name)) = (cache, Path::new(&r.path).file_name()) {
        out.push(dir.join(name));
    }
    out.dedup();
    out
}

/// `index` as a project at `project` holds it: from the project's folder when it's in it.
pub fn relative(index: &Path, project: Option<&Path>) -> String {
    project
        .and_then(Path::parent)
        .and_then(|dir| index.strip_prefix(dir).ok())
        .map_or_else(|| index.to_string_lossy().into_owned(), |p| p.to_string_lossy().replace('\\', "/"))
}

/// The tests that set what the cache may take, or that count what it holds, one at a time: the cache is every index's.
#[cfg(test)]
pub(crate) static SERIAL: Mutex<()> = Mutex::new(());

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
        let r = IndexRef {
            path: out.to_string_lossy().into_owned(),
            full: None,
            id: IndexRef::id_of(b.id),
            count: b.count,
            levels: b.levels,
            sources: Vec::new(),
            scale: None,
        };
        (out, r)
    }

    /// Node `i`'s level and points.
    fn node(ix: &Indexed, i: usize) -> (u8, u32) {
        ix.source().node(i).unwrap()
    }

    #[test]
    fn nearer_reads_deeper_and_within_the_points_asked() {
        let (path, r) = built("select", 60_000);
        let ix = Indexed::open(&path, &r).unwrap();
        let deepest = |px: f64, most: usize| {
            let s = ix.select(&|_, _| Some(px), most);
            let held: usize = s.iter().map(|&i| node(&ix, i).1 as usize).sum();
            (s.iter().map(|&i| node(&ix, i).0).max().unwrap_or(0), held, s)
        };
        let (far, ..) = deepest(0.01, usize::MAX);
        let (near, held, s) = deepest(10.0, usize::MAX);
        assert!(near > far, "{near} {far}");
        let top: u64 = (0..ix.source().len()).map(|i| node(&ix, i)).filter(|n| n.0 < r.levels).map(|n| u64::from(n.1)).sum();
        assert_eq!(held as u64 + top, r.count);
        assert!(s.iter().all(|&i| node(&ix, i).0 >= r.levels));
        let (_, capped, _) = deepest(10.0, 5_000);
        assert!(capped <= 5_000 + crate::cloud::index::MAX_NODE_POINTS as usize && capped < held);
        // Nothing the view doesn't show.
        assert!(ix.select(&|_, _| None, usize::MAX).is_empty());
        // Only what's in a corner of it.
        let s = ix.select(&|lo, _| (lo[0] < -6000.0 && lo[1] < -3000.0).then_some(10.0), usize::MAX);
        assert!(!s.is_empty());
        assert!(s.iter().all(|&i| ix.source().cube(i).0[0] < -6000.0));
        // The same view picks the same nodes in the same order, every time.
        assert_eq!(ix.select(&|_, _| Some(3.0), 20_000), ix.select(&|_, _| Some(3.0), 20_000));
    }

    #[test]
    fn one_budget_goes_to_the_scan_the_view_is_near_not_shared_out_evenly() {
        let (a_path, a) = built("near-a", 60_000);
        let (b_path, b) = built("near-b", 60_000);
        let (ia, ib) = (Indexed::open(&a_path, &a).unwrap(), Indexed::open(&b_path, &b).unwrap());
        let near = |_: [f64; 3], _: [f64; 3]| Some(10.0);
        let far = |_: [f64; 3], _: [f64; 3]| Some(0.02);
        let got = select(&[(&ia, &near), (&ib, &far)], 30_000);
        let of = |s: usize| got.iter().filter(|(k, _)| *k == s).map(|&(k, i)| node([&ia, &ib][k], i).1 as usize).sum::<usize>();
        assert!(of(0) > 10 * of(1).max(1), "near {} far {}", of(0), of(1));
        assert!(of(0) + of(1) <= 30_000 + crate::cloud::index::MAX_NODE_POINTS as usize);
        // The most needed come first, whichever scan they're of.
        assert!(got.iter().take(3).all(|(k, _)| *k == 0) || got.iter().all(|(k, _)| *k == 0), "{got:?}");
    }

    #[test]
    fn a_node_several_threads_ask_for_is_read_once() {
        let _serial = SERIAL.lock();
        let (path, r) = built("threads", 60_000);
        let ix = Indexed::open(&path, &r).unwrap();
        let some = ix.select(&|_, _| Some(100.0), usize::MAX);
        let got: Vec<Arc<Points>> = std::thread::scope(|t| {
            (0..8).map(|_| t.spawn(|| ix.fetch(some[0]).unwrap())).collect::<Vec<_>>().into_iter().map(|h| h.join().unwrap()).collect()
        });
        assert_eq!(ix.loads(), 1);
        assert!(got.windows(2).all(|w| Arc::ptr_eq(&w[0], &w[1])));
    }

    #[test]
    fn the_cache_holds_no_more_than_it_may_and_lets_go_of_what_any_scan_asked_for_longest_ago() {
        let _serial = SERIAL.lock();
        let (a_path, a) = built("cache-a", 60_000);
        let (b_path, b) = built("cache-b", 60_000);
        let (ia, ib) = (Indexed::open(&a_path, &a).unwrap(), Indexed::open(&b_path, &b).unwrap());
        let (sa, sb) = (ia.select(&|_, _| Some(100.0), usize::MAX), ib.select(&|_, _| Some(100.0), usize::MAX));
        let x = ia.fetch(sa[0]).unwrap();
        assert!(Arc::ptr_eq(&x, &ia.fetch(sa[0]).unwrap()));
        assert_eq!(ia.loads(), 1);
        // Room for about a third of a scan's nodes: A's fill it, then B's, which push A's out rather than their own.
        let one: u64 = sa.iter().map(|&i| ia.fetch(i).map_or(0, |p| p.bytes)).sum();
        set_memory(Some(one / 3));
        for &i in &sa {
            ia.fetch(i);
        }
        for &i in &sb {
            ib.fetch(i);
        }
        assert!(held() <= memory(), "{} > {}", held(), memory());
        assert!(ib.held() > ia.held(), "B kept {} and A {}", ib.held(), ia.held());
        assert!(ib.cached(*sb.last().unwrap()).is_some(), "the last B read is kept");
        set_memory(None);
        // Closed, an index's nodes go.
        drop(ib);
        assert!(held() >= ia.held());
        // Another scan's index isn't taken for it.
        let other = IndexRef { id: IndexRef::id_of(1), ..a };
        assert!(Indexed::open(&a_path, &other).unwrap_err().contains("another scan's index"));
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
