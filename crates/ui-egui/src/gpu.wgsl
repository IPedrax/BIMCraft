// The 3D view on the graphics card: the passes that make the picture the processor's rasterizer makes (core `view3d.rs`).
//   1. vs_scene / fs_scene: the opaque surfaces into a colour target (shaded and textured) and an id target (the plane a pixel
//      shows and its element), with their depth; then vs_instanced / fs_scene, each shape's copies, placed. Under a section box,
//      fs_scene_cut, which keeps only what lies in it. Then vs_dot / fs_dot, a scan's points, in the same targets and depth.
//   2. vs_scene and vs_instanced_clear / fs_clear: the surfaces seen through, the nearest of them where it is nearer than anything opaque, into a
//      colour target and an element target of their own (only when the view sees through something).
//   3. vs_full / fs_post: an edge wherever the opaque plane changes between a pixel and the one to its right or below it, else
//      the shaded colour (tinted halfway where selected), else the ground; and over it what is seen through, blended.
//   4. vs_line / fs_line: each level's plane and name as lines, where nothing opaque nearer hides them.
//   In wireframe, passes 1 and 2 have nothing to draw and pass 3 only paper, and vs_wire / fs_wire draw every edge over it; then
//   vs_wire_tinted / fs_wire, those tinted, over the rest.
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
    // xyz: the mesh's origin in the model, which a texture is laid by. w: 1 where paper and ink swap, on a dark canvas.
    origin: vec4<f32>,
    // rgb: the ground the surfaces stand on. w: 1 in hidden line, where surfaces and ground are paper.
    ground: vec4<f32>,
    // xy: the picture's size, px. z: how much nearer a surface may be than a line and still leave it showing, in depth (in
    // perspective, that over the depth a line is past the far end, 1.5 px over the focal length). w: 1 where the view sees
    // through something.
    size: vec4<f32>,
    // In perspective (x above 0), as the core's `Camera` projects: x the lens's focal length, px; y how far the eye is from
    // the aim, mm; and the depth (larger nearer, 1 at the near plane and 0 at the far one) z + w / ahead, for a point ahead mm
    // in front of the eye.
    lens: vec4<f32>,
    // The section box from the mesh's origin: its least corner (w: 1 where there is one) and its greatest.
    cut_lo: vec4<f32>,
    cut_hi: vec4<f32>,
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
// Each copy's triangles' plane numbers, from where its instance says.
@group(0) @binding(5) var<storage, read> planes: array<u32>;

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

// A surface's corner: where it is from the mesh's origin, its normal and its face's, and the rest as a vertex holds them.
fn scene_out(position: vec3<f32>, normal: vec3<f32>, face: vec3<f32>, color: u32, ids: vec2<u32>, tex: u32) -> SceneOut {
    var o: SceneOut;
    o.clip = to_clip(position - g.aim.xyz);
    let n = normalize(normal);
    o.light = vec2<f32>(lit(n), lit(-n));
    o.base = vec3<f32>(f32(color & 255u), f32((color >> 8u) & 255u), f32((color >> 16u) & 255u));
    o.ids = ids;
    o.world = position;
    o.face = normalize(face);
    o.tex = tex;
    return o;
}

@vertex
fn vs_scene(v: SceneIn) -> SceneOut {
    return scene_out(v.position, v.normal.xyz, v.face.xyz, v.color, vec2<u32>(v.plane, v.owner), v.tex);
}

// A copy of a shape: where its shape's origin is from the mesh's origin, where its axes go (square to each other: turned, and a
// run's tube stretched), its element, and where its planes are (less a third of where its shape's vertices start, so that this
// and the vertex's index find its own).
struct InstanceIn {
    @location(7) at: vec3<f32>,
    @location(8) x: vec3<f32>,
    @location(9) y: vec3<f32>,
    @location(10) z: vec3<f32>,
    @location(11) owner: u32,
    @location(12) planes: u32,
    // Its element as the part of it seen through.
    @location(13) clear_owner: u32,
}

fn moved(v: vec3<f32>, c: InstanceIn) -> vec3<f32> {
    return v.x * c.x + v.y * c.y + v.z * c.z;
}

