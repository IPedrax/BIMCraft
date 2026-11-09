//! The 2D canvas's triangles kept on the card: made once by egui's own tessellator from the shapes `canvas::paint` makes, and drawn
//! by the sums and blending of egui's own shader, so a frame that changes nothing costs a draw call, and the pixels are the ones
//! egui would have drawn. A window with no card, and a drawing with a picture, stay on `canvas::paint`.
//!
//! While a view moves they are drawn moved, whole, until a worker's exact drawing and triangles replace them ([`make`], [`Retained::adopt`]).
use crate::canvas::{self, Place};
use bimcraft_core::model::Id;
use bimcraft_core::render::Drawing;
use eframe::egui_wgpu::{self, wgpu};
use egui::epaint::{self, TessellationOptions, Tessellator};
use egui::{Context, Painter};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// eframe's own default, which the app doesn't change: what egui's renderer draws with beside these.
const DITHERING: bool = true;
/// A vertex as epaint lays it out: position, texture position (unused) and colour.
const STRIDE: u64 = 20;
/// The shader's `Locals`.
const LOCALS: u64 = 24;
/// How many primitives a thread tessellates at a time.
const PRIMS_AT_ONCE: usize = 2048;

/// How the card draws what is kept: one for the window.
pub(crate) struct Pipeline {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
}

impl Pipeline {
    /// For a window drawing to `format`, as egui's renderer is made there.
    pub(crate) fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Pipeline {
        let shader = device.create_shader_module(wgpu::include_wgsl!("retained.wgsl"));
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("canvas locals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(LOCALS),
                },
                count: None,
            }],
        });
        let shared = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("canvas"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("canvas"),
            layout: Some(&shared),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: STRIDE,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 0, shader_location: 0 },
                        wgpu::VertexAttribute { format: wgpu::VertexFormat::Uint32, offset: 16, shader_location: 2 },
                    ],
                })],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                unclipped_depth: false,
                conservative: false,
                cull_mode: None,
                front_face: wgpu::FrontFace::default(),
                polygon_mode: wgpu::PolygonMode::default(),
                strip_index_format: None,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState { alpha_to_coverage_enabled: false, count: 1, mask: !0 },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some(if format.is_srgb() { "fs_main_linear_framebuffer" } else { "fs_main_gamma_framebuffer" }),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::OneMinusDstAlpha,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Pipeline { device: device.clone(), queue: queue.clone(), pipeline, layout }
    }
}

/// Triangles on the card.
pub(crate) struct Mesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
    locals: wgpu::Buffer,
    group: wgpu::BindGroup,
}

impl Mesh {
    /// `parts`, one after another, on the card; none if it is more than one buffer of this card can hold.
    fn upload(pipeline: &Pipeline, parts: &[&epaint::Mesh]) -> Option<Mesh> {
        let device = &pipeline.device;
        let limit = device.limits().max_buffer_size;
        let (vertices, indices) = parts.iter().fold((0u64, 0u64), |(v, i), m| (v + m.vertices.len() as u64, i + m.indices.len() as u64));
        let count = u32::try_from(indices).ok()?;
        if (vertices * STRIDE).max(indices * 4) > limit {
            return None;
        }
        let buffer = |label, size, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: usage | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let vertex_buffer = buffer("canvas vertices", vertices * STRIDE, wgpu::BufferUsages::VERTEX);
        let index_buffer = buffer("canvas indices", indices * 4, wgpu::BufferUsages::INDEX);
        let (mut v, mut i) = (0, 0);
        for part in parts {
            pipeline.queue.write_buffer(&vertex_buffer, v, bytemuck::cast_slice(&part.vertices));
            pipeline.queue.write_buffer(&index_buffer, i, bytemuck::cast_slice(&part.indices));
            v += part.vertices.len() as u64 * STRIDE;
            i += part.indices.len() as u64 * 4;
        }
        let locals = buffer("canvas locals", LOCALS, wgpu::BufferUsages::UNIFORM);
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("canvas locals"),
            layout: &pipeline.layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: locals.as_entire_binding() }],
        });
        Some(Mesh { vertices: vertex_buffer, indices: index_buffer, count, locals, group })
    }
}

struct Draw {
    pipeline: Arc<Pipeline>,
    mesh: Arc<Mesh>,
    /// How far they are scaled and then moved from where they were made, points.
    k: f32,
    moved: egui::Vec2,
}

impl egui_wgpu::CallbackTrait for Draw {
    fn prepare(
        &self,
        _: &wgpu::Device,
        queue: &wgpu::Queue,
        screen: &egui_wgpu::ScreenDescriptor,
        _: &mut wgpu::CommandEncoder,
        _: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        // As egui's renderer works out its own.
        let (w, h) = (screen.size_in_pixels[0] as f32 / screen.pixels_per_point, screen.size_in_pixels[1] as f32 / screen.pixels_per_point);
        let mut locals = [0u8; LOCALS as usize];
        locals[0..4].copy_from_slice(&w.to_ne_bytes());
        locals[4..8].copy_from_slice(&h.to_ne_bytes());
        locals[8..12].copy_from_slice(&u32::from(DITHERING).to_ne_bytes());
        locals[12..16].copy_from_slice(&self.k.to_ne_bytes());
        locals[16..20].copy_from_slice(&self.moved.x.to_ne_bytes());
        locals[20..24].copy_from_slice(&self.moved.y.to_ne_bytes());
        queue.write_buffer(&self.mesh.locals, 0, &locals);
        Vec::new()
    }

    fn paint(&self, info: egui::PaintCallbackInfo, pass: &mut wgpu::RenderPass<'static>, _: &egui_wgpu::CallbackResources) {
        // egui narrows the viewport to the callback's rect; the vertices are of the whole screen, and its scissor is the clip.
        pass.set_viewport(0.0, 0.0, info.screen_size_px[0] as f32, info.screen_size_px[1] as f32, 0.0, 1.0);
        pass.set_pipeline(&self.pipeline.pipeline);
        pass.set_bind_group(0, &self.mesh.group, &[]);
        pass.set_vertex_buffer(0, self.mesh.vertices.slice(..));
        pass.set_index_buffer(self.mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.mesh.count, 0, 0..1);
    }
}

