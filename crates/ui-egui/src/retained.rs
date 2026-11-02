//! The 2D canvas's triangles kept on the card: made once by egui's own tessellator from the shapes `canvas::paint` makes, and drawn
//! by the sums and blending of egui's own shader, so a frame that changes nothing costs a draw call, and the pixels are the ones
//! egui would have drawn. A window with no card, and a drawing with a picture, stay on `canvas::paint`.
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
const LOCALS: u64 = 16;
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

/// All that what is kept depends on.
struct Key {
    drawing: Arc<Drawing>,
    origin: egui::Pos2,
    clip: egui::Rect,
    pixels_per_point: f32,
    options: TessellationOptions,
    dark: bool,
    thin: bool,
    hide: Option<Id>,
}

impl Key {
    fn same(&self, o: &Key) -> bool {
        Arc::ptr_eq(&self.drawing, &o.drawing)
            && self.origin == o.origin
            && self.clip == o.clip
            && self.pixels_per_point == o.pixels_per_point
            && self.options == o.options
            && (self.dark, self.thin, self.hide) == (o.dark, o.thin, o.hide)
    }
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
    /// Draws `d` through `p` as [`canvas::paint`] would, from what is kept. False where it can't, and nothing is drawn.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn paint(
        &mut self,
        ctx: &Context,
        pipeline: &Arc<Pipeline>,
        p: &Painter,
        at: Place,
        d: &Arc<Drawing>,
        selected: &[Id],
        hovered: Option<Id>,
        thin: bool,
    ) -> bool {
        // What the shapes' own painter would do to them, and what needs a texture, is left to it.
        if at.k != 1.0
            || at.paper
            || p.opacity() != 1.0
            || d.prims.iter().any(|prim| matches!(prim, bimcraft_core::render::Prim::Image { .. }))
        {
            return false;
        }
        let key = Key {
            drawing: Arc::clone(d),
            origin: at.origin,
            clip: p.clip_rect(),
            pixels_per_point: ctx.pixels_per_point(),
            options: ctx.tessellation_options(|o| *o),
            dark: crate::style::canvas_dark(),
            thin,
            hide: at.hide,
        };
        // Only the tints changed: the same triangles, their colours written where they are.
        if let Some((old, Kept::Mesh(mesh, tinted))) = &mut self.kept
            && old.same(&key)
            && (tinted.selected != selected || tinted.hovered != hovered)
        {
            if retint(pipeline, &key, at, mesh, tinted, selected, hovered) {
                #[cfg(test)]
                {
                    self.retinted += 1;
                }
            } else {
                self.kept = None;
            }
        }
        if !self.kept.as_ref().is_some_and(|(old, kept)| old.same(&key) && tints_hold(kept, selected, hovered)) {
            // Release the old triangles before making the new.
            self.kept = None;
            let kept = build(pipeline, &key, at, selected, hovered);
            self.kept = Some((key, kept));
            #[cfg(test)]
            {
                self.built += 1;
            }
        }
        match self.kept.as_ref().map(|(_, kept)| kept) {
            Some(Kept::Mesh(mesh, _)) => {
                let draw = Draw { pipeline: Arc::clone(pipeline), mesh: Arc::clone(mesh) };
                p.add(egui_wgpu::Callback::new_paint_callback(p.clip_rect(), draw));
                true
            }
            Some(Kept::Nothing) => true,
            _ => false,
        }
    }
}

/// Whether what is kept shows these tints. What draws nothing has none to show; what the card couldn't hold is drawn by `canvas::paint`.
fn tints_hold(kept: &Kept, selected: &[Id], hovered: Option<Id>) -> bool {
    match kept {
        Kept::Mesh(_, tinted) => tinted.selected == selected && tinted.hovered == hovered,
        Kept::Nothing | Kept::TooBig => true,
    }
}

