// A scan's points drawn by a compute pass, as the processor's `view3d::raster_points` draws them: compiled after gpu.wgsl (whose
// view, `to_pixel`, section box and `node` it reads) only on a card with 64-bit atomics (`gpu::POINT_FEATURES`); a card without
// them draws its points as quads (vs_dot, vs_node).
//   cs_points: each point (a mesh's `Dot`s, or a node's 12-byte records) puts one 64-bit number in each pixel of its 2x2 dot, and
//      the greatest stays: its depth above (larger nearer, as the depth target has it), and below, inverted so that the least
//      wins a tie as on the processor, its colour (red, green, blue) and its owner's slot in `slots`.
//   vs_full / fs_points: each pixel's number into the opaque targets, its depth written and tested, so that a point shows only
//      where it is nearer than the surfaces drawn there, its id the point's plane and its owner, which picking and the edges
//      read as they read a quad's.

// The owners of the points drawn, as the mesh numbers them, by their slot: x of `count` how many there are, then four to a vec4.
struct Slots {
    count: vec4<u32>,
    owners: array<vec4<u32>, 64>,
}

@group(2) @binding(5) var<storage, read_write> keys: array<atomic<u64>>;
@group(2) @binding(7) var<uniform> slots: Slots;
// The same numbers as two words each, low first, read where the atomics are not: a 64-bit load is more than min and max.
@group(2) @binding(8) var<storage, read> kept: array<vec2<u32>>;
// The points of a draw: five words each of a mesh's (`gpu::DOT`), three of a node's (`gpu::NODE`).
@group(3) @binding(6) var<storage, read> points: array<u32>;

fn slot_of(owner: u32) -> u32 {
    let n = min(slots.count.x, 256u);
    for (var s = 0u; s < n; s++) {
        if slots.owners[s / 4u][s % 4u] == owner {
            return s;
        }
    }
    return 255u;
}

// The draw is `node`: for a node's points its box and its owner (x), and how many (y); for a mesh's, y how many from the z-th,
// w 1.
@compute @workgroup_size(256)
fn cs_points(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let i = id.x + id.y * groups.x * 256u;
    if i >= node.owner.y {
        return;
    }
    var at: vec3<f32>;
    var color: u32;
    var owner: u32;
    if node.owner.w == 1u {
        let k = (node.owner.z + i) * 5u;
        at = vec3<f32>(bitcast<f32>(points[k]), bitcast<f32>(points[k + 1u]), bitcast<f32>(points[k + 2u]));
        color = points[k + 3u];
        owner = points[k + 4u];
    } else {
        let k = i * 3u;
        let xy = points[k];
        let q = vec3<f32>(f32(xy & 0xffffu), f32(xy >> 16u), f32(points[k + 1u]));
        at = node.min.xyz + q * node.step.xyz;
        color = points[k + 2u];
        owner = node.owner.x;
    }
    // Kept whole or not at all by a section box, and none behind the eye, as vs_dot.
    if g.cut_lo.w > 0.5 && (any(at < g.cut_lo.xyz) || any(at > g.cut_hi.xyz)) {
        return;
    }
    if g.lens.x > 0.0 && g.lens.y - dot(at - g.aim.xyz, g.back.xyz) <= 0.0 {
        return;
    }
    let seen = to_pixel(at);
    // A point nearer than the near plane as near as depth goes; one past the far plane (or not a number) is not drawn.
    let depth = min(seen.z, 1.0);
    let x0 = floor(seen.x - 0.5);
    let y0 = floor(seen.y - 0.5);
    if !(depth >= 0.0) || !(x0 > -2.0 && y0 > -2.0 && x0 < g.size.x && y0 < g.size.y) {
        return;
    }
    let rgb = ((color & 255u) << 16u) | (((color >> 8u) & 255u) << 8u) | ((color >> 16u) & 255u);
    let key = (u64(bitcast<u32>(depth)) << 32u) | u64(~((rgb << 8u) | slot_of(owner)));
    let w = i32(g.size.x);
    let h = i32(g.size.y);
    for (var dy = 0; dy < 2; dy++) {
        for (var dx = 0; dx < 2; dx++) {
            let x = i32(x0) + dx;
            let y = i32(y0) + dy;
            if x >= 0 && y >= 0 && x < w && y < h {
                atomicMax(&keys[u32(y) * u32(w) + u32(x)], key);
            }
        }
    }
}

struct Resolved {
    @builtin(frag_depth) depth: f32,
    @location(0) color: vec4<f32>,
    @location(1) ids: vec2<u32>,
}

// A pixel no point reached is as far as depth goes, which the depth test (greater) lets through nowhere.
@fragment
fn fs_points(@builtin(position) frag: vec4<f32>) -> Resolved {
    let p = vec2<u32>(frag.xy);
    let k = kept[p.y * u32(g.size.x) + p.x];
    var out: Resolved;
    out.depth = 0.0;
    if k.x != 0u || k.y != 0u {
        let low = ~k.x;
        let rgb = low >> 8u;
        let slot = low & 255u;
        out.depth = bitcast<f32>(k.y);
        out.color = vec4<f32>(f32(rgb >> 16u), f32((rgb >> 8u) & 255u), f32(rgb & 255u), 255.0) / 255.0;
        out.ids = vec2<u32>(DOT_PLANE, slots.owners[slot / 4u][slot % 4u]);
    }
    return out;
}
