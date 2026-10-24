//! Textures: what a material's surfaces look like in the 3D view. A pattern drawn in code (bricks,
//! boards, concrete…) or a picture the user adds (a PNG or a JPEG), laid on a face at its real size in
//! millimetres, turned and moved as its material says. A picture is kept in the project as a PNG, so
//! a project carries its own textures.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use zune_jpeg::JpegDecoder;
use zune_jpeg::zune_core::bytestream::ZCursor;
use zune_jpeg::zune_core::options::DecoderOptions;

use crate::model::{MAX_COORD, MAX_TEXT, mix};
use crate::render::Rgb;

/// Pictures one project may hold.
pub const MAX_IMAGES: usize = 24;
/// The longest side a picture is kept at, px: a larger one is halved down to it.
pub const MAX_SIDE: usize = 1024;
/// What a file may be before it's refused: bytes, and pixels.
pub const MAX_BYTES: usize = 32 << 20;
const MAX_PIXELS: u64 = 16_000_000;
/// The side of a pattern drawn in code, px.
const SIDE: usize = 256;
/// The longest a tile of a texture may be, mm, and the shortest.
const LONGEST: f64 = 1e6;
const SHORTEST: f64 = 1.0;

/// What a texture is: a pattern drawn in code, or a picture of the project's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Brick,
    Block,
    Boards,
    Concrete,
    Plaster,
    Tile,
    Roof,
    Fabric,
    Metal,
    Insulation,
    Image,
}

impl Kind {
    pub const ALL: [Kind; 11] = [
        Kind::Brick,
        Kind::Block,
        Kind::Boards,
        Kind::Concrete,
        Kind::Plaster,
        Kind::Tile,
        Kind::Roof,
        Kind::Fabric,
        Kind::Metal,
        Kind::Insulation,
        Kind::Image,
    ];

    /// Its name in commands and files.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Brick => "brick",
            Kind::Block => "block",
            Kind::Boards => "boards",
            Kind::Concrete => "concrete",
            Kind::Plaster => "plaster",
            Kind::Tile => "tile",
            Kind::Roof => "roof",
            Kind::Fabric => "fabric",
            Kind::Metal => "metal",
            Kind::Insulation => "insulation",
            Kind::Image => "image",
        }
    }

    /// The millimetres a tile of it covers, across and up, as its pattern is drawn to be: four bricks across and eight
    /// courses, boards 140 wide.
    pub fn natural(self) -> [f64; 2] {
        match self {
            Kind::Brick => [900.0, 600.0],
            Kind::Block => [800.0, 800.0],
            Kind::Boards => [1200.0, 1120.0],
            Kind::Concrete => [1000.0, 1000.0],
            Kind::Plaster => [500.0, 500.0],
            Kind::Tile => [1200.0, 1200.0],
            Kind::Roof => [1320.0, 1000.0],
            Kind::Fabric => [100.0, 100.0],
            Kind::Metal => [800.0, 800.0],
            Kind::Insulation => [600.0, 600.0],
            Kind::Image => [1000.0, 1000.0],
        }
    }
}

fn is_zero(x: &f64) -> bool {
    *x == 0.0
}

fn is_origin(p: &[f64; 2]) -> bool {
    *p == [0.0; 2]
}

/// A material's texture: a tile of it `size` mm across and up, turned `angle` degrees counter-clockwise on its face and
/// moved `offset` mm, repeated over the face. Of a picture, `image` names it among the project's.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Texture {
    pub kind: Kind,
    pub size: [f64; 2],
    #[serde(default, skip_serializing_if = "is_zero")]
    pub angle: f64,
    #[serde(default, skip_serializing_if = "is_origin")]
    pub offset: [f64; 2],
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub image: String,
}

impl Texture {
    /// A texture of `kind`, at its natural size.
    pub fn new(kind: Kind) -> Texture {
        Texture { kind, size: kind.natural(), angle: 0.0, offset: [0.0; 2], image: String::new() }
    }

    /// Whether it's sound: a tile of a usable size, turned and moved by numbers, and of a picture `images` holds.
    pub fn check(&self, images: &BTreeMap<String, Picture>) -> Result<(), String> {
        if !self.size.iter().all(|s| (SHORTEST..=LONGEST).contains(s)) {
            return Err(format!("a tile of it is {} by {} mm; each side is {SHORTEST} to {LONGEST} mm", self.size[0], self.size[1]));
        }
        if !(self.angle.is_finite() && self.angle.abs() <= 1e6 && self.offset.iter().all(|x| x.is_finite() && x.abs() <= MAX_COORD)) {
            return Err("its angle and its offset need to be numbers".into());
        }
        match (self.kind, self.image.as_str()) {
            (Kind::Image, name) if !images.contains_key(name) => {
                let names: Vec<&str> = images.keys().map(String::as_str).collect();
                Err(format!("it's of picture `{name}`, which the project doesn't hold (it has: {})", names.join(", ")))
            }
            (Kind::Image, _) => Ok(()),
            (_, "") => Ok(()),
            (_, _) => Err("only a texture of a picture names one".into()),
        }
    }
}

