//! The nodes of scans' indexes streamed into a 3D view, as Potree streams them: each frame the camera, the model or the look
//! changed, the nodes the picture wants are ranked again across every scan (`view3d::scan_wanted`); a few loader threads read
//! the top of that queue (`Pool`), and what the camera moved away from before a loader took it is dropped from the queue
//! (each `Pool::ask` is a generation of its own). On the card, a loader also packs the node for it (`view3d::node_dots`), and
//! a few nodes a frame go up (`FRAME_BYTES`) to stay there in a cache by node (`gpu::Gpu::upload_nodes`, its budget
//! `Resident`); the view draws what the card holds of what it wants, and keeps refining as the camera moves. On the processor,
//! a loader reads the node into the indexes' own cache and the picture draws what that holds (`scan_detail` without reading).

use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::Hash;
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::{Duration, Instant};

use bimcraft_core::graphics::Look;
use bimcraft_core::model::{Id, Model};
use bimcraft_core::view3d::gpu::Mesh;
use bimcraft_core::view3d::{self, Camera, NodeDots, Want};

/// A node of an index open ([`Want::key`]).
pub(crate) type Key = (u64, usize);

/// How many threads read nodes for a view.
const LOADERS: usize = 3;
/// The most bytes of nodes a frame sends to the card: a frame that sends more stutters.
pub(crate) const FRAME_BYTES: u64 = 16 << 20;
/// How often the processor's picture is drawn again while nodes come in: it draws the whole model each time.
const PICTURE_EVERY: Duration = Duration::from_millis(250);

/// A loader's piece of work: what it is of (a queue holds one of each), and doing it.
pub(crate) trait Job: Send + 'static {
    type Key: Clone + Eq + Hash + Send + 'static;
    type Out: Send + 'static;
    fn key(&self) -> Self::Key;
    fn run(self) -> Self::Out;
}

/// What the loaders share: the queue, most needed first; what they are doing; and what they have done (none where the work
/// failed), not yet taken.
struct State<J: Job> {
    queue: VecDeque<J>,
    running: HashSet<J::Key>,
    done: Vec<(J::Key, Option<J::Out>)>,
    stop: bool,
}

struct Shared<J: Job> {
    state: Mutex<State<J>>,
    more: Condvar,
    /// Said each time a piece of work is done: the window draws again.
    wake: Box<dyn Fn() + Send + Sync>,
}

/// A few threads doing the top of a queue that is asked afresh as a view moves. None at all (`threads` 0) does the work where
/// it is taken, as a test wants it.
pub(crate) struct Pool<J: Job> {
    shared: Arc<Shared<J>>,
    threads: usize,
}

impl<J: Job> Pool<J> {
    /// `threads` loaders, which call `wake` each time they have done something; where none can be started, the work is done
    /// where it is taken.
    pub(crate) fn new(threads: usize, wake: impl Fn() + Send + Sync + 'static) -> Pool<J> {
        let shared = Arc::new(Shared {
            state: Mutex::new(State { queue: VecDeque::new(), running: HashSet::new(), done: Vec::new(), stop: false }),
            more: Condvar::new(),
            wake: Box::new(wake),
        });
        let mut started = 0;
        for n in 0..threads {
            let shared = Arc::clone(&shared);
            if std::thread::Builder::new().name(format!("bimcraft-nodes-{n}")).spawn(move || serve(&shared)).is_ok() {
                started += 1;
            }
        }
        Pool { shared, threads: started }
    }

    /// A pool that does its work where it is taken.
    pub(crate) fn inline() -> Pool<J> {
        Pool::new(0, || {})
    }

    /// The queue from now on: `jobs`, most needed first, less any a loader is doing already. What was queued before and isn't
    /// asked again is dropped.
    pub(crate) fn ask(&self, jobs: Vec<J>) {
        let Ok(mut s) = self.shared.state.lock() else { return };
        let running = &s.running;
        let queue: VecDeque<J> = jobs.into_iter().filter(|j| !running.contains(&j.key())).collect();
        s.queue = queue;
        drop(s);
        self.shared.more.notify_all();
    }

