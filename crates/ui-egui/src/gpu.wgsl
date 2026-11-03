// The 3D view on the graphics card: the passes that make the picture the processor's rasterizer makes (core `view3d.rs`).
//   1. vs_scene / fs_scene: the opaque surfaces into a colour target (shaded and textured) and an id target (the plane a pixel
//      shows and its element), with their depth.
//   2. vs_scene / fs_clear: the surfaces seen through, the nearest of them where it is nearer than anything opaque, into a
//      colour target and an element target of their own (only when the view sees through something).
//   3. vs_full / fs_post: an edge wherever the opaque plane changes between a pixel and the one to its right or below it (where
//      the view shows its edges) and a silhouette as wide as the view says, else the shaded colour (in ambient shadow where the
//      view shows them, read off the opaque depth round the pixel; tinted halfway where selected), else the ground; and over it
//      what is seen through, blended.
//   4. vs_line / fs_line: each level's plane and name as lines, where nothing opaque nearer hides them.
// Lengths are mm. A colour is 0 to 255 in each channel, as the processor keeps it, and is divided down only on its way out.

struct Globals {
    // xyz: the picture's right, up and toward the camera (unit). w: how far a mm goes in clip space across and up (at a mm in
    // front of the eye, in perspective), and how far in depth (0.5 over the depth the mesh reaches, which is half the clip
    // volume, so depths are 0 to 1).
    right: vec4<f32>,
    up: vec4<f32>,
    back: vec4<f32>,
    // xyz: the model point in the middle of the picture, from the mesh's origin. w: pixels a mm.
    aim: vec4<f32>,
    // xyz: the mesh's origin in the model, which a texture is laid by.
    origin: vec4<f32>,
    // rgb: the ground the surfaces stand on.
    ground: vec4<f32>,
    // xy: the picture's size, px. z: how much nearer a surface may be than a line and still leave it showing, in depth (in
    // perspective, that over the depth a line is past the far end, 1.5 px over the focal length). w: 1 where the view sees
    // through something.
    size: vec4<f32>,
    // In perspective (x above 0), as the core's `Camera` projects: x the lens's focal length, px; y how far the eye is from
    // the aim, mm; and the depth (larger nearer, 1 at the near plane and 0 at the far one) z + w / ahead, for a point ahead mm
    // in front of the eye.
    lens: vec4<f32>,
    // The view's Graphic Display Options: x 1 where it shows its edges, y a silhouette's width (px, 0 for none), z 1 where it
    // shows ambient shadows.
    display: vec4<f32>,
}

// A point `d` from the aim in clip space: parallel, or seen from the eye.
fn to_clip(d: vec3<f32>) -> vec4<f32> {
    let x = dot(d, g.right.xyz);
    let y = dot(d, g.up.xyz);
    let z = dot(d, g.back.xyz);
    if g.lens.x > 0.0 {
        let ahead = g.lens.y - z;
        return vec4<f32>(x * g.right.w, y * g.up.w, g.lens.z * ahead + g.lens.w, ahead);
    }
    return vec4<f32>(x * g.right.w, y * g.up.w, 0.5 + z * g.back.w, 1.0);
}

struct Owner {
    // rgb: the edge colour; w: how much of the element shows over what is behind it, 1 for opaque.
    edge: vec4<f32>,
    // rgb: the tint; w: 1 where the element is tinted.
    tint: vec4<f32>,
}

struct Skin {
    size: vec2<f32>,
    turn: vec2<f32>,
    offset: vec2<f32>,
    spare: vec2<f32>,
    // The level looked up: where it starts in `texels`, its width and height (0 for no texture).
    level: vec4<u32>,
}

struct Seg {
    // xyz: an end; w: how many px wide.
    a: vec4<f32>,
    b: vec4<f32>,
    color: vec4<f32>,
}

@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var<storage, read> owners: array<Owner>;
@group(0) @binding(2) var<storage, read> segs: array<Seg>;
@group(0) @binding(3) var<storage, read> skins: array<Skin>;
@group(0) @binding(4) var<storage, read> texels: array<u32>;

@group(1) @binding(0) var scene_color: texture_2d<f32>;
@group(1) @binding(1) var scene_ids: texture_2d<u32>;
@group(1) @binding(2) var scene_depth: texture_depth_2d;

@group(2) @binding(0) var clear_color: texture_2d<f32>;
@group(2) @binding(1) var clear_ids: texture_2d<u32>;

// Lit from over the viewer's left shoulder; a face turned away from the light keeps 62%.
fn lit(n: vec3<f32>) -> f32 {
    let v = vec3<f32>(dot(n, g.right.xyz), dot(n, g.up.xyz), dot(n, g.back.xyz));
    return 0.62 + 0.38 * max(dot(v, normalize(vec3<f32>(-0.35, 0.55, 0.75))), 0.0);
}

