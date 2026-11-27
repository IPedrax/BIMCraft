//! A scan's points drawn on the card by a compute pass, as the processor's `view3d::raster_points` draws them (Schütz, Kerbl and
//! Wimmer, 2022): each point puts one 64-bit number, its depth over its colour and its owner's slot, in each pixel of its dot with
//! an atomic greatest, and a pass after puts each pixel's into the opaque targets where it is nearer than the surfaces there.
//! Only on a card with 64-bit atomics ([`point_features`]), asked for when the device is made and found once per device; any
//! other card, or a frame it can't draw so (more than 256 owners of points, a buffer bigger than a shader may read), draws them
//! as quads, as it always has. The shaders are `points.wgsl`, after `gpu.wgsl`.

use std::sync::{Arc, Weak};

use bimcraft_core::view3d::gpu::Mesh;
use eframe::egui_wgpu::wgpu;

/// What a card needs to draw points by a compute pass: 64-bit integers, and their atomic least and greatest in a storage buffer.
pub fn point_features() -> wgpu::Features {
    wgpu::Features::SHADER_INT64 | wgpu::Features::SHADER_INT64_ATOMIC_MIN_MAX
}

/// The features a device is asked for to draw any model: `asked`, and [`point_features`] where `adapter` has them. Never more
/// than the adapter has, so the device is made whatever it lacks.
pub fn features_for(adapter: &wgpu::Adapter, asked: wgpu::Features) -> wgpu::Features {
    if trying() && adapter.get_info().backend != wgpu::Backend::Gl && adapter.features().contains(point_features()) {
        asked | point_features()
    } else {
        asked
    }
}

/// Whether the compute pass is tried: only when `BIMCRAFT_COMPUTE_POINTS` is `1`. On an RTX 3090 under Vulkan it drew none of a
/// test's points and the process stopped, so until that's found the cards draw quads, as ever.
pub fn trying() -> bool {
    std::env::var("BIMCRAFT_COMPUTE_POINTS").is_ok_and(|v| v == "1")
}

/// How a device's 3D view draws a scan's points, for the log.
pub fn points_drawn_by(features: wgpu::Features) -> &'static str {
    if features.contains(point_features()) { "a compute pass with 64-bit atomics" } else { "quads" }
}

/// The most owners the points of one picture may have: a slot each in the 8 bits under the colour.
pub(super) const SLOTS: usize = 256;
/// The bytes of the slots' table, as `points.wgsl`'s `Slots`.
const SLOTS_BYTES: u64 = 16 + SLOTS as u64 * 4;
/// The points a workgroup draws.
const GROUP: u32 = 256;

/// One dispatch: the points of a node (its buffer bound, its box and owner) or a part of a mesh's points (their buffer, from
/// `first`, `count` of them).
pub(super) struct Job<'a> {
    pub points: &'a wgpu::BindGroup,
    pub min: [f32; 3],
    pub step: [f32; 3],
    pub owner: u32,
    pub first: u32,
    pub count: u32,
    pub mesh: bool,
}

/// The pipelines and buffers of the compute path.
pub(super) struct Raster {
    pipeline: wgpu::ComputePipeline,
    resolve: wgpu::RenderPipeline,
    keys_layout: wgpu::BindGroupLayout,
    resolve_layout: wgpu::BindGroupLayout,
    pub(super) points_layout: wgpu::BindGroupLayout,
    slots: wgpu::Buffer,
    /// Each pixel's number, for a picture of `size`, bound for the compute pass and the resolve.
    keys: Option<Keys>,
    /// Each dispatch's numbers (`gpu.wgsl`'s `Node`), a slot each `stride` bytes apart, and how many slots it has room for.
    params: Option<(wgpu::Buffer, u64, wgpu::BindGroup)>,
    stride: u64,
    /// The owners of a mesh's points, each once (none where there are more than [`SLOTS`]), found once for each mesh.
    dot_owners: Option<(Weak<Mesh>, Option<Vec<u32>>)>,
    /// The most workgroups across a dispatch.
    across: u32,
}

struct Keys {
    size: (u32, u32),
    buffer: wgpu::Buffer,
    raster: wgpu::BindGroup,
    resolve: wgpu::BindGroup,
}

