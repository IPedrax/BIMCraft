//! The 3D view drawn on the graphics card: the core's `view3d::gpu::Mesh` uploaded once for a model, and the passes for each
//! picture (the opaque surfaces; those seen through; edges, ground and the blend; the levels' lines), in `gpu.wgsl`, into a
//! texture that egui shows as it shows the CPU picture. What changes between pictures is small: the camera, the levels' lines,
//! and, only when they change, the selection's tints and the textures' mip levels. Opt-in (Settings > Performance); what the
//! card can't draw (hidden line, wireframe) is the processor's, and so is everything when the card fails: a validation or out-of-memory error from the device is caught
//! here, not left to the device's handler, and the window carries on without the card (`BimApp::gpu_failed`).

use std::collections::BTreeMap;
use std::sync::Arc;

use bimcraft_core::model::Id;
use bimcraft_core::render::Rgb;
use bimcraft_core::view3d::gpu::{Mesh, Vertex};
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
const GLOBALS: u64 = 7 * 16;
/// An owner's edge and tint colours, a skin's parts and a segment's ends and colour: 2, 3 and 3 vec4s.
const OWNER: u64 = 32;
const SKIN: u64 = 48;
const SEGMENT: u64 = 48;

/// The window's use of the card: whether the user asked for it, the wgpu state it would be made on (none where the window has
/// no wgpu device, as in every test), the card once it's made, and why the last picture was drawn on the processor, if it was.
#[derive(Default)]
pub struct Card {
    pub wanted: bool,
    pub state: Option<RenderState>,
    pub gpu: Option<Gpu>,
    pub note: Option<String>,
    /// The card was tried and can't draw the 3D view.
    pub(crate) failed: bool,
}

impl Card {
    /// A card already made, as a test has one.
    #[cfg(test)]
    pub(crate) fn with(gpu: Gpu) -> Card {
        Card { wanted: true, gpu: Some(gpu), ..Card::default() }
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
    vertices: wgpu::Buffer,
    count: u32,
    /// Where the vertices seen through start.
    clear_from: u32,
    owners: wgpu::Buffer,
    skins: wgpu::Buffer,
    texels: wgpu::Buffer,
    /// The tints the owners were written with (none yet), and the textures' lookups.
    tints: Option<Vec<(Id, Rgb)>>,
    lookups: Vec<[u32; 12]>,
    /// The buffers bound, made again when the segments' buffer is.
    bind: Option<wgpu::BindGroup>,
}

/// The card, and what it keeps between pictures.
pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    state: Option<RenderState>,
    scene: wgpu::RenderPipeline,
    clear: wgpu::RenderPipeline,
    post: wgpu::RenderPipeline,
    lines: wgpu::RenderPipeline,
    group0: wgpu::BindGroupLayout,
    group1: wgpu::BindGroupLayout,
    group2: wgpu::BindGroupLayout,
    globals: wgpu::Buffer,
    segments: (wgpu::Buffer, u64),
    mesh: Option<Uploaded>,
    targets: Option<Targets>,
    texture: Option<TextureId>,
    adapter: String,
}