fn texel(s: Skin, x: i32, y: i32) -> vec3<f32> {
    let w = i32(s.level.y);
    let h = i32(s.level.z);
    let t = texels[s.level.x + u32(((y % h) + h) % h * w + ((x % w) + w) % w)];
    return vec3<f32>(f32(t & 255u), f32((t >> 8u) & 255u), f32((t >> 16u) & 255u));
}

// What `Sampler::at` gives: the texture laid over the face's plane at the point `world` (from the mesh's origin), across
// the plane's level direction and up it, wrapping and blended between the four pixels round it.
fn skin_color(id: u32, world: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    let s = skins[id];
    if s.level.y == 0u || s.level.z == 0u {
        return vec3<f32>(128.0);
    }
    let level = vec3<f32>(-n.y, n.x, 0.0);
    var across = vec3<f32>(1.0, 0.0, 0.0);
    if dot(level, level) >= 0.0001 {
        across = normalize(level);
    }
    let up = normalize(cross(n, across));
    let p = world + g.origin.xyz;
    let ox = dot(p, across) - s.offset.x;
    let oy = dot(p, up) - s.offset.y;
    let x = ox * s.turn.y + oy * s.turn.x;
    let y = oy * s.turn.y - ox * s.turn.x;
    let fu = x / s.size.x;
    let fv = -y / s.size.y;
    let px = (fu - floor(fu)) * f32(s.level.y) - 0.5;
    let py = (fv - floor(fv)) * f32(s.level.z) - 0.5;
    let x0 = floor(px);
    let y0 = floor(py);
    let tx = px - x0;
    let ty = py - y0;
    let ix = i32(x0);
    let iy = i32(y0);
    let a = texel(s, ix, iy);
    let b = texel(s, ix + 1, iy);
    let c = texel(s, ix, iy + 1);
    let d = texel(s, ix + 1, iy + 1);
    let mixed = (a * (1.0 - tx) + b * tx) * (1.0 - ty) + (c * (1.0 - tx) + d * tx) * ty;
    return floor(clamp(mixed, vec3<f32>(0.0), vec3<f32>(255.0)));
}

// The normals come as 16-bit signed fractions.
struct SceneIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec4<f32>,
    @location(2) face: vec4<f32>,
    @location(3) color: u32,
    @location(4) owner: u32,
    @location(5) plane: u32,
    @location(6) tex: u32,
}

struct SceneOut {
    @builtin(position) clip: vec4<f32>,
    // How lit the corner is as the face shows its front, and as it shows its back.
    @location(0) light: vec2<f32>,
    @location(1) @interpolate(flat) base: vec3<f32>,
    @location(2) @interpolate(flat) ids: vec2<u32>,
    @location(3) world: vec3<f32>,
    @location(4) @interpolate(flat) face: vec3<f32>,
    @location(5) @interpolate(flat) tex: u32,
}

@vertex
fn vs_scene(v: SceneIn) -> SceneOut {
    var o: SceneOut;
    o.clip = to_clip(v.position - g.aim.xyz);
    let n = normalize(v.normal.xyz);
    o.light = vec2<f32>(lit(n), lit(-n));
    o.base = vec3<f32>(f32(v.color & 255u), f32((v.color >> 8u) & 255u), f32((v.color >> 16u) & 255u));
    o.ids = vec2<u32>(v.plane, v.owner);
    o.world = v.position;
    o.face = normalize(v.face.xyz);
    o.tex = v.tex;
    return o;
}

struct SceneTargets {
    @location(0) color: vec4<f32>,
    @location(1) ids: vec2<u32>,
}

// A surface's colour, lit (0 to 255), untinted: the tint is the picture's (fs_post), so a selection changes no surface.
fn shaded(in: SceneOut, front: bool) -> vec3<f32> {
    var c = in.base;
    if in.tex != 0u {
        c = skin_color(in.tex, in.world, in.face);
    }
    return floor(c * select(in.light.y, in.light.x, front) + vec3<f32>(0.5));
}

@fragment
fn fs_scene(in: SceneOut, @builtin(front_facing) front: bool) -> SceneTargets {
    var out: SceneTargets;
    out.color = vec4<f32>(shaded(in, front) / 255.0, 1.0);
    out.ids = in.ids;
    return out;
}

struct ClearTargets {
    @location(0) color: vec4<f32>,
    @location(1) owner: u32,
}

// Only nearer than what is opaque there: as near is behind it.
@fragment
fn fs_clear(in: SceneOut, @builtin(front_facing) front: bool) -> ClearTargets {
    if in.clip.z <= textureLoad(scene_depth, vec2<i32>(in.clip.xy), 0) {
        discard;
    }
    var out: ClearTargets;
    out.color = vec4<f32>(shaded(in, front) / 255.0, 1.0);
    out.owner = in.ids.y;
    return out;
}

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}

