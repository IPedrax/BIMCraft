//! egui's frame drawn on the processor: the triangles `Context::tessellate` makes, textured and coloured, into a buffer of
//! `0RGB` pixels that `softbuffer` shows. It draws as egui-wgpu does into the window's (non-sRGB) framebuffer: everything in
//! gamma space, the vertex colour times the texture, both premultiplied, blended `src + dst × (1 − src.a)`.
//!
//! Triangles are covered at pixel centres from vertices snapped to 1/256 of a pixel, with exact integer edge functions and the
//! top-left rule, as a graphics card does: two triangles that share an edge never both draw a pixel on it, so a translucent
//! rectangle has no line down its diagonal. Each row's span is found by solving the three edges for x rather than testing
//! each pixel; a triangle of one colour and no texture (most of what egui draws) is filled span by span, an opaque one
//! without blending. The window is drawn in bands of rows on a small pool of threads, one fewer than the processor has.
//!
//! Nothing a frame hands it can make it panic: a vertex that isn't finite or lies absurdly far off, an index past the
//! vertices, a texture never set or set with the wrong number of pixels, a buffer of the wrong size: what can't be drawn is
//! left out.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use egui::epaint::textures::{TextureFilter, TextureOptions, TextureWrapMode, TexturesDelta};
use egui::epaint::{ClippedPrimitive, ImageData, Mesh, Primitive, TextureId, Vertex};
use egui::{Color32, ColorImage};

/// Steps of a pixel a vertex is snapped to.
const SUB: i128 = 256;
/// How far off the window a vertex may lie, in pixels: a plan's line zoomed far in reaches past the window, but not this far.
const FAR: f32 = 1.0e9;
/// The largest texture side kept: egui never asks for more than the window says it has.
pub const MAX_TEXTURE_SIDE: usize = 16_384;
/// Rows a band has.
const BAND: usize = 32;

/// A texture as it was set: egui's colours (premultiplied sRGBA) and how it's sampled.
struct Texture {
    width: usize,
    height: usize,
    pixels: Vec<Color32>,
    options: TextureOptions,
}

impl Texture {
    fn from(image: &ColorImage, options: TextureOptions) -> Option<Texture> {
        let [width, height] = image.size;
        let ok = width > 0 && height > 0 && width <= MAX_TEXTURE_SIDE && height <= MAX_TEXTURE_SIDE;
        (ok && image.pixels.len() == width * height).then(|| Texture { width, height, pixels: image.pixels.clone(), options })
    }

    /// Writes `image` at `[x, y]`, what falls outside left out.
    fn patch(&mut self, image: &ColorImage, [x, y]: [usize; 2], options: TextureOptions) {
        let [w, h] = image.size;
        if image.pixels.len() != w * h {
            return;
        }
        self.options = options;
        for row in 0..h {
            let ty = y.saturating_add(row);
            if ty >= self.height || x >= self.width {
                break;
            }
            let n = w.min(self.width - x);
            let to = ty * self.width + x;
            if let (Some(dst), Some(src)) = (self.pixels.get_mut(to..to + n), image.pixels.get(row * w..row * w + n)) {
                dst.copy_from_slice(src);
            }
        }
    }

    fn texel(&self, x: i64, y: i64) -> [f32; 4] {
        let x = wrap(x, self.width, self.options.wrap_mode);
        let y = wrap(y, self.height, self.options.wrap_mode);
        let c = self.pixels.get(y * self.width + x).copied().unwrap_or(Color32::TRANSPARENT);
        [f32::from(c.r()), f32::from(c.g()), f32::from(c.b()), f32::from(c.a())]
    }

