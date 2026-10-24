// The 3D view on the graphics card: the passes that make the picture the processor's rasterizer makes (core `view3d.rs`).
//   1. vs_scene / fs_scene: the opaque surfaces into a colour target (shaded and textured) and an id target (the plane a pixel
//      shows and its element), with their depth.
//   2. vs_scene / fs_clear: the surfaces seen through, the nearest of them where it is nearer than anything opaque, into a
//      colour target and an element target of their own (only when the view sees through something).
//   3. vs_full / fs_post: an edge wherever the opaque plane changes between a pixel and the one to its right or below it, else
//      the shaded colour (tinted halfway where selected), else the ground; and over it what is seen through, blended.
//   4. vs_line / fs_line: each level's plane and name as lines, where nothing opaque nearer hides them.
// Lengths are mm. A colour is 0 to 255 in each channel, as the processor keeps it, and is divided down only on its way out.

struct Globals {
    // xyz: the picture's right, up and toward the camera (unit). w: how far a mm goes in clip space across and up, and how
    // far in depth (0.5 over the depth the mesh reaches, which is half the clip volume, so depths are 0 to 1).
    right: vec4<f32>,
    up: vec4<f32>,
    back: vec4<f32>,
    // xyz: the model point in the middle of the picture, from the mesh's origin. w: pixels a mm.
    aim: vec4<f32>,
    // xyz: the mesh's origin in the model, which a texture is laid by.
    origin: vec4<f32>,
    // rgb: the ground the surfaces stand on.
    ground: vec4<f32>,
    // xy: the picture's size, px. z: how much nearer a surface may be than a line and still leave it showing, in depth.
    // w: 1 where the view sees through something.
    size: vec4<f32>,
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
    let d = v.position - g.aim.xyz;
    var o: SceneOut;
    o.clip = vec4<f32>(dot(d, g.right.xyz) * g.right.w, dot(d, g.up.xyz) * g.up.w, 0.5 + dot(d, g.back.xyz) * g.back.w, 1.0);
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

@fragment
fn fs_post(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let x = i32(frag.x);
    let y = i32(frag.y);
    let size = vec2<i32>(g.size.xy);
    let here = ids_at(x, y);
    let right = ids_at(x + 1, y);
    let below = ids_at(x, y + 1);
    let edge = (x + 1 < size.x && right.x != here.x) || (y + 1 < size.y && below.x != here.x);
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
    if behind > 0.0 && behind > in.depth + g.size.z {
        discard;
    }
    return vec4<f32>(in.color / 255.0, 1.0);
}