    /// What has been done since last asked (with no loaders, all that is queued, done now).
    pub(crate) fn take(&self) -> Vec<(J::Key, Option<J::Out>)> {
        if self.threads == 0 {
            loop {
                let job = match self.shared.state.lock() {
                    Ok(mut s) => s.queue.pop_front(),
                    Err(_) => None,
                };
                let Some(job) = job else { break };
                let key = job.key();
                let out = std::panic::catch_unwind(AssertUnwindSafe(|| job.run())).ok();
                if let Ok(mut s) = self.shared.state.lock() {
                    s.done.push((key, out));
                }
            }
        }
        self.shared.state.lock().map(|mut s| std::mem::take(&mut s.done)).unwrap_or_default()
    }

    /// Whether there is work queued or being done.
    pub(crate) fn working(&self) -> bool {
        self.shared.state.lock().is_ok_and(|s| !s.queue.is_empty() || !s.running.is_empty())
    }
}

impl<J: Job> Drop for Pool<J> {
    /// The loaders stop once what they are doing is done; nothing waits for them.
    fn drop(&mut self) {
        if let Ok(mut s) = self.shared.state.lock() {
            s.stop = true;
            s.queue.clear();
        }
        self.shared.more.notify_all();
    }
}

/// A loader: the top of the queue, done, until the pool is let go.
fn serve<J: Job>(shared: &Shared<J>) {
    loop {
        let job = {
            let Ok(mut s) = shared.state.lock() else { return };
            loop {
                if s.stop {
                    return;
                }
                if let Some(job) = s.queue.pop_front() {
                    s.running.insert(job.key());
                    break job;
                }
                s = match shared.more.wait(s) {
                    Ok(s) => s,
                    Err(_) => return,
                };
            }
        };
        let key = job.key();
        let out = std::panic::catch_unwind(AssertUnwindSafe(|| job.run())).ok();
        {
            let Ok(mut s) = shared.state.lock() else { return };
            s.running.remove(&key);
            if s.stop {
                return;
            }
            s.done.push((key, out));
        }
        (shared.wake)();
    }
}

/// What a cache holds, each with its size, by when it was last used: past its budget, what was used longest ago goes first,
/// unless it is to be kept.
pub(crate) struct Resident<K, V> {
    held: HashMap<K, (V, u64, u64)>,
    bytes: u64,
    tick: u64,
}

impl<K: Clone + Eq + Hash, V> Default for Resident<K, V> {
    fn default() -> Self {
        Resident { held: HashMap::new(), bytes: 0, tick: 0 }
    }
}

impl<K: Clone + Eq + Hash, V> Resident<K, V> {
    pub(crate) fn contains(&self, k: &K) -> bool {
        self.held.contains_key(k)
    }

    pub(crate) fn get(&self, k: &K) -> Option<&V> {
        self.held.get(k).map(|(v, _, _)| v)
    }

    /// `k` used now.
    pub(crate) fn touch(&mut self, k: &K) {
        self.tick += 1;
        if let Some(entry) = self.held.get_mut(k) {
            entry.2 = self.tick;
        }
    }

    pub(crate) fn insert(&mut self, k: K, v: V, bytes: u64) {
        self.tick += 1;
        if let Some((_, old, _)) = self.held.insert(k, (v, bytes, self.tick)) {
            self.bytes -= old;
        }
        self.bytes += bytes;
    }

    pub(crate) fn remove(&mut self, k: &K) -> Option<V> {
        let (v, bytes, _) = self.held.remove(k)?;
        self.bytes -= bytes;
        Some(v)
    }

    /// What it holds, bytes.
    pub(crate) fn bytes(&self) -> u64 {
        self.bytes
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.held.len()
    }

    pub(crate) fn clear(&mut self) {
        self.held.clear();
        self.bytes = 0;
    }