    /// The colour at `(u, v)`, each channel 0 to 255, by `filter`.
    fn sample(&self, u: f32, v: f32, filter: TextureFilter) -> [f32; 4] {
        let (u, v) = (if u.is_finite() { u } else { 0.0 }, if v.is_finite() { v } else { 0.0 });
        let x = (u * self.width as f32).clamp(-1.0e7, 1.0e7);
        let y = (v * self.height as f32).clamp(-1.0e7, 1.0e7);
        match filter {
            TextureFilter::Nearest => self.texel(x.floor() as i64, y.floor() as i64),
            TextureFilter::Linear => {
                let (x, y) = (x - 0.5, y - 0.5);
                let (x0, y0) = (x.floor(), y.floor());
                let (fx, fy) = (x - x0, y - y0);
                let (x0, y0) = (x0 as i64, y0 as i64);
                let (a, b) = (self.texel(x0, y0), self.texel(x0 + 1, y0));
                let (c, d) = (self.texel(x0, y0 + 1), self.texel(x0 + 1, y0 + 1));
                std::array::from_fn(|i| {
                    let top = a[i] + (b[i] - a[i]) * fx;
                    let bottom = c[i] + (d[i] - c[i]) * fx;
                    top + (bottom - top) * fy
                })
            }
        }
    }
}

/// A texel's coordinate along a side of `n` texels, by the texture's wrap mode.
fn wrap(i: i64, n: usize, mode: TextureWrapMode) -> usize {
    let n = n.max(1) as i64;
    let i = match mode {
        TextureWrapMode::ClampToEdge => i.clamp(0, n - 1),
        TextureWrapMode::Repeat => i.rem_euclid(n),
        TextureWrapMode::MirroredRepeat => {
            let m = i.rem_euclid(2 * n);
            if m < n { m } else { 2 * n - 1 - m }
        }
    };
    usize::try_from(i).unwrap_or(0)
}

/// What the window's frames are drawn with: the textures egui has set, and the threads that draw the bands.
pub struct Raster {
    textures: HashMap<TextureId, Texture>,
    pool: Option<rayon::ThreadPool>,
}

impl Default for Raster {
    fn default() -> Raster {
        Raster::new(std::thread::available_parallelism().map_or(1, usize::from).saturating_sub(1).max(1))
    }
}

/// A mesh ready to draw: its clip rectangle in pixels (`x0, y0, x1, y1`), its texture, and its vertices snapped.
struct Ready<'a> {
    clip: [i64; 4],
    texture: Option<&'a Texture>,
    mesh: &'a Mesh,
    points: Vec<Option<[i128; 2]>>,
}

impl Raster {
    /// A raster that draws on `threads` threads (one: on the caller's).
    pub fn new(threads: usize) -> Raster {
        let pool = (threads > 1)
            .then(|| rayon::ThreadPoolBuilder::new().num_threads(threads).thread_name(|n| format!("bimcraft-paint-{n}")).build().ok())
            .flatten();
        Raster { textures: HashMap::new(), pool }
    }

    /// The textures `delta` sets, before a frame is drawn.
    pub fn set_textures(&mut self, delta: &TexturesDelta) {
        for (id, deltas) in &delta.set {
            for image in deltas {
                let ImageData::Color(color) = &image.image;
                match image.pos {
                    None => match Texture::from(color, image.options) {
                        Some(texture) => {
                            self.textures.insert(*id, texture);
                        }
                        None => {
                            self.textures.remove(id);
                        }
                    },
                    Some(at) => {
                        if let Some(texture) = self.textures.get_mut(id) {
                            texture.patch(color, at, image.options);
                        }
                    }
                }
            }
        }
    }

    /// The textures `delta` frees, after the frame is drawn; `delta` is then done with, and emptied (egui checks, in a debug
    /// build, that no delta is dropped unapplied).
    pub fn free_textures(&mut self, delta: &mut TexturesDelta) {
        for id in &delta.free {
            self.textures.remove(id);
        }
        delta.clear();
    }

    /// The textures kept.
    pub fn textures(&self) -> usize {
        self.textures.len()
    }