impl Raster {
    /// The compute path on `device`, if it has [`point_features`] and the limits it needs; none (the quads) otherwise. `group0`
    /// is the view's buffers, `node` a node's numbers bound at a dynamic offset `stride` apart.
    pub(super) fn new(device: &wgpu::Device, group0: &wgpu::BindGroupLayout, node: &wgpu::BindGroupLayout, stride: u64) -> Option<Raster> {
        let limits = device.limits();
        if !device.features().contains(point_features())
            || limits.max_bind_groups < 4
            || limits.max_compute_invocations_per_workgroup < GROUP
            || limits.max_compute_workgroup_size_x < GROUP
            || limits.max_storage_buffers_per_shader_stage < 4
        {
            return None;
        }
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("bimcraft-3d-points"),
            source: wgpu::ShaderSource::Wgsl(concat!(include_str!("../gpu.wgsl"), "\n", include_str!("points.wgsl")).into()),
        });
        let entry = |binding: u32, visibility, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Buffer { ty, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let (compute, fragment) = (wgpu::ShaderStages::COMPUTE, wgpu::ShaderStages::FRAGMENT);
        let uniform = wgpu::BufferBindingType::Uniform;
        let keys_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bimcraft-3d-points-keys"),
            entries: &[entry(5, compute, wgpu::BufferBindingType::Storage { read_only: false }), entry(7, compute, uniform)],
        });
        let resolve_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bimcraft-3d-points-kept"),
            entries: &[entry(8, fragment, wgpu::BufferBindingType::Storage { read_only: true }), entry(7, fragment, uniform)],
        });
        let points_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bimcraft-3d-points"),
            entries: &[entry(6, compute, wgpu::BufferBindingType::Storage { read_only: true })],
        });
        let layout = |label, groups: &[Option<&wgpu::BindGroupLayout>]| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: groups,
                immediate_size: 0,
            })
        };
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("bimcraft-3d-points"),
            layout: Some(&layout("bimcraft-3d-points", &[Some(group0), Some(node), Some(&keys_layout), Some(&points_layout)])),
            module: &module,
            entry_point: Some("cs_points"),
            compilation_options: Default::default(),
            cache: None,
        });
        let target = |format| Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL });
        let resolve = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("bimcraft-3d-points-kept"),
            layout: Some(&layout("bimcraft-3d-points-kept", &[Some(group0), None, Some(&resolve_layout)])),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_full"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: super::DEPTH,
                depth_write_enabled: Some(true),
                // Larger is nearer, as the surfaces' and the quads'.
                depth_compare: Some(wgpu::CompareFunction::Greater),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_points"),
                compilation_options: Default::default(),
                targets: &[target(super::COLOR), target(super::IDS)],
            }),
            multiview_mask: None,
            cache: None,
        });
        let slots = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bimcraft-3d-points-slots"),
            size: SLOTS_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Some(Raster {
            pipeline,
            resolve,
            keys_layout,
            resolve_layout,
            points_layout,
            slots,
            keys: None,
            params: None,
            stride,
            dot_owners: None,
            across: limits.max_compute_workgroups_per_dimension.max(1),
        })
    }

    /// `buffer` bound as the points a dispatch reads; none where a shader may not read so much at once.
    pub(super) fn bind_points(&self, device: &wgpu::Device, buffer: &wgpu::Buffer) -> Option<wgpu::BindGroup> {
        let size = buffer.size();
        if size == 0 || !size.is_multiple_of(4) || size > device.limits().max_storage_buffer_binding_size {
            return None;
        }
        Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bimcraft-3d-points"),
            layout: &self.points_layout,
            entries: &[super::entry(6, buffer)],
        }))
    }

    /// The owners of `mesh`'s points, each once, in their order; none where there are more than [`SLOTS`].
    pub(super) fn dot_owners(&mut self, mesh: &Arc<Mesh>) -> Option<&[u32]> {
        let known = self.dot_owners.as_ref().is_some_and(|(w, _)| std::ptr::eq(w.as_ptr(), Arc::as_ptr(mesh)));
        if !known {
            let mut owners: Vec<u32> = Vec::new();
            let mut many = false;
            for dot in &mesh.dots {
                if owners.last() == Some(&dot.owner) || owners.contains(&dot.owner) {
                    continue;
                }
                if owners.len() == SLOTS {
                    many = true;
                    break;
                }
                owners.push(dot.owner);
            }
            self.dot_owners = Some((Arc::downgrade(mesh), (!many).then_some(owners)));
        }
        self.dot_owners.as_ref().and_then(|(_, o)| o.as_deref())
    }

    /// Whether a picture of `size` can keep its numbers on `device`: a buffer of 8 bytes a pixel a shader can read whole.
    pub(super) fn fits(device: &wgpu::Device, (w, h): (u32, u32)) -> bool {
        let bytes = u64::from(w) * u64::from(h) * 8;
        let limits = device.limits();
        bytes <= limits.max_storage_buffer_binding_size && bytes <= limits.max_buffer_size
    }

    /// Draws `jobs` of points, whose owners are `owners` (no more than [`SLOTS`]), into the opaque targets `color`, `ids` and
    /// `depth` of a picture of `size`, where each is nearer than what they hold; `bind` is the view's buffers.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw(
        &mut self,
        (device, queue): (&wgpu::Device, &wgpu::Queue),
        encoder: &mut wgpu::CommandEncoder,
        bind: &wgpu::BindGroup,
        node: &wgpu::BindGroupLayout,
        (color, ids, depth): (&wgpu::TextureView, &wgpu::TextureView, &wgpu::TextureView),
        size: (u32, u32),
        jobs: &[Job<'_>],
        owners: &[u32],
    ) {
        if jobs.is_empty() || owners.len() > SLOTS {
            return;
        }
        // The keys for this size of picture, made again when it changes.
        if self.keys.as_ref().is_none_or(|k| k.size != size) {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("bimcraft-3d-points-keys"),
                size: (u64::from(size.0) * u64::from(size.1) * 8).max(8),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let group = |layout: &wgpu::BindGroupLayout, at: u32| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("bimcraft-3d-points-keys"),
                    layout,
                    entries: &[super::entry(at, &buffer), super::entry(7, &self.slots)],
                })
            };
            let (raster, resolve) = (group(&self.keys_layout, 5), group(&self.resolve_layout, 8));
            self.keys = Some(Keys { size, buffer, raster, resolve });
        }
        // Each dispatch's numbers, in a slot of its own.
        let slots = jobs.len() as u64;
        if self.params.as_ref().is_none_or(|(_, room, _)| *room < slots) {
            let room = slots.next_power_of_two().max(64);
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("bimcraft-3d-points-params"),
                size: room * self.stride,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("bimcraft-3d-points-params"),
                layout: node,
                entries: &[wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &buffer,
                        offset: 0,
                        size: wgpu::BufferSize::new(super::NODE_PARAMS),
                    }),
                }],
            });
            self.params = Some((buffer, room, group));
        }
        let (Some(keys), Some((params, _, params_bind))) = (&self.keys, &self.params) else { return };
        let stride = self.stride as usize;
        let mut bytes = vec![0u8; jobs.len() * stride];
        for (slot, job) in jobs.iter().enumerate() {
            let mut one = super::Words::with_capacity(super::NODE_PARAMS as usize);
            super::put(&mut one, &[job.min[0], job.min[1], job.min[2], 0.0, job.step[0], job.step[1], job.step[2], 0.0]);
            super::put_words(&mut one, &[job.owner, job.count, job.first, u32::from(job.mesh)]);
            if let Some(at) = bytes.get_mut(slot * stride..slot * stride + one.len()) {
                at.copy_from_slice(&one);
            }
        }
        queue.write_buffer(params, 0, &bytes);
        let mut table = super::Words::with_capacity(SLOTS_BYTES as usize);
        super::put_words(&mut table, &[owners.len() as u32, 0, 0, 0]);
        let mut all = [0u32; SLOTS];
        for (slot, owner) in all.iter_mut().zip(owners) {
            *slot = *owner;
        }
        super::put_words(&mut table, &all);
        queue.write_buffer(&self.slots, 0, &table);
        encoder.clear_buffer(&keys.buffer, 0, None);
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("points"), timestamp_writes: None });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, bind, &[]);
            pass.set_bind_group(2, &keys.raster, &[]);
            for (slot, job) in jobs.iter().enumerate() {
                if job.count == 0 {
                    continue;
                }
                let groups = job.count.div_ceil(GROUP);
                let x = groups.min(self.across);
                let y = groups.div_ceil(x);
                if y > self.across {
                    continue;
                }
                pass.set_bind_group(1, params_bind, &[(slot as u64 * self.stride) as u32]);
                pass.set_bind_group(3, job.points, &[]);
                pass.dispatch_workgroups(x, y, 1);
            }
        }
        let attachment = |view| {
            Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("points kept"),
            color_attachments: &[attachment(color), attachment(ids)],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.resolve);
        pass.set_bind_group(0, bind, &[]);
        pass.set_bind_group(2, &keys.resolve, &[]);
        pass.draw(0..3, 0..1);
    }
}