fn build(pipeline: &Pipeline, key: &Key, at: Place, selected: &[Id], hovered: Option<Id>) -> Kept {
    use rayon::prelude::*;
    let d = &key.drawing;
    let prims: Vec<usize> = (0..d.prims.len()).collect();
    // A part of the drawing's primitives at a time, each thread's triangles in their place in the order, so the whole is what one
    // thread would have made.
    let parts: Vec<(epaint::Mesh, Vec<(Id, Piece)>)> = prims
        .par_chunks(PRIMS_AT_ONCE)
        .map(|chunk| {
            // The canvas's look is kept per thread, and this may be another.
            crate::style::set_canvas_dark(key.dark);
            // The shapes `canvas::paint` makes hold no circles or text, the only things that read the two last.
            let mut tessellator = Tessellator::new(key.pixels_per_point, key.options, [1, 1], Vec::new());
            tessellator.set_clip_rect(key.clip);
            let (mut mesh, mut pieces) = (epaint::Mesh::default(), Vec::new());
            for &i in chunk {
                let (Some(prim), Some(owner), Some(part)) = (d.prims.get(i), d.owners.get(i), d.parts.get(i)) else { continue };
                let (first, indices) = (mesh.vertices.len(), mesh.indices.len());
                let out = &mut |shape| tessellator.tessellate_shape(shape, &mut mesh);
                canvas::paint_prim(out, key.clip, at, (prim, *owner, *part), selected, hovered, key.thin, &mut [].iter());
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
        return Kept::Nothing;
    }
    let meshes: Vec<&epaint::Mesh> = parts.iter().map(|(mesh, _)| mesh).collect();
    let Some(mesh) = Mesh::upload(pipeline, &meshes) else { return Kept::TooBig };
    let pieces = parts.into_iter().flat_map(|(_, pieces)| pieces).collect();
    Kept::Mesh(Arc::new(mesh), Tinted { pieces, owned: None, selected: selected.to_vec(), hovered })
}

/// `tinted` brought to `selected` and `hovered`, each element whose tint changes tessellated again and written where it is.
/// False when that can't be done (most of the drawing changes, or its triangles came out otherwise): make it again.
fn retint(pipeline: &Pipeline, key: &Key, at: Place, mesh: &Mesh, tinted: &mut Tinted, selected: &[Id], hovered: Option<Id>) -> bool {
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
    let mut tessellator = Tessellator::new(key.pixels_per_point, key.options, [1, 1], Vec::new());
    tessellator.set_clip_rect(key.clip);
    // All first: a piece that doesn't come out the same leaves what is kept as it was.
    let mut writes = Vec::with_capacity(pieces);
    for id in &changed {
        for piece in owned.get(id).map_or(&[][..], Vec::as_slice) {
            let i = piece.prim as usize;
            let (Some(prim), Some(part)) = (d.prims.get(i), d.parts.get(i)) else { return false };
            let mut again = epaint::Mesh::default();
            let out = &mut |shape| tessellator.tessellate_shape(shape, &mut again);
            canvas::paint_prim(out, key.clip, at, (prim, Some(*id), *part), selected, hovered, key.thin, &mut [].iter());
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
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
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
            for (ppp, view, clip, selected, hovered, thin, dark) in [
                (1.0, view, clips[0], vec![], None, false, false),
                (1.0, view, clips[0], vec![], None, false, true),
                (1.0, view, clips[1], vec![owner], None, false, false),
                (1.0, zoomed, clips[1], vec![], Some(owner), true, true),
                (1.5, zoomed, clips[0], vec![owner], None, false, false),
            ] {
                crate::style::set_canvas_dark(dark);
                let d = Arc::new(render::draw(&m, level, view));
                let at = Place::at(clip.min);
                let today = frame(&card, format, ppp, clip, |p, _| canvas::paint(p, at, &d, &selected, hovered, thin, &[]));
                let mut retained = Retained::default();
                let mut drew = false;
                let kept = frame(&card, format, ppp, clip, |p, ctx| {
                    drew = retained.paint(ctx, &pipeline, p, at, &d, &selected, hovered, thin);
                });
                assert!(drew, "the kept triangles draw");
                // And brought there from another tint, written in place.
                let mut moved = Retained::default();
                let other = frame(&card, format, ppp, clip, |p, ctx| {
                    moved.paint(ctx, &pipeline, p, at, &d, &[], Some(second), thin);
                });
                let through = frame(&card, format, ppp, clip, |p, ctx| {
                    moved.paint(ctx, &pipeline, p, at, &d, &selected, hovered, thin);
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
        crate::style::set_canvas_dark(false);
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
        let side = Rect::from_min_size(Pos2::ZERO, vec2(SIDE[0] as f32, SIDE[1] as f32));
        let mut outputs = Vec::new();
        for i in 0..6 {
            let mut raw = RawInput { screen_rect: Some(side), time: Some(f64::from(i) * 10.0), ..RawInput::default() };
            raw.viewports.entry(ViewportId::ROOT).or_default().native_pixels_per_point = Some(1.0);
            outputs.push(ctx.run_ui(raw, |ui| app.show(ui)));
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
            assert_eq!(differs, 0, "{name}: {differs} pixels of the window differ");
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
            assert!(kept.paint(&ctx, &pipeline, &p, at, &d, &[], None, false));
        }
        assert_eq!(kept.built, 1, "frames that change nothing make nothing");
        assert!(kept.paint(&ctx, &pipeline, &p, at, &d, &[], Some(owner), false));
        assert!(kept.paint(&ctx, &pipeline, &p, at, &d, &[owner], Some(owner), false));
        assert!(kept.paint(&ctx, &pipeline, &p, at, &d, &[owner], Some(owner), true));
        assert_eq!((kept.built, kept.retinted), (2, 2), "a tint writes its element where it is; Thin Lines makes the whole again");
        let again = Arc::new(render::draw(&m, level, view));
        assert!(kept.paint(&ctx, &pipeline, &p, at, &again, &[owner], Some(owner), true));
        assert_eq!(kept.built, 3, "another drawing shows");
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