    /// Draws `primitives`, tessellated at `pixels_per_point`, into `target`: `width` × `height` pixels of `0RGB`, first cleared
    /// to `clear`. A target of the wrong size is left as it is.
    pub fn draw(
        &self,
        target: &mut [u32],
        width: usize,
        height: usize,
        pixels_per_point: f32,
        clear: Color32,
        primitives: &[ClippedPrimitive],
    ) {
        let Some(pixels) = width.checked_mul(height) else { return };
        if pixels == 0 || target.len() != pixels || !pixels_per_point.is_finite() || pixels_per_point <= 0.0 {
            return;
        }
        let ready: Vec<Ready> = primitives.iter().filter_map(|p| self.ready(p, width, height, pixels_per_point)).collect();
        let background = pack(clear.r(), clear.g(), clear.b());
        let band = |(i, rows): (usize, &mut [u32])| {
            rows.fill(background);
            let y0 = (i * BAND) as i64;
            let y1 = y0 + (rows.len() / width) as i64;
            for r in &ready {
                draw_mesh(rows, width, y0, y1, r);
            }
        };
        match &self.pool {
            Some(pool) => pool.install(|| {
                use rayon::prelude::*;
                target.par_chunks_mut(BAND * width).enumerate().for_each(band);
            }),
            None => target.chunks_mut(BAND * width).enumerate().for_each(band),
        }
    }

    fn ready<'a>(&'a self, p: &'a ClippedPrimitive, width: usize, height: usize, ppp: f32) -> Option<Ready<'a>> {
        let Primitive::Mesh(mesh) = &p.primitive else { return None };
        let clip = p.clip_rect;
        let side = |v: f32, max: usize| -> i64 {
            let v = (v * ppp).round();
            if v.is_nan() { 0 } else { v.clamp(0.0, max as f32) as i64 }
        };
        let rect = [side(clip.min.x, width), side(clip.min.y, height), side(clip.max.x, width), side(clip.max.y, height)];
        if rect[0] >= rect[2] || rect[1] >= rect[3] || mesh.indices.is_empty() {
            return None;
        }
        let snap = |v: &Vertex| {
            let (x, y) = (v.pos.x * ppp, v.pos.y * ppp);
            (x.is_finite() && y.is_finite() && x.abs() < FAR && y.abs() < FAR)
                .then(|| [((f64::from(x)) * SUB as f64).round() as i128, ((f64::from(y)) * SUB as f64).round() as i128])
        };
        Some(Ready { clip: rect, texture: self.textures.get(&mesh.texture_id), mesh, points: mesh.vertices.iter().map(snap).collect() })
    }
}