/// A level of a picture's mip chain: rows of RGB, a pixel of each level the average of four of the one before.
pub struct Level {
    w: usize,
    h: usize,
    rgb: Vec<u8>,
}

impl Level {
    /// The pixel at `(x, y)`, wrapped round the picture.
    fn at(&self, x: i64, y: i64) -> [f64; 3] {
        let (x, y) = (x.rem_euclid(self.w.max(1) as i64) as usize, y.rem_euclid(self.h.max(1) as i64) as usize);
        match self.rgb.get((y * self.w + x) * 3..(y * self.w + x) * 3 + 3) {
            Some(&[r, g, b]) => [f64::from(r), f64::from(g), f64::from(b)],
            _ => [128.0; 3],
        }
    }

    /// The colour at a fraction `(fu, fv)` of the way across and down it, blended between the four pixels round it,
    /// wrapping at its edges, so a tile meets the next.
    fn bilinear(&self, fu: f64, fv: f64) -> [f64; 3] {
        let (x, y) = (fu * self.w as f64 - 0.5, fv * self.h as f64 - 0.5);
        let (x0, y0) = (x.floor(), y.floor());
        let (tx, ty) = (x - x0, y - y0);
        let (x0, y0) = (x0 as i64, y0 as i64);
        let (x1, y1) = (x0.saturating_add(1), y0.saturating_add(1));
        let (a, b, c, d) = (self.at(x0, y0), self.at(x1, y0), self.at(x0, y1), self.at(x1, y1));
        std::array::from_fn(|k| (a[k] * (1.0 - tx) + b[k] * tx) * (1.0 - ty) + (c[k] * (1.0 - tx) + d[k] * tx) * ty)
    }

    /// Half its size each way, a pixel the average of the four it covers.
    fn halved(&self) -> Level {
        let (w, h) = ((self.w / 2).max(1), (self.h / 2).max(1));
        let mut rgb = Vec::with_capacity(w * h * 3);
        for y in 0..h {
            for x in 0..w {
                let sum = [(0, 0), (1, 0), (0, 1), (1, 1)].iter().fold([0.0; 3], |s, (dx, dy)| {
                    let p = self.at((2 * x + dx) as i64, (2 * y + dy) as i64);
                    [s[0] + p[0], s[1] + p[1], s[2] + p[2]]
                });
                rgb.extend(sum.map(|v| (v / 4.0).round() as u8));
            }
        }
        Level { w, h, rgb }
    }
}

/// `base` and each level under it, down to one pixel.
fn chain(base: Level) -> Vec<Level> {
    let mut levels = vec![base];
    while let Some(last) = levels.last()
        && (last.w > 1 || last.h > 1)
        && levels.len() < 16
    {
        let next = last.halved();
        levels.push(next);
    }
    levels
}

/// A picture of a project: the PNG it's kept as, and its levels for texturing with.
#[derive(Clone)]
pub struct Picture {
    name: String,
    png: Arc<Vec<u8>>,
    levels: Arc<Vec<Level>>,
}

impl std::fmt::Debug for Picture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (w, h) = self.size();
        write!(f, "Picture({:?}, {w} x {h} px)", self.name)
    }
}

impl PartialEq for Picture {
    fn eq(&self, other: &Picture) -> bool {
        self.name == other.name && self.png == other.png
    }
}

/// How a picture goes in a project file: its name, and its PNG as base64.
#[derive(Serialize, Deserialize)]
struct PictureFile {
    name: String,
    png: String,
}

impl TryFrom<PictureFile> for Picture {
    type Error = String;

    fn try_from(f: PictureFile) -> Result<Picture, String> {
        let bytes = unbase64(&f.png).ok_or("a picture's data isn't base64")?;
        let base = decode(&bytes).map_err(|e| format!("picture `{}`: {e}", f.name))?;
        Ok(Picture { name: f.name, png: Arc::new(bytes), levels: Arc::new(chain(shrunk(base))) })
    }
}

impl Serialize for Picture {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        PictureFile { name: self.name.clone(), png: crate::commands::base64(&self.png) }.serialize(s)
    }
}

