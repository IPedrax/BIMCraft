//! The 3D view drawn on the graphics card: the core's `view3d::gpu::Mesh` uploaded once for a model, and the passes for each
//! picture (the opaque surfaces; those seen through; edges, ground and the blend; the levels' lines), in `gpu.wgsl`, into a
//! texture that egui shows as it shows the CPU picture. What changes between pictures is small: the camera, the levels' lines,
//! and, only when they change, the selection's tints and the textures' mip levels. A section box is a uniform each pixel is cut
//! to, so a face being dragged builds nothing. Opt-in (Settings > Performance); what the card can't draw (wireframe, a point cloud) is the processor's, and so is everything when the card fails: a validation or out-of-memory error from the device is caught
//! here, not left to the device's handler, and the window carries on without the card (`BimApp::gpu_failed`).

use std::collections::BTreeMap;
use std::sync::Arc;

use bimcraft_core::model::Id;
use bimcraft_core::render::Rgb;
use bimcraft_core::view3d::gpu::{Instance, Mesh, Vertex, Visible};
use bimcraft_core::view3d::{Camera, MAX_SIDE};
use eframe::egui_wgpu::{RenderState, wgpu};
use egui::TextureId;

const COLOR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// The plane a pixel shows and its element.
const IDS: wgpu::TextureFormat = wgpu::TextureFormat::Rg32Uint;
/// The element seen through at a pixel.
const CLEAR_IDS: wgpu::TextureFormat = wgpu::TextureFormat::R32Uint;
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// A vertex (see [`Vertex`]) as the card takes it: its position as three floats, its two normals as four 16-bit signed fractions
/// each (the fourth 0), and its four words. 44 bytes, not the 52 of floats.
const STRIDE: u64 = 44;
const GLOBALS: u64 = 10 * 16;
/// A section box: its least corner and its greatest, in the model.
type Cut = Option<([f64; 3], [f64; 3])>;
/// An owner's edge and tint colours, a skin's parts and a segment's ends and colour: 2, 3 and 3 vec4s.
const OWNER: u64 = 32;
const SKIN: u64 = 48;
const SEGMENT: u64 = 48;
/// A copy of a shape (see `Instance`): where it is as three floats, its turn as two, its owner, where its planes are, and its
/// owner seen through.
const INSTANCE: u64 = 60;
/// An edge a wireframe draws (see `Wire`): its ends from the mesh's origin as three floats each, and its owner.
const WIRE: u64 = 28;
/// A scan's point (see `Dot`): where it is from the mesh's origin as three floats, its colour and its owner.
const DOT: u64 = 20;

/// The window's use of the card: whether the user asked for it, the wgpu state it would be made on (none where the window has
/// no wgpu device, as in every test), the card once it's made, and why the last picture was drawn on the processor, if it was.
pub struct Card {
    pub wanted: bool,
    pub state: Option<RenderState>,
    pub gpu: Option<Gpu>,
    pub note: Option<String>,
    /// The card was tried and can't draw the 3D view.
    pub(crate) failed: bool,
    /// How the 2D canvas draws its kept triangles, on the card egui draws on: made the first time, whatever the 3D view's setting.
    pub(crate) canvas: Option<std::sync::Arc<crate::retained::Pipeline>>,
}

/// Wanted from the start: a window with a card draws on it unless the user says otherwise.
impl Default for Card {
    fn default() -> Card {
        Card { wanted: true, state: None, gpu: None, note: None, failed: false, canvas: None }
    }
}

impl Card {
    /// A card already made, as a test has one.
    #[cfg(test)]
    pub(crate) fn with(gpu: Gpu) -> Card {
        Card { wanted: true, gpu: Some(gpu), ..Card::default() }
    }

    /// How the 2D canvas's triangles are drawn on the window's card; none in a window with no wgpu device, as in every test.
    pub(crate) fn canvas(&mut self) -> Option<std::sync::Arc<crate::retained::Pipeline>> {
        if self.canvas.is_none()
            && let Some(state) = &self.state
        {
            self.canvas = Some(std::sync::Arc::new(crate::retained::Pipeline::new(&state.device, &state.queue, state.target_format)));
        }
        self.canvas.clone()
    }

    /// Whether there is a card the user can ask for.
    pub fn available(&self) -> bool {
        self.gpu.is_some() || (self.state.is_some() && !self.failed)
    }

    /// The card's name, as the driver gives it.
    pub fn adapter(&self) -> Option<String> {
        self.gpu.as_ref().map(|g| g.adapter().to_string()).or_else(|| self.state.as_ref().map(|s| s.adapter.get_info().name))
    }

    /// The card, if it's wanted and there is one: made the first time (making it compiles its shaders, which a start with it
    /// off doesn't wait for). One that can't be made is said why in `note`, and not tried again.
    pub fn ready(&mut self) -> Option<&mut Gpu> {
        if self.wanted
            && self.gpu.is_none()
            && !self.failed
            && let Some(state) = &self.state
        {
            match Gpu::new(state) {
                Ok(gpu) => self.gpu = Some(gpu),
                Err(e) => (self.failed, self.note) = (true, Some(format!("the card failed: {e}"))),
            }
        }
        self.gpu.as_mut().filter(|_| self.wanted)
    }
}

/// The opaque surfaces' three targets, those seen through's three, and the picture they are put together in.
struct Targets {
    size: (u32, u32),
    color: wgpu::TextureView,
    /// The colour target itself, which a wireframe drawn in parts is copied from.
    color_texture: wgpu::Texture,
    ids: wgpu::TextureView,
    depth: wgpu::TextureView,
    clear_color: wgpu::TextureView,
    clear_ids: wgpu::TextureView,
    clear_depth: wgpu::TextureView,
    // Kept for a test to read back.
    #[cfg_attr(not(test), allow(dead_code))]
    picture: wgpu::Texture,
    picture_view: wgpu::TextureView,
    /// The opaque targets, as the later passes read them; and those seen through, as the picture's pass does.
    bind: wgpu::BindGroup,
    clear_bind: wgpu::BindGroup,
}

/// A mesh on the card, the buffers that change with the pictures, and what they were last written with.
struct Uploaded {
    mesh: Arc<Mesh>,
    /// Its vertices, those the card holds now.
    vertices: Store,
    /// How many bytes of these, a wireframe's lines and a scan's points it holds, and the most it may.
    held: u64,
    room: u64,
    count: u64,
    /// Where the vertices seen through start.
    clear_from: u64,
    /// The shapes' vertices, in one buffer; their copies, as many buffers as they take, each but the last holding
    /// [`Uploaded::instances_per`]; and the copies' planes.
    shape_vertices: Option<wgpu::Buffer>,
    instances: Vec<wgpu::Buffer>,
    instances_per: u64,
    /// A wireframe's edges, those the card holds now, and how far a picture too big to hold at once is drawn.
    wires: Store,
    wire_stream: Option<WireStream>,
    /// A scan's points, those the card holds now.
    dots: Store,
    /// The copies a view sees, gathered in their order into one buffer (and how many it has room for) when it doesn't see them
    /// all: which they are, and where each shape's are in it.
    seen: Option<(wgpu::Buffer, u64)>,
    seen_runs: Vec<std::ops::Range<usize>>,
    seen_shapes: Vec<std::ops::Range<u32>>,
    /// The boxes standing in, in a frame of a moving view, and how many it has room for.
    stand_ins: Option<(wgpu::Buffer, u64)>,
    /// The camera and section box the surfaces in the targets are of, and how far they are drawn.
    progress: Option<(Camera, Cut, Stage)>,
    planes: wgpu::Buffer,
    owners: wgpu::Buffer,
    skins: wgpu::Buffer,
    texels: wgpu::Buffer,
    /// The tints the owners were written with (none yet), and the textures' lookups.
    tints: Option<Vec<(Id, Rgb)>>,
    lookups: Vec<[u32; 12]>,
    /// The buffers bound, made again when the segments' buffer is.
    bind: Option<wgpu::BindGroup>,
}

struct UploadJob {
    device: wgpu::Device,
    queue: wgpu::Queue,
    vertex_bytes: u64,
    room: Room,
    mesh: Arc<Mesh>,
}

struct UploadDone {
    mesh: Arc<Mesh>,
    uploaded: Result<Uploaded, String>,
}

fn build_upload(job: UploadJob, cancel: &crate::view_worker::Cancel) -> Option<UploadDone> {
    let guard = scopes(&job.device);
    let uploaded = Gpu::make_uploaded(&job.device, &job.queue, job.vertex_bytes, job.room, &job.mesh, &|| cancel.cancelled());
    let caught = caught(guard);
    if cancel.cancelled() {
        return None;
    }
    Some(UploadDone { mesh: job.mesh, uploaded: uploaded.and_then(|u| caught.map(|()| u)) })
}

/// The most bytes of vertices a card is asked to hold, and where a test says allocations fail.
#[derive(Clone, Copy, Debug)]
struct Room {
    bytes: u64,
    full_at: Option<u64>,
}

/// Why a buffer of vertices isn't on the card: it has no room, which is a signal to hold less, or something else went wrong.
enum Hold {
    Full,
    Failed(String),
}

const TOO_LITTLE: &str = "more memory than the card has: it can't hold one buffer of the model's vertices";

/// What of a mesh the card can let go of and bring back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Own,
    Wires,
    Dots,
}

const KINDS: [Kind; 3] = [Kind::Own, Kind::Wires, Kind::Dots];

/// One kind of it, in as many buffers as it takes, each but the last holding `per` items of `stride` bytes.
struct Store {
    buffers: Vec<Option<wgpu::Buffer>>,
    per: u64,
    count: u64,
    stride: u64,
}

impl Store {
    fn bytes_of(&self, k: usize) -> u64 {
        self.count.saturating_sub(k as u64 * self.per).min(self.per) * self.stride
    }

    /// The buffers items `r` lie in.
    fn of(&self, r: &std::ops::Range<usize>) -> std::ops::Range<usize> {
        let per = self.per.max(1) as usize;
        if r.is_empty() { 0..0 } else { r.start / per..(r.end - 1) / per + 1 }
    }

    fn holds(&self, r: &std::ops::Range<usize>) -> bool {
        self.of(r).all(|k| matches!(self.buffers.get(k), Some(Some(_))))
    }

    /// How many buffers it holds now.
    fn held(&self) -> usize {
        self.buffers.iter().flatten().count()
    }
}

/// A wireframe's picture drawn in parts because the card can't hold all its lines at once: what it is of, and how many parts
/// are done (a part being one buffer's lines, once for all of them and again for what is tinted).
struct WireStream {
    key: (Camera, Cut, Vec<(Id, Rgb)>, bool),
    done: usize,
}

/// The bytes of buffer `k` of a kind held `per` to a buffer, as the card reads them.
fn store_bytes(mesh: &Mesh, kind: Kind, k: usize, per: usize, cancelled: &dyn Fn() -> bool) -> Result<Words, Hold> {
    let stop = "3D upload was cancelled";
    let mut bytes = Words::new();
    let range = |len: usize| (k * per).min(len)..((k + 1) * per).min(len);
    match kind {
        Kind::Own => {
            let part = mesh.vertices.get(range(mesh.vertices.len())).unwrap_or_default();
            bytes.reserve(part.len() * STRIDE as usize);
            for (n, Vertex { position, normal, face, color, owner, plane, tex }) in part.iter().enumerate() {
                if n % 8192 == 0 && cancelled() {
                    return Err(Hold::Failed(stop.into()));
                }
                put(&mut bytes, position);
                put_unit(&mut bytes, normal);
                put_unit(&mut bytes, face);
                put_words(&mut bytes, &[*color, *owner, *plane, *tex]);
            }
        }
        // From the origin so that f32 holds them.
        Kind::Wires => {
            let part = mesh.wires.get(range(mesh.wires.len())).unwrap_or_default();
            bytes.reserve(part.len() * WIRE as usize);
            for (n, wire) in part.iter().enumerate() {
                if n % 8192 == 0 && cancelled() {
                    return Err(Hold::Failed(stop.into()));
                }
                let [a, b] = wire.ends.map(|p| std::array::from_fn::<f32, 3, _>(|i| (p[i] - mesh.origin[i]) as f32));
                put(&mut bytes, &a);
                put(&mut bytes, &b);
                put_words(&mut bytes, &[wire.owner]);
            }
        }
        Kind::Dots => {
            let part = mesh.dots.get(range(mesh.dots.len())).unwrap_or_default();
            bytes.reserve(part.len() * DOT as usize);
            for (n, dot) in part.iter().enumerate() {
                if n % 8192 == 0 && cancelled() {
                    return Err(Hold::Failed(stop.into()));
                }
                let at = std::array::from_fn::<f32, 3, _>(|i| (dot.at[i] - mesh.origin[i]) as f32);
                put(&mut bytes, &at);
                put_words(&mut bytes, &[dot.color, dot.owner]);
            }
        }
    }
    Ok(bytes)
}

/// `bytes`, on the card in a buffer of its own, with `held` bytes there already.
fn hold(device: &wgpu::Device, queue: &wgpu::Queue, bytes: &[u8], held: u64, full_at: Option<u64>) -> Result<wgpu::Buffer, Hold> {
    let size = bytes.len() as u64;
    if full_at.is_some_and(|at| held + size > at) {
        return Err(Hold::Full);
    }
    let guard = scopes(device);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("bimcraft-3d-held"),
        size,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&buffer, 0, bytes);
    queue.submit([]);
    match out_of_memory(guard) {
        Ok(false) => Ok(buffer),
        Ok(true) => Err(Hold::Full),
        Err(e) => Err(Hold::Failed(e)),
    }
}

impl Uploaded {
    fn store(&self, kind: Kind) -> &Store {
        match kind {
            Kind::Own => &self.vertices,
            Kind::Wires => &self.wires,
            Kind::Dots => &self.dots,
        }
    }

    fn store_mut(&mut self, kind: Kind) -> &mut Store {
        match kind {
            Kind::Own => &mut self.vertices,
            Kind::Wires => &mut self.wires,
            Kind::Dots => &mut self.dots,
        }
    }

    /// The buffers `draws` read, each once.
    fn wanted_by(&self, draws: &[(bool, Draw)]) -> Vec<(Kind, usize)> {
        let mut wanted = Vec::new();
        for (_, draw) in draws {
            let (kind, range) = match draw {
                Draw::Own(r) => (Kind::Own, self.vertices.of(r)),
                Draw::Dots(r) => (Kind::Dots, self.dots.of(r)),
                Draw::Copies { .. } => continue,
            };
            for k in range {
                if !wanted.contains(&(kind, k)) {
                    wanted.push((kind, k));
                }
            }
        }
        wanted
    }

    /// Lets go of a buffer it needn't `keep`, the last of the last kind first. False where there's none.
    fn let_go(&mut self, keep: &dyn Fn(Kind, usize) -> bool) -> bool {
        for kind in KINDS.into_iter().rev() {
            let store = self.store(kind);
            let Some(k) = (0..store.buffers.len()).rev().find(|&k| !keep(kind, k) && matches!(store.buffers.get(k), Some(Some(_)))) else {
                continue;
            };
            let size = store.bytes_of(k);
            if let Some(slot) = self.store_mut(kind).buffers.get_mut(k) {
                *slot = None;
            }
            self.held -= size;
            return true;
        }
        false
    }

    /// Puts buffer `k` of `kind` on the card, letting go of any it needn't `keep` to make room. False where there's none to be
    /// had.
    fn bring_in(
        &mut self,
        (device, queue): (&wgpu::Device, &wgpu::Queue),
        mesh: &Mesh,
        (kind, k): (Kind, usize),
        keep: &dyn Fn(Kind, usize) -> bool,
        full_at: Option<u64>,
    ) -> Result<bool, String> {
        let store = self.store(kind);
        if !matches!(store.buffers.get(k), Some(None)) {
            return Ok(true);
        }
        let (size, per) = (store.bytes_of(k), store.per as usize);
        let keeping = |n: Kind, m: usize| (n, m) == (kind, k) || keep(n, m);
        while self.held + size > self.room {
            if !self.let_go(&keeping) {
                return Ok(false);
            }
        }
        let bytes = store_bytes(mesh, kind, k, per, &|| false).map_err(|e| match e {
            Hold::Failed(e) => e,
            Hold::Full => TOO_LITTLE.to_string(),
        })?;
        loop {
            match hold(device, queue, &bytes, self.held, full_at) {
                Ok(buffer) => {
                    if let Some(slot) = self.store_mut(kind).buffers.get_mut(k) {
                        *slot = Some(buffer);
                    }
                    self.held += size;
                    return Ok(true);
                }
                Err(Hold::Full) => {
                    // The card has no more than it holds now: from here on, no more than that is asked of it.
                    self.room = self.held;
                    if !self.let_go(&keeping) {
                        return Ok(false);
                    }
                }
                Err(Hold::Failed(e)) => return Err(e),
            }
        }
    }
}