fn ids_at(x: i32, y: i32) -> vec2<u32> {
    let size = vec2<i32>(g.size.xy);
    if x < 0 || y < 0 || x >= size.x || y >= size.y {
        return vec2<u32>(0u, 0u);
    }
    return textureLoad(scene_ids, vec2<i32>(x, y), 0).xy;
}

// A colour target's pixel as 0 to 255.
fn bytes(c: vec4<f32>) -> vec3<f32> {
    return floor(c.xyz * 255.0 + vec3<f32>(0.5));
}

// The opaque depth at a pixel: 0 where nothing is, and outside the picture.
fn depth_at(x: i32, y: i32) -> f32 {
    let size = vec2<i32>(g.size.xy);
    if x < 0 || y < 0 || x >= size.x || y >= size.y {
        return 0.0;
    }
    return textureLoad(scene_depth, vec2<i32>(x, y), 0);
}

// A pixel's worth of depth where the depth is `d`, as the core's `Camera::slack` gives it.
fn worth(d: f32) -> f32 {
    if g.lens.x > 0.0 {
        return (d - g.lens.z) / g.lens.x;
    }
    return g.back.w / g.aim.w;
}

// How lit the surface at a pixel of depth `z` is under ambient shadows, as the core's `ambient` works it out: as dark as the
// surfaces round it rise toward the eye on both sides of it.
fn ambient(x: i32, y: i32, z: f32) -> f32 {
    var offsets = array<vec2<i32>, 8>(
        vec2<i32>(12, 0), vec2<i32>(0, 12), vec2<i32>(8, 8), vec2<i32>(8, -8),
        vec2<i32>(6, 0), vec2<i32>(0, 6), vec2<i32>(4, 4), vec2<i32>(4, -4),
    );
    let px = worth(z);
    var sum = 0.0;
    var n = 0.0;
    for (var i = 0; i < 8; i++) {
        let o = offsets[i];
        let a = depth_at(x + o.x, y + o.y);
        let b = depth_at(x - o.x, y - o.y);
        let unit = px * length(vec2<f32>(o));
        if a <= 0.0 || b <= 0.0 || !(unit > 0.0) || abs(a - z) > 4.0 * unit || abs(b - z) > 4.0 * unit {
            continue;
        }
        sum += ((a + b) * 0.5 - z) / unit;
        n += 1.0;
    }
    if n == 0.0 {
        return 1.0;
    }
    return 1.0 - 0.6 * clamp(sum / n, 0.0, 1.0);
}

// Whether two neighbouring pixels, each its depth and plane, have a silhouette between them: one on a surface and one on
// nothing, or on two surfaces four pixels' worth apart in depth.
fn gap(d0: f32, p0: u32, d1: f32, p1: u32) -> bool {
    if (d0 > 0.0) != (d1 > 0.0) {
        return true;
    }
    return d0 > 0.0 && p0 != p1 && abs(d0 - d1) > 4.0 * worth(max(d0, d1));
}

// Whether a pixel is in a silhouette as wide as the view says: whether the square one wider than it, from half its width before
// the pixel, holds two neighbours with a silhouette between them, as the core's `outlines` finds.
fn outlined(x: i32, y: i32) -> bool {
    let width = min(i32(g.display.y), 10);
    if width <= 0 {
        return false;
    }
    let size = vec2<i32>(g.size.xy);
    let corner = vec2<i32>(x - width / 2, y - width / 2);
    var above_d: array<f32, 11>;
    var above_p: array<u32, 11>;
    var above_in: array<bool, 11>;
    for (var j = 0; j <= width; j++) {
        var left_d = 0.0;
        var left_p = 0u;
        var left_in = false;
        for (var i = 0; i <= width; i++) {
            let q = corner + vec2<i32>(i, j);
            let inside = q.x >= 0 && q.y >= 0 && q.x < size.x && q.y < size.y;
            var d = 0.0;
            var p = 0u;
            if inside {
                d = textureLoad(scene_depth, q, 0);
                p = textureLoad(scene_ids, q, 0).x;
            }
            if inside && left_in && gap(left_d, left_p, d, p) {
                return true;
            }
            if inside && above_in[i] && gap(above_d[i], above_p[i], d, p) {
                return true;
            }
            above_d[i] = d;
            above_p[i] = p;
            above_in[i] = inside;
            left_d = d;
            left_p = p;
            left_in = inside;
        }
    }
    return false;
}