    /// Room for `need` bytes more within `budget`, what was used longest ago and isn't to be kept let go until there is:
    /// whether there is.
    pub(crate) fn make_room(&mut self, need: u64, budget: u64, keep: &dyn Fn(&K) -> bool) -> bool {
        while self.bytes.saturating_add(need) > budget {
            let oldest = self.held.iter().filter(|(k, _)| !keep(k)).min_by_key(|(_, (_, _, t))| *t).map(|(k, _)| k.clone());
            let Some(k) = oldest else { return false };
            self.remove(&k);
        }
        true
    }
}

/// How the card's nodes are packed: for which mesh (its origin, and the numbers it gives its owners) and look. Another mesh
/// or look packs them again.
pub(crate) struct Packing {
    pub(crate) id: u64,
    look: Arc<Look>,
    origin: [f64; 3],
    owners: HashMap<Id, u32>,
}

/// A node to read, and to pack for the card with `pack`.
pub(crate) struct Fetch {
    want: Want,
    pack: Option<Arc<Packing>>,
}

/// A node read: packed for the card with packing `pack`, or only read into the indexes' cache.
pub(crate) struct Fetched {
    pack: Option<u64>,
    dots: Option<NodeDots>,
}

impl Job for Fetch {
    type Key = Key;
    type Out = Fetched;

    fn key(&self) -> Key {
        self.want.key()
    }

    fn run(self) -> Fetched {
        let points = self.want.index.fetch(self.want.node);
        let dots = match (&self.pack, &points) {
            (Some(p), Some(points)) => Some(view3d::node_dots(&p.look, p.origin, &self.want, points)),
            _ => None,
        };
        Fetched { pack: self.pack.map(|p| p.id), dots }
    }
}

/// A node packed for the card, waiting to go up: its key, the number its owner has in the mesh, and its points.
pub(crate) struct Ready {
    pub(crate) key: Key,
    pub(crate) owner: u32,
    pub(crate) dots: NodeDots,
}

/// What the 3D view draws the scans' nodes with: the card's mesh, or the processor.
pub(crate) enum On<'a> {
    Card(&'a Arc<Mesh>),
    Processor,
}

/// What the nodes were last ranked for.
struct Asked {
    model: Weak<Model>,
    view: Option<Id>,
    cam: Camera,
    reboxed: Option<([f64; 3], [f64; 3])>,
    most: usize,
    pack: Option<u64>,
}

impl Asked {
    fn same(&self, other: &Asked) -> bool {
        Weak::ptr_eq(&self.model, &other.model)
            && self.view == other.view
            && self.cam == other.cam
            && self.reboxed == other.reboxed
            && self.most == other.most
            && self.pack == other.pack
    }
}

/// A 3D view's streaming of the scans' nodes.
#[derive(Default)]
pub(crate) struct Stream {
    pool: Option<Pool<Fetch>>,
    /// The look of which model and view.
    look: Option<(Weak<Model>, Option<Id>, Arc<Look>)>,
    asked: Option<Asked>,
    /// The nodes the picture wants, most needed first, and the same as a set.
    wanted: Vec<Key>,
    wanted_set: HashSet<Key>,
    /// How the card's nodes are packed now, for which mesh.
    packing: Option<(Weak<Mesh>, Arc<Packing>)>,
    next_pack: u64,
    /// Nodes packed for the card and not yet sent.
    ready: VecDeque<Ready>,
    /// How many nodes have come in; how many the processor's picture was last drawn for, and when.
    arrived: u64,
    shown: u64,
    shown_at: Option<Instant>,
}

impl Stream {
    /// Whether `model` has a scan whose index is open: nothing is streamed otherwise.
    pub(crate) fn wanted_in(model: &Model) -> bool {
        model.clouds.values().any(|c| c.indexed().is_some())
    }

    /// Lets go of all of it: the loaders stop.
    pub(crate) fn reset(&mut self) {
        if self.pool.is_some() || !self.wanted.is_empty() || self.packing.is_some() {
            *self = Stream::default();
        }
    }