/// The card, and what it keeps between pictures.
pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    state: Option<RenderState>,
    scene: wgpu::RenderPipeline,
    /// The opaque surfaces cut to a section box. Pipelines of their own, as a shader that may discard gives up what the depth
    /// test saves before it runs, which a view with no box keeps.
    scene_cut: wgpu::RenderPipeline,
    instanced_cut: wgpu::RenderPipeline,
    instanced: wgpu::RenderPipeline,
    instanced_clear: wgpu::RenderPipeline,
    clear: wgpu::RenderPipeline,
    post: wgpu::RenderPipeline,
    lines: wgpu::RenderPipeline,
    /// A wireframe's edges; then, over them, only those tinted.
    wires: wgpu::RenderPipeline,
    wires_tinted: wgpu::RenderPipeline,
    /// A scan's points, in the opaque surfaces' targets.
    dots: wgpu::RenderPipeline,
    group0: wgpu::BindGroupLayout,
    group1: wgpu::BindGroupLayout,
    group2: wgpu::BindGroupLayout,
    globals: wgpu::Buffer,
    segments: (wgpu::Buffer, u64),
    mesh: Option<Uploaded>,
    upload_worker: Option<crate::view_worker::Latest<UploadJob, UploadDone>>,
    upload_requested: Option<Arc<Mesh>>,
    targets: Option<Targets>,
    texture: Option<TextureId>,
    adapter: String,
    /// The most bytes of vertices in one buffer.
    pub(crate) vertex_bytes: u64,
    /// Whether what a view can't see is left out ([`Mesh::visible`]); off only to test that the picture is the same, and to
    /// measure what it saves.
    pub(crate) cull: bool,
    /// The most triangles a frame draws ([`FRAME_TRIANGLES`]; fewer in a test).
    pub(crate) budget: usize,
    /// The most bytes of a mesh's own vertices the card holds at once: all there are, until an allocation says otherwise.
    pub(crate) card_bytes: u64,
    /// Where allocations fail, for a test: a card can't be made to run out to order.
    pub(crate) full_at: Option<u64>,
    /// Hidden line is drawn as a section is, paper and ink swapped on a dark canvas (`canvas::color`).
    pub(crate) swapped_ink: bool,
}

/// The most bytes a buffer of vertices holds, whatever the card allows: some drivers place a very large buffer badly.
const VERTEX_BUFFER: u64 = 256 << 20;

/// What a device is asked for to draw any model: the buffers as big as the adapter has them, the rest as `asked`. OpenGL isn't the card's.
pub fn limits_for(adapter: &wgpu::Adapter, asked: wgpu::Limits) -> wgpu::Limits {
    if adapter.get_info().backend == wgpu::Backend::Gl {
        return asked;
    }
    let has = adapter.limits();
    wgpu::Limits { max_buffer_size: has.max_buffer_size, max_storage_buffer_binding_size: has.max_storage_buffer_binding_size, ..asked }
}

/// Which vertices of which buffer `range` is, buffers holding `per` vertices each, in order.
fn spans(per: u64, range: std::ops::Range<u64>) -> Vec<(usize, std::ops::Range<u32>)> {
    let per = per.max(3);
    let (mut out, mut at) = (Vec::new(), range.start);
    while at < range.end {
        let k = at / per;
        let stop = ((k + 1) * per).min(range.end);
        out.push((k as usize, (at - k * per) as u32..(stop - k * per) as u32));
        at = stop;
    }
    out
}

/// The bytes of a buffer, 32-bit words as the card's byte order has them.
type Words = Vec<u8>;

/// The most triangles a frame draws: past it, while the view moves, the nearest of what it sees and a box for each of the
/// rest, and once it stays, the exact picture over as many frames as it takes. About 12 ms of an RTX 2060.
const FRAME_TRIANGLES: usize = 12_000_000;

/// How far the surfaces in the targets are drawn for the camera they're of: what a moving frame drew, or the exact drawing up
/// to `done` of its `total` triangles.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Stage {
    Moving,
    Exact { done: usize, total: usize },
}

/// What one frame does: the picture again over surfaces already whole, the next part of the exact drawing from a triangle,
/// or a frame of a moving view.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Frame {
    Picture,
    Exact(usize),
    Moving,
}

/// Where a draw's copies are read from: the buffers the mesh went up in (by their place in [`Mesh::instances`]), those a view
/// sees gathered, or the boxes standing in (each by its place in its buffer).
#[derive(Clone, Copy, Debug, PartialEq)]
enum From {
    Mesh,
    Seen,
    StandIns,
}

/// One draw of the surfaces: triangles drawn of their own, by their vertices, copies of the shape whose vertices are `corners`,
/// or a scan's points, by their place in [`Mesh::dots`].
#[derive(Clone, Debug, PartialEq)]
enum Draw {
    Own(std::ops::Range<usize>),
    Copies { corners: std::ops::Range<u32>, from: From, copies: std::ops::Range<usize> },
    Dots(std::ops::Range<usize>),
}

impl Draw {
    /// The triangles it draws.
    fn cost(&self) -> usize {
        match self {
            Draw::Own(r) => r.len() / 3,
            Draw::Copies { corners, copies, .. } => corners.len() / 3 * copies.len(),
            // As much of a frame as a triangle takes, measured, though it is drawn in two very small ones.
            Draw::Dots(r) => r.len(),
        }
    }

    /// What of it lies from triangle `a` to `b` of it: whole triangles, and of copies those that end there, so that parts
    /// taken one after another draw each copy once.
    fn part(&self, a: usize, b: usize) -> Draw {
        match self {
            Draw::Own(r) => Draw::Own((r.start + 3 * a).min(r.end)..(r.start + 3 * b).min(r.end)),
            Draw::Copies { corners, from, copies } => {
                let each = (corners.len() / 3).max(1);
                let (first, last) = ((copies.start + a / each).min(copies.end), (copies.start + b / each).min(copies.end));
                Draw::Copies { corners: corners.clone(), from: *from, copies: first..last }
            }
            Draw::Dots(r) => Draw::Dots((r.start + a).min(r.end)..(r.start + b).min(r.end)),
        }
    }
}

/// What of `draws` lies from triangle `a` to `b` of them all, in order, with whether each is of what is seen through.
fn part_of(draws: &[(bool, Draw)], a: usize, b: usize) -> Vec<(bool, Draw)> {
    let mut at = 0;
    let mut out = Vec::new();
    for (clear, d) in draws {
        let (start, end) = (at, at + d.cost());
        at = end;
        if end <= a || start >= b {
            continue;
        }
        let part = d.part(a.max(start) - start, b.min(end) - start);
        if part.cost() > 0 {
            out.push((*clear, part));
        }
    }
    out
}

/// The draws of what `seen` holds and of `stand_ins`, in the order the picture needs them: what is opaque of the triangles drawn
/// of their own, then of each shape's copies, then the boxes, then a scan's points; then what is seen through of each. The copies a view sees, when it
/// doesn't see them all, are gathered into one buffer in their order (one draw a shape), and the boxes into another, each
/// written again only when it changes.
fn surfaces(
    mesh: &Mesh,
    up: &mut Uploaded,
    queue: &wgpu::Queue,
    device: &wgpu::Device,
    seen: &Visible,
    stand_ins: &[Instance],
    limit: u64,
) -> Vec<(bool, Draw)> {
    let room_for = |up_slot: &mut Option<(wgpu::Buffer, u64)>, n: u64, label: &str| {
        if up_slot.as_ref().is_none_or(|(_, room)| *room < n) {
            let room = n.max(1024).next_power_of_two().min((limit / INSTANCE).max(1));
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: room * INSTANCE,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            *up_slot = Some((buffer, room));
        }
    };
    let all = seen.instances.len() == 1 && seen.instances.first() == Some(&(0..mesh.instances.len()));
    let gathered = seen.instances.iter().map(|r| r.len() as u64).sum::<u64>();
    let gather = !all && gathered > 0 && gathered * INSTANCE <= limit;
    if !gather {
        up.seen_runs.clear();
    } else if up.seen_runs != seen.instances {
        let mut bytes = Words::with_capacity(gathered as usize * INSTANCE as usize);
        up.seen_shapes.clear();
        let mut at = 0u32;
        for shape in &mesh.shapes {
            let start = (shape.vertices.start / 3) as u32;
            let first = at;
            for run in within(&seen.instances, shape.instances.clone()) {
                for c in mesh.instances.get(run).unwrap_or_default() {
                    put_copy(&mut bytes, c, start);
                    at += 1;
                }
            }
            up.seen_shapes.push(first..at);
        }
        room_for(&mut up.seen, gathered, "bimcraft-3d-seen-copies");
        if let Some((buffer, _)) = &up.seen
            && !bytes.is_empty()
        {
            queue.write_buffer(buffer, 0, &bytes);
        }
        up.seen_runs.clone_from(&seen.instances);
    }
    // As many boxes as a buffer holds: they only stand in.
    let boxes = stand_ins.len().min((limit / INSTANCE) as usize);
    if boxes > 0 {
        let start = (mesh.cube.start / 3) as u32;
        let mut bytes = Words::with_capacity(boxes * INSTANCE as usize);
        stand_ins.iter().take(boxes).for_each(|c| put_copy(&mut bytes, c, start));
        room_for(&mut up.stand_ins, boxes as u64, "bimcraft-3d-stand-ins");
        if let Some((buffer, _)) = &up.stand_ins {
            queue.write_buffer(buffer, 0, &bytes);
        }
    }
    let mut draws = Vec::new();
    for clear in [false, true] {
        let own = if clear { up.clear_from as usize..up.count as usize } else { 0..up.clear_from as usize };
        draws.extend(within(&seen.vertices, own).into_iter().map(|r| (clear, Draw::Own(r))));
        for (n, shape) in mesh.shapes.iter().enumerate() {
            let corners = if clear { shape.clear_from..shape.vertices.end } else { shape.vertices.start..shape.clear_from };
            if corners.is_empty() {
                continue;
            }
            let corners = corners.start as u32..corners.end as u32;
            if gather {
                if let Some(copies) = up.seen_shapes.get(n).filter(|c| !c.is_empty()) {
                    draws.push((clear, Draw::Copies { corners, from: From::Seen, copies: copies.start as usize..copies.end as usize }));
                }
            } else {
                for run in within(&seen.instances, shape.instances.clone()) {
                    draws.push((clear, Draw::Copies { corners: corners.clone(), from: From::Mesh, copies: run }));
                }
            }
        }
        if !clear && boxes > 0 {
            let corners = mesh.cube.start as u32..mesh.cube.end as u32;
            draws.push((false, Draw::Copies { corners, from: From::StandIns, copies: 0..boxes }));
        }
        if !clear && !mesh.dots.is_empty() {
            draws.push((false, Draw::Dots(0..mesh.dots.len())));
        }
    }
    draws
}

/// A copy as the card reads it: where, its axes, its element, where its planes are less where its shape's vertices `start`, a
/// third, and its element as seen through.
fn put_copy(into: &mut Words, c: &Instance, start: u32) {
    put(into, &c.at);
    c.axes.iter().for_each(|v| put(into, v));
    put_words(into, &[c.owner, c.planes.wrapping_sub(start), c.clear_owner]);
}

/// What of `span` the runs `seen` (in order, apart) hold, in order.
fn within(seen: &[std::ops::Range<usize>], span: std::ops::Range<usize>) -> Vec<std::ops::Range<usize>> {
    let first = seen.partition_point(|r| r.end <= span.start);
    seen[first..]
        .iter()
        .take_while(|r| r.start < span.end)
        .map(|r| r.start.max(span.start)..r.end.min(span.end))
        .filter(|r| !r.is_empty())
        .collect()
}

fn put(into: &mut Words, floats: &[f32]) {
    into.extend(floats.iter().flat_map(|f| f.to_ne_bytes()));
}

fn put_words(into: &mut Words, words: &[u32]) {
    into.extend(words.iter().flat_map(|w| w.to_ne_bytes()));
}

fn entry(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry { binding, resource: buffer.as_entire_binding() }
}

/// A unit vector as four 16-bit signed fractions, the last 0.
fn put_unit(into: &mut Words, v: &[f32; 3]) {
    let q = |x: f32| (x.clamp(-1.0, 1.0) * 32767.0).round() as i16;
    into.extend([q(v[0]), q(v[1]), q(v[2]), 0].iter().flat_map(|q| q.to_ne_bytes()));
}

fn rgb(c: Rgb) -> [f32; 3] {
    [c.0, c.1, c.2].map(f32::from)
}

/// Whether the work since `scope` began ran the card out of memory; what else went wrong, as text.
fn out_of_memory(scopes: [wgpu::ErrorScopeGuard; 2]) -> Result<bool, String> {
    let [first, second] = scopes;
    let (memory, other) = (pollster::block_on(second.pop()), pollster::block_on(first.pop()));
    match (memory, other) {
        (Some(_), _) => Ok(true),
        (None, Some(e)) => Err(e.to_string()),
        (None, None) => Ok(false),
    }
}

/// Waits for the device to say whether the work since `scope` began went wrong: validation and out-of-memory errors come back
/// here as text rather than going to the device's handler, which would end the program.
fn caught(scopes: [wgpu::ErrorScopeGuard; 2]) -> Result<(), String> {
    let [first, second] = scopes;
    // Popped in the order they were pushed in reverse.
    let (inner, outer) = (pollster::block_on(second.pop()), pollster::block_on(first.pop()));
    match inner.or(outer) {
        Some(e) => Err(e.to_string()),
        None => Ok(()),
    }
}

fn scopes(device: &wgpu::Device) -> [wgpu::ErrorScopeGuard; 2] {
    [device.push_error_scope(wgpu::ErrorFilter::Validation), device.push_error_scope(wgpu::ErrorFilter::OutOfMemory)]
}