@fragment
fn fs_post(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let x = i32(frag.x);
    let y = i32(frag.y);
    let size = vec2<i32>(g.size.xy);
    let here = ids_at(x, y);
    let right = ids_at(x + 1, y);
    let below = ids_at(x, y + 1);
    let differs = (x + 1 < size.x && right.x != here.x) || (y + 1 < size.y && below.x != here.x);
    let edge = (g.display.x > 0.5 && differs) || outlined(x, y);
    var rgb = g.ground.xyz;
    if edge {
        // In the tint where the pixel's own element is tinted, else in the edge colour of the first element of it, the pixel
        // to its right and the one below it that shows one.
        var color = vec3<f32>(48.0);
        if here.y != 0u && owners[here.y - 1u].tint.w > 0.5 {
            color = owners[here.y - 1u].tint.xyz;
        } else if here.y != 0u {
            color = owners[here.y - 1u].edge.xyz;
        } else if right.y != 0u {
            color = owners[right.y - 1u].edge.xyz;
        } else if below.y != 0u {
            color = owners[below.y - 1u].edge.xyz;
        }
        rgb = color;
    } else if here.y != 0u {
        rgb = bytes(textureLoad(scene_color, vec2<i32>(x, y), 0));
        if g.display.z > 0.5 {
            rgb = floor(rgb * ambient(x, y, textureLoad(scene_depth, vec2<i32>(x, y), 0)) + vec3<f32>(0.5));
        }
        if owners[here.y - 1u].tint.w > 0.5 {
            rgb = floor((rgb + owners[here.y - 1u].tint.xyz) / 2.0);
        }
    }
    // Over it, the nearest surface seen through, as much of it as the view lets show (and a tinted one always a little, or
    // selecting it would show nothing).
    if g.size.w > 0.5 {
        let clear = textureLoad(clear_ids, vec2<i32>(x, y), 0).x;
        if clear != 0u {
            let owner = owners[clear - 1u];
            var s = bytes(textureLoad(clear_color, vec2<i32>(x, y), 0));
            var weight = owner.edge.w;
            if owner.tint.w > 0.5 {
                s = floor((s + owner.tint.xyz) / 2.0);
                weight = max(weight, 0.35);
            }
            rgb = floor(rgb + (s - rgb) * weight + vec3<f32>(0.5));
        }
    }
    return vec4<f32>(rgb / 255.0, 1.0);
}

struct LineOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) @interpolate(flat) color: vec3<f32>,
    @location(1) depth: f32,
}

// Where a model point is in the picture: its pixel, and its depth (0 to 1; larger is nearer).
fn to_pixel(p: vec3<f32>) -> vec3<f32> {
    let d = p - g.aim.xyz;
    if g.lens.x > 0.0 {
        let ahead = g.lens.y - dot(d, g.back.xyz);
        let k = g.lens.x / ahead;
        return vec3<f32>(
            g.size.x * 0.5 + dot(d, g.right.xyz) * k,
            g.size.y * 0.5 - dot(d, g.up.xyz) * k,
            g.lens.z + g.lens.w / ahead,
        );
    }
    return vec3<f32>(
        g.size.x * 0.5 + dot(d, g.right.xyz) * g.aim.w,
        g.size.y * 0.5 - dot(d, g.up.xyz) * g.aim.w,
        0.5 + dot(d, g.back.xyz) * g.back.w,
    );
}

// Each segment is a quad of two triangles, as wide as it says and half a pixel longer at each end.
@vertex
fn vs_line(@builtin(vertex_index) i: u32, @builtin(instance_index) k: u32) -> LineOut {
    let s = segs[k];
    let a = to_pixel(s.a.xyz);
    let b = to_pixel(s.b.xyz);
    let d = b.xy - a.xy;
    var along = vec2<f32>(1.0, 0.0);
    if length(d) > 0.0001 {
        along = normalize(d);
    }
    var corners = array<u32, 6>(0u, 1u, 2u, 2u, 1u, 3u);
    let c = corners[i];
    let at_b = (c & 2u) != 0u;
    let end = select(a, b, at_b);
    let side = select(-1.0, 1.0, (c & 1u) == 0u);
    let xy = end.xy + select(-along, along, at_b) * 0.5 + vec2<f32>(-along.y, along.x) * (side * s.a.w * 0.5);
    var o: LineOut;
    o.clip = vec4<f32>(xy.x / g.size.x * 2.0 - 1.0, 1.0 - xy.y / g.size.y * 2.0, 0.5, 1.0);
    o.color = s.color.xyz;
    o.depth = end.z;
    return o;
}

@fragment
fn fs_line(in: LineOut) -> @location(0) vec4<f32> {
    let behind = textureLoad(scene_depth, vec2<i32>(in.clip.xy), 0);
    var slack = g.size.z;
    if g.lens.x > 0.0 {
        slack = g.size.z * (in.depth - g.lens.z);
    }
    if behind > 0.0 && behind > in.depth + slack {
        discard;
    }
    return vec4<f32>(in.color / 255.0, 1.0);
}