/// The bytes of a buffer, 32-bit words as the card's byte order has them.
type Words = Vec<u8>;

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
            clear,
            post,
            lines,
            group0,
            group1,
            group2,
            globals,
            segments,
            mesh: None,
            targets: None,
            texture: None,
            adapter: String::new(),
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

    fn write(&self, buffer: &wgpu::Buffer, bytes: &[u8]) {
        if !bytes.is_empty() {
            self.queue.write_buffer(buffer, 0, bytes);
        }
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

    /// Draws `mesh` as `cam` sees it, `tints` colouring (halfway, and the edges whole) what they name, on `ground` where nothing is.
    /// The picture is in [`texture`](Self::texture). An error leaves the card with nothing kept: the caller does without it.
    pub fn draw(&mut self, mesh: &Arc<Mesh>, cam: &Camera, tints: &[(Id, Rgb)], ground: Rgb) -> Result<(), String> {
        let guard = scopes(&self.device);
        let drawn = self.encode(mesh, cam, tints, ground);
        let caught = caught(guard);
        if drawn.is_err() || caught.is_err() {
            (self.mesh, self.targets) = (None, None);
        }
        drawn.and(caught)
    }

    fn encode(&mut self, mesh: &Arc<Mesh>, cam: &Camera, tints: &[(Id, Rgb)], ground: Rgb) -> Result<(), String> {
        let (w, h) = (cam.width.clamp(1, MAX_SIDE), cam.height.clamp(1, MAX_SIDE));
        if w.max(h) > self.device.limits().max_texture_dimension_2d {
            return Err("the picture is bigger than the card draws".into());
        }
        self.upload(mesh)?;
        self.size(w, h);
        let (device, queue) = (&self.device, &self.queue);
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
        for marks in mesh.marks(cam, tints) {
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
        let through = up.clear_from < up.count;
        let mut globals = Words::new();
        put(&mut globals, &[view.right[0], view.right[1], view.right[2], 2.0 * view.scale / width]);
        put(&mut globals, &[view.up[0], view.up[1], view.up[2], 2.0 * view.scale / height]);
        put(&mut globals, &[view.back[0], view.back[1], view.back[2], 0.5 / view.depth]);
        put(&mut globals, &[view.target[0], view.target[1], view.target[2], view.scale]);
        put(&mut globals, &[mesh.origin[0] as f32, mesh.origin[1] as f32, mesh.origin[2] as f32, 0.0]);
        put(&mut globals, &[rgb(ground)[0], rgb(ground)[1], rgb(ground)[2], 0.0]);
        // 1.5 px, as the processor lets a line show through a surface that near.
        put(&mut globals, &[width, height, 1.5 / view.scale * 0.5 / view.depth, if through { 1.0 } else { 0.0 }]);
        write(&self.globals, &globals);
        let bind = up.bind.get_or_insert_with(|| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("bimcraft-3d-buffers"),
                layout: &self.group0,
                entries: &[
                    entry(0, &self.globals),
                    entry(1, &up.owners),
                    entry(2, &self.segments.0),
                    entry(3, &up.skins),
                    entry(4, &up.texels),
                ],
            })
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("bimcraft-3d") });
        let attachment = |view| wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
        };
        let depth = |view| wgpu::RenderPassDepthStencilAttachment {
            view,
            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Store }),
            stencil_ops: None,
        };
        // The opaque surfaces, then (when the view sees through any) those seen through, the nearest in front of them.
        for (name, pipeline, color, ids, z, vertices) in [
            ("bimcraft-3d-surfaces", &self.scene, &targets.color, &targets.ids, &targets.depth, 0..up.clear_from),
            (
                "bimcraft-3d-seen-through",
                &self.clear,
                &targets.clear_color,
                &targets.clear_ids,
                &targets.clear_depth,
                up.clear_from..up.count,
            ),
        ] {
            let through_pass = name == "bimcraft-3d-seen-through";
            if through_pass && !through {
                continue;
            }
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(name),
                color_attachments: &[Some(attachment(color)), Some(attachment(ids))],
                depth_stencil_attachment: Some(depth(z)),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if !vertices.is_empty() {
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, &*bind, &[]);
                // What is seen through reads the opaque depth, which the opaque pass is writing.
                if through_pass {
                    pass.set_bind_group(1, &targets.bind, &[]);
                }
                pass.set_vertex_buffer(0, up.vertices.slice(..));
                pass.draw(vertices, 0..1);
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
            pass.set_bind_group(0, &*bind, &[]);
            pass.set_bind_group(1, &targets.bind, &[]);
            if name == "edges" {
                pass.set_bind_group(2, &targets.clear_bind, &[]);
            }
            // The edges are one triangle over the whole picture; the levels' lines two to a segment.
            pass.draw(0..if name == "edges" { 3 } else { 6 }, 0..instances);
        }
        self.queue.submit([encoder.finish()]);
        Ok(())
    }

    /// Puts `mesh` on the card, unless it's there already.
    fn upload(&mut self, mesh: &Arc<Mesh>) -> Result<(), String> {
        if self.mesh.as_ref().is_some_and(|u| Arc::ptr_eq(&u.mesh, mesh)) {
            return Ok(());
        }
        self.mesh = None;
        let (size, limits) = (mesh.vertices.len() as u64 * STRIDE, self.device.limits());
        if size > limits.max_buffer_size || mesh.texels.len() as u64 * 4 > limits.max_storage_buffer_binding_size {
            return Err("the model is bigger than the card holds".into());
        }
        let mut bytes = Words::with_capacity(size as usize);
        for Vertex { position, normal, face, color, owner, plane, tex } in &mesh.vertices {
            put(&mut bytes, position);
            put_unit(&mut bytes, normal);
            put_unit(&mut bytes, face);
            put_words(&mut bytes, &[*color, *owner, *plane, *tex]);
        }
        let make = |label: &str, size: u64, usage: wgpu::BufferUsages| {
            self.device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: size.max(4), usage, mapped_at_creation: false })
        };
        let vertices = make("bimcraft-3d-vertices", size, wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST);
        self.write(&vertices, &bytes);
        let texels = Gpu::storage(&self.device, mesh.texels.len() as u64 * 4);
        let mut words = Words::with_capacity(mesh.texels.len() * 4);
        put_words(&mut words, &mesh.texels);
        self.write(&texels, &words);
        self.mesh = Some(Uploaded {
            mesh: Arc::clone(mesh),
            vertices,
            count: mesh.vertices.len() as u32,
            clear_from: mesh.clear_from.min(mesh.vertices.len()) as u32,
            tints: None,
            lookups: Vec::new(),
            bind: None,
            // An array in a shader is at least one element long.
            owners: Gpu::storage(&self.device, mesh.owners.len().max(1) as u64 * OWNER),
            skins: Gpu::storage(&self.device, mesh.skins.len().max(1) as u64 * SKIN),
            texels,
        });
        Ok(())
    }

    /// Targets of `w` by `h` pixels, made again where the picture's size changed, and put in egui's reach.
    fn size(&mut self, w: u32, h: u32) {
        if self.targets.as_ref().is_some_and(|t| t.size == (w, h)) {
            return;
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
        let (color, ids, depth) = (
            view(&texture("bimcraft-3d-color", COLOR, render | sampled)),
            view(&texture("bimcraft-3d-ids", IDS, render | sampled)),
            view(&texture("bimcraft-3d-depth", DEPTH, render | sampled)),
        );
        let (clear_color, clear_ids, clear_depth) = (
            view(&texture("bimcraft-3d-clear-color", COLOR, render | sampled)),
            view(&texture("bimcraft-3d-clear-ids", CLEAR_IDS, render | sampled)),
            view(&texture("bimcraft-3d-clear-depth", DEPTH, render)),
        );
        let picture = texture("bimcraft-3d-picture", COLOR, render | sampled | wgpu::TextureUsages::COPY_SRC);
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
        self.targets =
            Some(Targets { size: (w, h), color, ids, depth, clear_color, clear_ids, clear_depth, picture, picture_view, bind, clear_bind });
    }
}

/// The machine's graphics card with no window to show on, for tests; none on a runner that has no adapter (those tests say so
/// and pass).
#[cfg(test)]
pub(crate) fn headless() -> Option<Gpu> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
    // OpenGL is not the card's (see `Gpu::new`).
    if adapter.get_info().backend == wgpu::Backend::Gl {
        return None;
    }
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()?;
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
        let mesh = Arc::new(Mesh::new(&scene, &look).unwrap());
        let cam = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
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