impl Gpu {
    /// The card egui draws on, if it can draw this view: it needs storage buffers in its shaders, which a few older ones lack.
    pub fn new(state: &RenderState) -> Result<Gpu, String> {
        // OpenGL can't read what the shaders need (a depth texture, buffers in a vertex shader), and when it can't wgpu panics in
        // the shader compiler rather than saying so: the card is for the backends that can.
        if state.adapter.get_info().backend == wgpu::Backend::Gl {
            return Err("needs Vulkan, Metal or DirectX 12, and the window is on OpenGL".into());
        }
        // Whatever else a driver's compiler stops at, it is a reason, not a crash.
        let made = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Gpu::from_device(state.device.clone(), state.queue.clone())));
        let mut gpu = made.unwrap_or_else(|_| Err("stopped the shader compiler".into()))?;
        gpu.adapter = state.adapter.get_info().name;
        gpu.state = Some(state.clone());
        Ok(gpu)
    }

    /// A card with no window to show it in: a picture is read back, not shown.
    pub fn from_device(device: wgpu::Device, queue: wgpu::Queue) -> Result<Gpu, String> {
        let limits = device.limits();
        if limits.max_storage_buffers_per_shader_stage < 3 || limits.max_color_attachment_bytes_per_sample < 20 {
            return Err("it can't read buffers in its shaders".into());
        }
        let guard = scopes(&device);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("bimcraft-3d"),
            source: wgpu::ShaderSource::Wgsl(include_str!("gpu.wgsl").into()),
        });
        let buffer = |ty: wgpu::BufferBindingType, binding: u32, visibility| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Buffer { ty, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let storage = wgpu::BufferBindingType::Storage { read_only: true };
        let (vertex, fragment) = (wgpu::ShaderStages::VERTEX, wgpu::ShaderStages::FRAGMENT);
        let group0 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bimcraft-3d-buffers"),
            entries: &[
                buffer(wgpu::BufferBindingType::Uniform, 0, vertex | fragment),
                buffer(storage, 1, vertex | fragment),
                buffer(storage, 2, vertex),
                buffer(storage, 3, fragment),
                buffer(storage, 4, fragment),
                buffer(storage, 5, vertex),
            ],
        });
        let texture = |binding: u32, sample_type| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: fragment,
            ty: wgpu::BindingType::Texture { sample_type, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
            count: None,
        };
        let (float, uint) = (wgpu::TextureSampleType::Float { filterable: false }, wgpu::TextureSampleType::Uint);
        let group1 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bimcraft-3d-targets"),
            entries: &[texture(0, float), texture(1, uint), texture(2, wgpu::TextureSampleType::Depth)],
        });
        let group2 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bimcraft-3d-clear-targets"),
            entries: &[texture(0, float), texture(1, uint)],
        });
        let layout = |label, groups: &[Option<&wgpu::BindGroupLayout>]| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: groups,
                immediate_size: 0,
            })
        };
        let surface_layout = layout("bimcraft-3d-scene", &[Some(&group0)]);
        let targets_layout = layout("bimcraft-3d-over", &[Some(&group0), Some(&group1)]);
        let picture_layout = layout("bimcraft-3d-picture", &[Some(&group0), Some(&group1), Some(&group2)]);
        let attributes =
            wgpu::vertex_attr_array![0 => Float32x3, 1 => Snorm16x4, 2 => Snorm16x4, 3 => Uint32, 4 => Uint32, 5 => Uint32, 6 => Uint32];
        let target = |format| Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL });
        let pipeline = |label,
                        layout: &wgpu::PipelineLayout,
                        vs: &'static str,
                        fs: &'static str,
                        buffers: &[Option<wgpu::VertexBufferLayout>],
                        targets: &[Option<wgpu::ColorTargetState>],
                        depth: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState { module: &module, entry_point: Some(vs), compilation_options: Default::default(), buffers },
                // A triangle that winds counter-clockwise in the picture faces the camera: `fs_scene` reads which from `front_facing`.
                primitive: wgpu::PrimitiveState { cull_mode: None, front_face: wgpu::FrontFace::Ccw, ..Default::default() },
                depth_stencil: depth.then(|| wgpu::DepthStencilState {
                    format: DEPTH,
                    depth_write_enabled: Some(true),
                    // Larger is nearer; the first of surfaces as near as each other stays, as in the processor's.
                    depth_compare: Some(wgpu::CompareFunction::Greater),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets,
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let vertex_layout =
            wgpu::VertexBufferLayout { array_stride: STRIDE, step_mode: wgpu::VertexStepMode::Vertex, attributes: &attributes };
        let copy_attributes = wgpu::vertex_attr_array![7 => Float32x3, 8 => Float32x3, 9 => Float32x3, 10 => Float32x3, 11 => Uint32, 12 => Uint32, 13 => Uint32];
        let instance_layout =
            wgpu::VertexBufferLayout { array_stride: INSTANCE, step_mode: wgpu::VertexStepMode::Instance, attributes: &copy_attributes };
        let instanced = pipeline(
            "bimcraft-3d-instanced",
            &surface_layout,
            "vs_instanced",
            "fs_scene",
            &[Some(vertex_layout.clone()), Some(instance_layout.clone())],
            &[target(COLOR), target(IDS)],
            true,
        );
        let instanced_cut = pipeline(
            "bimcraft-3d-instanced-cut",
            &surface_layout,
            "vs_instanced",
            "fs_scene_cut",
            &[Some(vertex_layout.clone()), Some(instance_layout.clone())],
            &[target(COLOR), target(IDS)],
            true,
        );
        let scene_cut = pipeline(
            "bimcraft-3d-scene-cut",
            &surface_layout,
            "vs_scene",
            "fs_scene_cut",
            &[Some(vertex_layout.clone())],
            &[target(COLOR), target(IDS)],
            true,
        );
        let instanced_clear = pipeline(
            "bimcraft-3d-instanced-clear",
            &targets_layout,
            "vs_instanced_clear",
            "fs_clear",
            &[Some(vertex_layout.clone()), Some(instance_layout)],
            &[target(COLOR), target(CLEAR_IDS)],
            true,
        );
        let scene = pipeline(
            "bimcraft-3d-scene",
            &surface_layout,
            "vs_scene",
            "fs_scene",
            &[Some(vertex_layout.clone())],
            &[target(COLOR), target(IDS)],
            true,
        );
        let clear = pipeline(
            "bimcraft-3d-clear",
            &targets_layout,
            "vs_scene",
            "fs_clear",
            &[Some(vertex_layout)],
            &[target(COLOR), target(CLEAR_IDS)],
            true,
        );
        let post = pipeline("bimcraft-3d-post", &picture_layout, "vs_full", "fs_post", &[], &[target(COLOR)], false);
        let lines = pipeline("bimcraft-3d-lines", &targets_layout, "vs_line", "fs_line", &[], &[target(COLOR)], false);
        let wire_attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Uint32];
        let wire_layout =
            wgpu::VertexBufferLayout { array_stride: WIRE, step_mode: wgpu::VertexStepMode::Instance, attributes: &wire_attributes };
        let wires =
            pipeline("bimcraft-3d-wires", &surface_layout, "vs_wire", "fs_wire", &[Some(wire_layout.clone())], &[target(COLOR)], false);
        let wires_tinted = pipeline(
            "bimcraft-3d-wires-tinted",
            &surface_layout,
            "vs_wire_tinted",
            "fs_wire",
            &[Some(wire_layout)],
            &[target(COLOR)],
            false,
        );
        let dot_attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Uint32, 2 => Uint32];
        let dot_layout =
            wgpu::VertexBufferLayout { array_stride: DOT, step_mode: wgpu::VertexStepMode::Instance, attributes: &dot_attributes };
        let dots =
            pipeline("bimcraft-3d-dots", &surface_layout, "vs_dot", "fs_dot", &[Some(dot_layout)], &[target(COLOR), target(IDS)], true);
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bimcraft-3d-globals"),
            size: GLOBALS,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let segments = (Gpu::storage(&device, SEGMENT), SEGMENT);
        caught(guard)?;
        Ok(Gpu {
            device,
            queue,
            state: None,
            scene,
            scene_cut,
            instanced_cut,
            instanced,
            instanced_clear,
            clear,
            post,
            lines,
            wires,
            wires_tinted,
            dots,
            group0,
            group1,
            group2,
            globals,
            segments,
            mesh: None,
            upload_worker: None,
            upload_requested: None,
            targets: None,
            texture: None,
            adapter: String::new(),
            vertex_bytes: VERTEX_BUFFER,
            cull: true,
            budget: FRAME_TRIANGLES,
            card_bytes: u64::MAX,
            full_at: None,
            swapped_ink: false,
        })
    }

    /// The card's name, as the driver gives it.
    pub fn adapter(&self) -> &str {
        &self.adapter
    }

    fn storage(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bimcraft-3d-storage"),
            size: size.max(4),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Where egui finds the last picture drawn.
    pub fn texture(&self) -> Option<TextureId> {
        self.texture
    }

    /// The picture's texture, for a test to read back.
    #[cfg(test)]
    pub(crate) fn picture(&self) -> Option<&wgpu::Texture> {
        self.targets.as_ref().map(|t| &t.picture)
    }

    /// [`draw_cut`](Self::draw_cut), to the mesh's own section box.
    #[cfg(test)]
    pub(crate) fn draw(&mut self, mesh: &Arc<Mesh>, cam: &Camera, tints: &[(Id, Rgb)], ground: Rgb) -> Result<bool, String> {
        self.draw_cut(mesh, cam, tints, ground, mesh.section_box)
    }

    /// Draws `mesh` as `cam` sees it, `tints` colouring (halfway, and the edges whole) what they name, on `ground` where nothing is,
    /// cut to section box `cut`: the mesh's own, or one being dragged, before its command runs. The picture is in
    /// [`texture`](Self::texture). An error leaves the card with nothing kept: the caller does without it. True when the picture
    /// is the exact one; false while it stands in, or the exact one is on its way: then draw again with the same camera on the
    /// next frame, until it is.
    pub fn draw_cut(&mut self, mesh: &Arc<Mesh>, cam: &Camera, tints: &[(Id, Rgb)], ground: Rgb, cut: Cut) -> Result<bool, String> {
        let guard = scopes(&self.device);
        let drawn = self.encode(mesh, cam, tints, ground, cut);
        let full = out_of_memory(guard);
        if let Some(up) = &self.mesh {
            self.card_bytes = self.card_bytes.min(up.room);
        }
        // The card ran out in the frame's own work: hold less and draw again, rather than give it up.
        if matches!(full, Ok(true)) && drawn.is_ok() && self.hold_less() {
            return Ok(false);
        }
        let caught = match full {
            Ok(true) => Err(TOO_LITTLE.to_string()),
            Ok(false) => Ok(()),
            Err(e) => Err(e),
        };
        if drawn.is_err() || caught.is_err() {
            (self.mesh, self.targets) = (None, None);
        }
        caught.and(drawn)
    }

    /// Asks the card to hold half of what it does, and starts the picture again. False where it holds no more than one buffer.
    pub(crate) fn hold_less(&mut self) -> bool {
        let Some(up) = &mut self.mesh else { return false };
        let buffers = |up: &Uploaded| KINDS.iter().map(|kind| up.store(*kind).held()).sum::<usize>();
        if buffers(up) < 2 {
            return false;
        }
        up.room = up.held / 2;
        while up.held > up.room && buffers(up) > 1 && up.let_go(&|_, _| false) {}
        up.room = up.room.max(up.held);
        up.progress = None;
        up.wire_stream = None;
        self.card_bytes = self.card_bytes.min(up.room);
        true
    }

    /// Starts the large first upload away from the interface thread. A result belongs to its exact mesh snapshot.
    /// Tests that call `draw` directly retain the synchronous path there.
    pub(crate) fn prepare_upload(&mut self, ctx: &egui::Context, mesh: &Arc<Mesh>) -> Result<bool, String> {
        if self.mesh.as_ref().is_some_and(|u| Arc::ptr_eq(&u.mesh, mesh)) {
            return Ok(true);
        }
        if let Some(done) = self.upload_worker.as_ref().and_then(crate::view_worker::Latest::take)
            && Arc::ptr_eq(&done.mesh, mesh)
        {
            let up = done.uploaded?;
            self.card_bytes = self.card_bytes.min(up.room);
            self.mesh = Some(up);
            return Ok(true);
        }
        if !self.upload_requested.as_ref().is_some_and(|m| Arc::ptr_eq(m, mesh)) {
            self.mesh = None;
            let job = UploadJob {
                device: self.device.clone(),
                queue: self.queue.clone(),
                vertex_bytes: self.vertex_bytes,
                room: Room { bytes: self.card_bytes, full_at: self.full_at },
                mesh: Arc::clone(mesh),
            };
            let wake = ctx.clone();
            let worker =
                self.upload_worker.get_or_insert_with(|| crate::view_worker::Latest::new(build_upload, move || wake.request_repaint()));
            worker.request(job);
            self.upload_requested = Some(Arc::clone(mesh));
        }
        if let Some(worker) = &self.upload_worker {
            worker.start()?;
            if let Some(error) = worker.take_error() {
                return Err(error);
            }
            if worker.busy() {
                ctx.request_repaint_after(std::time::Duration::from_millis(16));
            }
        }
        Ok(false)
    }

    /// How many bytes of the mesh's own vertices the card holds now.
    #[cfg(test)]
    pub(crate) fn holding(&self) -> u64 {
        self.mesh.as_ref().map_or(0, |up| up.held)
    }

    /// Waits for what was sent to the card to be done, for a measurement.
    #[cfg(test)]
    pub(crate) fn wait(&self) {
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
    }

    pub(crate) fn suspend_upload(&mut self) {
        self.upload_worker = None;
        self.upload_requested = None;
    }

    pub(crate) fn forget_mesh(&mut self) {
        self.suspend_upload();
        self.mesh = None;
    }

    fn encode(&mut self, mesh: &Arc<Mesh>, cam: &Camera, tints: &[(Id, Rgb)], ground: Rgb, cut: Cut) -> Result<bool, String> {
        let (w, h) = (cam.width.clamp(1, MAX_SIDE), cam.height.clamp(1, MAX_SIDE));
        if w.max(h) > self.device.limits().max_texture_dimension_2d {
            return Err("the picture is bigger than the card draws".into());
        }
        self.upload(mesh)?;
        self.size(w, h);
        // What the view can see, the rest left out as leaving no pixel; in the order it is drawn either way.
        let seen = if self.cull {
            mesh.visible_cut(cam, cut)
        } else {
            Visible {
                vertices: std::iter::once(0..mesh.vertices.len()).collect(),
                instances: std::iter::once(0..mesh.instances.len()).collect(),
            }
        };
        let (device, queue, full_at) = (&self.device, &self.queue, self.full_at);
        let write = |buffer: &wgpu::Buffer, bytes: &[u8]| {
            if !bytes.is_empty() {
                queue.write_buffer(buffer, 0, bytes);
            }
        };
        let (Some(up), Some(targets)) = (&mut self.mesh, &self.targets) else { return Err("nothing to draw on".into()) };
        // Each owner's edge, how much of it shows, and its tint: written again only when the tints change.
        if up.tints.as_deref() != Some(tints) {
            let tinted: BTreeMap<Id, Rgb> = tints.iter().copied().collect();
            let mut owners = Words::with_capacity(mesh.owners.len() * OWNER as usize);
            for o in &mesh.owners {
                let (e, t) = (rgb(o.edge), tinted.get(&o.id));
                let c = t.map_or([0.0; 3], |c| rgb(*c));
                let shows = 1.0 - f32::from(o.transparency) / 100.0;
                put(&mut owners, &[e[0], e[1], e[2], shows, c[0], c[1], c[2], if t.is_some() { 1.0 } else { 0.0 }]);
            }
            write(&up.owners, &owners);
            up.tints = Some(tints.to_vec());
        }
        // Each skin as the zoom looks it up: written again only when a level changes.
        let lookups = mesh.lookups(cam);
        if lookups != up.lookups {
            let mut skins = Words::new();
            for words in &lookups {
                put_words(&mut skins, words);
            }
            write(&up.skins, &skins);
            up.lookups = lookups;
        }
        // Each mark's lines, from the mesh's origin so that f32 holds them.
        let mut lines = Words::new();
        let mut count = 0u32;
        for marks in mesh.marks_cut(cam, tints, cut) {
            let c = rgb(marks.color);
            for ([a, b], wide) in &marks.lines {
                let rel = |p: &[f64; 3]| std::array::from_fn::<f32, 3, _>(|i| (p[i] - mesh.origin[i]) as f32);
                let (a, b) = (rel(a), rel(b));
                put(&mut lines, &[a[0], a[1], a[2], f32::from(*wide), b[0], b[1], b[2], 0.0, c[0], c[1], c[2], 0.0]);
                count += 1;
            }
        }
        if lines.len() as u64 > self.segments.1 {
            self.segments = (Gpu::storage(device, lines.len() as u64), lines.len() as u64);
            up.bind = None;
        }
        write(&self.segments.0, &lines);
        // The view: scaled so that a mm is as many pixels in clip space as in the picture, and the mesh's depth is 0 to 1.
        let view = mesh.view(cam);
        let (width, height) = (w as f32, h as f32);
        let through = up.clear_from < up.count || mesh.shapes.iter().any(|s| s.clear_from < s.vertices.end && !s.instances.is_empty());
        let mut globals = Words::new();
        // In perspective, a mm a mm in front of the eye: the focal length.
        let across = view.perspective.map_or(view.scale, |p| p.focal);
        put(&mut globals, &[view.right[0], view.right[1], view.right[2], 2.0 * across / width]);
        put(&mut globals, &[view.up[0], view.up[1], view.up[2], 2.0 * across / height]);
        put(&mut globals, &[view.back[0], view.back[1], view.back[2], 0.5 / view.depth]);
        put(&mut globals, &[view.target[0], view.target[1], view.target[2], view.scale]);
        let swapped = if mesh.lines && self.swapped_ink { 1.0 } else { 0.0 };
        put(&mut globals, &[mesh.origin[0] as f32, mesh.origin[1] as f32, mesh.origin[2] as f32, swapped]);
        put(&mut globals, &[rgb(ground)[0], rgb(ground)[1], rgb(ground)[2], if mesh.lines { 1.0 } else { 0.0 }]);
        // 1.5 px, as the processor lets a line show through a surface that near.
        let slack = view.perspective.map_or(1.5 / view.scale * 0.5 / view.depth, |p| 1.5 / p.focal);
        put(&mut globals, &[width, height, slack, if through { 1.0 } else { 0.0 }]);
        let lens = view.perspective.map_or([0.0; 4], |p| [p.focal, p.eye, p.a, p.b]);
        put(&mut globals, &lens);
        // The section box from the mesh's origin, as a surface's point is.
        let (lo, hi) = cut.unwrap_or_default();
        let from = |p: [f64; 3]| std::array::from_fn::<f32, 3, _>(|i| (p[i] - mesh.origin[i]) as f32);
        let (lo, hi) = (from(lo), from(hi));
        put(&mut globals, &[lo[0], lo[1], lo[2], if cut.is_some() { 1.0 } else { 0.0 }]);
        put(&mut globals, &[hi[0], hi[1], hi[2], 0.0]);
        write(&self.globals, &globals);
        // What this frame draws of the surfaces: nothing where they are drawn already for this camera (a tint changes only the
        // picture); the next part of the exact drawing while the camera stays; else, the camera moved, all it sees if that fits
        // in a frame, or the nearest of it and a box for each of the rest, finished exactly once the camera stays.
        let budget = self.budget.max(1);
        // A picture's points are few enough that they are all drawn: what is left of a frame is the surfaces'.
        let dots = mesh.dots.len();
        let frame = match &up.progress {
            Some((c, k, Stage::Exact { done, total })) if c == cam && *k == cut && done >= total => Frame::Picture,
            Some((c, k, Stage::Exact { done, .. })) if c == cam && *k == cut => Frame::Exact(*done),
            Some((c, k, Stage::Moving)) if c == cam && *k == cut => Frame::Exact(0),
            _ if mesh.cost(&seen).saturating_add(dots) <= budget
                && seen.vertices.iter().all(|r| up.vertices.holds(r))
                && up.dots.holds(&(0..mesh.dots.len())) =>
            {
                Frame::Exact(0)
            }
            _ => Frame::Moving,
        };
        let limit = self.vertex_bytes.min(device.limits().max_buffer_size);
        let (draws, first) = match frame {
            Frame::Picture => (Vec::new(), false),
            Frame::Exact(done) => {
                let all = surfaces(mesh, up, queue, device, &seen, &[], limit);
                let total: usize = all.iter().map(|(_, d)| d.cost()).sum();
                // A budget's worth, or as much as the card holds at once, what it has drawn let go for what comes next: the
                // picture is in the targets, whatever is held, so however little it holds it comes to the same one.
                let mut end = done.saturating_add(budget).min(total);
                let part = loop {
                    let part = part_of(&all, done, end);
                    let wanted = up.wanted_by(&part);
                    let bytes: u64 = wanted.iter().map(|(kind, k)| up.store(*kind).bytes_of(*k)).sum();
                    if bytes <= up.room {
                        let keep = |kind: Kind, k: usize| wanted.contains(&(kind, k));
                        let mut whole = true;
                        for &one in &wanted {
                            whole &= up.bring_in((device, queue), mesh, one, &keep, full_at)?;
                        }
                        if whole {
                            break part;
                        }
                    }
                    if end <= done.saturating_add(1) {
                        return Err(TOO_LITTLE.into());
                    }
                    end = done + ((end - done) / 2).max(1);
                };
                up.progress = Some((*cam, cut, Stage::Exact { done: end, total }));
                (part, done == 0)
            }
            Frame::Moving => {
                // A buffer a frame, the nearest the view wants and the card doesn't hold, in place of one the view doesn't see;
                // then the points, where there's room with what the view sees.
                let seen_now: Vec<usize> = seen.vertices.iter().flat_map(|r| up.vertices.of(r)).collect();
                let keep = |kind: Kind, k: usize| kind == Kind::Own && seen_now.contains(&k);
                let next = mesh.nearest_not_held(cam, &seen, &|r| up.vertices.holds(r));
                if let Some(next) = next {
                    for k in up.vertices.of(&next) {
                        up.bring_in((device, queue), mesh, (Kind::Own, k), &keep, full_at)?;
                    }
                } else if let Some(k) = up.dots.buffers.iter().position(Option::is_none) {
                    let keep_points = |kind: Kind, n: usize| kind == Kind::Dots || keep(kind, n);
                    up.bring_in((device, queue), mesh, (Kind::Dots, k), &keep_points, full_at)?;
                }
                let near = mesh.nearest_first_held(cam, &seen, budget.saturating_sub(dots), &|r| up.vertices.holds(r));
                up.progress = Some((*cam, cut, Stage::Moving));
                (surfaces(mesh, up, queue, device, &near.seen, &near.stand_ins, limit), true)
            }
        };
        let complete = matches!(up.progress, Some((_, _, Stage::Exact { done, total })) if done >= total);
        // The picture is made again when the surfaces are whole, exact or standing in; while the exact ones are on their way it
        // stays what the camera last showed.
        let picture = complete || frame == Frame::Moving;
        let bind = up
            .bind
            .get_or_insert_with(|| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("bimcraft-3d-buffers"),
                    layout: &self.group0,
                    entries: &[
                        entry(0, &self.globals),
                        entry(1, &up.owners),
                        entry(2, &self.segments.0),
                        entry(3, &up.skins),
                        entry(4, &up.texels),
                        entry(5, &up.planes),
                    ],
                })
            })
            .clone();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("bimcraft-3d") });
        // The opaque surfaces, then (when the view sees through any) those seen through, the nearest in front of them: begun
        // afresh in a frame's first part, and carried on in the same targets in the next.
        let load = if first { wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT) } else { wgpu::LoadOp::Load };
        let depth_load = if first { wgpu::LoadOp::Clear(0.0) } else { wgpu::LoadOp::Load };
        for (clear, color, ids, z) in
            [(false, &targets.color, &targets.ids, &targets.depth), (true, &targets.clear_color, &targets.clear_ids, &targets.clear_depth)]
        {
            if (clear && !through) || (!first && !draws.iter().any(|(c, _)| *c == clear)) {
                continue;
            }
            let attachment = |view| wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(if clear { "bimcraft-3d-seen-through" } else { "bimcraft-3d-surfaces" }),
                color_attachments: &[Some(attachment(color)), Some(attachment(ids))],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: z,
                    depth_ops: Some(wgpu::Operations { load: depth_load, store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let (own, copies) = match (clear, cut.is_some()) {
                (true, _) => (&self.clear, &self.instanced_clear),
                (false, true) => (&self.scene_cut, &self.instanced_cut),
                (false, false) => (&self.scene, &self.instanced),
            };
            let mut set = None;
            for (_, draw) in draws.iter().filter(|(c, _)| *c == clear) {
                let pipeline = match draw {
                    Draw::Own(_) => own,
                    Draw::Copies { .. } => copies,
                    Draw::Dots(_) => &self.dots,
                };
                if set != Some(pipeline as *const wgpu::RenderPipeline) {
                    pass.set_pipeline(pipeline);
                    pass.set_bind_group(0, &bind, &[]);
                    // What is seen through reads the opaque depth, which the opaque pass wrote.
                    if clear {
                        pass.set_bind_group(1, &targets.bind, &[]);
                    }
                    set = Some(pipeline as *const wgpu::RenderPipeline);
                }
                match draw {
                    Draw::Own(range) => {
                        for (k, part) in spans(up.vertices.per, range.start as u64..range.end as u64) {
                            if let Some(Some(buffer)) = up.vertices.buffers.get(k) {
                                pass.set_vertex_buffer(0, buffer.slice(..));
                                pass.draw(part, 0..1);
                            }
                        }
                    }
                    Draw::Copies { corners, from, copies } => {
                        let Some(shapes) = &up.shape_vertices else { continue };
                        pass.set_vertex_buffer(0, shapes.slice(..));
                        let one = match from {
                            From::Mesh => None,
                            From::Seen => up.seen.as_ref().map(|(b, _)| b),
                            From::StandIns => up.stand_ins.as_ref().map(|(b, _)| b),
                        };
                        match (from, one) {
                            (From::Mesh, _) => {
                                for (k, part) in spans(up.instances_per, copies.start as u64..copies.end as u64) {
                                    if let Some(buffer) = up.instances.get(k) {
                                        pass.set_vertex_buffer(1, buffer.slice(..));
                                        pass.draw(corners.clone(), part);
                                    }
                                }
                            }
                            (_, Some(buffer)) => {
                                pass.set_vertex_buffer(1, buffer.slice(..));
                                pass.draw(corners.clone(), copies.start as u32..copies.end as u32);
                            }
                            (_, None) => {}
                        }
                    }
                    Draw::Dots(range) => {
                        for (k, part) in spans(up.dots.per, range.start as u64..range.end as u64) {
                            if let Some(Some(buffer)) = up.dots.buffers.get(k) {
                                pass.set_vertex_buffer(0, buffer.slice(..));
                                pass.draw(0..6, part);
                            }
                        }
                    }
                }
            }
        }
        if !picture {
            self.queue.submit([encoder.finish()]);
            return Ok(false);
        }
        // A wireframe past what the card holds: what it holds stands in on a moved view, and the exact picture is drawn a part at a
        // time into the colour target, shown once it is whole.
        let lines = mesh.wires.len() as u64;
        let streams = mesh.wireframe && lines > 0 && up.wires.count * WIRE > up.room;
        let mut stands_in = false;
        if streams {
            let key = (*cam, cut, tints.to_vec(), self.swapped_ink);
            let passes: &[&wgpu::RenderPipeline] = if tints.is_empty() { &[&self.wires] } else { &[&self.wires, &self.wires_tinted] };
            let buffers = up.wires.buffers.len();
            let total = buffers * passes.len();
            match up.wire_stream.as_ref().filter(|stream| stream.key == key).map(|stream| stream.done) {
                Some(done) if done >= total => return Ok(true),
                Some(done) => {
                    // As many parts as it holds at once and a frame's budget lets it draw, what it has drawn let go.
                    let (mut end, mut bytes, mut drawn) = (done, 0, 0);
                    let mut wanted: Vec<usize> = Vec::new();
                    while end < total {
                        let k = end % buffers;
                        let (size, count) = (up.wires.bytes_of(k), (up.wires.bytes_of(k) / WIRE) as usize);
                        let fresh = !wanted.contains(&k);
                        if end > done && ((fresh && bytes + size > up.room) || (drawn + count) * 2 > budget) {
                            break;
                        }
                        if fresh {
                            bytes += size;
                            wanted.push(k);
                        }
                        drawn += count;
                        end += 1;
                    }
                    let keep = |kind: Kind, k: usize| kind == Kind::Wires && wanted.contains(&k);
                    for &k in &wanted {
                        if !up.bring_in((device, queue), mesh, (Kind::Wires, k), &keep, full_at)? {
                            // It held less than it said: try again, for less, next frame; a buffer alone it can't hold at all.
                            if wanted.len() == 1 {
                                return Err(TOO_LITTLE.into());
                            }
                            self.queue.submit([encoder.finish()]);
                            return Ok(false);
                        }
                    }
                    let paper = if swapped > 0.5 { 26.0 / 255.0 } else { 1.0 };
                    let load = if done == 0 {
                        wgpu::LoadOp::Clear(wgpu::Color { r: paper, g: paper, b: paper, a: 1.0 })
                    } else {
                        wgpu::LoadOp::Load
                    };
                    {
                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("wires, a part"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: &targets.color,
                                depth_slice: None,
                                resolve_target: None,
                                ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
                            })],
                            depth_stencil_attachment: None,
                            timestamp_writes: None,
                            occlusion_query_set: None,
                            multiview_mask: None,
                        });
                        pass.set_bind_group(0, &bind, &[]);
                        for part in done..end {
                            if let Some((pipeline, Some(Some(buffer)))) =
                                passes.get(part / buffers).zip(Some(up.wires.buffers.get(part % buffers)))
                            {
                                pass.set_pipeline(pipeline);
                                pass.set_vertex_buffer(0, buffer.slice(..));
                                pass.draw(0..6, 0..(up.wires.bytes_of(part % buffers) / WIRE) as u32);
                            }
                        }
                    }
                    let whole = end >= total;
                    if whole {
                        let size = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };
                        encoder.copy_texture_to_texture(targets.color_texture.as_image_copy(), targets.picture.as_image_copy(), size);
                    }
                    up.wire_stream = Some(WireStream { key, done: end });
                    self.queue.submit([encoder.finish()]);
                    return Ok(whole);
                }
                None => {
                    up.wire_stream = Some(WireStream { key, done: 0 });
                    stands_in = true;
                }
            }
        } else if mesh.wireframe {
            let keep = |kind: Kind, _: usize| kind == Kind::Wires;
            for k in 0..up.wires.buffers.len() {
                up.bring_in((device, queue), mesh, (Kind::Wires, k), &keep, full_at)?;
            }
        }
        for (name, pipeline, load, instances) in
            [("edges", &self.post, wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), 1), ("levels", &self.lines, wgpu::LoadOp::Load, count)]
        {
            if instances == 0 {
                continue;
            }
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(name),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &targets.picture_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.set_bind_group(1, &targets.bind, &[]);
            if name == "edges" {
                pass.set_bind_group(2, &targets.clear_bind, &[]);
            }
            // The edges are one triangle over the whole picture; the levels' lines two to a segment.
            pass.draw(0..if name == "edges" { 3 } else { 6 }, 0..instances);
        }
        // A wireframe's edges over the paper, in their order, and then those tinted over them.
        if mesh.wireframe && !mesh.wires.is_empty() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("wires"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &targets.picture_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let passes: &[&wgpu::RenderPipeline] = if tints.is_empty() { &[&self.wires] } else { &[&self.wires, &self.wires_tinted] };
            for pipeline in passes {
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, &bind, &[]);
                for (k, part) in spans(up.wires.per, 0..lines) {
                    if let Some(Some(buffer)) = up.wires.buffers.get(k) {
                        pass.set_vertex_buffer(0, buffer.slice(..));
                        pass.draw(0..6, part);
                    }
                }
            }
        }
        self.queue.submit([encoder.finish()]);
        Ok(complete && !stands_in)
    }

    /// Puts `mesh` on the card, unless it's there already.
    fn upload(&mut self, mesh: &Arc<Mesh>) -> Result<(), String> {
        if self.mesh.as_ref().is_some_and(|u| Arc::ptr_eq(&u.mesh, mesh)) {
            return Ok(());
        }
        self.mesh = None;
        let room = Room { bytes: self.card_bytes, full_at: self.full_at };
        self.mesh = Some(Self::make_uploaded(&self.device, &self.queue, self.vertex_bytes, room, mesh, &|| false)?);
        Ok(())
    }

    fn make_uploaded(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        vertex_bytes: u64,
        room: Room,
        mesh: &Arc<Mesh>,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Uploaded, String> {
        let binding = device.limits().max_storage_buffer_binding_size;
        if mesh.texels.len() as u64 * 4 > binding
            || mesh.owners.len() as u64 * OWNER > binding
            || mesh.instance_planes.len() as u64 * 4 > binding
        {
            return Err("the model is bigger than the card holds".into());
        }
        if mesh.shape_vertices.len() as u64 * STRIDE > device.limits().max_buffer_size {
            return Err("the model's shapes are bigger than a buffer the card holds".into());
        }
        // Whole triangles to a buffer, written and sent one at a time, so that what waits to go up is one buffer's.
        let per = (vertex_bytes.min(device.limits().max_buffer_size) / STRIDE / 3).max(1) * 3;
        // As many as the card holds; the rest come when a view wants them.
        let (mut held, mut most) = (0u64, room.bytes);
        let limit = vertex_bytes.min(device.limits().max_buffer_size);
        let mut fill = |kind: Kind, count: usize, per: u64, stride: u64| -> Result<Store, String> {
            let mut store = Store { buffers: Vec::new(), per, count: count as u64, stride };
            for k in 0..count.div_ceil(per as usize) {
                if cancelled() {
                    return Err("3D upload was cancelled".into());
                }
                let size = store.bytes_of(k);
                if held + size > most {
                    store.buffers.push(None);
                    continue;
                }
                let made =
                    store_bytes(mesh, kind, k, per as usize, cancelled).and_then(|bytes| hold(device, queue, &bytes, held, room.full_at));
                match made {
                    Ok(buffer) => {
                        store.buffers.push(Some(buffer));
                        held += size;
                    }
                    Err(Hold::Full) => {
                        most = held;
                        store.buffers.push(None);
                    }
                    Err(Hold::Failed(e)) => return Err(e),
                }
            }
            Ok(store)
        };
        let vertices = fill(Kind::Own, mesh.vertices.len(), per, STRIDE)?;
        let wires = fill(Kind::Wires, mesh.wires.len(), (limit / WIRE).max(3), WIRE)?;
        let dots = fill(Kind::Dots, mesh.dots.len(), (limit / DOT).max(3), DOT)?;
        if held == 0 && !(mesh.vertices.is_empty() && mesh.wires.is_empty() && mesh.dots.is_empty()) {
            return Err(TOO_LITTLE.into());
        }
        let mut bytes = Words::new();
        if cancelled() {
            return Err("3D upload was cancelled".into());
        }
        let vertex_buffer = |label: &str, bytes: &[u8]| {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: bytes.len() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            queue.write_buffer(&buffer, 0, bytes);
            buffer
        };
        // The shapes, each vertex in its family's axes.
        let shape_vertices = (!mesh.shape_vertices.is_empty()).then(|| {
            bytes.clear();
            for Vertex { position, normal, face, color, owner, plane, tex } in &mesh.shape_vertices {
                put(&mut bytes, position);
                put_unit(&mut bytes, normal);
                put_unit(&mut bytes, face);
                put_words(&mut bytes, &[*color, *owner, *plane, *tex]);
            }
            vertex_buffer("bimcraft-3d-shapes", &bytes)
        });
        // The copies, each knowing where its shape's vertices start, so that a vertex's index finds its triangle's plane.
        let instances_per = (vertex_bytes.min(device.limits().max_buffer_size) / INSTANCE).max(3);
        let mut instances = Vec::new();
        let starts: Vec<u32> =
            mesh.shapes.iter().flat_map(|s| std::iter::repeat_n((s.vertices.start / 3) as u32, s.instances.len())).collect();
        for (part, starts) in mesh.instances.chunks(instances_per as usize).zip(starts.chunks(instances_per as usize)) {
            if cancelled() {
                return Err("3D upload was cancelled".into());
            }
            bytes.clear();
            for (c, start) in part.iter().zip(starts) {
                put_copy(&mut bytes, c, *start);
            }
            instances.push(vertex_buffer("bimcraft-3d-instances", &bytes));
            queue.submit([]);
        }
        let planes = Gpu::storage(device, mesh.instance_planes.len() as u64 * 4);
        if !mesh.instance_planes.is_empty() {
            let mut words = Words::with_capacity(mesh.instance_planes.len() * 4);
            put_words(&mut words, &mesh.instance_planes);
            queue.write_buffer(&planes, 0, &words);
        }
        let texels = Gpu::storage(device, mesh.texels.len() as u64 * 4);
        let mut words = Words::with_capacity(mesh.texels.len() * 4);
        put_words(&mut words, &mesh.texels);
        if !words.is_empty() {
            queue.write_buffer(&texels, 0, &words);
        }
        Ok(Uploaded {
            mesh: Arc::clone(mesh),
            vertices,
            held,
            room: most,
            count: mesh.vertices.len() as u64,
            clear_from: mesh.clear_from.min(mesh.vertices.len()) as u64,
            shape_vertices,
            instances,
            instances_per,
            wires,
            wire_stream: None,
            dots,
            seen: None,
            seen_runs: Vec::new(),
            seen_shapes: Vec::new(),
            stand_ins: None,
            progress: None,
            planes,
            tints: None,
            lookups: Vec::new(),
            bind: None,
            // An array in a shader is at least one element long.
            owners: Gpu::storage(device, mesh.owners.len().max(1) as u64 * OWNER),
            skins: Gpu::storage(device, mesh.skins.len().max(1) as u64 * SKIN),
            texels,
        })
    }

    /// Targets of `w` by `h` pixels, made again where the picture's size changed, and put in egui's reach.
    fn size(&mut self, w: u32, h: u32) {
        if self.targets.as_ref().is_some_and(|t| t.size == (w, h)) {
            return;
        }
        // New targets hold no surfaces yet.
        if let Some(up) = &mut self.mesh {
            up.progress = None;
            up.wire_stream = None;
        }
        let texture = |label: &str, format: wgpu::TextureFormat, usage: wgpu::TextureUsages| {
            self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let (render, sampled) = (wgpu::TextureUsages::RENDER_ATTACHMENT, wgpu::TextureUsages::TEXTURE_BINDING);
        let view = |t: &wgpu::Texture| t.create_view(&wgpu::TextureViewDescriptor::default());
        let color_texture = texture("bimcraft-3d-color", COLOR, render | sampled | wgpu::TextureUsages::COPY_SRC);
        let (color, ids, depth) = (
            view(&color_texture),
            view(&texture("bimcraft-3d-ids", IDS, render | sampled)),
            view(&texture("bimcraft-3d-depth", DEPTH, render | sampled)),
        );
        let (clear_color, clear_ids, clear_depth) = (
            view(&texture("bimcraft-3d-clear-color", COLOR, render | sampled)),
            view(&texture("bimcraft-3d-clear-ids", CLEAR_IDS, render | sampled)),
            view(&texture("bimcraft-3d-clear-depth", DEPTH, render)),
        );
        let picture =
            texture("bimcraft-3d-picture", COLOR, render | sampled | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST);
        let picture_view = view(&picture);
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bimcraft-3d-targets"),
            layout: &self.group1,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&color) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&ids) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&depth) },
            ],
        });
        let clear_bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bimcraft-3d-clear-targets"),
            layout: &self.group2,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&clear_color) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&clear_ids) },
            ],
        });
        if let Some(state) = &self.state {
            let mut renderer = state.renderer.write();
            match self.texture {
                Some(id) => renderer.update_egui_texture_from_wgpu_texture(&self.device, &picture_view, wgpu::FilterMode::Nearest, id),
                None => self.texture = Some(renderer.register_native_texture(&self.device, &picture_view, wgpu::FilterMode::Nearest)),
            }
        }
        self.targets = Some(Targets {
            size: (w, h),
            color,
            color_texture,
            ids,
            depth,
            clear_color,
            clear_ids,
            clear_depth,
            picture,
            picture_view,
            bind,
            clear_bind,
        });
    }
}