    /// The nodes the picture wants, most needed first.
    pub(crate) fn wanted(&self) -> &[Key] {
        &self.wanted
    }

    /// The nodes waiting to go to the card, and those the picture wants.
    pub(crate) fn waiting(&mut self) -> (&mut VecDeque<Ready>, &HashSet<Key>) {
        (&mut self.ready, &self.wanted_set)
    }

    /// How the card's nodes are packed now (none on the processor).
    pub(crate) fn pack(&self) -> Option<u64> {
        self.packing.as_ref().map(|(_, p)| p.id)
    }

    /// How many nodes have come in.
    #[cfg(test)]
    pub(crate) fn arrived(&self) -> u64 {
        self.arrived
    }

    /// Whether nodes are still being read, or wait to go to the card.
    pub(crate) fn busy(&self) -> bool {
        !self.ready.is_empty() || self.pool.as_ref().is_some_and(Pool::working)
    }

    /// A frame of the 3D view showing `view` of `model` through `cam` (its section box `reboxed` while one is dragged), no more
    /// than `most` points of the scans, `on` the card or the processor, the card holding the nodes `held` says: ranks the nodes
    /// again where any of that changed, asks the loaders for those not held, and takes what they have done.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn frame(
        &mut self,
        ctx: &egui::Context,
        model: &Arc<Model>,
        view: Option<Id>,
        cam: &Camera,
        reboxed: Option<([f64; 3], [f64; 3])>,
        most: usize,
        on: On<'_>,
        held: &dyn Fn(&Key) -> bool,
    ) {
        if !Stream::wanted_in(model) {
            self.reset();
            return;
        }
        let wake = ctx.clone();
        // A test's loaders do their work in the frame, as it makes caps.
        let pool =
            self.pool.get_or_insert_with(|| if cfg!(test) { Pool::inline() } else { Pool::new(LOADERS, move || wake.request_repaint()) });
        // The view's look, made again only for another model or view.
        let look = match &self.look {
            Some((m, v, look)) if Weak::ptr_eq(m, &Arc::downgrade(model)) && *v == view => Arc::clone(look),
            _ => {
                let look = Arc::new(Look::of_3d_view(model, view));
                self.look = Some((Arc::downgrade(model), view, Arc::clone(&look)));
                look
            }
        };
        // The card's packing: made again for another mesh, or a look that colours otherwise. The card lets go of the old
        // packing's nodes after this frame, so what it holds now is asked for again.
        let mut repacked = false;
        match on {
            On::Card(mesh) => {
                let same = self
                    .packing
                    .as_ref()
                    .is_some_and(|(m, p)| Weak::ptr_eq(m, &Arc::downgrade(mesh)) && (Arc::ptr_eq(&p.look, &look) || *p.look == *look));
                if !same {
                    self.next_pack += 1;
                    let owners = mesh.owners.iter().enumerate().map(|(k, o)| (o.id, k as u32 + 1)).collect();
                    let packing = Packing { id: self.next_pack, look: Arc::clone(&look), origin: mesh.origin, owners };
                    self.packing = Some((Arc::downgrade(mesh), Arc::new(packing)));
                    self.ready.clear();
                    repacked = true;
                }
            }
            On::Processor => {
                if self.packing.take().is_some() {
                    self.ready.clear();
                }
            }
        }
        let held = |k: &Key| !repacked && held(k);
        let packing = self.packing.as_ref().map(|(_, p)| Arc::clone(p));
        let asked = Asked { model: Arc::downgrade(model), view, cam: *cam, reboxed, most, pack: packing.as_ref().map(|p| p.id) };
        if !self.asked.as_ref().is_some_and(|a| a.same(&asked)) {
            let ranked = match reboxed {
                Some(b) => view3d::scan_wanted(model, cam, &Look::of_3d_view(model, view).with_section_box(Some(b)), most),
                None => view3d::scan_wanted(model, cam, &look, most),
            };
            self.wanted = ranked.iter().map(Want::key).collect();
            self.wanted_set = self.wanted.iter().copied().collect();
            // What waits to go to the card and is still wanted stays; the rest is dropped.
            let wanted = &self.wanted_set;
            self.ready.retain(|r| wanted.contains(&r.key));
            let waiting: HashSet<Key> = self.ready.iter().map(|r| r.key).collect();
            let card = packing.is_some();
            let jobs = ranked
                .into_iter()
                .filter(|w| !waiting.contains(&w.key()) && !held(&w.key()) && (card || !w.index.settled(w.node)))
                .map(|want| Fetch { want, pack: packing.clone() })
                .collect();
            pool.ask(jobs);
            self.asked = Some(asked);
        }
        for (key, fetched) in pool.take() {
            self.arrived += 1;
            let (Some(packing), Some(Fetched { pack: Some(pack), dots: Some(dots) })) = (&packing, fetched) else { continue };
            if pack != packing.id || !self.wanted_set.contains(&key) || held(&key) {
                continue;
            }
            if let Some(&owner) = packing.owners.get(&dots.owner) {
                self.ready.push_back(Ready { key, owner, dots });
            }
        }
        if !self.ready.is_empty() {
            ctx.request_repaint();
        }
    }

    /// Which nodes come in the processor's picture is drawn for: all there are once the loaders are done, else those there
    /// were when it was last drawn, at most [`PICTURE_EVERY`] ago.
    pub(crate) fn shown(&mut self, ctx: &egui::Context) -> u64 {
        if self.arrived != self.shown {
            let now = Instant::now();
            let since = self.shown_at.map_or(PICTURE_EVERY, |at| now.duration_since(at));
            if !self.pool.as_ref().is_some_and(Pool::working) || since >= PICTURE_EVERY {
                (self.shown, self.shown_at) = (self.arrived, Some(now));
            } else {
                ctx.request_repaint_after(PICTURE_EVERY - since);
            }
        }
        self.shown
    }
}