/// All but the drawing that the triangles depend on: what a frame or a worker makes them for.
#[derive(Clone, Copy)]
pub(crate) struct Look {
    origin: egui::Pos2,
    clip: egui::Rect,
    pixels_per_point: f32,
    options: TessellationOptions,
    dark: bool,
    /// The paper's colour the user picked, which `canvas::color` paints the drawing's white in.
    ground: Option<egui::Color32>,
    thin: bool,
    hide: Option<Id>,
}

impl Look {
    /// A canvas whose top left is `origin`, painted through `p`.
    pub(crate) fn now(ctx: &Context, p: &Painter, origin: egui::Pos2, hide: Option<Id>, thin: bool) -> Look {
        Look {
            origin,
            clip: p.clip_rect(),
            pixels_per_point: ctx.pixels_per_point(),
            options: ctx.tessellation_options(|o| *o),
            dark: crate::style::canvas_dark(),
            ground: crate::style::canvas_ground(),
            thin,
            hide,
        }
    }

    /// Whether triangles of this look, wherever they were made, are those of `o`.
    fn same_ink(&self, o: &Look) -> bool {
        self.pixels_per_point == o.pixels_per_point
            && self.options == o.options
            && (self.dark, self.ground, self.thin, self.hide) == (o.dark, o.ground, o.thin, o.hide)
    }

    fn same_place(&self, o: &Look) -> bool {
        self.origin == o.origin && self.clip == o.clip
    }

    fn place(&self) -> Place {
        Place { hide: self.hide, ..Place::at(self.origin) }
    }

    /// How far round the canvas triangles are made: half of it again each way, so a pan shows what is there before the exact drawing
    /// comes. What lies past the clip is cut by the scissor, so the pixels on the canvas don't change.
    fn reach(&self) -> egui::Rect {
        self.clip.expand2(self.clip.size() / 2.0)
    }
}

/// All that what is kept depends on.
struct Key {
    drawing: Arc<Drawing>,
    look: Look,
}

enum Kept {
    /// It draws nothing.
    Nothing,
    Mesh(Arc<Mesh>, Tinted),
    /// More than the card's buffers hold: today's path.
    TooBig,
}

/// Each element's place among what is kept, and how it is tinted now: a change of tint writes its vertices where they are, so it stays
/// in its place in the drawing's order and costs its own triangles.
struct Tinted {
    /// Each primitive that drew anything, of its element: its first vertex, its vertices and its indices.
    pieces: Vec<(Id, Piece)>,
    /// `pieces` by element, once a tint has asked: a drawing panned past is never tinted.
    owned: Option<HashMap<Id, Vec<Piece>>>,
    selected: Vec<Id>,
    hovered: Option<Id>,
}

#[derive(Clone, Copy)]
struct Piece {
    prim: u32,
    first: u32,
    vertices: u32,
    indices: u32,
}

/// One canvas's kept triangles, of the last frame that drew it.
#[derive(Default)]
pub(crate) struct Retained {
    kept: Option<(Key, Kept)>,
    #[cfg(test)]
    pub(crate) built: u64,
    #[cfg(test)]
    pub(crate) retinted: u64,
}

/// A copy of a canvas makes its own: tints are written into the buffers, which two canvases can't share.
impl Clone for Retained {
    fn clone(&self) -> Self {
        Retained::default()
    }
}

impl Retained {
    /// Draws `d` through `p`, where `at` puts it, on a canvas whose top left is `base`, as [`canvas::paint`] would, from what is kept.
    /// Put anywhere else than its own place, what is kept of `d` is drawn moved rather than made again. False where it can't, and nothing is drawn.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn paint(
        &mut self,
        ctx: &Context,
        pipeline: &Arc<Pipeline>,
        p: &Painter,
        at: Place,
        base: egui::Pos2,
        d: &Arc<Drawing>,
        selected: &[Id],
        hovered: Option<Id>,
        thin: bool,
    ) -> bool {
        // What the shapes' own painter would do to them, and what needs a texture, is left to it.
        if at.paper || p.opacity() != 1.0 || holds_picture(d) {
            return false;
        }
        let look = Look::now(ctx, p, base, at.hide, thin);
        let moved = at.k != 1.0 || at.origin != base;
        let holds = |old: &Key| Arc::ptr_eq(&old.drawing, d) && old.look.same_ink(&look) && (moved || old.look.same_place(&look));
        // Only the tints changed: the same triangles, their colours written where they are.
        if let Some((old, Kept::Mesh(mesh, tinted))) = &mut self.kept
            && holds(old)
            && (tinted.selected != selected || tinted.hovered != hovered)
        {
            if retint(pipeline, old, mesh, tinted, selected, hovered) {
                #[cfg(test)]
                {
                    self.retinted += 1;
                }
            } else {
                self.kept = None;
            }
        }
        if !self.kept.as_ref().is_some_and(|(old, kept)| holds(old) && tints_hold(kept, selected, hovered)) {
            // Release the old triangles before making the new.
            self.kept = None;
            let key = Key { drawing: Arc::clone(d), look };
            let kept = build(pipeline, &key, selected, hovered, &|| false).unwrap_or(Kept::Nothing);
            self.kept = Some((key, kept));
            #[cfg(test)]
            {
                self.built += 1;
            }
        }
        match self.kept.as_ref() {
            Some((old, Kept::Mesh(mesh, _))) => {
                let draw = Draw {
                    pipeline: Arc::clone(pipeline),
                    mesh: Arc::clone(mesh),
                    k: at.k,
                    moved: if moved { at.origin - old.look.origin * at.k } else { egui::Vec2::ZERO },
                };
                p.add(egui_wgpu::Callback::new_paint_callback(p.clip_rect(), draw));
                true
            }
            Some((_, Kept::Nothing)) => true,
            _ => false,
        }
    }

    /// What a worker made takes the place of what is kept.
    pub(crate) fn adopt(&mut self, made: Made) {
        self.kept = Some((made.key, made.kept));
    }

    /// The tints what is kept shows, for what's made next to start from.
    pub(crate) fn tints(&self) -> (Vec<Id>, Option<Id>) {
        match &self.kept {
            Some((_, Kept::Mesh(_, tinted))) => (tinted.selected.clone(), tinted.hovered),
            _ => (Vec::new(), None),
        }
    }
}

