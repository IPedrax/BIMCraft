// The triangles of the 2D canvas, kept on the card. Adapted from egui-wgpu's egui.wgsl (MIT or Apache-2.0): the same sums, so
// the same pixels, without its texture, as what `canvas::paint` makes uses only egui's white pixel, which multiplies by one.

struct VertexOutput {
    @location(0) color: vec4<f32>, // gamma 0-1
    @builtin(position) position: vec4<f32>,
};

struct Locals {
    screen_size: vec2<f32>,
    dithering: u32,
    // How they are scaled and moved from where they were made, while a view moves: 1 and nothing once its drawing is the exact one.
    k: f32,
    moved: vec2<f32>,
    // Physical pixels to a point: the motion shader fades the last pixel on screen, whatever the zoom.
    pixels_per_point: f32,
};
@group(0) @binding(0) var<uniform> r_locals: Locals;

fn interleaved_gradient_noise(n: vec2<f32>) -> f32 {
    let f = 0.06711056 * n.x + 0.00583715 * n.y;
    return fract(52.9829189 * fract(f));
}

fn dither_interleaved(rgb: vec3<f32>, levels: f32, frag_coord: vec4<f32>) -> vec3<f32> {
    var noise = interleaved_gradient_noise(frag_coord.xy);
    // scale down the noise slightly to ensure flat colors aren't getting dithered
    noise = (noise - 0.5) * 0.95;
    return rgb + noise / (levels - 1.0);
}

// 0-1 linear  from  0-1 sRGB gamma
fn linear_from_gamma_rgb(srgb: vec3<f32>) -> vec3<f32> {
    let cutoff = srgb < vec3<f32>(0.04045);
    let lower = srgb / vec3<f32>(12.92);
    let higher = pow((srgb + vec3<f32>(0.055)) / vec3<f32>(1.055), vec3<f32>(2.4));
    return select(higher, lower, cutoff);
}

// [u8; 4] SRGB as u32 -> [r, g, b, a] in 0.-1
fn unpack_color(color: u32) -> vec4<f32> {
    return vec4<f32>(
        f32(color & 255u),
        f32((color >> 8u) & 255u),
        f32((color >> 16u) & 255u),
        f32((color >> 24u) & 255u),
    ) / 255.0;
}

@vertex
fn vs_main(@location(0) a_pos: vec2<f32>, @location(2) a_color: u32) -> VertexOutput {
    var out: VertexOutput;
    out.color = unpack_color(a_color);
    let at = a_pos * r_locals.k + r_locals.moved;
    out.position = vec4<f32>(
        2.0 * at.x / r_locals.screen_size.x - 1.0,
        1.0 - 2.0 * at.y / r_locals.screen_size.y,
        0.0,
        1.0,
    );
    return out;
}

@fragment
fn fs_main_linear_framebuffer(in: VertexOutput) -> @location(0) vec4<f32> {
    var out_color_gamma = in.color;
    if r_locals.dithering == 1 {
        let out_color_gamma_rgb = dither_interleaved(out_color_gamma.rgb, 256.0, in.position);
        out_color_gamma = vec4<f32>(out_color_gamma_rgb, out_color_gamma.a);
    }
    let out_color_linear = linear_from_gamma_rgb(out_color_gamma.rgb);
    return vec4<f32>(out_color_linear, out_color_gamma.a);
}

@fragment
fn fs_main_gamma_framebuffer(in: VertexOutput) -> @location(0) vec4<f32> {
    var out_color_gamma = in.color;
    if r_locals.dithering == 1 {
        let out_color_gamma_rgb = dither_interleaved(out_color_gamma.rgb, 256.0, in.position);
        out_color_gamma = vec4<f32>(out_color_gamma_rgb, out_color_gamma.a);
    }
    return out_color_gamma;
}

// While a view zooms (motion.rs): a line's quad placed by its centre line, as wide on screen as it was drawn, its last pixel each
// side faded on screen, so it stays sharp at any scale. A fill's corners are only placed.
struct MotionOutput {
    @location(0) color: vec4<f32>,
    // How far across its line, and half how wide it is drawn, points; 0 and 0 for a fill.
    @location(1) across: f32,
    @location(2) half: f32,
    @builtin(position) position: vec4<f32>,
};

@vertex
fn vs_motion(@location(0) a_pos: vec2<f32>, @location(1) a_out: vec2<f32>, @location(2) a_half: f32, @location(3) a_color: u32) -> MotionOutput {
    var out: MotionOutput;
    out.color = unpack_color(a_color);
    let fade = 1.0 / r_locals.pixels_per_point;
    let half = abs(a_half);
    let reach = select(half + fade, 0.0, a_half == 0.0);
    let at = a_pos * r_locals.k + r_locals.moved + a_out * reach;
    out.across = reach * sign(a_half);
    out.half = select(half, -1.0, a_half == 0.0);
    out.position = vec4<f32>(2.0 * at.x / r_locals.screen_size.x - 1.0, 1.0 - 2.0 * at.y / r_locals.screen_size.y, 0.0, 1.0);
    return out;
}

// How much of the pixel the line covers: all within its half width, fading to none a pixel past it. A fill covers all of it.
fn motion_cover(in: MotionOutput) -> f32 {
    if in.half < 0.0 {
        return 1.0;
    }
    let ppp = r_locals.pixels_per_point;
    return clamp((in.half - abs(in.across)) * ppp + 0.5, 0.0, 1.0);
}

@fragment
fn fs_motion_linear_framebuffer(in: MotionOutput) -> @location(0) vec4<f32> {
    var c = in.color * motion_cover(in);
    if r_locals.dithering == 1 {
        c = vec4<f32>(dither_interleaved(c.rgb, 256.0, in.position), c.a);
    }
    return vec4<f32>(linear_from_gamma_rgb(c.rgb), c.a);
}

@fragment
fn fs_motion_gamma_framebuffer(in: MotionOutput) -> @location(0) vec4<f32> {
    var c = in.color * motion_cover(in);
    if r_locals.dithering == 1 {
        c = vec4<f32>(dither_interleaved(c.rgb, 256.0, in.position), c.a);
    }
    return c;
}