impl<'de> Deserialize<'de> for Picture {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Picture, D::Error> {
        Picture::try_from(PictureFile::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

impl Picture {
    /// A picture named `name` from the bytes of a PNG or a JPEG file; refused if it's neither, or too big to hold. It's
    /// kept as a PNG, at most [`MAX_SIDE`] px a side.
    pub fn import(name: &str, bytes: &[u8]) -> Result<Picture, String> {
        let base = shrunk(decode(bytes)?);
        let rgba: Vec<u8> = base.rgb.as_chunks::<3>().0.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect();
        let size = tiny_skia::IntSize::from_wh(base.w as u32, base.h as u32).ok_or("it has no size")?;
        let png = tiny_skia::Pixmap::from_vec(rgba, size).ok_or("it has no size")?.encode_png().map_err(|e| e.to_string())?;
        Ok(Picture { name: name.chars().take(MAX_TEXT).collect(), png: Arc::new(png), levels: Arc::new(chain(base)) })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Its width and height, px.
    pub fn size(&self) -> (usize, usize) {
        self.levels.first().map_or((0, 0), |l| (l.w, l.h))
    }

    /// An id for it among a project's pictures: the same for the same bytes, so the same picture comes in once.
    pub fn id(&self) -> String {
        let hash = self.png.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3));
        format!("img-{hash:016x}")
    }
}

/// `level` halved down until neither side is longer than [`MAX_SIDE`].
fn shrunk(mut level: Level) -> Level {
    while level.w.max(level.h) > MAX_SIDE {
        level = level.halved();
    }
    level
}

/// Whether `w` by `h` px is a size to take.
fn too_big(w: u64, h: u64) -> Result<(), String> {
    if w == 0 || h == 0 || w * h > MAX_PIXELS {
        return Err(format!("it's {w} by {h} px; a picture has up to {MAX_PIXELS} pixels"));
    }
    Ok(())
}

/// The pixels of a PNG or a JPEG, as RGB.
fn decode(bytes: &[u8]) -> Result<Level, String> {
    if bytes.len() > MAX_BYTES {
        return Err(format!("it's {} bytes; up to {MAX_BYTES}", bytes.len()));
    }
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        // The size is in the header, right after the signature and the chunk's length and name: checked before
        // the decoder makes room for it.
        let size = bytes.get(16..24).ok_or("it's cut short")?;
        let word = |i: usize| u64::from(u32::from_be_bytes([size[i], size[i + 1], size[i + 2], size[i + 3]]));
        too_big(word(0), word(4))?;
        let pixmap = tiny_skia::Pixmap::decode_png(bytes).map_err(|e| format!("it isn't a PNG BIMCraft can read: {e}"))?;
        // Over white where it lets it through: its pixels are premultiplied.
        let rgb = pixmap
            .data()
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| {
                let clear = 255 - p[3];
                [p[0].saturating_add(clear), p[1].saturating_add(clear), p[2].saturating_add(clear)]
            })
            .collect();
        Ok(Level { w: pixmap.width() as usize, h: pixmap.height() as usize, rgb })
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        let options = DecoderOptions::default().set_max_width(8192).set_max_height(8192);
        let mut decoder = JpegDecoder::new_with_options(ZCursor::new(bytes), options);
        decoder.decode_headers().map_err(|e| format!("it isn't a JPEG BIMCraft can read: {e:?}"))?;
        let (w, h) = decoder.dimensions().ok_or("it has no size")?;
        too_big(w as u64, h as u64)?;
        let rgb = decoder.decode().map_err(|e| format!("it isn't a JPEG BIMCraft can read: {e:?}"))?;
        if rgb.len() != w * h * 3 {
            return Err("it isn't a JPEG of colour BIMCraft can read".into());
        }
        Ok(Level { w, h, rgb })
    } else {
        Err("it's neither a PNG nor a JPEG".into())
    }
}

/// Bytes from standard base64, or none if it isn't.
pub(crate) fn unbase64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut held, mut bits) = (0_u32, 0_u32);
    for c in text.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b'\n' | b'\r' | b' ' => continue,
            _ => return None,
        };
        held = held << 6 | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((held >> bits) as u8);
            held &= (1 << bits) - 1;
        }
    }
    Some(out)
}

// ---------- patterns drawn in code ----------