/// What the tests of streaming share.
#[cfg(test)]
pub(crate) mod testing {
    use bimcraft_core::cloud::{self, Cloud};
    use bimcraft_core::engine::Session;
    use bimcraft_core::model::{Element, Id};

    /// A red face 4 m by 3 m standing at `at` (mm), 1 cm between its points, in a scan whose index (built in `dir`) has levels
    /// below the few points the project holds: what the 3D view streams.
    pub(crate) fn indexed_scan(s: &mut Session, dir: &std::path::Path, at: [f64; 2]) -> Id {
        let path = dir.join("face.xyz");
        let text: String = (0..400)
            .flat_map(|i| (0..300).map(move |k| format!("{} 0 {} 220 30 30\n", f64::from(i) * 0.01, f64::from(k) * 0.01)))
            .collect();
        std::fs::write(&path, text).unwrap();
        s.execute("point_cloud", &serde_json::json!({"path": path, "at": at})).unwrap();
        let (id, name) = s
            .model
            .elements
            .iter()
            .rev()
            .find_map(|(id, e)| match e {
                Element::PointCloud(pc) => Some((*id, pc.cloud.clone())),
                _ => None,
            })
            .unwrap();
        let out = dir.join("face.bimpc");
        let file = std::fs::File::open(&path).unwrap();
        let input = cloud::index::Input { name: "face.xyz".into(), src: Box::new(file), format: cloud::Format::Xyz };
        let built = cloud::index::build(vec![input], 1000.0, &out, 2_000, None, None).unwrap();
        let r = cloud::lod::IndexRef {
            path: out.to_string_lossy().into_owned(),
            full: None,
            id: cloud::lod::IndexRef::id_of(built.id),
            count: built.count,
            levels: built.levels,
            sources: Vec::new(),
            scale: Some(1000.0),
            kind: None,
        };
        let indexed = Cloud::new("face.xyz", built.scan).unwrap().with_index(r).unwrap();
        indexed.attach(None).unwrap();
        s.model_mut().clouds.insert(name, indexed);
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    /// A job that waits to be let go, and says it ran.
    struct Gate {
        key: u32,
        go: Arc<Mutex<mpsc::Receiver<()>>>,
        ran: mpsc::Sender<u32>,
    }

    impl Job for Gate {
        type Key = u32;
        type Out = u32;
        fn key(&self) -> u32 {
            self.key
        }
        fn run(self) -> u32 {
            if let Ok(go) = self.go.lock() {
                let _ = go.recv_timeout(Duration::from_secs(10));
            }
            let _ = self.ran.send(self.key);
            self.key
        }
    }

    #[test]
    fn what_the_camera_moved_away_from_before_a_loader_took_it_is_never_read() {
        let (open, go) = mpsc::channel();
        let go = Arc::new(Mutex::new(go));
        let (ran, said) = mpsc::channel();
        let pool = Pool::new(1, || {});
        let gate = |key| Gate { key, go: Arc::clone(&go), ran: ran.clone() };
        pool.ask(vec![gate(1), gate(2), gate(3)]);
        // The loader takes 1 and waits on it; meanwhile the view moves and wants 4 and 1 instead.
        let start = Instant::now();
        while !pool.shared.state.lock().unwrap().running.contains(&1) {
            assert!(start.elapsed() < Duration::from_secs(10), "the loader never took the first");
            std::thread::yield_now();
        }
        pool.ask(vec![gate(4), gate(1)]);
        for _ in 0..2 {
            open.send(()).unwrap();
        }
        let mut done = Vec::new();
        while done.len() < 2 {
            done.push(said.recv_timeout(Duration::from_secs(10)).unwrap());
        }
        assert_eq!(done, vec![1, 4], "1 was being read and is read once; 4 is next; 2 and 3 are dropped");
        assert!(said.recv_timeout(Duration::from_millis(200)).is_err(), "nothing else is read");
        let mut taken: Vec<u32> = pool.take().into_iter().filter_map(|(_, out)| out).collect();
        taken.sort_unstable();
        assert_eq!(taken, vec![1, 4]);
        assert!(!pool.working());
    }

    #[test]
    fn a_pool_with_no_loaders_does_its_work_where_it_is_taken() {
        let (open, go) = mpsc::channel();
        let go = Arc::new(Mutex::new(go));
        let (ran, _said) = mpsc::channel();
        let pool = Pool::inline();
        for _ in 0..3 {
            open.send(()).unwrap();
        }
        pool.ask((1..=3).map(|key| Gate { key, go: Arc::clone(&go), ran: ran.clone() }).collect());
        assert!(pool.working());
        let done: Vec<u32> = pool.take().into_iter().map(|(k, _)| k).collect();
        assert_eq!(done, vec![1, 2, 3], "in the order asked");
        assert!(!pool.working());
    }

    #[test]
    fn a_view_reads_the_nodes_it_wants_and_on_the_card_packs_them_for_its_mesh_once() {
        use bimcraft_core::engine::Session;
        use bimcraft_core::view3d::{AZIMUTH, ELEVATION, Scene};
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new();
        let id = testing::indexed_scan(&mut s, dir.path(), [0.0, 0.0]);
        let model = Arc::clone(&s.model);
        let (scene, look) = (Scene::new(&model), Look::of_3d(&model));
        let far = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
        let near = Camera { scale: far.scale * 4.0, ..far };
        let ctx = egui::Context::default();
        let mut stream = Stream::default();
        // On the processor: what it wants is read into the indexes' cache, and the picture finds it all there.
        stream.frame(&ctx, &model, None, &near, None, 10_000_000, On::Processor, &|_| false);
        let wanted = stream.wanted().len();
        assert!(wanted > 1, "{wanted} nodes wanted");
        assert_eq!(stream.arrived() as usize, wanted, "read in the frame, as a test's loaders do");
        assert_eq!(view3d::scan_detail(&model, &near, &look, 10_000_000, false).missing, 0, "all of it cached");
        assert!(!stream.busy());
        // The same frame again asks for nothing.
        stream.frame(&ctx, &model, None, &near, None, 10_000_000, On::Processor, &|_| false);
        assert_eq!(stream.arrived() as usize, wanted);
        // On the card: each node packed for its mesh, numbered as the mesh numbers the scan.
        let mesh = Arc::new(Mesh::new(&scene, &look).unwrap());
        stream.frame(&ctx, &model, None, &near, None, 10_000_000, On::Card(&mesh), &|_| false);
        let first = stream.pack().unwrap();
        let owner = mesh.owners.iter().position(|o| o.id == id).unwrap() as u32 + 1;
        assert_eq!(stream.ready.len(), wanted);
        assert!(stream.ready.iter().all(|r| r.owner == owner && r.dots.count > 0 && r.dots.words.len() == 3 * r.dots.count as usize));
        // Another mesh packs them again, even those the card holds in the old packing, which it lets go of.
        let other = Arc::new(Mesh::new(&scene, &look).unwrap());
        stream.frame(&ctx, &model, None, &near, None, 10_000_000, On::Card(&other), &|_| true);
        let second = stream.pack().unwrap();
        assert_ne!(second, first);
        assert_eq!(stream.ready.len(), wanted, "every node packed again for the new packing");
        assert!(stream.ready.iter().all(|r| r.dots.count > 0));
        // Once the card holds them in this packing, the same frame asks for nothing.
        stream.ready.clear();
        stream.frame(&ctx, &model, None, &near, None, 10_000_000, On::Card(&other), &|_| true);
        assert_eq!(stream.pack(), Some(second));
        assert!(stream.ready.is_empty(), "what the card holds is never asked for again");
        // A look that colours otherwise (eye-dome lighting turned off in the view) packs them all again too.
        s.execute("graphic_display_options", &serde_json::json!({"eye_dome": false})).unwrap();
        let unlit = Arc::clone(&s.model);
        stream.frame(&ctx, &unlit, None, &near, None, 10_000_000, On::Card(&other), &|_| true);
        assert!(stream.pack().is_some_and(|p| p != second));
        assert_eq!(stream.ready.len(), wanted, "every node asked for again, though the card held them");
        // A model with no index open streams nothing.
        let mut plain = (*model).clone();
        plain.clouds.clear();
        stream.frame(&ctx, &Arc::new(plain), None, &near, None, 10_000_000, On::Processor, &|_| false);
        assert!(stream.wanted().is_empty() && stream.pack().is_none() && !stream.busy());
    }

    #[test]
    fn the_cache_lets_go_of_what_was_used_longest_ago_to_stay_within_its_budget_and_keeps_what_is_wanted() {
        let mut r: Resident<u32, ()> = Resident::default();
        for k in 0..4 {
            assert!(r.make_room(100, 400, &|_| false));
            r.insert(k, (), 100);
        }
        assert_eq!(r.bytes(), 400);
        // 0 used again: 1 is now the oldest, and goes for the next.
        r.touch(&0);
        assert!(r.make_room(100, 400, &|_| false));
        r.insert(4, (), 100);
        assert!(!r.contains(&1) && r.contains(&0) && r.bytes() == 400);
        // Two hundred more, 2 wanted: 3 and 0 go, the oldest not wanted.
        assert!(r.make_room(200, 400, &|k| *k == 2));
        assert!(r.contains(&2) && r.contains(&4) && !r.contains(&3) && !r.contains(&0), "{:?}", r.held.keys().collect::<Vec<_>>());
        assert_eq!(r.bytes(), 200);
        // What can't fit without letting go of what's wanted doesn't.
        assert!(!r.make_room(500, 400, &|_| true));
        assert_eq!(r.len(), 2);
        assert_eq!(r.remove(&2), Some(()));
        assert_eq!(r.bytes(), 100);
    }
}