/// `r, g, b` as softbuffer's `0RGB`.
fn pack(r: u8, g: u8, b: u8) -> u32 {
    (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
}

/// `x / 255`, rounded, for `x` up to 255 × 255.
fn div255(x: u32) -> u32 {
    let x = x + 128;
    (x + (x >> 8)) >> 8
}

/// A premultiplied colour over the pixel `dst`.
fn blend(dst: u32, [r, g, b, a]: [u8; 4]) -> u32 {
    if a == 255 {
        return pack(r, g, b);
    }
    if a == 0 && r == 0 && g == 0 && b == 0 {
        return dst;
    }
    let k = 255 - u32::from(a);
    let over = |src: u8, shift: u32| (u32::from(src) + div255(((dst >> shift) & 255) * k)).min(255);
    (over(r, 16) << 16) | (over(g, 8) << 8) | over(b, 0)
}

/// Rounds a colour channel worked out in floats, 0 to 255.
fn channel(v: f32) -> u8 {
    (v + 0.5).clamp(0.0, 255.0) as u8
}

fn ceil_div(a: i128, b: i128) -> i128 {
    let d = a.div_euclid(b);
    if a.rem_euclid(b) == 0 { d } else { d + 1 }
}

fn draw_mesh(rows: &mut [u32], width: usize, y0: i64, y1: i64, r: &Ready) {
    let clip = [r.clip[0], r.clip[1].max(y0), r.clip[2], r.clip[3].min(y1)];
    if clip[1] >= clip[3] {
        return;
    }
    for tri in r.mesh.indices.as_chunks::<3>().0 {
        let at = |i: u32| {
            let i = usize::try_from(i).ok()?;
            Some((r.points.get(i).copied().flatten()?, r.mesh.vertices.get(i)?))
        };
        if let (Some(a), Some(b), Some(c)) = (at(tri[0]), at(tri[1]), at(tri[2])) {
            triangle(rows, width, y0, clip, [a, b, c], r.texture);
        }
    }
}

/// The edge from `p` to `q` as `A x + B y + C`, positive inside a triangle wound with positive area.
fn edge(p: [i128; 2], q: [i128; 2]) -> [i128; 3] {
    let (a, b) = (p[1] - q[1], q[0] - p[0]);
    [a, b, -(a * p[0] + b * p[1])]
}

fn triangle(rows: &mut [u32], width: usize, y0: i64, clip: [i64; 4], mut v: [([i128; 2], &Vertex); 3], texture: Option<&Texture>) {
    let cross = |a: [i128; 2], b: [i128; 2], c: [i128; 2]| (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
    let mut area = cross(v[0].0, v[1].0, v[2].0);
    if area == 0 {
        return;
    }
    if area < 0 {
        v.swap(1, 2);
        area = -area;
    }
    let [p0, p1, p2] = [v[0].0, v[1].0, v[2].0];
    // The pixels the triangle can reach, within the clip.
    let lo = |i: usize| p0[i].min(p1[i]).min(p2[i]).div_euclid(SUB);
    let hi = |i: usize| ceil_div(p0[i].max(p1[i]).max(p2[i]), SUB);
    let (x_lo, x_hi) = (lo(0).max(i128::from(clip[0])), hi(0).min(i128::from(clip[2])));
    let (y_lo, y_hi) = (lo(1).max(i128::from(clip[1])), hi(1).min(i128::from(clip[3])));
    if x_lo >= x_hi || y_lo >= y_hi {
        return;
    }
    // Edge k is opposite vertex k: its value over the area is vertex k's weight.
    let edges = [edge(p1, p2), edge(p2, p0), edge(p0, p1)];
    let shade = Shade::new(&v, texture, area);
    let inv = 1.0 / area as f64;
    for y in y_lo..y_hi {
        let yc = y * SUB + SUB / 2;
        let (mut xs, mut xe) = (x_lo, x_hi);
        for [a, b, c] in edges {
            let k = b * yc + c;
            if a > 0 {
                // A left edge: a pixel centre on it is inside.
                xs = xs.max(ceil_div(-k - a * SUB / 2, a * SUB));
            } else if a < 0 {
                // A right edge: on it is outside.
                xe = xe.min(ceil_div(k + a * SUB / 2, -a * SUB));
            } else if k < 0 || (k == 0 && b <= 0) {
                // Level with the row: the row is inside only above a bottom edge's or on a top edge's line.
                xe = xs;
            }
        }
        if xs >= xe {
            continue;
        }
        let row = usize::try_from(y - i128::from(y0)).unwrap_or(usize::MAX);
        let (Ok(xs), Ok(xe)) = (usize::try_from(xs), usize::try_from(xe)) else { continue };
        let Some(span) = rows.get_mut(row.saturating_mul(width) + xs..row.saturating_mul(width) + xe) else { continue };
        match &shade {
            Shade::Flat(rgba) if rgba[3] == 255 => span.fill(pack(rgba[0], rgba[1], rgba[2])),
            Shade::Flat(rgba) => span.iter_mut().for_each(|px| *px = blend(*px, *rgba)),
            Shade::Smooth { colors, texel, uv, filter } => {
                let xc = xs as i128 * SUB + SUB / 2;
                let w = |k: usize| ((edges[k][0] * xc + edges[k][1] * yc + edges[k][2]) as f64 * inv) as f32;
                let dw = |k: usize| ((edges[k][0] * SUB) as f64 * inv) as f32;
                let (w0, w1, w2) = (w(0), w(1), w(2));
                let (d0, d1, d2) = (dw(0), dw(1), dw(2));
                for (i, px) in span.iter_mut().enumerate() {
                    let i = i as f32;
                    let (a, b, c) = (w0 + d0 * i, w1 + d1 * i, w2 + d2 * i);
                    let t = match (texture, uv) {
                        (Some(tex), Some(uv)) => {
                            tex.sample(a * uv[0][0] + b * uv[1][0] + c * uv[2][0], a * uv[0][1] + b * uv[1][1] + c * uv[2][1], *filter)
                        }
                        _ => *texel,
                    };
                    let rgba: [u8; 4] = std::array::from_fn(|ch| {
                        let col = a * colors[0][ch] + b * colors[1][ch] + c * colors[2][ch];
                        channel(col * t[ch] / 255.0)
                    });
                    *px = blend(*px, rgba);
                }
            }
        }
    }
}

/// How a triangle's pixels are coloured.
enum Shade {
    /// One colour over the whole triangle: one colour at its vertices, and one texel under them.
    Flat([u8; 4]),
    /// Colours to interpolate, and either one texel (`uv` none) or the texture at the interpolated `uv`.
    Smooth { colors: [[f32; 4]; 3], texel: [f32; 4], uv: Option<[[f32; 2]; 3]>, filter: TextureFilter },
}

impl Shade {
    fn new(v: &[([i128; 2], &Vertex); 3], texture: Option<&Texture>, area: i128) -> Shade {
        let rgba = |c: Color32| [f32::from(c.r()), f32::from(c.g()), f32::from(c.b()), f32::from(c.a())];
        let same_uv = v[0].1.uv == v[1].1.uv && v[1].1.uv == v[2].1.uv;
        // Magnified or minified: the texels the triangle covers against its pixels.
        let filter = texture.map_or(TextureFilter::Linear, |t| {
            let [a, b, c] = [v[0].1.uv, v[1].1.uv, v[2].1.uv];
            let uv_area = ((b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)).abs() * (t.width * t.height) as f32;
            let px_area = area as f32 / (SUB * SUB) as f32;
            if uv_area > px_area { t.options.minification } else { t.options.magnification }
        });
        // No texture egui knows of draws as white, as a missing texture's white pixel would.
        let texel = match texture {
            Some(t) if same_uv => t.sample(v[0].1.uv.x, v[0].1.uv.y, filter),
            Some(_) => [255.0; 4],
            None => [255.0; 4],
        };
        let same_color = v[0].1.color == v[1].1.color && v[1].1.color == v[2].1.color;
        if same_uv && same_color {
            let c = rgba(v[0].1.color);
            return Shade::Flat(std::array::from_fn(|i| channel(c[i] * texel[i] / 255.0)));
        }
        let uv = (!same_uv && texture.is_some()).then(|| [0, 1, 2].map(|i| [v[i].1.uv.x, v[i].1.uv.y]));
        Shade::Smooth { colors: [rgba(v[0].1.color), rgba(v[1].1.color), rgba(v[2].1.color)], texel, uv, filter }
    }
}

/// A hash of what a frame draws: two frames with the same one, the same textures and the same size look the same.
pub fn fingerprint(primitives: &[ClippedPrimitive], pixels_per_point: f32, size: [usize; 2]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (pixels_per_point.to_bits(), size, primitives.len()).hash(&mut h);
    for p in primitives {
        let r = p.clip_rect;
        [r.min.x, r.min.y, r.max.x, r.max.y].map(f32::to_bits).hash(&mut h);
        match &p.primitive {
            Primitive::Mesh(m) => {
                m.texture_id.hash(&mut h);
                m.indices.hash(&mut h);
                for v in &m.vertices {
                    [v.pos.x, v.pos.y, v.uv.x, v.uv.y].map(f32::to_bits).hash(&mut h);
                    v.color.to_array().hash(&mut h);
                }
            }
            Primitive::Callback(_) => 1u8.hash(&mut h),
        }
    }
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::epaint::{ImageDelta, WHITE_UV};
    use egui::{Pos2, Rect, pos2};

    const W: usize = 16;
    const H: usize = 16;
    const BLACK: Color32 = Color32::BLACK;

    fn quad(rect: Rect, color: Color32) -> ClippedPrimitive {
        let mut mesh = Mesh::default();
        mesh.add_colored_rect(rect, color);
        ClippedPrimitive { clip_rect: Rect::EVERYTHING, primitive: Primitive::Mesh(mesh) }
    }

    fn draw(raster: &Raster, prims: &[ClippedPrimitive]) -> Vec<u32> {
        let mut px = vec![0u32; W * H];
        raster.draw(&mut px, W, H, 1.0, BLACK, prims);
        px
    }

    fn rgb(p: u32) -> [u8; 3] {
        [(p >> 16) as u8, (p >> 8) as u8, p as u8]
    }

    #[test]
    fn a_rectangle_fills_exactly_its_pixels() {
        let px = draw(&Raster::new(1), &[quad(Rect::from_min_max(pos2(2.0, 3.0), pos2(6.0, 8.0)), Color32::from_rgb(200, 10, 20))]);
        for y in 0..H {
            for x in 0..W {
                let inside = (2..6).contains(&x) && (3..8).contains(&y);
                assert_eq!(rgb(px[y * W + x]), if inside { [200, 10, 20] } else { [0, 0, 0] }, "({x}, {y})");
            }
        }
    }

    #[test]
    fn a_translucent_rectangle_blends_once_even_on_its_diagonal() {
        // Half-transparent white (premultiplied 128) over black: 128 everywhere, the diagonal shared by its two triangles too.
        let px = draw(&Raster::new(1), &[quad(Rect::from_min_max(pos2(0.0, 0.0), pos2(16.0, 16.0)), Color32::from_white_alpha(128))]);
        assert!(
            px.iter().all(|p| rgb(*p) == [128, 128, 128]),
            "{:?}",
            px.iter().map(|p| rgb(*p)).collect::<std::collections::BTreeSet<_>>()
        );
    }

    #[test]
    fn premultiplied_colour_blends_over_what_is_there() {
        assert_eq!(blend(pack(200, 100, 0), [0, 0, 0, 0]), pack(200, 100, 0));
        assert_eq!(blend(pack(200, 100, 0), [10, 20, 30, 255]), pack(10, 20, 30));
        // 50 % red over white: 128 + 255 × 127/255.
        assert_eq!(rgb(blend(pack(255, 255, 255), [128, 0, 0, 128])), [255, 127, 127]);
        // A colour brighter than its alpha (not premultiplied) saturates rather than wraps.
        assert_eq!(rgb(blend(pack(255, 255, 255), [255, 255, 255, 10])), [255, 255, 255]);
    }

    #[test]
    fn the_clip_rectangle_cuts_what_is_drawn() {
        let mut p = quad(Rect::from_min_max(pos2(0.0, 0.0), pos2(16.0, 16.0)), Color32::WHITE);
        p.clip_rect = Rect::from_min_max(pos2(4.0, 5.0), pos2(9.0, 7.0));
        let px = draw(&Raster::new(1), &[p]);
        let lit: Vec<(usize, usize)> = (0..W * H).filter(|i| px[*i] != 0).map(|i| (i % W, i / W)).collect();
        assert_eq!(lit.len(), 5 * 2);
        assert!(lit.iter().all(|(x, y)| (4..9).contains(x) && (5..7).contains(y)));
    }

    #[test]
    fn colours_are_interpolated_across_a_triangle() {
        let mut mesh = Mesh::default();
        for (x, c) in [(0.0, Color32::from_rgb(0, 0, 0)), (16.0, Color32::from_rgb(255, 0, 0))] {
            mesh.vertices.push(Vertex { pos: pos2(x, 0.0), uv: WHITE_UV, color: c });
            mesh.vertices.push(Vertex { pos: pos2(x, 16.0), uv: WHITE_UV, color: c });
        }
        mesh.indices = vec![0, 1, 2, 2, 1, 3];
        let px = draw(&Raster::new(1), &[ClippedPrimitive { clip_rect: Rect::EVERYTHING, primitive: Primitive::Mesh(mesh) }]);
        // Pixel x's centre is (x + 0.5) / 16 of the way: red 255 × that.
        for x in 0..W {
            let want = (255.0 * (x as f32 + 0.5) / 16.0).round() as i32;
            let got = i32::from(rgb(px[8 * W + x])[0]);
            assert!((got - want).abs() <= 1, "x {x}: {got} for {want}");
        }
    }

    fn texture(raster: &mut Raster, id: TextureId, image: ColorImage, options: TextureOptions, pos: Option<[usize; 2]>) {
        let mut delta = TexturesDelta::default();
        delta.push(id, ImageDelta { image: ImageData::Color(std::sync::Arc::new(image)), options, pos });
        raster.set_textures(&delta);
        delta.clear();
    }

    fn image_quad(id: TextureId, rect: Rect) -> ClippedPrimitive {
        let mut mesh = Mesh::with_texture(id);
        mesh.add_rect_with_uv(rect, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);
        ClippedPrimitive { clip_rect: Rect::EVERYTHING, primitive: Primitive::Mesh(mesh) }
    }

    #[test]
    fn a_texture_is_drawn_texel_for_pixel_and_a_patch_changes_part_of_it() {
        let mut raster = Raster::new(1);
        let id = TextureId::User(7);
        let red = ColorImage::new([4, 4], vec![Color32::RED; 16]);
        texture(&mut raster, id, red, TextureOptions::NEAREST, None);
        texture(&mut raster, id, ColorImage::new([2, 1], vec![Color32::BLUE; 2]), TextureOptions::NEAREST, Some([1, 2]));
        let px = draw(&raster, &[image_quad(id, Rect::from_min_max(pos2(4.0, 4.0), pos2(8.0, 8.0)))]);
        for y in 0..4 {
            for x in 0..4 {
                let want = if y == 2 && (1..3).contains(&x) { [0, 0, 255] } else { [255, 0, 0] };
                assert_eq!(rgb(px[(4 + y) * W + 4 + x]), want, "texel ({x}, {y})");
            }
        }
        // A patch reaching past the edge keeps what fits; one of the wrong pixel count is left out.
        texture(&mut raster, id, ColorImage::new([8, 8], vec![Color32::GREEN; 64]), TextureOptions::NEAREST, Some([3, 3]));
        let mut bad = ColorImage::new([2, 2], vec![Color32::BLUE; 4]);
        bad.pixels.truncate(3);
        texture(&mut raster, id, bad, TextureOptions::NEAREST, Some([0, 0]));
        let px = draw(&raster, &[image_quad(id, Rect::from_min_max(pos2(4.0, 4.0), pos2(8.0, 8.0)))]);
        assert_eq!(rgb(px[7 * W + 7]), [0, 255, 0]);
        assert_eq!(rgb(px[4 * W + 4]), [255, 0, 0]);
    }

    #[test]
    fn a_freed_texture_is_gone_and_a_mesh_on_a_texture_never_set_draws_white() {
        let mut raster = Raster::new(1);
        let id = TextureId::User(1);
        texture(&mut raster, id, ColorImage::new([1, 1], vec![Color32::RED]), TextureOptions::LINEAR, None);
        assert_eq!(raster.textures(), 1);
        let mut delta = TexturesDelta::default();
        delta.free(id);
        raster.free_textures(&mut delta);
        assert_eq!(raster.textures(), 0);
        let px = draw(&raster, &[image_quad(id, Rect::from_min_max(pos2(0.0, 0.0), pos2(2.0, 2.0)))]);
        assert_eq!(rgb(px[0]), [255, 255, 255]);
    }

    #[test]
    fn hostile_meshes_and_buffers_are_left_out_without_a_panic() {
        let raster = Raster::new(1);
        let mut mesh = Mesh::default();
        let v = |x: f32, y: f32| Vertex { pos: pos2(x, y), uv: WHITE_UV, color: Color32::WHITE };
        mesh.vertices = vec![v(f32::NAN, 0.0), v(1e30, 1e30), v(-1e30, 5.0), v(0.0, 0.0), v(16.0, 0.0), v(0.0, 16.0)];
        mesh.indices = vec![0, 1, 2, 3, 4, 99, u32::MAX, 3, 4, 1, 4, 5];
        let mut prims = vec![ClippedPrimitive { clip_rect: Rect::EVERYTHING, primitive: Primitive::Mesh(mesh) }];
        prims.push(ClippedPrimitive {
            clip_rect: Rect::from_min_max(pos2(f32::NAN, f32::INFINITY), pos2(f32::NEG_INFINITY, 3.0)),
            primitive: Primitive::Mesh(Mesh::default()),
        });
        let px = draw(&raster, &prims);
        assert!(px.iter().all(|p| *p == 0), "nothing drawable was drawn");
        // A buffer of the wrong size, a window of no size, a pixels-per-point that isn't one: nothing at all.
        let mut short = vec![7u32; 10];
        raster.draw(&mut short, W, H, 1.0, Color32::RED, &prims);
        raster.draw(&mut short, 0, 0, 1.0, Color32::RED, &prims);
        raster.draw(&mut short, 5, 2, f32::NAN, Color32::RED, &prims);
        raster.draw(&mut short, usize::MAX, 2, 1.0, Color32::RED, &prims);
        assert!(short.iter().all(|p| *p == 7));
        // A texture too big, or with too few pixels, is not kept.
        let mut r = Raster::new(1);
        let mut few = ColorImage::new([3, 3], vec![Color32::RED; 9]);
        few.pixels.pop();
        texture(&mut r, TextureId::User(2), few, TextureOptions::LINEAR, None);
        assert_eq!(r.textures(), 0);
    }

    #[test]
    fn a_line_zoomed_far_past_the_window_is_still_drawn_where_it_crosses_it() {
        let mut mesh = Mesh::default();
        let v = |x: f32, y: f32| Vertex { pos: pos2(x, y), uv: WHITE_UV, color: Color32::WHITE };
        mesh.vertices = vec![v(-5.0e7, 7.0), v(5.0e7, 7.0), v(-5.0e7, 9.0), v(5.0e7, 9.0)];
        mesh.indices = vec![0, 1, 2, 2, 1, 3];
        let px = draw(&Raster::new(1), &[ClippedPrimitive { clip_rect: Rect::EVERYTHING, primitive: Primitive::Mesh(mesh) }]);
        for x in 0..W {
            assert_eq!(rgb(px[7 * W + x]), [255; 3]);
            assert_eq!(rgb(px[8 * W + x]), [255; 3]);
            assert_eq!(rgb(px[9 * W + x]), [0; 3]);
        }
    }

    #[test]
    fn the_bands_drawn_on_threads_match_one_thread() {
        let prims: Vec<ClippedPrimitive> = (0..40)
            .map(|i| {
                let f = i as f32;
                quad(
                    Rect::from_min_max(pos2(f * 3.1 % 13.0, f * 1.7 % 11.0), pos2(f * 3.1 % 13.0 + 3.3, f * 1.7 % 11.0 + 40.0)),
                    Color32::from_rgba_premultiplied(i * 6, 50, 255 - i * 6, 100 + i * 3),
                )
            })
            .collect();
        let (w, h) = (37, 101);
        let mut one = vec![0u32; w * h];
        let mut four = vec![0u32; w * h];
        Raster::new(1).draw(&mut one, w, h, 1.5, Color32::DARK_GRAY, &prims);
        Raster::new(4).draw(&mut four, w, h, 1.5, Color32::DARK_GRAY, &prims);
        assert_eq!(one, four);
    }

    #[test]
    fn the_fingerprint_changes_with_what_is_drawn_and_the_size() {
        let a = [quad(Rect::from_min_max(pos2(0.0, 0.0), pos2(4.0, 4.0)), Color32::RED)];
        let b = [quad(Rect::from_min_max(pos2(0.0, 0.0), pos2(4.0, 4.0)), Color32::BLUE)];
        assert_eq!(fingerprint(&a, 1.0, [8, 8]), fingerprint(&a, 1.0, [8, 8]));
        assert_ne!(fingerprint(&a, 1.0, [8, 8]), fingerprint(&b, 1.0, [8, 8]));
        assert_ne!(fingerprint(&a, 1.0, [8, 8]), fingerprint(&a, 1.0, [9, 8]));
        assert_ne!(fingerprint(&a, 1.0, [8, 8]), fingerprint(&a, 2.0, [8, 8]));
    }
}