/// A pseudo-random number from 0 up to 1 for lattice point `(i, j)`, the same each time.
fn hash(seed: u64, i: i64, j: i64) -> f64 {
    let z = mix(seed ^ (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (j as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F));
    (z >> 11) as f64 / (1_u64 << 53) as f64
}

/// Smooth value noise from 0 to 1 over a tile (`fx` and `fy` from 0 to 1 across it) with `px` by `py` lattice cells, wrapping,
/// so the tile meets the next.
fn noise(seed: u64, fx: f64, fy: f64, px: i64, py: i64) -> f64 {
    let (x, y) = (fx * px as f64, fy * py as f64);
    let (i, j, tx, ty) = (x.floor(), y.floor(), x - x.floor(), y - y.floor());
    let smooth = |t: f64| t * t * (3.0 - 2.0 * t);
    let (tx, ty) = (smooth(tx), smooth(ty));
    let at = |a: i64, b: i64| hash(seed, a.rem_euclid(px.max(1)), b.rem_euclid(py.max(1)));
    let (i, j) = (i as i64, j as i64);
    (at(i, j) * (1.0 - tx) + at(i + 1, j) * tx) * (1.0 - ty) + (at(i, j + 1) * (1.0 - tx) + at(i + 1, j + 1) * tx) * ty
}

/// Noise of `octaves` scales, each twice as fine and half as strong as the one before.
fn fbm(seed: u64, fx: f64, fy: f64, px: i64, py: i64, octaves: u32) -> f64 {
    let (mut sum, mut weight, mut all) = (0.0, 1.0, 0.0);
    for k in 0..octaves {
        sum += weight * noise(seed + u64::from(k), fx, fy, px << k, py << k);
        all += weight;
        weight /= 2.0;
    }
    sum / all
}

/// What the pattern of `kind` shows at pixel `(x, y)` of its tile, over `color`.
fn shade(kind: Kind, color: [f64; 3], x: f64, y: f64) -> [f64; 3] {
    let (u, v) = (x / SIDE as f64, y / SIDE as f64);
    let scale = |c: [f64; 3], k: f64| c.map(|a| a * k);
    // A grid of `cw` by `rh` cells, every other row moved half a cell: the cell's own column and row, where in it, and
    // whether that is in a joint `joint` px wide along its left and top.
    let cells = |cw: f64, rh: f64, joint: f64, running: bool| {
        let row = (y / rh).floor();
        let shifted = x + if running && row as i64 % 2 != 0 { cw / 2.0 } else { 0.0 };
        let col = (shifted / cw).floor();
        let (lx, ly) = (shifted - col * cw, y - row * rh);
        let per_row = (SIDE as f64 / cw).round() as i64;
        ((col as i64).rem_euclid(per_row.max(1)), row as i64, lx, ly, lx < joint || ly < joint)
    };
    match kind {
        // Four bricks across and eight courses, in running bond, each brick of its own tint.
        Kind::Brick => {
            let (col, row, _, _, joint) = cells(64.0, 32.0, 3.0, true);
            if joint {
                scale([200.0, 196.0, 186.0], 0.94 + 0.1 * noise(1, u, v, 64, 64))
            } else {
                scale(color, (0.8 + 0.3 * hash(2, col, row)) * (0.93 + 0.14 * fbm(3, u, v, 32, 32, 3)))
            }
        }
        // Two blocks across and four courses, thin joints, a fine grain.
        Kind::Block => {
            let (col, row, _, _, joint) = cells(128.0, 64.0, 3.0, true);
            if joint {
                scale([176.0, 174.0, 168.0], 0.94 + 0.1 * noise(4, u, v, 64, 64))
            } else {
                scale(color, (0.92 + 0.12 * hash(5, col, row)) * (0.93 + 0.12 * fbm(6, u, v, 48, 48, 3)))
            }
        }
        // Eight boards across, a grain along each, a joint across it somewhere along the board.
        Kind::Boards => {
            let row = (y / 32.0).floor();
            let (ly, joint_at) = (y - row * 32.0, hash(7, row as i64, 0) * SIDE as f64);
            if ly < 1.5 {
                scale(color, 0.35)
            } else if (x - joint_at).abs() < 1.0 {
                scale(color, 0.45)
            } else {
                scale(color, (0.82 + 0.24 * hash(8, row as i64, 1)) * (0.8 + 0.35 * fbm(9 + row as u64, u, v, 2, 20, 3)))
            }
        }
        Kind::Concrete => {
            let pore = hash(10, x as i64, y as i64) > 0.997;
            scale(color, (0.9 + 0.14 * fbm(11, u, v, 12, 12, 4)) * if pore { 0.62 } else { 1.0 })
        }
        Kind::Plaster => scale(color, 0.96 + 0.08 * noise(12, u, v, 96, 96)),
        // Four tiles across and down, a grout between.
        Kind::Tile => {
            let (col, row, lx, _, grout) = cells(64.0, 64.0, 2.5, false);
            if grout { [186.0, 184.0, 178.0] } else { scale(color, (0.94 + 0.08 * hash(13, col, row)) * (1.0 - 0.04 * lx / 64.0)) }
        }
        // Four courses of four tiles, each darker toward the foot where the course below overlaps it.
        Kind::Roof => {
            let (col, row, lx, ly, _) = cells(64.0, 64.0, 0.0, true);
            let foot = 1.0 - 0.3 * (ly / 64.0).powi(2);
            let edge = if lx < 1.5 {
                0.55
            } else if ly < 3.0 {
                0.7
            } else {
                1.0
            };
            scale(color, (0.85 + 0.25 * hash(14, col, row)) * foot * edge)
        }
        // A plain weave: over and under in a checker of 4 px cells.
        Kind::Fabric => {
            let (cx, cy) = ((x / 4.0).floor(), (y / 4.0).floor());
            let (fx, fy) = (x / 4.0 - cx, y / 4.0 - cy);
            let thread = if (cx + cy) as i64 % 2 == 0 { 1.0 - (fy - 0.5).abs() * 2.0 } else { 1.0 - (fx - 0.5).abs() * 2.0 };
            scale(color, (0.78 + 0.3 * thread) * (0.97 + 0.06 * noise(15, u, v, 128, 128)))
        }
        // Brushed: fine streaks along it.
        Kind::Metal => scale(color, (0.84 + 0.2 * noise(16, u, v, 2, 160)) * (0.97 + 0.06 * hash(17, x as i64, y as i64))),
        Kind::Insulation => scale(color, (0.78 + 0.32 * fbm(18, u, v, 20, 20, 3)) * (0.94 + 0.12 * noise(19, u, v, 4, 90))),
        Kind::Image => [128.0; 3],
    }
}

/// A tile of a pattern drawn in code, over `color`, with its levels; the same one each time it's asked for.
fn pattern(kind: Kind, color: [u8; 3]) -> Arc<Vec<Level>> {
    type Drawn = Mutex<HashMap<(Kind, [u8; 3]), Arc<Vec<Level>>>>;
    static DRAWN: OnceLock<Drawn> = OnceLock::new();
    let drawn = DRAWN.get_or_init(Default::default);
    if let Ok(map) = drawn.lock()
        && let Some(hit) = map.get(&(kind, color))
    {
        return Arc::clone(hit);
    }
    let c = color.map(f64::from);
    let mut rgb = Vec::with_capacity(SIDE * SIDE * 3);
    for y in 0..SIDE {
        for x in 0..SIDE {
            rgb.extend(shade(kind, c, x as f64 + 0.5, y as f64 + 0.5).map(|k| k.round().clamp(0.0, 255.0) as u8));
        }
    }
    let made = Arc::new(chain(Level { w: SIDE, h: SIDE, rgb }));
    // Two that drew it at once keep the first's.
    match drawn.lock() {
        Ok(mut map) => {
            // A few hundred colours of them at most: more is somebody sweeping through the colours.
            if map.len() >= 256 {
                map.clear();
            }
            Arc::clone(map.entry((kind, color)).or_insert(made))
        }
        Err(_) => made,
    }
}

/// A sampler's parts: the tile's size in mm, its turn as `(sin, cos)` and its offset, and each level of its mip chain, the
/// finest first, as its width, its height and its rows of red, green and blue.
pub struct Plan<'a> {
    pub size: [f64; 2],
    pub turn: (f64, f64),
    pub offset: [f64; 2],
    /// Millimetres a pixel of the finest level covers.
    pub texel: f64,
    pub levels: Vec<(usize, usize, &'a [u8])>,
}

/// The level of a mip chain of `count` levels, whose finest pixel covers `texel` mm, that a pixel covering `foot` mm looks up in.
pub fn level_for(texel: f64, foot: f64, count: usize) -> usize {
    ((foot / texel).log2().max(0.0).round() as usize).min(count.saturating_sub(1))
}

/// A texture ready to look up: what colour a point `(u, v)` of a face's plane, in mm, shows.
#[derive(Clone)]
pub struct Sampler {
    levels: Arc<Vec<Level>>,
    size: [f64; 2],
    turn: (f64, f64),
    offset: [f64; 2],
    /// Millimetres a pixel of the finest level covers.
    texel: f64,
}

impl Sampler {
    /// `texture` over a material of `color`, of a picture `images` holds; none for one that can't be: a picture it hasn't.
    pub fn new(texture: &Texture, color: [u8; 3], images: &BTreeMap<String, Picture>) -> Option<Sampler> {
        let levels = match texture.kind {
            Kind::Image => Arc::clone(&images.get(&texture.image)?.levels),
            kind => pattern(kind, color),
        };
        let base = levels.first()?;
        let texel = (texture.size[0] / base.w as f64).max(texture.size[1] / base.h as f64);
        let (s, c) = texture.angle.to_radians().sin_cos();
        (texel.is_finite() && texel > 0.0).then_some(Sampler { levels, size: texture.size, turn: (s, c), offset: texture.offset, texel })
    }

    /// The colour at `(u, v)` mm over its face, for a pixel that covers `foot` mm of it: the coarser level of the picture
    /// where a pixel covers more than one of its own.
    pub fn at(&self, u: f64, v: f64, foot: f64) -> Rgb {
        let (x, y) = (u - self.offset[0], v - self.offset[1]);
        let (x, y) = (x * self.turn.1 + y * self.turn.0, y * self.turn.1 - x * self.turn.0);
        let (fu, fv) = (x / self.size[0], -y / self.size[1]);
        let (fu, fv) = (fu - fu.floor(), fv - fv.floor());
        let Some(l) = self.levels.get(self.level_for(foot)) else { return Rgb(128, 128, 128) };
        let [r, g, b] = l.bilinear(fu, fv);
        Rgb(r.clamp(0.0, 255.0) as u8, g.clamp(0.0, 255.0) as u8, b.clamp(0.0, 255.0) as u8)
    }

    /// The level of the mip chain a pixel that covers `foot` mm of the face looks up in.
    pub fn level_for(&self, foot: f64) -> usize {
        level_for(self.texel, foot, self.levels.len())
    }

    /// Everything a graphics card needs to look up the colours [`at`](Self::at) does.
    pub fn plan(&self) -> Plan<'_> {
        Plan {
            size: self.size,
            turn: self.turn,
            offset: self.offset,
            texel: self.texel,
            levels: self.levels.iter().map(|l| (l.w, l.h, l.rgb.as_slice())).collect(),
        }
    }

    /// One tile of it as `side` by `side` pixels of RGB, for a swatch.
    pub fn swatch(&self, side: usize) -> Vec<u8> {
        let side = side.max(1);
        let foot = self.size[0].max(self.size[1]) / side as f64;
        (0..side * side)
            .flat_map(|k| {
                let (x, y) = ((k % side) as f64 + 0.5, (k / side) as f64 + 0.5);
                let Rgb(r, g, b) = self.at(self.offset[0] + x * foot, self.offset[1] - y * foot, foot);
                [r, g, b]
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A JPEG of 16 by 8 pixels: red rising across it, green down it, blue falling across it.
    const JPEG: &[u8] = include_bytes!("../testdata/gradient.jpg");

    fn png(w: u32, h: u32, color: [u8; 4]) -> Vec<u8> {
        let rgba: Vec<u8> = (0..w * h).flat_map(|_| color).collect();
        tiny_skia::Pixmap::from_vec(rgba, tiny_skia::IntSize::from_wh(w, h).unwrap()).unwrap().encode_png().unwrap()
    }

    #[test]
    fn every_pattern_is_drawn_tiles_and_comes_in_its_own_colours() {
        let brown = [178, 92, 70];
        for kind in Kind::ALL.into_iter().filter(|k| *k != Kind::Image) {
            let levels = pattern(kind, brown);
            let base = &levels[0];
            assert_eq!((base.w, base.h, levels.len()), (SIDE, SIDE, 9), "{kind:?}: 256 down to one pixel");
            // It has something to it: its pixels aren't all one, and none is out of hand.
            let spread = base.rgb.iter().map(|b| i32::from(*b)).max().unwrap() - base.rgb.iter().map(|b| i32::from(*b)).min().unwrap();
            assert!(spread > 20, "{kind:?}: a spread of {spread}");
            // The tile meets the next: moved a whole tile along either way, what it draws is what it drew, but for the grain
            // of single pixels, which is a few levels at most against the spread of the whole.
            let c = [178.0, 92.0, 70.0];
            for (dx, dy) in [(SIDE as f64, 0.0), (0.0, SIDE as f64)] {
                let apart: f64 = (0..64)
                    .flat_map(|i| (0..64).map(move |j| (f64::from(i) * 4.0 + 0.5, f64::from(j) * 4.0 + 0.5)))
                    .map(|(x, y)| {
                        shade(kind, c, x, y).iter().zip(shade(kind, c, x + dx, y + dy)).map(|(a, b)| (a - b).abs()).sum::<f64>() / 3.0
                    })
                    .sum::<f64>()
                    / 4096.0;
                assert!(apart < f64::from(spread) / 4.0, "{kind:?}: {apart} between a tile and the next, against a spread of {spread}");
            }
            // The same one each time it's asked for, over another colour another.
            assert!(Arc::ptr_eq(&levels, &pattern(kind, brown)));
        }
        let (red, blue) = (pattern(Kind::Plaster, [200, 40, 40]), pattern(Kind::Plaster, [40, 40, 200]));
        let mean = |l: &Level, c: usize| l.rgb.iter().skip(c).step_by(3).map(|b| f64::from(*b)).sum::<f64>() / (SIDE * SIDE) as f64;
        assert!(mean(&red[0], 0) > mean(&red[0], 2) + 80.0 && mean(&blue[0], 2) > mean(&blue[0], 0) + 80.0);
    }

    #[test]
    fn a_sampler_lays_its_tile_at_its_size_turned_and_moved_and_a_pixel_wider_than_a_texel_reads_a_coarser_level() {
        let images = BTreeMap::new();
        let mut t = Texture::new(Kind::Brick);
        let plain = Sampler::new(&t, [178, 92, 70], &images).unwrap();
        let at = |s: &Sampler, u: f64, v: f64| s.at(u, v, 1.0);
        // Repeated across and up every 900 by 600 mm, however far along.
        for (du, dv) in [(900.0, 0.0), (0.0, 600.0), (-1800.0, 1200.0)] {
            assert_eq!(at(&plain, 123.0, 77.0), at(&plain, 123.0 + du, 77.0 + dv));
        }
        assert_ne!(at(&plain, 123.0, 77.0), at(&plain, 460.0, 77.0));
        // Moved by its offset, turned about its origin by its angle: a quarter turn takes what lay along u to lie up v.
        t.offset = [100.0, 0.0];
        let moved = Sampler::new(&t, [178, 92, 70], &images).unwrap();
        assert_eq!(at(&moved, 223.0, 77.0), at(&plain, 123.0, 77.0));
        t = Texture::new(Kind::Brick);
        t.size = [600.0, 600.0];
        let straight = Sampler::new(&t, [178, 92, 70], &images).unwrap();
        t.angle = 90.0;
        let turned = Sampler::new(&t, [178, 92, 70], &images).unwrap();
        let (a, b) = (at(&turned, 0.0, 250.0), at(&straight, 250.0, 0.0));
        assert!(a.0.abs_diff(b.0) <= 1 && a.1.abs_diff(b.1) <= 1 && a.2.abs_diff(b.2) <= 1, "{a:?} {b:?}");
        // A pixel the size of the whole tile reads its coarsest level: one colour, the tile's average.
        let wide = plain.at(10.0, 10.0, 1e6);
        let blurred = [(0.0, 0.0), (300.0, 200.0), (700.0, 450.0)].map(|(u, v)| plain.at(u, v, 1e6));
        assert!(blurred.iter().all(|c| *c == wide), "{blurred:?}");
        // And a swatch of it is one tile, as many pixels as asked for.
        assert_eq!(plain.swatch(32).len(), 32 * 32 * 3);
    }

    #[test]
    fn a_pictures_top_left_lies_at_the_origin_running_across_and_down_and_its_angle_turns_it_counter_clockwise() {
        // Red and green above, blue and yellow below, a texel each, in a tile of 200 by 200 mm.
        let (red, green, blue, yellow) = (Rgb(255, 0, 0), Rgb(0, 255, 0), Rgb(0, 0, 255), Rgb(255, 255, 0));
        let rgba: Vec<u8> = [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255], [255, 255, 0, 255]].concat();
        let png = tiny_skia::Pixmap::from_vec(rgba, tiny_skia::IntSize::from_wh(2, 2).unwrap()).unwrap().encode_png().unwrap();
        let picture = Picture::import("quad", &png).unwrap();
        let mut texture = Texture::new(Kind::Image);
        (texture.image, texture.size) = (picture.id(), [200.0, 200.0]);
        let images = BTreeMap::from([(picture.id(), picture)]);
        let colours = |t: &Texture, at: [(f64, f64); 4]| at.map(|(u, v)| Sampler::new(t, [0, 0, 0], &images).unwrap().at(u, v, 1.0));
        // Hanging from its origin: across the top row, then the bottom one below it.
        let hung = [(50.0, -50.0), (150.0, -50.0), (50.0, -150.0), (150.0, -150.0)];
        assert_eq!(colours(&texture, hung), [red, green, blue, yellow]);
        // Turned a quarter counter-clockwise about its origin, its top-left texel lies to the north-east of it: red there, green
        // further up, blue further across (to within a level, the turn being a sine and a cosine).
        texture.angle = 90.0;
        let turned = colours(&texture, [(50.0, 50.0), (50.0, 150.0), (150.0, 50.0), (150.0, 150.0)]);
        let near = |a: Rgb, b: Rgb| a.0.abs_diff(b.0) <= 2 && a.1.abs_diff(b.1) <= 2 && a.2.abs_diff(b.2) <= 2;
        assert!(turned.iter().zip([red, green, blue, yellow]).all(|(a, b)| near(*a, b)), "{turned:?}");
    }

    #[test]
    fn pictures_come_in_as_pngs_or_jpegs_are_kept_as_pngs_and_a_picture_names_its_texture() {
        let from_png = Picture::import("wall", &png(8, 4, [200, 100, 50, 255])).unwrap();
        assert_eq!((from_png.name(), from_png.size()), ("wall", (8, 4)));
        let from_jpeg = Picture::import("grad", JPEG).unwrap();
        assert_eq!(from_jpeg.size(), (16, 8));
        // What it holds: red rising across it, green down it (a JPEG is lossy, so to within a few levels).
        let (a, b) = (from_jpeg.levels[0].at(0, 0), from_jpeg.levels[0].at(15, 7));
        for (got, want) in a.iter().chain(&b).zip([0.0, 40.0, 200.0, 240.0, 180.0, 80.0]) {
            assert!((got - want).abs() < 12.0, "{a:?} {b:?}");
        }
        // Over white where a PNG lets it through: a pixel of 50 premultiplied and half clear shows 50 and the half of 255 under it.
        let clear = Picture::import("clear", &png(2, 2, [50, 50, 50, 128])).unwrap();
        assert!((clear.levels[0].at(0, 0)[0] - 177.0).abs() < 3.0, "{:?}", clear.levels[0].at(0, 0));
        // A picture goes in a project file as its PNG and comes back the same; its id is the same for the same bytes.
        let text = serde_json::to_string(&from_jpeg).unwrap();
        let back: Picture = serde_json::from_str(&text).unwrap();
        assert_eq!((&back, back.id()), (&from_jpeg, from_jpeg.id()));
        assert_ne!(from_jpeg.id(), from_png.id());
        // A larger one is halved down to the side it's kept at.
        let big = Picture::import("big", &png(2500, 1300, [9, 9, 9, 255])).unwrap();
        assert_eq!(big.size(), (625, 325));
        // A texture of a picture needs the picture; of a pattern, none.
        let images = BTreeMap::from([("img".to_string(), from_png)]);
        let mut t = Texture::new(Kind::Image);
        assert!(t.check(&images).unwrap_err().contains("which the project doesn't hold"));
        t.image = "img".into();
        assert_eq!(t.check(&images), Ok(()));
        assert!(Sampler::new(&t, [0; 3], &images).is_some() && Sampler::new(&t, [0; 3], &BTreeMap::new()).is_none());
        t.kind = Kind::Brick;
        assert!(t.check(&images).unwrap_err().contains("only a texture of a picture"));
    }

    #[test]
    fn hostile_pictures_and_textures_are_refused_and_never_crash_it() {
        let ok = png(4, 4, [1, 2, 3, 255]);
        // Not a picture, cut short, a size that isn't what it says, a header that claims more than may be held.
        let mut huge = ok.clone();
        huge[16..20].copy_from_slice(&60_000u32.to_be_bytes());
        huge[20..24].copy_from_slice(&60_000u32.to_be_bytes());
        let mut empty = ok.clone();
        empty[16..24].copy_from_slice(&[0; 8]);
        for (bytes, says) in [
            (b"GIF89a....".to_vec(), "neither"),
            (Vec::new(), "neither"),
            (ok[..12].to_vec(), "cut short"),
            (ok[..30].to_vec(), "PNG"),
            (huge, "pixels"),
            (empty, "pixels"),
            (vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 4, 0, 0], "JPEG"),
        ] {
            let e = Picture::import("x", &bytes).unwrap_err();
            assert!(e.contains(says), "{says}: {e}");
        }
        assert!(Picture::import("x", &vec![0u8; MAX_BYTES + 1]).unwrap_err().contains("bytes"));
        // A project file with a picture that isn't one.
        for text in [r#"{"name": "x", "png": "!!!"}"#, r#"{"name": "x", "png": "AAAA"}"#, r#"{"name": "x"}"#] {
            assert!(serde_json::from_str::<Picture>(text).is_err(), "{text}");
        }
        // A tile that's no size, a turn that's no number: said, and a sampler is never made of one.
        for (size, angle) in [([0.0, 10.0], 0.0), ([f64::NAN, 10.0], 0.0), ([10.0, 2e6], 0.0), ([10.0, 10.0], f64::INFINITY)] {
            let t = Texture { size, angle, ..Texture::new(Kind::Brick) };
            assert!(t.check(&BTreeMap::new()).is_err(), "{size:?} {angle}");
        }
        // Whatever point it's asked about, and however wide the pixel, it answers with a colour.
        for size in [[1e-300, 1e-300], [1e300, 1e300], [1.0, 1.0]] {
            let sampler = Sampler::new(&Texture { size, ..Texture::new(Kind::Brick) }, [0; 3], &BTreeMap::new());
            for (u, v, foot) in [(f64::NAN, f64::INFINITY, f64::NAN), (1e300, -1e300, f64::INFINITY), (0.0, 0.0, 0.0), (-1.0, 1e-300, -5.0)]
            {
                if let Some(s) = &sampler {
                    let _ = s.at(u, v, foot);
                }
            }
        }
    }

    #[test]
    fn base64_reads_what_the_replies_write() {
        for bytes in [vec![], vec![0], vec![0, 1], vec![0, 1, 2], (0..=255).collect::<Vec<u8>>()] {
            assert_eq!(unbase64(&crate::commands::base64(&bytes)), Some(bytes));
        }
        assert_eq!(unbase64("AB!D"), None);
    }
}