// A normal along each axis by its share over the axis's length squared: square to the surface however it is stretched.
fn normal_of(n: vec3<f32>, c: InstanceIn) -> vec3<f32> {
    let w = vec3<f32>(1.0 / dot(c.x, c.x), 1.0 / dot(c.y, c.y), 1.0 / dot(c.z, c.z));
    return normalize(moved(n * w, c));
}

fn placed(v: SceneIn, c: InstanceIn, k: u32, owner: u32) -> SceneOut {
    let plane = planes[c.planes + k / 3u];
    let position = moved(v.position, c) + c.at;
    return scene_out(position, normal_of(v.normal.xyz, c), normal_of(v.face.xyz, c), v.color, vec2<u32>(plane, owner), v.tex);
}

@vertex
fn vs_instanced(v: SceneIn, c: InstanceIn, @builtin(vertex_index) k: u32) -> SceneOut {
    return placed(v, c, k, c.owner);
}

// What is seen through of a copy, drawn with those seen through (fs_clear).
@vertex
fn vs_instanced_clear(v: SceneIn, c: InstanceIn, @builtin(vertex_index) k: u32) -> SceneOut {
    return placed(v, c, k, c.clear_owner);
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

fn scene_targets(in: SceneOut, front: bool) -> SceneTargets {
    var out: SceneTargets;
    out.color = vec4<f32>(shaded(in, front) / 255.0, 1.0);
    out.ids = in.ids;
    return out;
}

@fragment
fn fs_scene(in: SceneOut, @builtin(front_facing) front: bool) -> SceneTargets {
    return scene_targets(in, front);
}

// Whether a surface's point lies outside the section box, as `Boxed::holds` asks it.
fn cut_away(world: vec3<f32>) -> bool {
    return any(world < g.cut_lo.xyz) || any(world > g.cut_hi.xyz);
}

@fragment
fn fs_scene_cut(in: SceneOut, @builtin(front_facing) front: bool) -> SceneTargets {
    if cut_away(in.world) {
        discard;
    }
    return scene_targets(in, front);
}

struct ClearTargets {
    @location(0) color: vec4<f32>,
    @location(1) owner: u32,
}

// Only nearer than what is opaque there: as near is behind it. Two surfaces in one plane, a wall's face and a copy's back flush
// against it, come out apart here where the processor's depths are equal, as a card snaps each triangle's corners to 1/256 of a pixel:
// so as near is within a sixty-fourth of how far the depth runs across a pixel, and a few of f32's steps.
@fragment
fn fs_clear(in: SceneOut, @builtin(front_facing) front: bool) -> ClearTargets {
    let tie = max(fwidth(in.clip.z) / 64.0, 2.5e-7);
    if in.clip.z <= textureLoad(scene_depth, vec2<i32>(in.clip.xy), 0) + tie || (g.cut_lo.w > 0.5 && cut_away(in.world)) {
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

// A colour on a dark canvas, where paper and ink swap, as `canvas::color` has it.
fn inked(c: vec3<f32>) -> vec3<f32> {
    if g.origin.w < 0.5 {
        return c;
    }
    let shift = 255.0 - max(c.x, max(c.y, c.z)) - min(c.x, min(c.y, c.z));
    return floor(vec3<f32>(26.0) + floor(clamp(c + vec3<f32>(shift), vec3<f32>(0.0), vec3<f32>(255.0)) * 206.0 / 255.0));
}

@fragment
fn fs_post(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let x = i32(frag.x);
    let y = i32(frag.y);
    let size = vec2<i32>(g.size.xy);
    let here = ids_at(x, y);
    let right = ids_at(x + 1, y);
    let below = ids_at(x, y + 1);
    // A surface's edge is where it meets another surface or nothing; a point is not one.
    let on_dot = here.x == DOT_PLANE;
    let edge = !on_dot && ((x + 1 < size.x && right.x != DOT_PLANE && right.x != here.x) || (y + 1 < size.y && below.x != DOT_PLANE && below.x != here.x));
    let lines = g.ground.w > 0.5;
    var rgb = select(g.ground.xyz, vec3<f32>(255.0), lines);
    if edge {
        // In the tint where the pixel's own element is tinted, else in the edge colour of the first element of it, the pixel
        // to its right and the one below it that shows one.
        var color = vec3<f32>(48.0);
        if here.y != 0u && owners[here.y - 1u].tint.w > 0.5 {
            color = owners[here.y - 1u].tint.xyz;
        } else if here.y != 0u {
            color = owners[here.y - 1u].edge.xyz;
        } else if right.y != 0u && right.x != DOT_PLANE {
            color = owners[right.y - 1u].edge.xyz;
        } else if below.y != 0u && below.x != DOT_PLANE {
            color = owners[below.y - 1u].edge.xyz;
        }
        rgb = color;
    } else if here.y != 0u {
        if !lines || on_dot {
            rgb = bytes(textureLoad(scene_color, vec2<i32>(x, y), 0));
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
    return vec4<f32>(inked(rgb) / 255.0, 1.0);
}

// The plane a pixel showing a scan's point holds: past any surface's.
const DOT_PLANE: u32 = 0xffffffffu;

struct DotIn {
    @location(0) at: vec3<f32>,
    @location(1) color: u32,
    @location(2) owner: u32,
}

struct DotOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) @interpolate(flat) color: vec3<f32>,
    @location(1) @interpolate(flat) ids: vec2<u32>,
}

// A point as the processor's `dots` draws it: two pixels each way from the one nearest, all at the point's own depth, kept whole
// or not at all by a section box. One with nothing to draw is a point of nothing.
@vertex
fn vs_dot(v: DotIn, @builtin(vertex_index) i: u32) -> DotOut {
    var o: DotOut;
    o.clip = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    if g.cut_lo.w > 0.5 && (any(v.at < g.cut_lo.xyz) || any(v.at > g.cut_hi.xyz)) {
        return o;
    }
    if g.lens.x > 0.0 && g.lens.y - dot(v.at - g.aim.xyz, g.back.xyz) <= 0.0 {
        return o;
    }
    let seen = to_pixel(v.at);
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(2.0, 0.0), vec2<f32>(0.0, 2.0),
        vec2<f32>(0.0, 2.0), vec2<f32>(2.0, 0.0), vec2<f32>(2.0, 2.0),
    );
    let xy = floor(seen.xy - vec2<f32>(0.5)) + corners[i];
    // A point nearer than the near plane is drawn all the same, as near as depth goes.
    o.clip = vec4<f32>(xy.x / g.size.x * 2.0 - 1.0, 1.0 - xy.y / g.size.y * 2.0, min(seen.z, 1.0), 1.0);
    o.color = vec3<f32>(f32(v.color & 255u), f32((v.color >> 8u) & 255u), f32((v.color >> 16u) & 255u));
    o.ids = vec2<u32>(DOT_PLANE, v.owner);
    return o;
}

@fragment
fn fs_dot(in: DotOut) -> SceneTargets {
    var out: SceneTargets;
    out.color = vec4<f32>(in.color / 255.0, 1.0);
    out.ids = in.ids;
    return out;
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
    return vec4<f32>(inked(in.color) / 255.0, 1.0);
}

struct WireIn {
    @location(0) a: vec3<f32>,
    @location(1) b: vec3<f32>,
    @location(2) owner: u32,
}

struct WireOut {
    @builtin(position) clip: vec4<f32>,
    // The line's ends in the picture, as the processor's `wire_line` has them.
    @location(0) @interpolate(flat) p: vec2<f32>,
    @location(1) @interpolate(flat) q: vec2<f32>,
    @location(2) @interpolate(flat) color: vec3<f32>,
}

// The core's `NEAR`: how far in front of the eye a line must be, mm.
const NEAR: f32 = 10.0;

// A clipping range (Liang and Barsky's) less what lies past one side, as the processor's `within` and `clip` have it. Empty is
// a range that ends before it starts.
fn narrowed(t: vec2<f32>, p: f32, q: f32) -> vec2<f32> {
    if p == 0.0 {
        return select(t, vec2<f32>(1.0, 0.0), q < 0.0);
    }
    if p < 0.0 {
        return vec2<f32>(max(t.x, q / p), t.y);
    }
    return vec2<f32>(t.x, min(t.y, q / p));
}

// Two triangles a little wider and longer than the line, each pixel of which `fs_wire` asks of the line's steps: every pixel a
// step lands in has its centre within a pixel of the line. One with nothing to draw is a point.
fn wire_quad(w: WireIn, i: u32) -> WireOut {
    var o: WireOut;
    o.clip = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    var a = w.a;
    var b = w.b;
    if g.cut_lo.w > 0.5 {
        let d = b - a;
        var t = vec2<f32>(0.0, 1.0);
        for (var k = 0; k < 3; k += 1) {
            t = narrowed(t, -d[k], a[k] - g.cut_lo[k]);
            t = narrowed(t, d[k], g.cut_hi[k] - a[k]);
        }
        if t.x > t.y {
            return o;
        }
        let start = a;
        a = start + d * t.x;
        b = start + d * t.y;
    }
    if g.lens.x > 0.0 {
        let da = g.lens.y - dot(a - g.aim.xyz, g.back.xyz) - NEAR;
        let db = g.lens.y - dot(b - g.aim.xyz, g.back.xyz) - NEAR;
        if da < 0.0 && db < 0.0 {
            return o;
        }
        if da < 0.0 || db < 0.0 {
            let cut = a + (b - a) * (da / (da - db));
            if da >= 0.0 {
                b = cut;
            } else {
                a = cut;
            }
        }
    }
    let pa = to_pixel(a).xy;
    let d = to_pixel(b).xy - pa;
    var t = vec2<f32>(0.0, 1.0);
    t = narrowed(t, -d.x, pa.x);
    t = narrowed(t, d.x, g.size.x - pa.x);
    t = narrowed(t, -d.y, pa.y);
    t = narrowed(t, d.y, g.size.y - pa.y);
    if t.x > t.y {
        return o;
    }
    let p = pa + d * t.x;
    let q = pa + d * t.y;
    var along = vec2<f32>(1.0, 0.0);
    if length(q - p) > 0.0001 {
        along = normalize(q - p);
    }
    var corners = array<u32, 6>(0u, 1u, 2u, 2u, 1u, 3u);
    let c = corners[i];
    let at_q = (c & 2u) != 0u;
    let side = select(-1.0, 1.0, (c & 1u) == 0u);
    let xy = select(p, q, at_q) + select(-along, along, at_q) + vec2<f32>(-along.y, along.x) * side;
    o.clip = vec4<f32>(xy.x / g.size.x * 2.0 - 1.0, 1.0 - xy.y / g.size.y * 2.0, 0.5, 1.0);
    o.p = p;
    o.q = q;
    let owner = owners[w.owner - 1u];
    o.color = select(owner.edge.xyz, owner.tint.xyz, owner.tint.w > 0.5);
    return o;
}

@vertex
fn vs_wire(w: WireIn, @builtin(vertex_index) i: u32) -> WireOut {
    return wire_quad(w, i);
}

// Only what is tinted, which is drawn over the rest. A point for the others.
@vertex
fn vs_wire_tinted(w: WireIn, @builtin(vertex_index) i: u32) -> WireOut {
    if owners[w.owner - 1u].tint.w < 0.5 {
        var o: WireOut;
        o.clip = vec4<f32>(0.0, 0.0, 0.0, 1.0);
        return o;
    }
    return wire_quad(w, i);
}

// A pixel the line's steps land in, as the processor's `wire_pixels` takes them: a step no longer than a pixel, each point
// floored. Only the few steps along its longer way that can land in this pixel are asked.
@fragment
fn fs_wire(in: WireOut) -> @location(0) vec4<f32> {
    let at = floor(in.clip.xy);
    let d = in.q - in.p;
    let steps = max(ceil(length(d)), 1.0);
    let way = select(0, 1, abs(d.y) > abs(d.x));
    var first = 0.0;
    var last = steps;
    if abs(d[way]) > 0.000001 {
        let i = (at[way] - in.p[way]) / d[way] * steps;
        let j = (at[way] + 1.0 - in.p[way]) / d[way] * steps;
        first = max(floor(min(i, j)), 0.0);
        last = min(ceil(max(i, j)), steps);
    }
    for (var k = first; k <= last; k += 1.0) {
        if all(floor(in.p + d * (k / steps)) == at) {
            return vec4<f32>(inked(in.color) / 255.0, 1.0);
        }
    }
    discard;
}