/// The machine's graphics card with no window to show on, for tests; none on a runner that has no adapter (those tests say so
/// and pass).
#[cfg(test)]
pub(crate) fn headless() -> Option<Gpu> {
    // Without the debug names a debug build gives each object: tests make cards side by side, and the Vulkan loader looks a name's
    // device up in lists it doesn't lock against another thread making or dropping an instance, which crashes one run in a few.
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        flags: wgpu::InstanceFlags::empty(),
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
    // OpenGL is not the card's (see `Gpu::new`).
    if adapter.get_info().backend == wgpu::Backend::Gl {
        return None;
    }
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_limits: limits_for(&adapter, wgpu::Limits::default()),
            ..Default::default()
        }))
        .ok()?;
    match Gpu::from_device(device, queue) {
        Ok(gpu) => Some(gpu),
        // A software adapter may lack what the card needs; a real one that can't draw is a bug to hear about.
        Err(_) if adapter.get_info().device_type == wgpu::DeviceType::Cpu => None,
        Err(e) => panic!("{} can't draw the 3D view: {e}", adapter.get_info().name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bimcraft_core::engine::Session;
    use bimcraft_core::graphics::Look;
    use bimcraft_core::render::PAPER;
    use bimcraft_core::view3d::{AZIMUTH, ELEVATION, Scene};

    /// The picture's pixels as rows of red, green, blue and alpha.
    fn read(gpu: &Gpu) -> Vec<u8> {
        let texture = gpu.picture().expect("a picture drawn");
        let (w, h) = (texture.width() as usize, texture.height() as usize);
        let row = (w * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize);
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (row * h) as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row as u32), rows_per_image: None },
            },
            texture.size(),
        );
        gpu.queue.submit([encoder.finish()]);
        buffer.slice(..).map_async(wgpu::MapMode::Read, |_| ());
        gpu.device.poll(wgpu::PollType::wait_indefinitely()).expect("the card finishes");
        let mapped = buffer.slice(..).get_mapped_range().expect("a mapped buffer");
        mapped.chunks_exact(row).flat_map(|r| r[..w * 4].to_vec()).collect()
    }

    /// Whether the card's picture of `s`'s 3D view, with `tints`, is the processor's: no more than one pixel in 200 further
    /// than six levels of a channel from it.
    fn draws_as_the_processor(gpu: &mut Gpu, s: &Session, tints: &[(Id, Rgb)]) {
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        draws_through(gpu, s, tints, scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look));
    }

    /// [`draws_as_the_processor`] through `cam`.
    fn draws_through(gpu: &mut Gpu, s: &Session, tints: &[(Id, Rgb)], cam: Camera) {
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let mesh = Arc::new(Mesh::new(&scene, &look).unwrap());
        let cpu = scene.render_in(&cam, tints, &look);
        gpu.draw(&mesh, &cam, tints, PAPER).unwrap();
        let pixels = read(gpu);
        assert_eq!(pixels.len(), cpu.rgba.len());
        let far = |a: &[u8], b: &[u8]| a[..3].iter().zip(&b[..3]).any(|(x, y)| x.abs_diff(*y) > 6);
        let wrong: Vec<usize> = (0..pixels.len() / 4).filter(|&k| far(&pixels[4 * k..4 * k + 4], &cpu.rgba[4 * k..4 * k + 4])).collect();
        // DUMP=<folder> keeps both pictures, and the pixels that differ, as raw rows of red, green, blue and alpha.
        if let Ok(dir) = std::env::var("DUMP") {
            let raw = |name: &str, rgba: &[u8]| std::fs::write(format!("{dir}/{name}.rgba"), rgba).unwrap();
            raw("card", &pixels);
            raw("processor", &cpu.rgba);
            let mut diff = vec![0u8; pixels.len()];
            for &k in &wrong {
                diff[4 * k..4 * k + 4].copy_from_slice(&[255, 0, 0, 255]);
            }
            raw("difference", &diff);
        }
        assert!(wrong.len() * 200 < pixels.len() / 4, "{} of {} pixels differ from the processor's picture", wrong.len(), pixels.len() / 4);
    }

    fn sample() -> Session {
        let mut s = Session::new();
        s.script(bimcraft_core::sample::SAMPLE).unwrap();
        s
    }

    #[test]
    fn worker_upload_keeps_only_the_latest_mesh_and_draws_the_same_pixels() {
        let Some(mut gpu) = headless() else { return };
        let s = sample();
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let cam = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
        let perspective = scene.fit_from(Camera { lens: Some(bimcraft_core::view3d::LENS), ..cam }, &look);
        let first = Arc::new(Mesh::new(&scene, &look).unwrap());
        let latest = Arc::new(Mesh::new(&scene, &look).unwrap());
        gpu.draw(&first, &cam, &[], PAPER).unwrap();
        let pixels = read(&gpu);
        gpu.draw(&first, &perspective, &[], PAPER).unwrap();
        let perspective_pixels = read(&gpu);
        gpu.mesh = None;
        let ctx = egui::Context::default();
        assert!(!gpu.prepare_upload(&ctx, &first).unwrap());
        assert!(!gpu.prepare_upload(&ctx, &latest).unwrap());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !gpu.prepare_upload(&ctx, &latest).unwrap() {
            assert!(std::time::Instant::now() < deadline, "worker upload did not finish");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(gpu.mesh.as_ref().is_some_and(|up| Arc::ptr_eq(&up.mesh, &latest)));
        gpu.draw(&latest, &cam, &[], PAPER).unwrap();
        assert_eq!(read(&gpu), pixels);
        gpu.draw(&latest, &perspective, &[], PAPER).unwrap();
        assert_eq!(read(&gpu), perspective_pixels);
    }

    /// The sample with copies of every kind: three lamps, each a frame and a lens seen through, and round runs, one tube
    /// stretched and turned (level, sloped, upright and at an angle in plan).
    fn with_copies() -> Session {
        let mut s = sample();
        // And three lamps, each a frame and a lens seen through.
        s.execute("material", &serde_json::json!({"name": "Lens", "color": [250, 250, 230], "transparency": 60})).unwrap();
        s.execute(
            "family",
            &serde_json::json!({"name": "Lamp", "types": {"A": {}}, "forms": [
                {"profile": [[0,0],[600,0],[600,600],[0,600]], "from": 2400, "to": 2500, "material": "Concrete"},
                {"profile": [[50,50],[550,50],[550,550],[50,550]], "from": 2300, "to": 2400, "material": "Lens"}]}),
        )
        .unwrap();
        for (x, angle) in [(2000, 0), (5000, 30), (8000, 90)] {
            s.execute("component", &serde_json::json!({"family": "Lamp", "at": [x, 3000], "angle": angle})).unwrap();
        }
        // And round runs, one tube stretched and turned: level, sloped, upright and at an angle in plan.
        for (command, points, offset, end) in [
            ("pipe", [[1000, 1000], [7000, 1000]], 2600, 2600),
            ("pipe", [[1000, 2000], [6000, 5000]], 2700, 2200),
            ("pipe", [[3000, 3000], [3000, 3000]], 0, 2800),
            ("conduit", [[2000, 6000], [2000, 1000]], 2800, 2500),
        ] {
            s.execute(command, &serde_json::json!({"points": points, "offset": offset, "end_offset": end})).unwrap();
        }
        s
    }

    #[test]
    fn copies_placed_draw_the_pixels_of_their_own_triangles_and_tubes_of_their_runs() {
        let Some(mut gpu) = headless() else { return };
        let s = with_copies();
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let (mesh, flat) = (Arc::new(Mesh::new(&scene, &look).unwrap()), Arc::new(Mesh::flat(&scene, &look).unwrap()));
        assert!(mesh.instances.len() > 8 && flat.instances.is_empty());
        assert!(mesh.instances.iter().any(|i| i.clear_owner != 0), "the lamps' lenses are copies too");
        let chair = mesh.owners[mesh.instances[0].owner as usize - 1].id;
        let parallel = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
        let perspective = scene.fit_from(Camera { lens: Some(bimcraft_core::view3d::LENS), ..parallel }, &look);
        for (name, cam) in [("parallel", parallel), ("perspective", perspective), ("inside", inside(&s, perspective))] {
            for tints in [vec![], vec![(chair, Rgb(255, 160, 0))]] {
                gpu.draw(&flat, &cam, &tints, PAPER).unwrap();
                let own = read(&gpu);
                gpu.draw(&mesh, &cam, &tints, PAPER).unwrap();
                let placed = read(&gpu);
                let differ = own.chunks(4).zip(placed.chunks(4)).filter(|(a, b)| a != b).count();
                assert_eq!(differ, 0, "{name}, {} tints: {differ} pixels differ", tints.len());
            }
        }
    }

    #[test]
    fn leaving_out_what_the_view_cannot_see_draws_the_same_pixels() {
        let Some(mut all) = headless() else { return };
        let Some(mut culled) = headless() else { return };
        all.cull = false;
        // A few triangles to a buffer, so that what is left out falls across buffers too.
        culled.vertex_bytes = STRIDE * 3 * 25;
        let mut s = with_copies();
        for seen_through in [false, true] {
            if seen_through {
                s.execute("graphics", &serde_json::json!({"view": "{3D}", "categories": {"walls": {"transparency": 50}}})).unwrap();
            }
            let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
            let mesh = Arc::new(Mesh::new(&scene, &look).unwrap());
            let chair = mesh.owners[mesh.instances[0].owner as usize - 1].id;
            let parallel = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
            let near =
                Camera { scale: parallel.scale * 8.0, target: [parallel.target[0] - 3000.0, parallel.target[1] - 2000.0, 0.0], ..parallel };
            let perspective = scene.fit_from(Camera { lens: Some(bimcraft_core::view3d::LENS), ..parallel }, &look);
            for (name, cam) in [("parallel", parallel), ("near", near), ("perspective", perspective), ("inside", inside(&s, perspective))] {
                let seen = mesh.visible(&cam);
                let drawn = seen.vertices.iter().map(|r| r.len()).sum::<usize>() + seen.instances.iter().map(|r| r.len()).sum::<usize>();
                if matches!(name, "near" | "inside") {
                    assert!(drawn < mesh.vertices.len() + mesh.instances.len(), "{name}: something is left out");
                    let copies: usize = seen.instances.iter().map(|r| r.len()).sum();
                    assert!(0 < copies && copies < mesh.instances.len(), "{name}: {copies} of {} copies gathered", mesh.instances.len());
                }
                for tints in [vec![], vec![(chair, Rgb(255, 160, 0))]] {
                    for gpu in [&mut all, &mut culled] {
                        gpu.draw(&mesh, &cam, &tints, PAPER).unwrap();
                    }
                    let (a, b) = (read(&all), read(&culled));
                    let differ = a.chunks(4).zip(b.chunks(4)).filter(|(a, b)| a != b).count();
                    assert_eq!(differ, 0, "{name}, seen through {seen_through}, {} tints: {differ} pixels differ", tints.len());
                }
            }
        }
    }

    #[test]
    fn past_a_frame_s_budget_the_view_stands_in_while_moving_and_settles_on_the_exact_picture() {
        let Some(mut once) = headless() else { return };
        let Some(mut parts) = headless() else { return };
        let mut s = with_copies();
        for seen_through in [false, true] {
            if seen_through {
                s.execute("graphics", &serde_json::json!({"view": "{3D}", "categories": {"walls": {"transparency": 50}}})).unwrap();
            }
            let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
            let mesh = Arc::new(Mesh::new(&scene, &look).unwrap());
            let chair = mesh.owners[mesh.instances[0].owner as usize - 1].id;
            let parallel = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
            let perspective = scene.fit_from(Camera { lens: Some(bimcraft_core::view3d::LENS), ..parallel }, &look);
            for (name, cam) in [("parallel", parallel), ("perspective", perspective), ("inside", inside(&s, perspective))] {
                // A budget of a seventh of what the view sees: many frames to finish, each part's end inside a copy or a run.
                parts.budget = mesh.cost(&mesh.visible(&cam)) / 7 + 1;
                assert!(once.draw(&mesh, &cam, &[], PAPER).unwrap(), "{name}: within its budget, exact at once");
                let exact = read(&once);
                // Come to from another camera: the first frame stands in.
                let turned = Camera { azimuth: cam.azimuth + 10.0, ..cam };
                parts.draw(&mesh, &turned, &[], PAPER).unwrap();
                let standing = parts.draw(&mesh, &cam, &[], PAPER).unwrap();
                let first = read(&parts);
                assert!(!standing, "{name}: past the budget, the first frame stands in");
                let mut frames = 1;
                while !parts.draw(&mesh, &cam, &[], PAPER).unwrap() {
                    frames += 1;
                    assert!(frames < 50, "{name}: never settles");
                    // While the exact surfaces come, the picture stays what stood in.
                    assert_eq!(read(&parts), first, "{name}: the picture changed before the exact one was whole");
                }
                assert!(frames >= 3, "{name}: {frames} frames");
                let settled = read(&parts);
                let differ = exact.chunks(4).zip(settled.chunks(4)).filter(|(a, b)| a != b).count();
                assert_eq!(differ, 0, "{name}, seen through {seen_through}: {differ} pixels differ from the picture drawn at once");
                assert_ne!(first, settled, "{name}: what stood in is not the exact picture");
                // A tint over surfaces already whole is the picture again, at once and as if drawn afresh.
                let tints = [(chair, Rgb(255, 160, 0))];
                assert!(parts.draw(&mesh, &cam, &tints, PAPER).unwrap(), "{name}: a tint draws no surfaces");
                once.draw(&mesh, &cam, &tints, PAPER).unwrap();
                assert_eq!(read(&parts), read(&once), "{name}: tinted");
            }
        }
    }

    #[test]
    fn the_card_draws_what_the_processor_does() {
        let Some(mut gpu) = headless() else {
            eprintln!("no graphics adapter here: the card's picture is not compared");
            return;
        };
        let s = sample();
        let level = s.model.levels().first().unwrap().0;
        let wall = s.model.elements.iter().find(|(_, e)| e.category() == "wall").unwrap().0;
        draws_as_the_processor(&mut gpu, &s, &[(level, Rgb(0, 120, 215)), (*wall, Rgb(255, 160, 0))]);
    }

    fn inside(s: &Session, cam: Camera) -> Camera {
        bimcraft_core::view3d::inside(&s.model, cam)
    }

    #[test]
    fn the_card_draws_in_perspective_what_the_processor_does_from_afar_and_from_among_the_walls() {
        let Some(mut gpu) = headless() else {
            eprintln!("no graphics adapter here: the card's picture is not compared");
            return;
        };
        let s = sample();
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let flat = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
        let cam = scene.fit_from(Camera { lens: Some(bimcraft_core::view3d::LENS), ..flat }, &look);
        let level = s.model.levels().first().unwrap().0;
        draws_through(&mut gpu, &s, &[(level, Rgb(0, 120, 215))], cam);
        // The eye inside the house, 1.6 m up, through a wide lens: what is behind it is cut away alike.
        draws_through(&mut gpu, &s, &[], inside(&s, cam));
    }

    /// The sample house with a section box through it: from its least corner to `(x, y, z)` of the way across what it holds.
    fn cut_through(s: &mut Session, [x, y, z]: [f64; 3]) -> ([f64; 3], [f64; 3]) {
        s.execute("section_box", &serde_json::json!({"on": true})).unwrap();
        let (lo, hi) = Look::of_3d(&s.model).section_box().unwrap();
        let to = [lo[0] + (hi[0] - lo[0]) * x, lo[1] + (hi[1] - lo[1]) * y, lo[2] + (hi[2] - lo[2]) * z];
        s.execute("section_box", &serde_json::json!({"min": lo, "max": to})).unwrap();
        Look::of_3d(&s.model).section_box().unwrap()
    }

    #[test]
    fn the_card_cuts_to_a_section_box_as_the_processor_does() {
        let Some(mut gpu) = headless() else {
            eprintln!("no graphics adapter here: the card's picture is not compared");
            return;
        };
        let mut s = sample();
        cut_through(&mut s, [0.6, 0.7, 0.45]);
        let wall = s.model.elements.iter().find(|(_, e)| e.category() == "wall").unwrap().0;
        let level = s.model.levels().first().unwrap().0;
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let flat = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
        draws_through(&mut gpu, &s, &[], flat);
        draws_through(&mut gpu, &s, &[(*wall, Rgb(255, 160, 0)), (level, Rgb(0, 120, 215))], flat);
        let cam = scene.fit_from(Camera { lens: Some(bimcraft_core::view3d::LENS), ..flat }, &look);
        draws_through(&mut gpu, &s, &[], cam);
        draws_through(&mut gpu, &s, &[], inside(&s, cam));
        // What is seen through is cut too.
        s.execute("graphics", &serde_json::json!({"view": "{3D}", "categories": {"walls": {"transparency": 50}}})).unwrap();
        draws_through(&mut gpu, &s, &[], flat);
    }

    #[test]
    fn what_is_seen_through_flush_against_what_is_opaque_lies_behind_it_as_on_the_processor() {
        let Some(mut gpu) = headless() else {
            eprintln!("no graphics adapter here: the card's picture is not compared");
            return;
        };
        // A wardrobe's back in the plane of a wall's face, the box cutting away what stood in front of them.
        let mut s = sample();
        cut_through(&mut s, [0.6, 0.7, 0.45]);
        s.execute("graphics", &serde_json::json!({"view": "{3D}", "categories": {"walls": {"transparency": 50}}})).unwrap();
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let cam = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
        let mesh = Arc::new(Mesh::new(&scene, &look).unwrap());
        let cpu = scene.render_in(&cam, &[], &look);
        gpu.draw(&mesh, &cam, &[], PAPER).unwrap();
        let pixels = read(&gpu);
        // What differs lies along edges: a pixel whose four neighbours differ too is a surface drawn otherwise.
        let differs = |k: usize| pixels[4 * k..4 * k + 3].iter().zip(&cpu.rgba[4 * k..4 * k + 3]).any(|(a, b)| a.abs_diff(*b) > 6);
        let w = 640;
        let solid = (w..w * 479).filter(|&k| [k, k - 1, k + 1, k - w, k + w].iter().all(|&n| differs(n))).count();
        assert!(solid < 40, "{solid} pixels differ across an area of the picture");
    }

    #[test]
    fn the_card_draws_hidden_line_as_the_processor_does() {
        let Some(mut gpu) = headless() else {
            eprintln!("no graphics adapter here: the card's picture is not compared");
            return;
        };
        let mut s = sample();
        s.execute("visual_style", &serde_json::json!({"style": "hidden_line"})).unwrap();
        let wall = *s.model.elements.iter().find(|(_, e)| e.category() == "wall").unwrap().0;
        let level = s.model.levels().first().unwrap().0;
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let flat = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
        let cam = scene.fit_from(Camera { lens: Some(bimcraft_core::view3d::LENS), ..flat }, &look);
        draws_through(&mut gpu, &s, &[], flat);
        draws_through(&mut gpu, &s, &[(wall, Rgb(255, 160, 0)), (level, Rgb(0, 120, 215))], flat);
        draws_through(&mut gpu, &s, &[], cam);
        draws_through(&mut gpu, &s, &[], inside(&s, cam));
        // Paper and ink: nothing shaded is left on the card.
        let mesh = Arc::new(Mesh::new(&scene, &look).unwrap());
        gpu.draw(&mesh, &flat, &[], PAPER).unwrap();
        let pixels = read(&gpu);
        let cpu = scene.render_in(&flat, &[], &look);
        let far = |a: &[u8], b: &[u8]| a[..3].iter().zip(&b[..3]).any(|(x, y)| x.abs_diff(*y) > 6);
        let wrong = (0..pixels.len() / 4).filter(|&k| far(&pixels[4 * k..4 * k + 4], &cpu.rgba[4 * k..4 * k + 4])).count();
        assert!(wrong * 400 < pixels.len() / 4, "{wrong} pixels differ");
        // And cut to a box.
        cut_through(&mut s, [0.6, 0.7, 0.45]);
        draws_through(&mut gpu, &s, &[], flat);
    }

    #[test]
    fn in_a_dark_canvas_hidden_line_swaps_paper_and_ink_as_the_processor_s_picture_is_swapped() {
        let Some(mut gpu) = headless() else {
            eprintln!("no graphics adapter here: the card's picture is not compared");
            return;
        };
        let mut s = sample();
        s.execute("visual_style", &serde_json::json!({"style": "hidden_line"})).unwrap();
        let wall = *s.model.elements.iter().find(|(_, e)| e.category() == "wall").unwrap().0;
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let cam = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
        let mesh = Arc::new(Mesh::new(&scene, &look).unwrap());
        let tints = [(wall, Rgb(255, 160, 0))];
        gpu.draw(&mesh, &cam, &tints, PAPER).unwrap();
        let light = read(&gpu);
        gpu.swapped_ink = true;
        gpu.draw(&mesh, &cam, &tints, PAPER).unwrap();
        let dark = read(&gpu);
        // canvas::color, with its dark canvas, from the light picture.
        let over = |c: [u8; 3]| {
            let [r, g, b] = c.map(i32::from);
            let shift = 255 - r.max(g).max(b) - r.min(g).min(b);
            [r, g, b].map(|v| (26 + (v + shift).clamp(0, 255) * 206 / 255) as u8)
        };
        assert_eq!(over([255, 255, 255]), [26, 26, 26], "paper is the dark ground");
        for (l, d) in light.as_chunks::<4>().0.iter().zip(dark.as_chunks::<4>().0) {
            assert_eq!(over([l[0], l[1], l[2]]), [d[0], d[1], d[2]], "from {l:?}");
        }
    }

    /// The pixels of the card's picture of `s` through `cam` that aren't the processor's, to the byte.
    fn wire_differences(gpu: &mut Gpu, s: &Session, tints: &[(Id, Rgb)], cam: Camera) -> (usize, usize) {
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let mesh = Arc::new(Mesh::new(&scene, &look).unwrap());
        let cpu = scene.render_in(&cam, tints, &look);
        gpu.draw(&mesh, &cam, tints, PAPER).unwrap();
        let pixels = read(gpu);
        assert_eq!(pixels.len(), cpu.rgba.len());
        let (card, cpu) = (pixels.as_chunks::<4>().0, cpu.rgba.as_chunks::<4>().0);
        let ink = cpu.iter().filter(|c| c[..3] != [255, 255, 255]).count();
        (card.iter().zip(cpu).filter(|(a, b)| a[..3] != b[..3]).count(), ink)
    }

    #[test]
    fn the_card_draws_wireframe_as_the_processor_does_a_pixel_at_a_time() {
        let Some(mut gpu) = headless() else {
            eprintln!("no graphics adapter here: the card's picture is not compared");
            return;
        };
        let mut s = sample();
        s.execute("visual_style", &serde_json::json!({"style": "wireframe"})).unwrap();
        let wall = *s.model.elements.iter().find(|(_, e)| e.category() == "wall").unwrap().0;
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let flat = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
        let cam = scene.fit_from(Camera { lens: Some(bimcraft_core::view3d::LENS), ..flat }, &look);
        let near = Camera { scale: flat.scale * 6.0, ..flat };
        let check = |what: &str, gpu: &mut Gpu, s: &Session, tints: &[(Id, Rgb)], cam: Camera| {
            let (wrong, ink) = wire_differences(gpu, s, tints, cam);
            eprintln!("{what}: {wrong} pixels differ, of {ink} inked");
            assert!(ink > 500, "{what}: a picture with lines in it, {ink}");
            assert!(wrong * 500 < ink, "{what}: {wrong} of the {ink} pixels inked differ");
        };
        // DUMP=<folder> keeps a tinted picture to look at.
        draws_through(&mut gpu, &s, &[(wall, Rgb(255, 160, 0))], flat);
        check("parallel", &mut gpu, &s, &[], flat);
        check("tinted", &mut gpu, &s, &[(wall, Rgb(255, 160, 0))], flat);
        check("near, lines past the picture", &mut gpu, &s, &[], near);
        check("perspective", &mut gpu, &s, &[], cam);
        check("inside, lines past the eye", &mut gpu, &s, &[], inside(&s, cam));
        // And cut to a box; then what the view hides.
        cut_through(&mut s, [0.6, 0.7, 0.45]);
        check("cut", &mut gpu, &s, &[], flat);
        check("cut, perspective", &mut gpu, &s, &[], cam);
        s.execute("section_box", &serde_json::json!({"on": false})).unwrap();
        s.execute("graphics", &serde_json::json!({"view": "{3D}", "categories": {"walls": {"hidden": true}}})).unwrap();
        check("hidden", &mut gpu, &s, &[], flat);
    }

    #[test]
    fn in_a_dark_canvas_wireframe_swaps_paper_and_ink_and_a_box_being_dragged_cuts_its_lines() {
        let Some(mut gpu) = headless() else {
            eprintln!("no graphics adapter here: the card's picture is not compared");
            return;
        };
        let mut s = sample();
        s.execute("visual_style", &serde_json::json!({"style": "wireframe"})).unwrap();
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let cam = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
        let mesh = Arc::new(Mesh::new(&scene, &look).unwrap());
        gpu.draw(&mesh, &cam, &[], PAPER).unwrap();
        let light = read(&gpu);
        gpu.swapped_ink = true;
        gpu.draw(&mesh, &cam, &[], PAPER).unwrap();
        let dark = read(&gpu);
        let over = |c: [u8; 3]| {
            let [r, g, b] = c.map(i32::from);
            let shift = 255 - r.max(g).max(b) - r.min(g).min(b);
            [r, g, b].map(|v| (26 + (v + shift).clamp(0, 255) * 206 / 255) as u8)
        };
        for (l, d) in light.as_chunks::<4>().0.iter().zip(dark.as_chunks::<4>().0) {
            assert_eq!(over([l[0], l[1], l[2]]), [d[0], d[1], d[2]], "from {l:?}");
        }
        gpu.swapped_ink = false;
        // The mesh of the whole model cut as it is dragged: the picture of that box's own mesh.
        let cut = cut_through(&mut s, [0.6, 0.7, 0.45]);
        gpu.draw_cut(&mesh, &cam, &[], PAPER, Some(cut)).unwrap();
        let dragged = read(&gpu);
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let own = Arc::new(Mesh::new(&scene, &look).unwrap());
        gpu.draw(&own, &cam, &[], PAPER).unwrap();
        let settled = read(&gpu);
        // Its origin is its own, as both are the same model's, which is all f32 rounds differently by.
        let wrong = dragged.iter().zip(&settled).filter(|(a, b)| a != b).count() / 3;
        assert!(wrong < 20, "{wrong} pixels differ");
        assert_ne!(dragged, light, "the box cut something away");
    }

    #[test]
    fn a_box_being_dragged_draws_the_pixels_of_the_box_it_will_be_and_leaves_out_none_of_them() {
        let Some(mut gpu) = headless() else {
            eprintln!("no graphics adapter here: the card's picture is not compared");
            return;
        };
        let mut s = sample();
        let before = cut_through(&mut s, [0.8, 0.9, 0.6]);
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let held = Arc::new(Mesh::new(&scene, &look).unwrap());
        let flat = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
        let perspective = scene.fit_from(Camera { lens: Some(bimcraft_core::view3d::LENS), ..flat }, &look);
        let after = cut_through(&mut s, [0.5, 0.6, 0.4]);
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let made = Arc::new(Mesh::new(&scene, &look).unwrap());
        assert_ne!(before, after);
        for cam in [flat, perspective] {
            gpu.draw(&made, &cam, &[], PAPER).unwrap();
            let want = read(&gpu);
            // The same camera, and the box moved: nothing of the last box's picture is kept.
            gpu.draw(&held, &cam, &[], PAPER).unwrap();
            let start = read(&gpu);
            assert_ne!(start, want, "the two boxes differ in the picture");
            gpu.draw_cut(&held, &cam, &[], PAPER, Some(after)).unwrap();
            assert!(read(&gpu) == want, "the mesh of the box before, cut to the box after");
            // Back to its own: exact, and with nothing left out that shows.
            gpu.draw(&held, &cam, &[], PAPER).unwrap();
            assert!(read(&gpu) == start);
            gpu.cull = false;
            gpu.draw(&made, &cam, &[], PAPER).unwrap();
            assert!(read(&gpu) == want, "drawing all of it is the same picture");
            gpu.cull = true;
        }
    }

    /// A scan of a red face 4 m across standing in the sample house.
    fn scanned(s: &mut Session) -> Id {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scan.xyz");
        let text: String = (0..=200)
            .flat_map(|i| (0..=150).map(move |k| format!("{} 0 {} 220 30 30\n", f64::from(i) * 0.02, f64::from(k) * 0.02)))
            .collect();
        std::fs::write(&path, text).unwrap();
        s.execute("point_cloud", &serde_json::json!({"path": path, "at": [4000, 3000]})).unwrap();
        s.model.elements.iter().rev().find(|(_, e)| e.category() == "pointcloud").map(|(id, _)| *id).unwrap()
    }

    /// Holds the card's picture of `s` to the processor's through `cam`, and its points to theirs: of the pixels either picture
    /// shows in the scan's red, no more than one in 100 only one of them does.
    fn dots_as_the_processor(gpu: &mut Gpu, s: &Session, tints: &[(Id, Rgb)], cam: Camera, expect: bool) {
        draws_through(gpu, s, tints, cam);
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let mesh = Arc::new(Mesh::new(&scene, &look).unwrap());
        let cpu = scene.render_in(&cam, tints, &look);
        gpu.draw(&mesh, &cam, tints, PAPER).unwrap();
        let pixels = read(gpu);
        let red = |c: &[u8; 4]| c[0] > 200 && c[1] < 60 && c[2] < 60;
        let (card, cpu) = (pixels.as_chunks::<4>().0, cpu.rgba.as_chunks::<4>().0);
        let both = card.iter().zip(cpu).filter(|(a, b)| red(a) && red(b)).count();
        let one = card.iter().zip(cpu).filter(|(a, b)| red(a) != red(b)).count();
        assert_eq!(both > 200, expect, "{both} red pixels in both");
        assert!(one * 100 <= both.max(100), "{one} red pixels in one picture only, {both} in both");
    }

    #[test]
    fn the_card_draws_a_scan_s_points_as_the_processor_does() {
        let Some(mut gpu) = headless() else {
            eprintln!("no graphics adapter here: the card's picture is not compared");
            return;
        };
        let mut s = sample();
        let scan = scanned(&mut s);
        let wall = *s.model.elements.iter().find(|(_, e)| e.category() == "wall").unwrap().0;
        s.execute("graphics", &serde_json::json!({"view": "{3D}", "categories": {"roofs": {"hidden": true}}})).unwrap();
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let flat = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
        let cam = scene.fit_from(Camera { lens: Some(bimcraft_core::view3d::LENS), ..flat }, &look);
        dots_as_the_processor(&mut gpu, &s, &[], flat, true);
        // Selected: the points mixed with the tint, no longer red, and a wall's edges unmoved by them.
        draws_through(&mut gpu, &s, &[(scan, Rgb(0, 120, 215)), (wall, Rgb(255, 160, 0))], flat);
        dots_as_the_processor(&mut gpu, &s, &[], cam, true);
        draws_through(&mut gpu, &s, &[], inside(&s, cam));
        // Hidden line: the faces paper, the points still in their colours and hiding and hidden as ever.
        s.execute("visual_style", &serde_json::json!({"style": "hidden_line"})).unwrap();
        dots_as_the_processor(&mut gpu, &s, &[], flat, true);
        s.execute("visual_style", &serde_json::json!({"style": "shaded"})).unwrap();
        // What is seen through blends over the points behind it.
        s.execute("graphics", &serde_json::json!({"view": "{3D}", "categories": {"walls": {"transparency": 60}}})).unwrap();
        draws_through(&mut gpu, &s, &[], flat);
        s.execute("graphics", &serde_json::json!({"view": "{3D}", "categories": {"walls": {"transparency": 0}}})).unwrap();
        // A section box keeps a point whole or not at all.
        cut_through(&mut s, [0.6, 0.7, 0.45]);
        dots_as_the_processor(&mut gpu, &s, &[], flat, true);
        s.execute("section_box", &serde_json::json!({"on": false})).unwrap();
        // What the view hides.
        s.execute("graphics", &serde_json::json!({"view": "{3D}", "categories": {"pointcloud": {"hidden": true}}})).unwrap();
        dots_as_the_processor(&mut gpu, &s, &[], flat, false);
    }

    #[test]
    fn a_scan_alone_is_drawn_with_no_edges_round_its_points() {
        let Some(mut gpu) = headless() else {
            eprintln!("no graphics adapter here: the card's picture is not compared");
            return;
        };
        let mut s = Session::new();
        scanned(&mut s);
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let cam = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
        dots_as_the_processor(&mut gpu, &s, &[], cam, true);
        // Nothing but points and a level's lines: no edge is drawn round them.
        gpu.draw(&Arc::new(Mesh::new(&scene, &look).unwrap()), &cam, &[], PAPER).unwrap();
        let pixels = read(&gpu);
        let edges = pixels.as_chunks::<4>().0.iter().filter(|c| c[..3] == [48, 48, 48]).count();
        assert_eq!(edges, 0, "pixels in the edges' colour");
    }

    #[test]
    fn past_a_frame_s_budget_a_scan_settles_on_the_picture_drawn_in_one_go() {
        let Some(mut once) = headless() else {
            eprintln!("no graphics adapter here: the card's picture is not compared");
            return;
        };
        let Some(mut parts) = headless() else { return };
        let mut s = sample();
        scanned(&mut s);
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let mesh = Arc::new(Mesh::new(&scene, &look).unwrap());
        let cam = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
        once.draw(&mesh, &cam, &[], PAPER).unwrap();
        let want = read(&once);
        parts.budget = (mesh.cost(&mesh.visible(&cam)) + mesh.dots.len()) / 5 + 1;
        let turned = Camera { azimuth: cam.azimuth + 20.0, ..cam };
        parts.draw(&mesh, &turned, &[], PAPER).unwrap();
        let mut frames = 0;
        while !parts.draw(&mesh, &cam, &[], PAPER).unwrap() {
            frames += 1;
            assert!(frames < 64, "it settles");
        }
        assert!(frames >= 2, "{frames} frames");
        assert!(read(&parts) == want, "the settled picture is the one drawn in one go");
    }

    #[test]
    fn past_a_frame_s_budget_a_section_box_settles_on_the_picture_drawn_in_one_go() {
        let Some(mut once) = headless() else {
            eprintln!("no graphics adapter here: the card's picture is not compared");
            return;
        };
        let Some(mut parts) = headless() else { return };
        let mut s = sample();
        cut_through(&mut s, [0.6, 0.7, 0.45]);
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let mesh = Arc::new(Mesh::new(&scene, &look).unwrap());
        let cam = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
        once.draw(&mesh, &cam, &[], PAPER).unwrap();
        let want = read(&once);
        parts.budget = mesh.cost(&mesh.visible(&cam)) / 5 + 1;
        let turned = Camera { azimuth: cam.azimuth + 20.0, ..cam };
        parts.draw(&mesh, &turned, &[], PAPER).unwrap();
        let mut frames = 0;
        while !parts.draw(&mesh, &cam, &[], PAPER).unwrap() {
            frames += 1;
            assert!(frames < 64, "it settles");
        }
        assert!(frames >= 2, "{frames} frames");
        assert!(read(&parts) == want, "the settled picture is the one drawn in one go");
    }

    #[test]
    fn the_card_draws_what_the_view_sees_through_as_the_processor_does() {
        let Some(mut gpu) = headless() else {
            eprintln!("no graphics adapter here: the card's picture is not compared");
            return;
        };
        let mut s = sample();
        s.execute("graphics", &serde_json::json!({"view": "{3D}", "categories": {"walls": {"transparency": 60}}})).unwrap();
        draws_as_the_processor(&mut gpu, &s, &[]);
        // A wall seen through and selected shows its tint, however see-through it is.
        let wall = *s.model.elements.iter().find(|(_, e)| e.category() == "wall").unwrap().0;
        s.execute("graphics", &serde_json::json!({"view": "{3D}", "elements": {wall.to_string(): {"transparency": 90}}})).unwrap();
        draws_as_the_processor(&mut gpu, &s, &[(wall, Rgb(255, 160, 0))]);
        // A mass is seen through unless the view says otherwise.
        s.execute("mass", &serde_json::json!({"level": "Level 1", "boundary": [[-3000, -3000], [3000, -3000], [3000, 3000], [-3000, 3000]], "height": 9000}))
            .unwrap();
        draws_as_the_processor(&mut gpu, &s, &[]);
    }

    #[test]
    fn the_card_draws_a_glass_layer_as_the_processor_does() {
        let Some(mut gpu) = headless() else {
            eprintln!("no graphics adapter here: the card's glass is not compared");
            return;
        };
        let mut s = sample();
        s.script(
            r#"type {"category": "wall", "name": "Curtain", "layers": [{"material": "Glass", "thickness": 50}, {"material": "Concrete block", "thickness": 100}]}
               wall {"points": [[-6000,-6000],[6000,-6000]], "type": "Curtain"}
               column {"at": [0, -7000]}"#,
        )
        .unwrap();
        draws_as_the_processor(&mut gpu, &s, &[]);
    }

    #[test]
    fn a_model_with_nothing_to_draw_draws_the_ground() {
        let Some(mut gpu) = headless() else { return };
        let s = Session::new();
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let mesh = Arc::new(Mesh::new(&scene, &look).unwrap());
        let cam = scene.fit_in(AZIMUTH, ELEVATION, 64, 48, &look);
        gpu.draw(&mesh, &cam, &[], Rgb(10, 20, 30)).unwrap();
        let pixels = read(&gpu);
        // Nothing but the ground, and the new project's level lines over it.
        let ground = pixels.as_chunks::<4>().0.iter().filter(|p| **p == [10, 20, 30, 255]).count();
        assert!(ground * 10 >= pixels.len() / 4 * 9 && ground < pixels.len() / 4, "{ground} of {} pixels are the ground", pixels.len() / 4);
    }

    /// `cargo test -p bimcraft-ui-egui --release gpu_speed -- --ignored --nocapture`: the 20-storey tower at 1920 x 1080, orbited.
    /// The card's frames are timed back to back, apart from the processor's: timed straight after a processor's picture, a frame
    /// waits for the card to wake from idle (24 ms on an RTX 2060 for a 1.5 ms frame), which a window turning the view never does.
    #[test]
    fn within_finds_what_of_a_span_is_seen_in_order() {
        let seen = [0..3, 6..12, 15..30];
        assert_eq!(within(&seen, 0..30), vec![0..3, 6..12, 15..30]);
        assert_eq!(within(&seen, 3..6), vec![]);
        assert_eq!(within(&seen, 9..18), vec![9..12, 15..18]);
        assert_eq!(within(&seen, 30..40), vec![]);
        assert_eq!(within(&[], 0..9), vec![]);
    }

    #[test]
    fn parts_taken_one_after_another_draw_each_triangle_and_copy_once() {
        let draws = vec![
            (false, Draw::Own(0..30)),
            (false, Draw::Copies { corners: 6..18, from: From::Seen, copies: 2..9 }),
            (false, Draw::Dots(5..12)),
            (true, Draw::Copies { corners: 0..9, from: From::Mesh, copies: 0..4 }),
        ];
        let total: usize = draws.iter().map(|(_, d)| d.cost()).sum();
        assert_eq!(total, 10 + 28 + 7 + 12);
        for step in [1, 3, 5, 7, 11, 50] {
            let (mut own, mut copies, mut dots) = (Vec::new(), Vec::new(), Vec::new());
            let mut a = 0;
            while a < total {
                let b = (a + step).min(total);
                for (clear, d) in part_of(&draws, a, b) {
                    match d {
                        Draw::Own(r) => own.extend(r),
                        Draw::Copies { copies: c, from, .. } => copies.extend(c.map(|k| (clear, from, k))),
                        Draw::Dots(r) => dots.extend(r),
                    }
                }
                a = b;
            }
            assert_eq!(own, (0..30).collect::<Vec<_>>(), "step {step}");
            assert_eq!(dots, (5..12).collect::<Vec<_>>(), "step {step}");
            let mut want: Vec<_> = (2..9).map(|k| (false, From::Seen, k)).collect();
            want.extend((0..4).map(|k| (true, From::Mesh, k)));
            assert_eq!(copies, want, "step {step}");
        }
    }

    #[test]
    fn spans_are_whole_and_in_order_across_buffers() {
        assert_eq!(spans(6, 0..15), vec![(0, 0..6), (1, 0..6), (2, 0..3)]);
        assert_eq!(spans(6, 3..9), vec![(0, 3..6), (1, 0..3)]);
        assert_eq!(spans(6, 6..12), vec![(1, 0..6)]);
        assert_eq!(spans(6, 4..4), vec![]);
        // Past what 32 bits count, each buffer's own stay small.
        let far = 9_000_000_000 / 300 * 300;
        assert_eq!(spans(300, far..far + 600), vec![((far / 300) as usize, 0..300), ((far / 300 + 1) as usize, 0..300)]);
    }

    /// Draws until the picture is exact, the card never holding more than `room` bytes of vertices; how many frames it took.
    fn settles(gpu: &mut Gpu, mesh: &Arc<Mesh>, cam: &Camera, room: u64) -> usize {
        let mut frames = 1;
        while !gpu.draw(mesh, cam, &[], PAPER).unwrap() {
            frames += 1;
            assert!(gpu.holding() <= room, "{} bytes held, of {room}", gpu.holding());
            assert!(frames < 400, "it never settles");
        }
        assert!(gpu.holding() <= room, "{} bytes held, of {room}", gpu.holding());
        frames
    }

    /// A buffer of 200 triangles, so that the sample house takes many.
    const CHUNK: u64 = STRIDE * 3 * 200;

    #[test]
    fn a_card_that_holds_little_settles_on_the_picture_of_one_that_holds_all() {
        let Some(mut whole) = headless() else {
            eprintln!("no graphics adapter here: the card's picture is not compared");
            return;
        };
        let Some(mut small) = headless() else { return };
        small.vertex_bytes = CHUNK;
        small.card_bytes = 2 * CHUNK;
        let mut s = sample();
        for seen_through in [false, true] {
            if seen_through {
                s.execute("graphics", &serde_json::json!({"view": "{3D}", "categories": {"walls": {"transparency": 50}}})).unwrap();
            }
            let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
            let mesh = Arc::new(Mesh::new(&scene, &look).unwrap());
            assert!(mesh.vertices.len() as u64 * STRIDE > 8 * CHUNK, "far more than it holds: {} vertices", mesh.vertices.len());
            let parallel = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
            let cam = scene.fit_from(Camera { lens: Some(bimcraft_core::view3d::LENS), ..parallel }, &look);
            for cam in [parallel, cam, inside(&s, cam)] {
                whole.draw(&mesh, &cam, &[], PAPER).unwrap();
                let want = read(&whole);
                let turned = Camera { azimuth: cam.azimuth + 10.0, ..cam };
                small.draw(&mesh, &turned, &[], PAPER).unwrap();
                let frames = settles(&mut small, &mesh, &cam, 2 * CHUNK);
                assert!(frames >= 3, "{frames} frames");
                assert!(read(&small) == want, "seen through {seen_through}: the settled picture is the one drawn with all of it held");
            }
        }
    }

    #[test]
    fn an_allocation_that_fails_is_a_signal_to_hold_less_not_a_card_that_failed() {
        let Some(mut whole) = headless() else {
            eprintln!("no graphics adapter here: the card's picture is not compared");
            return;
        };
        let Some(mut full) = headless() else { return };
        full.vertex_bytes = CHUNK;
        // It says nothing of how much it has until it has no more.
        assert_eq!(full.card_bytes, u64::MAX);
        full.full_at = Some(3 * CHUNK);
        let s = sample();
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let mesh = Arc::new(Mesh::new(&scene, &look).unwrap());
        let cam = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
        whole.draw(&mesh, &cam, &[], PAPER).unwrap();
        settles(&mut full, &mesh, &cam, 3 * CHUNK);
        assert!(full.card_bytes <= 3 * CHUNK, "{} bytes", full.card_bytes);
        // Asked to hold less again, as a frame's own work running out asks: half of it, and the same picture.
        assert!(full.hold_less());
        assert!(full.card_bytes < 3 * CHUNK, "{} bytes", full.card_bytes);
        let less = full.card_bytes;
        settles(&mut full, &mesh, &cam, less);
        assert!(read(&full) == read(&whole), "the same picture, for less held");
        // And so is the next mesh, from what the last one found: it isn't asked to fail again.
        let again = Arc::new(Mesh::new(&scene, &look).unwrap());
        settles(&mut full, &again, &cam, 3 * CHUNK);
        assert!(read(&full) == read(&whole));
    }

    #[test]
    fn a_view_that_stays_within_what_the_card_holds_comes_to_be_drawn_whole_and_what_it_cannot_see_is_let_go() {
        let Some(mut small) = headless() else {
            eprintln!("no graphics adapter here: the card's picture is not compared");
            return;
        };
        small.vertex_bytes = CHUNK;
        let s = sample();
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let mesh = Arc::new(Mesh::new(&scene, &look).unwrap());
        let fit = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
        // Near the last of the vertices, seeing few of the buffers: room for those and no more, and they aren't the ones a first
        // upload holds.
        let last = mesh.vertices.last().map_or(fit.target, |v| std::array::from_fn(|i| f64::from(v.position[i]) + mesh.origin[i]));
        let near = Camera { scale: fit.scale * 12.0, target: last, ..fit };
        let chunks = |seen: &Visible| -> std::collections::BTreeSet<u64> {
            let per = CHUNK / STRIDE;
            seen.vertices.iter().flat_map(|r| r.start as u64 / per..=(r.end as u64 - 1) / per).collect()
        };
        let seen = chunks(&mesh.visible(&near));
        let all = (mesh.vertices.len() as u64 * STRIDE).div_ceil(CHUNK);
        assert!((seen.len() as u64) < all, "{} of {all} buffers seen", seen.len());
        let room = seen.len() as u64 * CHUNK;
        small.card_bytes = room;
        // Past what it holds, standing in, at first; moving a little each frame, and the card bringing in what the view wants.
        let moved = |i: u32| Camera { azimuth: near.azimuth + f64::from(i) * 1e-6, ..near };
        small.draw(&mesh, &fit, &[], PAPER).unwrap();
        let first: std::collections::BTreeSet<u64> =
            small.mesh.as_ref().unwrap().vertices.buffers.iter().enumerate().filter(|(_, b)| b.is_some()).map(|(k, _)| k as u64).collect();
        assert!(!seen.is_subset(&first), "seen {seen:?}, held at first {first:?}");
        let mut whole = false;
        for i in 0..seen.len() as u32 + 8 {
            let cam = moved(i);
            whole = small.draw(&mesh, &cam, &[], PAPER).unwrap();
            assert!(small.holding() <= room, "{} bytes held, of {room}", small.holding());
        }
        assert!(whole, "all it sees is held, so a moved frame is drawn at once");
        let up = small.mesh.as_ref().unwrap();
        let held: std::collections::BTreeSet<u64> =
            up.vertices.buffers.iter().enumerate().filter(|(_, b)| b.is_some()).map(|(k, _)| k as u64).collect();
        assert!(chunks(&mesh.visible(&moved(0))).is_subset(&held), "held {held:?}");
    }

    /// The card run out for real: filled with buffers until an allocation fails, which takes all of its memory from whatever else
    /// is using it, so it is run by hand, on a machine with nothing else to lose: `cargo test -p bimcraft-ui-egui -- --ignored
    /// a_card_really_out_of_memory --nocapture`.
    #[test]
    #[ignore]
    fn a_card_really_out_of_memory_still_settles_on_the_picture() {
        let Some(mut whole) = headless() else { return };
        let Some(mut full) = headless() else { return };
        full.vertex_bytes = CHUNK;
        let s = sample();
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let mesh = Arc::new(Mesh::new(&scene, &look).unwrap());
        let cam = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
        whole.draw(&mesh, &cam, &[], PAPER).unwrap();
        let want = read(&whole);
        // The targets first, then all that is left in buffers, each written so that the card must place it.
        full.draw(&Arc::new(Mesh::new(&Scene::new(&Session::new().model), &look).unwrap()), &cam, &[], PAPER).unwrap();
        let mut filler = Vec::new();
        for _ in 0..1024 {
            let guard = scopes(&full.device);
            let buffer = full.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("filler"),
                size: VERTEX_BUFFER,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            full.queue.submit([]);
            let out = out_of_memory(guard).unwrap();
            if out {
                break;
            }
            filler.push(buffer);
        }
        eprintln!("the card took {} MB of buffers before it said it was full", filler.len() * 256);
        let frames = settles(&mut full, &mesh, &cam, u64::MAX);
        eprintln!("settled in {frames} frames, holding {} bytes, room for {}", full.holding(), full.card_bytes);
        assert!(read(&full) == want);
    }

    #[test]
    fn a_wireframe_on_a_card_that_holds_little_settles_on_the_picture_of_one_that_holds_all() {
        let Some(mut whole) = headless() else {
            eprintln!("no graphics adapter here: the card's picture is not compared");
            return;
        };
        let Some(mut small) = headless() else { return };
        small.vertex_bytes = WIRE * 300;
        let room = 2 * WIRE * 300;
        small.card_bytes = room;
        let mut s = sample();
        s.execute("visual_style", &serde_json::json!({"style": "wireframe"})).unwrap();
        let wall = *s.model.elements.iter().find(|(_, e)| e.category() == "wall").unwrap().0;
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let mesh = Arc::new(Mesh::new(&scene, &look).unwrap());
        assert!(mesh.wires.len() as u64 > 6 * 300, "far more than it holds: {} lines", mesh.wires.len());
        let flat = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
        let cam = scene.fit_from(Camera { lens: Some(bimcraft_core::view3d::LENS), ..flat }, &look);
        let tinted = [(wall, Rgb(255, 160, 0))];
        for (cam, tints, dark) in [(flat, &[][..], false), (flat, &tinted[..], false), (cam, &[][..], false), (flat, &tinted[..], true)] {
            (whole.swapped_ink, small.swapped_ink) = (dark, dark);
            assert!(whole.draw(&mesh, &cam, tints, PAPER).unwrap());
            let want = read(&whole);
            // Come to from another camera: what the card holds stands in, and the picture says it isn't exact.
            let turned = Camera { azimuth: cam.azimuth + 10.0, ..cam };
            assert!(!small.draw(&mesh, &turned, tints, PAPER).unwrap(), "past what it holds, not whole");
            assert!(small.holding() <= room, "{} bytes held, of {room}", small.holding());
            assert!(!small.draw(&mesh, &cam, tints, PAPER).unwrap(), "the camera moved: this frame stands in as well");
            let standing = read(&small);
            assert_ne!(standing, want, "what stands in is not all of it");
            let mut frames = 0;
            while !small.draw(&mesh, &cam, tints, PAPER).unwrap() {
                frames += 1;
                assert!(frames < 200, "it never settles");
                assert!(small.holding() <= room, "{} bytes held, of {room}", small.holding());
                assert_eq!(read(&small), standing, "the picture changed before the exact one was whole");
            }
            assert!(frames >= 3, "{frames} frames");
            assert!(read(&small) == want, "dark {dark}, {} tints: the same picture", tints.len());
            // Settled, drawing it again does nothing and is exact at once.
            assert!(small.draw(&mesh, &cam, tints, PAPER).unwrap());
            assert!(read(&small) == want);
        }
        // Past what it holds a cut box settles on the same as well.
        let cut = cut_through(&mut s, [0.6, 0.7, 0.45]);
        whole.swapped_ink = false;
        small.swapped_ink = false;
        whole.draw_cut(&mesh, &flat, &[], PAPER, Some(cut)).unwrap();
        let mut frames = 0;
        while !small.draw_cut(&mesh, &flat, &[], PAPER, Some(cut)).unwrap() {
            frames += 1;
            assert!(frames < 200, "it never settles");
        }
        assert!(read(&small) == read(&whole));
    }

    #[test]
    fn a_scan_on_a_card_that_holds_little_settles_on_the_picture_of_one_that_holds_all() {
        let Some(mut whole) = headless() else {
            eprintln!("no graphics adapter here: the card's picture is not compared");
            return;
        };
        let Some(mut small) = headless() else { return };
        small.vertex_bytes = CHUNK;
        small.card_bytes = 2 * CHUNK;
        let mut s = sample();
        scanned(&mut s);
        s.execute("graphics", &serde_json::json!({"view": "{3D}", "categories": {"roofs": {"hidden": true}}})).unwrap();
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let mesh = Arc::new(Mesh::new(&scene, &look).unwrap());
        assert!(mesh.dots.len() as u64 * DOT > 8 * CHUNK, "{} points", mesh.dots.len());
        let cam = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
        whole.draw(&mesh, &cam, &[], PAPER).unwrap();
        let turned = Camera { azimuth: cam.azimuth + 10.0, ..cam };
        small.draw(&mesh, &turned, &[], PAPER).unwrap();
        assert!(small.holding() <= 2 * CHUNK, "{} bytes held", small.holding());
        settles(&mut small, &mesh, &cam, 2 * CHUNK);
        let up = small.mesh.as_ref().unwrap();
        assert!(up.dots.held() < up.dots.buffers.len(), "{} of {} buffers of points held", up.dots.held(), up.dots.buffers.len());
        assert!(read(&small) == read(&whole), "the same picture, its points streamed through");
    }

    #[test]
    fn a_card_too_small_for_one_buffer_says_so_and_nothing_is_kept() {
        let Some(mut tiny) = headless() else { return };
        tiny.card_bytes = 16;
        let s = sample();
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let mesh = Arc::new(Mesh::new(&scene, &look).unwrap());
        let err = tiny.draw(&mesh, &scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look), &[], PAPER).unwrap_err();
        assert!(err.contains("memory"), "{err}");
        assert!(tiny.mesh.is_none());
    }

    #[test]
    fn a_mesh_in_many_buffers_draws_the_pixels_of_one() {
        let Some(mut whole) = headless() else { return };
        let Some(mut split) = headless() else { return };
        // A few triangles to a buffer, so that many draws go across the line from what is opaque to what is seen through.
        split.vertex_bytes = STRIDE * 3 * 25;
        let mut s = sample();
        for (name, cam_lens) in [("parallel", None), ("perspective", Some(35.0))] {
            for seen_through in [false, true] {
                if seen_through {
                    s.execute("graphics", &serde_json::json!({"view": "{3D}", "categories": {"walls": {"transparency": 50}}})).unwrap();
                }
                let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
                let mesh = Arc::new(Mesh::new(&scene, &look).unwrap());
                assert!(mesh.vertices.len() > 75 * 4, "enough for several buffers");
                let cam = Camera { lens: cam_lens, ..scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look) };
                let cam = scene.fit_from(cam, &look);
                let tints = [(mesh.owners[0].id, Rgb(0, 120, 215))];
                for gpu in [&mut whole, &mut split] {
                    gpu.draw(&mesh, &cam, &tints, PAPER).unwrap();
                    gpu.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                }
                let up = split.mesh.as_ref().unwrap();
                assert!(up.vertices.buffers.len() > 3, "{} buffers", up.vertices.buffers.len());
                assert_eq!(whole.mesh.as_ref().unwrap().vertices.buffers.len(), 1);
                assert_eq!(read(&whole), read(&split), "{name}, seen through {seen_through}");
            }
        }
    }

    #[test]
    #[ignore]
    fn gpu_speed() {
        use std::time::Instant;
        let Some(mut gpu) = headless() else { return };
        let mut s = Session::new();
        s.execute("highrise", &serde_json::json!({})).unwrap();
        let ms = |t: Instant| t.elapsed().as_secs_f64() * 1000.0;
        let stats = |mut v: Vec<f64>| {
            v.sort_by(f64::total_cmp);
            format!("median {:.2} ms, p95 {:.2} ms", v[v.len() / 2], v[(v.len() * 95).div_ceil(100) - 1])
        };
        let walls: Vec<Id> = s.model.elements.iter().filter(|(_, e)| e.category() == "wall").map(|(id, _)| *id).collect();
        let selected: Vec<(Id, Rgb)> = walls.iter().map(|id| (*id, Rgb(0, 120, 215))).collect();
        let mut cases = vec![("textured", s.model.clone())];
        let mut plain = Session::new();
        plain.execute("highrise", &serde_json::json!({})).unwrap();
        let names: Vec<String> = plain.model.materials.iter().filter(|(_, m)| m.texture.is_some()).map(|(n, _)| n.clone()).collect();
        for name in names {
            plain.execute("material", &serde_json::json!({"name": name, "texture": null})).unwrap();
        }
        cases.push(("untextured", plain.model.clone()));
        let mut clear = plain;
        clear.execute("graphics", &serde_json::json!({"view": "{3D}", "categories": {"windows": {"transparency": 60}}})).unwrap();
        cases.push(("windows see-through", clear.model.clone()));
        for (name, model) in cases {
            let (scene, look) = (Scene::new(&model), Look::of_3d(&model));
            let t = Instant::now();
            let Ok(mesh) = Mesh::new(&scene, &look).map(Arc::new) else {
                eprintln!("{name}: the card doesn't draw it");
                continue;
            };
            eprintln!("{name}: mesh of {} vertices in {:.1} ms", mesh.vertices.len(), ms(t));
            let cams: Vec<_> = (0..60).map(|i| scene.fit_in(AZIMUTH + f64::from(i) * 3.0, ELEVATION, 1920, 1080, &look)).collect();
            let t = Instant::now();
            gpu.draw(&mesh, &cams[0], &[], PAPER).unwrap();
            gpu.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            eprintln!("  first frame (uploads the mesh): {:.1} ms", ms(t));
            for (what, tints) in [("nothing selected", &[][..]), ("every wall selected", &selected[..])] {
                let (mut submit, mut frame) = (Vec::new(), Vec::new());
                for (i, cam) in cams.iter().enumerate() {
                    let t = Instant::now();
                    gpu.draw(&mesh, cam, tints, PAPER).unwrap();
                    let sent = ms(t);
                    gpu.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                    if i >= 5 {
                        submit.push(sent);
                        frame.push(ms(t));
                    }
                }
                eprintln!("  {what}: frame {}; of it, sending {}", stats(frame), stats(submit));
            }
            let cpu: Vec<f64> = cams[..9]
                .iter()
                .map(|cam| {
                    let t = Instant::now();
                    std::hint::black_box(scene.render_in(cam, &[], &look));
                    ms(t)
                })
                .collect();
            eprintln!("  processor: {}", stats(cpu));
            let t = Instant::now();
            let hit = mesh.pick(&cams[0], &[], 960.0, 540.0);
            eprintln!("  first pick (builds the hierarchy): {:.1} ms, {hit:?}", ms(t));
            let t = Instant::now();
            for i in 0..100 {
                std::hint::black_box(mesh.pick(&cams[0], &[], 400.0 + f64::from(i) * 10.0, 540.0));
            }
            eprintln!("  next picks: {:.3} ms each", ms(t) / 100.0);
        }
    }
}