fn holds_picture(d: &Drawing) -> bool {
    d.prims.iter().any(|prim| matches!(prim, bimcraft_core::render::Prim::Image { .. }))
}

/// A drawing's triangles, made away from the interface's thread.
pub(crate) struct Made {
    key: Key,
    kept: Kept,
}

/// `drawing`'s triangles on the card, for [`Retained::adopt`]; none for what stays on `canvas::paint`, or when `cancelled`.
pub(crate) fn make(
    pipeline: &Pipeline,
    drawing: Arc<Drawing>,
    look: Look,
    selected: &[Id],
    hovered: Option<Id>,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Option<Made> {
    if holds_picture(&drawing) {
        return None;
    }
    let key = Key { drawing, look };
    let kept = build(pipeline, &key, selected, hovered, cancelled)?;
    Some(Made { key, kept })
}

/// Whether what is kept shows these tints. What draws nothing has none to show; what the card couldn't hold is drawn by `canvas::paint`.
fn tints_hold(kept: &Kept, selected: &[Id], hovered: Option<Id>) -> bool {
    match kept {
        Kept::Mesh(_, tinted) => tinted.selected == selected && tinted.hovered == hovered,
        Kept::Nothing | Kept::TooBig => true,
    }
}

/// The triangles of `key`; none once `cancelled`.
fn build(pipeline: &Pipeline, key: &Key, selected: &[Id], hovered: Option<Id>, cancelled: &(dyn Fn() -> bool + Sync)) -> Option<Kept> {
    use rayon::prelude::*;
    let d = &key.drawing;
    let (at, reach, look) = (key.look.place(), key.look.reach(), &key.look);
    let set: HashSet<Id> = selected.iter().copied().collect();
    let prims: Vec<usize> = (0..d.prims.len()).collect();
    // A part of the drawing's primitives at a time, each thread's triangles in their place in the order, so the whole is what one
    // thread would have made.
    let parts: Vec<(epaint::Mesh, Vec<(Id, Piece)>)> = prims
        .par_chunks(PRIMS_AT_ONCE)
        .map(|chunk| {
            if cancelled() {
                return Default::default();
            }
            // The canvas's look is kept per thread, and this may be another.
            crate::style::set_canvas(look.dark, look.ground);
            // The shapes `canvas::paint` makes hold no circles or text, the only things that read the two last.
            let mut tessellator = Tessellator::new(look.pixels_per_point, look.options, [1, 1], Vec::new());
            tessellator.set_clip_rect(reach);
            let (mut mesh, mut pieces) = (epaint::Mesh::default(), Vec::new());
            for &i in chunk {
                let (Some(prim), Some(owner), Some(part)) = (d.prims.get(i), d.owners.get(i), d.parts.get(i)) else { continue };
                let (first, indices) = (mesh.vertices.len(), mesh.indices.len());
                let out = &mut |shape| tessellator.tessellate_shape(shape, &mut mesh);
                canvas::paint_prim(out, reach, at, (prim, *owner, *part), &set, hovered, look.thin, &mut [].iter());
                if let Some(id) = owner
                    && mesh.vertices.len() > first
                {
                    let piece = Piece {
                        prim: i as u32,
                        first: first as u32,
                        vertices: (mesh.vertices.len() - first) as u32,
                        indices: (mesh.indices.len() - indices) as u32,
                    };
                    pieces.push((*id, piece));
                }
            }
            (mesh, pieces)
        })
        .collect();
    if cancelled() {
        return None;
    }
    // Each part's place in the whole, and its indices and pieces put there. Written straight to the card: no whole on this side.
    let mut parts = parts;
    let mut base = 0;
    let bases: Vec<u32> = parts
        .iter()
        .map(|(mesh, _)| {
            let at = base;
            base += mesh.vertices.len() as u32;
            at
        })
        .collect();
    parts.par_iter_mut().zip(&bases).for_each(|((mesh, pieces), base)| {
        mesh.indices.iter_mut().for_each(|i| *i += base);
        pieces.iter_mut().for_each(|(_, piece)| piece.first += base);
    });
    if parts.iter().all(|(mesh, _)| mesh.indices.is_empty()) {
        return Some(Kept::Nothing);
    }
    let meshes: Vec<&epaint::Mesh> = parts.iter().map(|(mesh, _)| mesh).collect();
    let Some(mesh) = Mesh::upload(pipeline, &meshes) else { return Some(Kept::TooBig) };
    let pieces = parts.into_iter().flat_map(|(_, pieces)| pieces).collect();
    Some(Kept::Mesh(Arc::new(mesh), Tinted { pieces, owned: None, selected: selected.to_vec(), hovered }))
}

/// `tinted` brought to `selected` and `hovered`, each element whose tint changes tessellated again and written where it is.
/// False when that can't be done (most of the drawing changes, or its triangles came out otherwise): make it again.
fn retint(pipeline: &Pipeline, key: &Key, mesh: &Mesh, tinted: &mut Tinted, selected: &[Id], hovered: Option<Id>) -> bool {
    #[derive(PartialEq)]
    enum Tint {
        Selected,
        Hovered,
        Plain,
    }
    let (was, now): (HashSet<Id>, HashSet<Id>) = (tinted.selected.iter().copied().collect(), selected.iter().copied().collect());
    let tint = |id: Id, set: &HashSet<Id>, over: Option<Id>| {
        if set.contains(&id) {
            Tint::Selected
        } else if over == Some(id) {
            Tint::Hovered
        } else {
            Tint::Plain
        }
    };
    let touched = was.iter().chain(&now).copied().chain(tinted.hovered).chain(hovered).collect::<HashSet<Id>>();
    let changed: Vec<Id> = touched.into_iter().filter(|id| tint(*id, &was, tinted.hovered) != tint(*id, &now, hovered)).collect();
    let d = &key.drawing;
    let owned = tinted.owned.get_or_insert_with(|| {
        let mut owned: HashMap<Id, Vec<Piece>> = HashMap::new();
        for (id, piece) in &tinted.pieces {
            owned.entry(*id).or_default().push(*piece);
        }
        owned
    });
    let pieces: usize = changed.iter().filter_map(|id| owned.get(id)).map(Vec::len).sum();
    if pieces > d.prims.len() / 4 + 16 {
        return false;
    }
    let (at, reach, look) = (key.look.place(), key.look.reach(), &key.look);
    let mut tessellator = Tessellator::new(look.pixels_per_point, look.options, [1, 1], Vec::new());
    tessellator.set_clip_rect(reach);
    // All first: a piece that doesn't come out the same leaves what is kept as it was.
    let mut writes = Vec::with_capacity(pieces);
    for id in &changed {
        for piece in owned.get(id).map_or(&[][..], Vec::as_slice) {
            let i = piece.prim as usize;
            let (Some(prim), Some(part)) = (d.prims.get(i), d.parts.get(i)) else { return false };
            let mut again = epaint::Mesh::default();
            let out = &mut |shape| tessellator.tessellate_shape(shape, &mut again);
            canvas::paint_prim(out, reach, at, (prim, Some(*id), *part), &now, hovered, look.thin, &mut [].iter());
            if again.vertices.len() != piece.vertices as usize || again.indices.len() != piece.indices as usize {
                return false;
            }
            writes.push((piece.first, again.vertices));
        }
    }
    for (first, vertices) in writes {
        pipeline.queue.write_buffer(&mesh.vertices, u64::from(first) * STRIDE, bytemuck::cast_slice(&vertices));
    }
    tinted.selected = selected.to_vec();
    tinted.hovered = hovered;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use bimcraft_core::engine::Session;
    use bimcraft_core::render::{self, View};
    use egui::{Color32, Pos2, RawInput, Rect, Stroke, ViewportId, pos2, vec2};

    const SIDE: [u32; 2] = [900, 640];

    fn card() -> Option<(wgpu::Device, wgpu::Queue)> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            // As `gpu::headless`: without the debug names the Vulkan loader crashes on, between tests side by side.
            flags: wgpu::InstanceFlags::empty(),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
        crate::gpu::alone_on(adapter.get_info().device_type);
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()
    }

    /// The window's pixels, drawn by egui's own renderer as the window's, what `draw` paints between a ground and an overlay.
    fn frame(
        (device, queue): &(wgpu::Device, wgpu::Queue),
        format: wgpu::TextureFormat,
        ppp: f32,
        clip: Rect,
        mut draw: impl FnMut(&Painter, &Context),
    ) -> Vec<u8> {
        let ctx = Context::default();
        ctx.options_mut(crate::style::tessellation);
        let mut raw = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(SIDE[0] as f32, SIDE[1] as f32) / ppp)),
            ..RawInput::default()
        };
        raw.viewports.entry(ViewportId::ROOT).or_default().native_pixels_per_point = Some(ppp);
        let output = ctx.run_ui(raw, |ui| {
            let ctx = ui.ctx().clone();
            let window = ctx.layer_painter(egui::LayerId::background());
            window.rect_filled(ctx.content_rect(), 0.0, Color32::from_rgb(40, 60, 90));
            let p = window.with_clip_rect(clip);
            p.rect_filled(clip, 0.0, canvas::color(render::PAPER));
            draw(&p, &ctx);
            // What the tools draw over it must stay over it.
            p.add(egui::Shape::line_segment([clip.min, clip.max], Stroke::new(3.0, Color32::from_rgba_unmultiplied(200, 30, 30, 160))));
        });
        pixels((device, queue), format, &ctx, vec![output])
    }

    /// `outputs`, frames of `ctx` one after another, drawn as the window's renderer would; the last frame's pixels.
    fn pixels(
        (device, queue): (&wgpu::Device, &wgpu::Queue),
        format: wgpu::TextureFormat,
        ctx: &Context,
        mut outputs: Vec<egui::FullOutput>,
    ) -> Vec<u8> {
        let mut renderer = egui_wgpu::Renderer::new(
            device,
            format,
            egui_wgpu::RendererOptions { msaa_samples: 1, depth_stencil_format: None, dithering: DITHERING, ..Default::default() },
        );
        for output in &mut outputs {
            for (id, deltas) in output.textures_delta.set.drain() {
                for delta in deltas {
                    renderer.update_texture(device, queue, id, &delta);
                }
            }
            output.textures_delta.free.clear();
        }
        let Some(mut output) = outputs.pop() else { return Vec::new() };
        let jobs = ctx.tessellate(std::mem::take(&mut output.shapes), output.pixels_per_point);
        let screen = egui_wgpu::ScreenDescriptor { size_in_pixels: SIDE, pixels_per_point: output.pixels_per_point };
        let size = wgpu::Extent3d { width: SIDE[0], height: SIDE[1], depth_or_array_layers: 1 };
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let mut encoder = device.create_command_encoder(&Default::default());
        let prepared = renderer.update_buffers(device, queue, &mut encoder, &jobs, &screen);
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            renderer.render(&mut pass.forget_lifetime(), &jobs, &screen);
        }
        let row = (SIDE[0] * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(row * SIDE[1]),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &read,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None },
            },
            size,
        );
        queue.submit(prepared.into_iter().chain([encoder.finish()]));
        read.slice(..).map_async(wgpu::MapMode::Read, |_| ());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let mapped = read.slice(..).get_mapped_range().unwrap();
        mapped.chunks(row as usize).flat_map(|r| r[..SIDE[0] as usize * 4].iter().copied()).collect()
    }

    /// A house with dashed lines, fills and lettering, its plan fitted to the window; and an element of it.
    fn drawing() -> (Arc<bimcraft_core::model::Model>, Id, View, Id) {
        let mut s = Session::new();
        s.script(bimcraft_core::sample::SAMPLE).unwrap();
        let m = Arc::clone(&s.model);
        let level = m.levels()[0].0;
        let view = render::plan(&m, level, SIDE[0], SIDE[1]).view;
        let owner = render::draw(&m, level, view).owners.iter().flatten().next().copied().unwrap();
        (m, level, view, owner)
    }

    #[test]
    fn kept_triangles_draw_the_pixels_egui_draws() {
        let Some(card) = card() else {
            eprintln!("no card: the comparison needs one");
            return;
        };
        let (m, level, view, owner) = drawing();
        let second = render::draw(&m, level, view).owners.iter().flatten().copied().find(|id| *id != owner).unwrap();
        let zoomed = View { scale: view.scale * 6.0, ..view };
        let clips = [Rect::from_min_size(pos2(0.0, 0.0), vec2(900.0, 640.0)), Rect::from_min_max(pos2(37.0, 21.0), pos2(611.0, 503.0))];
        for format in [wgpu::TextureFormat::Rgba8Unorm, wgpu::TextureFormat::Rgba8UnormSrgb] {
            let pipeline = Arc::new(Pipeline::new(&card.0, &card.1, format));
            // A paper of the user's colour, which the canvas paints the drawing's white in.
            let tan = Some(egui::Color32::from_rgb(232, 220, 196));
            for (ppp, view, clip, selected, hovered, thin, dark, ground) in [
                (1.0, view, clips[0], vec![], None, false, false, None),
                (1.0, view, clips[0], vec![], None, false, true, None),
                (1.0, view, clips[1], vec![owner], None, false, false, None),
                (1.0, zoomed, clips[1], vec![], Some(owner), true, true, None),
                (1.5, zoomed, clips[0], vec![owner], None, false, false, None),
                (1.0, view, clips[0], vec![owner], None, false, false, tan),
            ] {
                crate::style::set_canvas(dark, ground);
                let d = Arc::new(render::draw(&m, level, view));
                let at = Place::at(clip.min);
                let today = frame(&card, format, ppp, clip, |p, _| canvas::paint(p, at, &d, &selected, hovered, thin, &[]));
                let mut retained = Retained::default();
                let mut drew = false;
                let kept = frame(&card, format, ppp, clip, |p, ctx| {
                    drew = retained.paint(ctx, &pipeline, p, at, at.origin, &d, &selected, hovered, thin);
                });
                assert!(drew, "the kept triangles draw");
                // And brought there from another tint, written in place.
                let mut moved = Retained::default();
                let other = frame(&card, format, ppp, clip, |p, ctx| {
                    moved.paint(ctx, &pipeline, p, at, at.origin, &d, &[], Some(second), thin);
                });
                let through = frame(&card, format, ppp, clip, |p, ctx| {
                    moved.paint(ctx, &pipeline, p, at, at.origin, &d, &selected, hovered, thin);
                });
                assert_eq!(moved.built, 1, "a change of tint keeps the triangles");
                assert!(moved.retinted <= 1);
                let differs = today.chunks(4).zip(through.chunks(4)).filter(|(a, b)| a != b).count();
                assert_eq!(differs, 0, "{format:?} at {ppp}: {differs} pixels differ once retinted (dark {dark}, thin {thin})");
                let second_today = frame(&card, format, ppp, clip, |p, _| canvas::paint(p, at, &d, &[], Some(second), thin, &[]));
                assert!(other.chunks(4).zip(second_today.chunks(4)).all(|(a, b)| a == b), "the tint it came from");
                assert!(today.iter().any(|b| *b != today[0]), "the frame holds something");
                let differs = today.chunks(4).zip(kept.chunks(4)).filter(|(a, b)| a != b).count();
                assert_eq!(differs, 0, "{format:?} at {ppp}: {differs} pixels differ from egui's (dark {dark}, thin {thin})");
            }
        }
        crate::style::set_canvas(false, None);
    }

    /// Lettering a third of a pixel below the clip: its outline, a stroke thinner than a pixel, is drawn with a soft edge a pixel
    /// wide, which reaches the clip's last row. egui's coarse culling measured it by half its width and left it out, so the kept
    /// triangles, made for more than the clip, drew a row egui didn't (a fallback font on Linux ARM put a room tag there).
    #[test]
    fn lettering_just_past_the_clip_shows_its_soft_edge_as_egui_draws_it() {
        let Some(card) = card() else {
            eprintln!("no card: the comparison needs one");
            return;
        };
        let (m, level, view, _) = drawing();
        let clip = Rect::from_min_max(pos2(37.0, 21.0), pos2(611.0, 503.0));
        let mut d = render::draw(&m, level, view);
        // In the drawing's pixels, which the canvas puts at the clip's corner.
        let (left, top) = (400.0, clip.max.y - clip.min.y + 0.3);
        d.prims.push(render::Prim::Fill {
            rings: vec![vec![(left, top), (left + 10.0, top), (left + 10.0, top + 10.0), (left, top + 10.0)]],
            color: render::Rgb(0, 0, 0),
        });
        d.owners.push(None);
        d.parts.push(render::Part::Text);
        d.pens.push(None);
        let d = Arc::new(d);
        let at = Place::at(clip.min);
        crate::style::set_canvas(false, None);
        for format in [wgpu::TextureFormat::Rgba8Unorm, wgpu::TextureFormat::Rgba8UnormSrgb] {
            let pipeline = Arc::new(Pipeline::new(&card.0, &card.1, format));
            let today = frame(&card, format, 1.0, clip, |p, _| canvas::paint(p, at, &d, &[], None, false, &[]));
            let mut retained = Retained::default();
            let kept = frame(&card, format, 1.0, clip, |p, ctx| {
                retained.paint(ctx, &pipeline, p, at, at.origin, &d, &[], None, false);
            });
            let last = |v: &[u8]| {
                let row = (clip.max.y as usize - 1) * SIDE[0] as usize * 4;
                v[row + (clip.min.x + left) as usize * 4..row + (clip.min.x + left + 10.0) as usize * 4].to_vec()
            };
            assert!(last(&today).chunks(4).any(|px| px[0] < 250), "{format:?}: the soft edge shows on the clip's last row");
            let differs = today.chunks(4).zip(kept.chunks(4)).filter(|(a, b)| a != b).count();
            assert_eq!(differs, 0, "{format:?}: {differs} pixels differ from egui's");
        }
    }

    /// The whole app's window on a plan, with a wall selected: `card` given or not.
    fn window(
        card: &(wgpu::Device, wgpu::Queue),
        format: wgpu::TextureFormat,
        pipeline: Option<Arc<Pipeline>>,
        dark: bool,
        light_canvas: bool,
    ) -> (Vec<u8>, bool) {
        let mut s = Session::new();
        s.script(bimcraft_core::sample::SAMPLE).unwrap();
        let level = s.model.levels()[0].0;
        let wall = s.model.elements.iter().find_map(|(&id, e)| matches!(e, bimcraft_core::model::Element::Wall(_)).then_some(id)).unwrap();
        let mut app = crate::BimApp::new(s);
        app.light_canvas = light_canvas;
        app.open_sheet(crate::Sheet::Plan(level));
        app.selection = vec![wall];
        app.card.canvas = pipeline;
        app.theme_wanted = Some(if dark { egui::ThemePreference::Dark } else { egui::ThemePreference::Light });
        let ctx = Context::default();
        let mut outputs: Vec<_> = (0..6).map(|i| step(&mut app, &ctx, i)).collect();
        // The layout settles over the first frames, which moves the plan's view: on the card the frame shows the last drawing moved,
        // which may round a pixel otherwise, until a worker brings the one for where it settled.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let mut i = 6;
        while app.canvas.drawing.shown_drawing().map(|d| d.view) != app.canvas.current_view(level) {
            assert!(std::time::Instant::now() < deadline, "the worker published nothing");
            std::thread::sleep(std::time::Duration::from_millis(5));
            outputs.push(step(&mut app, &ctx, i));
            i += 1;
        }
        assert_eq!(
            (crate::style::dark(), crate::style::canvas_dark()),
            (dark, dark && !light_canvas),
            "the look asked for is the one drawn"
        );
        let kept = app.canvas.drawing.retained.built > 0;
        (pixels((&card.0, &card.1), format, &ctx, outputs), kept)
    }

    #[test]
    #[ignore = "exports family document pixels for visual inspection"]
    fn export_family_document_looks() {
        let card = card().expect("a graphics adapter for visual inspection");
        let dir = std::path::Path::new("target/a3-verification/looks");
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("size.txt"), format!("{} {}", SIDE[0], SIDE[1])).unwrap();
        for profile in [crate::style::Profile::Rev, crate::style::Profile::BimCraft] {
            for (look, dark, light_canvas) in [("light", false, false), ("dark", true, false), ("dark-ui-light-canvas", true, true)] {
                let mut app = crate::BimApp::new(Session::new());
                app.profile = profile;
                app.light_canvas = light_canvas;
                app.theme_wanted = Some(if dark { egui::ThemePreference::Dark } else { egui::ThemePreference::Light });
                app.new_family(bimcraft_core::family::Category::Generic);
                let ctx = Context::default();
                let side = Rect::from_min_size(Pos2::ZERO, vec2(SIDE[0] as f32, SIDE[1] as f32));
                let mut outputs = Vec::new();
                for i in 0..6 {
                    let mut raw = RawInput { screen_rect: Some(side), time: Some(f64::from(i) * 10.), ..RawInput::default() };
                    raw.viewports.entry(ViewportId::ROOT).or_default().native_pixels_per_point = Some(1.);
                    outputs.push(ctx.run_ui(raw, |ui| app.show(ui)));
                }
                let rgba = pixels((&card.0, &card.1), wgpu::TextureFormat::Rgba8Unorm, &ctx, outputs);
                std::fs::write(dir.join(format!("{profile:?}-{look}.rgba")), rgba).unwrap();
            }
        }
    }

    #[test]
    fn the_whole_window_is_the_same_on_kept_triangles_in_each_look() {
        let Some(card) = card() else { return };
        let format = wgpu::TextureFormat::Bgra8Unorm;
        let pipeline = Arc::new(Pipeline::new(&card.0, &card.1, format));
        let mut looks: Vec<Vec<u8>> = Vec::new();
        for (name, dark, light_canvas) in [("light", false, false), ("dark", true, false), ("dark-ui-light-canvas", true, true)] {
            let (today, off) = window(&card, format, None, dark, light_canvas);
            let (kept, on) = window(&card, format, Some(Arc::clone(&pipeline)), dark, light_canvas);
            assert!(!off && on, "{name}: the two paths are the two paths");
            let differs = today.chunks(4).zip(kept.chunks(4)).filter(|(a, b)| a != b).count();
            assert!(differs <= crate::gpu::pixels_that_may_differ(), "{name}: {differs} pixels of the window differ");
            assert!(!looks.contains(&kept), "{name} looks like a look before it");
            looks.push(kept);
        }
    }

    #[test]
    fn what_nothing_changed_in_is_not_made_again() {
        let Some(card) = card() else { return };
        let (m, level, view, owner) = drawing();
        let d = Arc::new(render::draw(&m, level, view));
        let pipeline = Arc::new(Pipeline::new(&card.0, &card.1, wgpu::TextureFormat::Rgba8Unorm));
        let clip = Rect::from_min_size(Pos2::ZERO, vec2(900.0, 640.0));
        let ctx = Context::default();
        let p = Painter::new(ctx.clone(), egui::LayerId::background(), clip);
        let at = Place::at(clip.min);
        let mut kept = Retained::default();
        for _ in 0..5 {
            assert!(kept.paint(&ctx, &pipeline, &p, at, at.origin, &d, &[], None, false));
        }
        assert_eq!(kept.built, 1, "frames that change nothing make nothing");
        assert!(kept.paint(&ctx, &pipeline, &p, at, at.origin, &d, &[], Some(owner), false));
        assert!(kept.paint(&ctx, &pipeline, &p, at, at.origin, &d, &[owner], Some(owner), false));
        assert!(kept.paint(&ctx, &pipeline, &p, at, at.origin, &d, &[owner], Some(owner), true));
        assert_eq!((kept.built, kept.retinted), (2, 2), "a tint writes its element where it is; Thin Lines makes the whole again");
        let again = Arc::new(render::draw(&m, level, view));
        assert!(kept.paint(&ctx, &pipeline, &p, at, at.origin, &again, &[owner], Some(owner), true));
        assert_eq!(kept.built, 3, "another drawing shows");
    }

    #[test]
    fn what_is_kept_moves_a_whole_pixel_to_what_egui_draws_there_and_is_not_made_again() {
        let Some(card) = card() else { return };
        let (m, level, view, owner) = drawing();
        let zoomed = View { scale: view.scale * 3.0, ..view };
        let d = Arc::new(render::draw(&m, level, zoomed));
        let clip = Rect::from_min_max(pos2(37.0, 21.0), pos2(611.0, 503.0));
        let base = clip.min;
        for format in [wgpu::TextureFormat::Rgba8Unorm, wgpu::TextureFormat::Rgba8UnormSrgb] {
            let pipeline = Arc::new(Pipeline::new(&card.0, &card.1, format));
            let mut kept = Retained::default();
            frame(&card, format, 1.0, clip, |p, ctx| {
                kept.paint(ctx, &pipeline, p, Place::at(base), base, &d, &[owner], None, false);
            });
            for by in [vec2(40.0, -25.0), vec2(-90.0, 60.0)] {
                let at = Place::at(base + by);
                let moved = frame(&card, format, 1.0, clip, |p, ctx| {
                    assert!(kept.paint(ctx, &pipeline, p, at, base, &d, &[owner], None, false));
                });
                let today = frame(&card, format, 1.0, clip, |p, _| canvas::paint(p, at, &d, &[owner], None, false, &[]));
                // A vertex's position is summed otherwise, in floats, so an edge's coverage may round the other way: this is what shows
                // only until the exact drawing comes.
                let differs: Vec<u8> = today.iter().zip(&moved).filter(|(a, b)| a != b).map(|(a, b)| a.abs_diff(*b)).collect();
                // (Three levels where sRGB's curve is steep, at its darkest.)
                let rounding = if format.is_srgb() { 3 } else { 1 };
                assert!(differs.iter().all(|d| *d <= rounding), "{format:?} moved {by:?}: a pixel is further off than rounding");
                assert!(differs.len() < 200, "{format:?} moved {by:?}: {} channels differ", differs.len());
            }
            assert_eq!(kept.built, 1, "moving makes nothing");
            // A zoom scales them; and home again, the pixels they were made for.
            let home = frame(&card, format, 1.0, clip, |p, _| canvas::paint(p, Place::at(base), &d, &[owner], None, false, &[]));
            let scaled = frame(&card, format, 1.0, clip, |p, ctx| {
                kept.paint(ctx, &pipeline, p, Place { k: 1.5, ..Place::at(base) }, base, &d, &[owner], None, false);
            });
            assert_ne!(home, scaled);
            let back = frame(&card, format, 1.0, clip, |p, ctx| {
                kept.paint(ctx, &pipeline, p, Place::at(base), base, &d, &[owner], None, false);
            });
            assert_eq!(home, back);
            assert_eq!(kept.built, 1);
        }
    }

    /// One frame of the app in `ctx`.
    fn step(app: &mut crate::BimApp, ctx: &Context, i: u32) -> egui::FullOutput {
        let side = Rect::from_min_size(Pos2::ZERO, vec2(SIDE[0] as f32, SIDE[1] as f32));
        let mut raw = RawInput { screen_rect: Some(side), time: Some(f64::from(i) * 10.0), ..RawInput::default() };
        raw.viewports.entry(ViewportId::ROOT).or_default().native_pixels_per_point = Some(1.0);
        ctx.run_ui(raw, |ui| app.show(ui))
    }

    #[test]
    fn a_view_moved_shows_what_is_kept_until_a_worker_replaces_it_with_the_exact_pixels() {
        let Some(card) = card() else { return };
        let format = wgpu::TextureFormat::Bgra8Unorm;
        let pipeline = Arc::new(Pipeline::new(&card.0, &card.1, format));
        let (_, level, _, _) = drawing();
        let start = |pipeline: Option<Arc<Pipeline>>| {
            let mut s = Session::new();
            s.script(bimcraft_core::sample::SAMPLE).unwrap();
            let mut app = crate::BimApp::new(s);
            app.open_sheet(crate::Sheet::Plan(level));
            app.card.canvas = pipeline;
            app
        };
        let (lo, hi) = (bimcraft_core::geom::P2(1000.0, 1000.0), bimcraft_core::geom::P2(6000.0, 5000.0));

        let mut app = start(Some(Arc::clone(&pipeline)));
        let ctx = Context::default();
        let mut outputs = Vec::new();
        for i in 0..4 {
            outputs.push(step(&mut app, &ctx, i));
        }
        assert_eq!(app.canvas.drawing.retained.built, 1, "the plan's first drawing is made in the frame");
        // The window's layout settles over its first frames, which is a view that moved too.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let mut i = 4;
        while app.canvas.drawing.shown_drawing().unwrap().view != app.canvas.current_view(level).unwrap() {
            assert!(std::time::Instant::now() < deadline, "the worker published nothing");
            std::thread::sleep(std::time::Duration::from_millis(5));
            outputs.push(step(&mut app, &ctx, i));
            i += 1;
        }
        let first = app.canvas.drawing.shown_drawing().unwrap();
        app.canvas.show(level, lo, hi);
        outputs.push(step(&mut app, &ctx, i));
        i += 1;
        assert!(Arc::ptr_eq(&first, &app.canvas.drawing.shown_drawing().unwrap()), "the frame that moved doesn't wait for a drawing");
        let wanted = app.canvas.current_view(level).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while app.canvas.drawing.shown_drawing().unwrap().view != wanted {
            assert!(std::time::Instant::now() < deadline, "the worker published nothing");
            std::thread::sleep(std::time::Duration::from_millis(5));
            outputs.push(step(&mut app, &ctx, i));
            i += 1;
        }
        for _ in 0..3 {
            outputs.push(step(&mut app, &ctx, i));
            i += 1;
        }
        assert_eq!(app.canvas.drawing.retained.built, 1, "nothing was made on the interface's thread for the move");
        let kept = pixels((&card.0, &card.1), format, &ctx, outputs);

        // The same window drawn the way a window with no card draws it, at the same view.
        let mut today = start(None);
        let ctx = Context::default();
        let mut outputs = Vec::new();
        for i in 0..2 {
            outputs.push(step(&mut today, &ctx, i));
        }
        today.canvas.show(level, lo, hi);
        for i in 2..6 {
            outputs.push(step(&mut today, &ctx, i));
        }
        let differs = pixels((&card.0, &card.1), format, &ctx, outputs).chunks(4).zip(kept.chunks(4)).filter(|(a, b)| a != b).count();
        assert_eq!(differs, 0, "{differs} pixels differ once the exact drawing is in");
    }

    #[test]
    fn an_edit_while_a_worker_draws_shows_at_once_and_what_the_worker_made_of_before_is_dropped() {
        let Some(card) = card() else { return };
        let pipeline = Arc::new(Pipeline::new(&card.0, &card.1, wgpu::TextureFormat::Bgra8Unorm));
        let (_, level, _, _) = drawing();
        let mut s = Session::new();
        s.script(bimcraft_core::sample::SAMPLE).unwrap();
        let mut app = crate::BimApp::new(s);
        app.open_sheet(crate::Sheet::Plan(level));
        app.card.canvas = Some(pipeline);
        let ctx = Context::default();
        let mut outputs = Vec::new();
        for i in 0..4 {
            outputs.push(step(&mut app, &ctx, i));
        }
        let before = app.canvas.drawing.shown_drawing().unwrap().prims.len();
        app.canvas.show(level, bimcraft_core::geom::P2(1000.0, 1000.0), bimcraft_core::geom::P2(6000.0, 5000.0));
        outputs.push(step(&mut app, &ctx, 4));
        app.session.execute("wall", &serde_json::json!({"points": [[500, 500], [500, 3000]]})).unwrap();
        outputs.push(step(&mut app, &ctx, 5));
        let edited = app.canvas.drawing.shown_drawing().unwrap();
        let wanted = app.canvas.current_view(level).unwrap();
        assert_eq!(edited.view, wanted, "the edit's frame made its own drawing");
        assert_ne!(edited.prims.len(), before, "and it holds the edit");
        for i in 6..40 {
            outputs.push(step(&mut app, &ctx, i));
            std::thread::sleep(std::time::Duration::from_millis(10));
            assert!(
                Arc::ptr_eq(&edited, &app.canvas.drawing.shown_drawing().unwrap()),
                "what the worker made of the model before never shows"
            );
        }
        // The frames' textures must be handed on, as a window's are.
        pixels((&card.0, &card.1), wgpu::TextureFormat::Bgra8Unorm, &ctx, outputs);
    }

    #[test]
    fn what_was_moved_is_picked_where_it_shows() {
        let (m, level, view, _) = drawing();
        let d = Arc::new(render::draw(&m, level, view));
        let (q, owner) = d
            .prims
            .iter()
            .zip(&d.owners)
            .find_map(|(prim, owner)| match prim {
                render::Prim::Line { pts, .. } => owner.and_then(|o| pts.first().map(|p| (*p, o))),
                _ => None,
            })
            .unwrap();
        // Half as far in, and moved: the same element is where it now shows.
        let later = View { scale: view.scale * 2.0, cx: view.cx + 500.0, cy: view.cy - 300.0, ..view };
        let shown = canvas::Shown::new(Arc::clone(&d), later, Pos2::ZERO);
        let there = later.px(view.mm(q));
        assert!(shown.pick_all(there, 2.0).contains(&owner));
        let held = canvas::Shown::new(Arc::clone(&d), view, Pos2::ZERO);
        assert_eq!(held.pick_all(q, 2.0), d.pick_all(q, 2.0));
        assert_eq!((held.place.k, held.place.origin), (1.0, Pos2::ZERO), "at its own view it is exactly where it was");
    }

    #[test]
    fn what_cant_reach_the_canvas_makes_no_shapes_and_a_picture_stays_on_todays_path() {
        let (m, level, view, _) = drawing();
        let zoomed = View { scale: view.scale * 64.0, ..view };
        let d = render::draw(&m, level, zoomed);
        let count = |clip| {
            let mut n = 0;
            canvas::paint_to(&mut |_| n += 1, clip, Place::at(Pos2::ZERO), &d, &[], None, false, &[]);
            n
        };
        let everywhere = count(Rect::EVERYTHING);
        let canvas = count(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 640.0)));
        assert!(canvas < everywhere, "{canvas} shapes on the canvas of {everywhere}");
        assert_eq!(count(Rect::from_min_size(pos2(1e7, 1e7), vec2(10.0, 10.0))), 0);
    }
}
