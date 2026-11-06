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
/// How far a pixel may be from the mean, in each of its channels, for a picture to count as one colour: what compression leaves
/// of a flat one.
const FLAT_TOLERANCE: f64 = 6.0;
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
    /// A picture laid over the ground by where it is, once: `size` is the ground it covers, `offset` its west and north corner, and a
    /// point beyond it shows its edge. The terrain's aerial imagery is one. Not repeated, so not for a face that is not the ground.
    #[serde(default, skip_serializing_if = "is_false")]
    pub planar: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl Texture {
    /// A texture of `kind`, at its natural size.
    pub fn new(kind: Kind) -> Texture {
        Texture { kind, size: kind.natural(), angle: 0.0, offset: [0.0; 2], image: String::new(), planar: false }
    }

    /// Whether it's sound: a tile of a usable size, turned and moved by numbers, and of a picture `images` holds.
    pub fn check(&self, images: &BTreeMap<String, Picture>) -> Result<(), String> {
        // A tile that repeats is a few metres at most; a picture laid over the ground is as big as the ground.
        let longest = if self.planar { MAX_COORD } else { LONGEST };
        if !self.size.iter().all(|s| (SHORTEST..=longest).contains(s)) {
            return Err(format!("a tile of it is {} by {} mm; each side is {SHORTEST} to {longest} mm", self.size[0], self.size[1]));
        }
        if self.planar && self.kind != Kind::Image {
            return Err("only a picture can be laid over the ground (planar): a pattern drawn in code repeats".into());
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
    /// How much of each pixel is there, 255 all and 0 none, when any is not all there. `rgb` is over white either way.
    alpha: Option<Vec<u8>>,
}

impl Level {
    /// How much of the picture is there at a fraction `(fu, fv)` across and down it, 0 to 1, blended and wrapped as the colours
    /// are; 1 for a level that lets nothing through.
    fn bilinear_alpha(&self, fu: f64, fv: f64) -> f64 {
        let Some(plane) = &self.alpha else { return 1.0 };
        let (x, y) = (fu * self.w as f64 - 0.5, fv * self.h as f64 - 0.5);
        let (x0, y0) = (x.floor(), y.floor());
        let (tx, ty) = (x - x0, y - y0);
        let (x0, y0) = (x0 as i64, y0 as i64);
        let at = |x: i64, y: i64| {
            let (x, y) = (x.rem_euclid(self.w.max(1) as i64) as usize, y.rem_euclid(self.h.max(1) as i64) as usize);
            plane.get(y * self.w + x).map_or(1.0, |a| f64::from(*a))
        };
        let top = at(x0, y0) * (1.0 - tx) + at(x0.saturating_add(1), y0) * tx;
        let bottom = at(x0, y0.saturating_add(1)) * (1.0 - tx) + at(x0.saturating_add(1), y0.saturating_add(1)) * tx;
        ((top * (1.0 - ty) + bottom * ty) / 255.0).clamp(0.0, 1.0)
    }

    /// The same pixels with `plane` for what is there (a byte for each pixel; ignored if it is not the size).
    fn with_alpha(mut self, plane: Option<&[u8]>) -> Level {
        self.alpha = plane.filter(|p| p.len() == self.w * self.h).map(<[u8]>::to_vec);
        self
    }

    /// The pixel at `(x, y)`, wrapped round the picture.
    fn at(&self, x: i64, y: i64) -> [f64; 3] {
        let (x, y) = (x.rem_euclid(self.w.max(1) as i64) as usize, y.rem_euclid(self.h.max(1) as i64) as usize);
        match self.rgb.get((y * self.w + x) * 3..(y * self.w + x) * 3 + 3) {
            Some(&[r, g, b]) => [f64::from(r), f64::from(g), f64::from(b)],
            _ => [128.0; 3],
        }
    }

    /// The bytes of pixel row `y` (held to the first and last), if it has any.
    fn row(&self, y: i64) -> Option<&[u8]> {
        let y = y.clamp(0, self.h.max(1) as i64 - 1) as usize;
        self.rgb.get(y * self.w * 3..(y + 1) * self.w * 3)
    }

    /// Pixel column `x` of `w`, held to the first and last.
    fn column(x: i64, w: usize) -> usize {
        x.clamp(0, w.max(1) as i64 - 1) as usize
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
        let mut alpha = self.alpha.as_ref().map(|_| Vec::with_capacity(w * h));
        for y in 0..h {
            for x in 0..w {
                if let (Some(out), Some(plane)) = (&mut alpha, &self.alpha) {
                    let sum: u32 = [(0, 0), (1, 0), (0, 1), (1, 1)]
                        .iter()
                        .map(|(dx, dy)| {
                            let (px, py) = ((2 * x + dx) % self.w.max(1), (2 * y + dy) % self.h.max(1));
                            plane.get(py * self.w + px).map_or(255, |a| u32::from(*a))
                        })
                        .sum();
                    out.push(((f64::from(sum)) / 4.0).round() as u8);
                }
                let sum = [(0, 0), (1, 0), (0, 1), (1, 1)].iter().fold([0.0; 3], |s, (dx, dy)| {
                    let p = self.at((2 * x + dx) as i64, (2 * y + dy) as i64);
                    [s[0] + p[0], s[1] + p[1], s[2] + p[2]]
                });
                rgb.extend(sum.map(|v| (v / 4.0).round() as u8));
            }
        }
        Level { w, h, rgb, alpha }
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
    /// What its PNG lets through: a byte for each pixel of the finest level, 255 all there and 0 none, when any pixel is not all
    /// there. The levels are over white (3D shows white where it lets through); a plan leaves it clear.
    alpha: Option<Arc<Vec<u8>>>,
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
        let alpha = (base.w.max(base.h) <= MAX_SIDE).then(|| clear_pixmap(&bytes)).flatten().map(|pm| alpha_plane(&pm));
        let base = shrunk(base).with_alpha(alpha.as_deref());
        Ok(Picture { name: f.name, png: Arc::new(bytes), levels: Arc::new(chain(base)), alpha: alpha.map(Arc::new) })
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
        let decoded = decode(bytes)?;
        // What a PNG lets through is kept (a plan leaves it clear), when the picture needs no shrinking: a shrunk one is over white.
        let clear = (decoded.w.max(decoded.h) <= MAX_SIDE).then(|| clear_pixmap(bytes)).flatten();
        let base = shrunk(decoded);
        let (png, alpha) = match clear {
            Some(pixmap) => (pixmap.encode_png().map_err(|e| e.to_string())?, Some(Arc::new(alpha_plane(&pixmap)))),
            None => {
                let rgba: Vec<u8> = base.rgb.as_chunks::<3>().0.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect();
                let size = tiny_skia::IntSize::from_wh(base.w as u32, base.h as u32).ok_or("it has no size")?;
                (tiny_skia::Pixmap::from_vec(rgba, size).ok_or("it has no size")?.encode_png().map_err(|e| e.to_string())?, None)
            }
        };
        let base = base.with_alpha(alpha.as_ref().map(|a| a.as_slice()));
        Ok(Picture { name: name.chars().take(MAX_TEXT).collect(), png: Arc::new(png), levels: Arc::new(chain(base)), alpha })
    }

    /// What the picture lets through, a byte for each pixel of its finest level (255 all there, 0 none), row after row; none if it lets
    /// nothing through.
    pub fn alpha(&self) -> Option<&[u8]> {
        self.alpha.as_deref().map(Vec::as_slice)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Its width and height, px.
    pub fn size(&self) -> (usize, usize) {
        self.levels.first().map_or((0, 0), |l| (l.w, l.h))
    }

    /// The colour every pixel has, give or take [`FLAT_TOLERANCE`], if there is one. A map service's tile for a place it has no
    /// imagery of is often one flat colour, and is not imagery.
    pub fn flat(&self) -> Option<[u8; 3]> {
        flat_rgb(&self.levels.first()?.rgb)
    }

    /// An id for it among a project's pictures: the same for the same bytes, so the same picture comes in once.
    pub fn id(&self) -> String {
        let hash = self.png.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3));
        format!("img-{hash:016x}")
    }
}

/// The pixels of `bytes`, a PNG `decode` has already passed, if some of them let the paper through.
fn clear_pixmap(bytes: &[u8]) -> Option<tiny_skia::Pixmap> {
    if !bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return None;
    }
    let pixmap = tiny_skia::Pixmap::decode_png(bytes).ok()?;
    pixmap.data().as_chunks::<4>().0.iter().any(|p| p[3] != 255).then_some(pixmap)
}

/// A byte for each pixel of `pixmap`: how much of it is there.
fn alpha_plane(pixmap: &tiny_skia::Pixmap) -> Vec<u8> {
    pixmap.data().as_chunks::<4>().0.iter().map(|p| p[3]).collect()
}

/// `level` halved down until neither side is longer than [`MAX_SIDE`].
fn shrunk(mut level: Level) -> Level {
    while level.w.max(level.h) > MAX_SIDE {
        level = level.halved();
    }
    level
}

/// The width, height and pixels (RGB, row after row) of a PNG or a JPEG, for what is fetched and is not a project's picture. The
/// header is checked before anything of its size is made, as for [`Picture::import`].
pub fn decode_rgb(bytes: &[u8]) -> Result<(usize, usize, Vec<u8>), String> {
    let level = decode(bytes)?;
    Ok((level.w, level.h, level.rgb))
}

/// The colour every pixel of `rgb` (three bytes each) has, give or take [`FLAT_TOLERANCE`], if there is one.
pub fn flat_rgb(rgb: &[u8]) -> Option<[u8; 3]> {
    let pixels = rgb.as_chunks::<3>().0;
    if pixels.is_empty() {
        return None;
    }
    let n = pixels.len() as f64;
    let mean: [f64; 3] = std::array::from_fn(|k| pixels.iter().map(|p| f64::from(p[k])).sum::<f64>() / n);
    let one = pixels.iter().all(|p| (0..3).all(|k| (f64::from(p[k]) - mean[k]).abs() <= FLAT_TOLERANCE));
    one.then(|| mean.map(|m| m.round() as u8))
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
        Ok(Level { w: pixmap.width() as usize, h: pixmap.height() as usize, rgb, alpha: None })
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
        Ok(Level { w, h, rgb, alpha: None })
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
    let made = Arc::new(chain(Level { w: SIDE, h: SIDE, rgb, alpha: None }));
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

/// A level of a [`Plan`]: its width, its height, its rows of red, green and blue (over white) and how much of each pixel is there, if
/// any is not all there.
pub type PlanLevel<'a> = (usize, usize, &'a [u8], Option<&'a [u8]>);

/// A sampler's parts: the tile's size in mm, its turn as `(sin, cos)` and its offset, and each level of its mip chain, the
/// finest first, as its width, its height and its rows of red, green and blue.
pub struct Plan<'a> {
    pub size: [f64; 2],
    pub turn: (f64, f64),
    pub offset: [f64; 2],
    /// Millimetres a pixel of the finest level covers.
    pub texel: f64,
    /// Laid over the ground by where it is, by the ground's x and y, once ([`Texture::planar`]).
    pub planar: bool,
    /// The material's colour, which shows through where a level lets the picture through.
    pub color: [u8; 3],
    /// Each level's width, height, rows of red, green and blue (over white) and how much is there of each pixel, if any is not all.
    pub levels: Vec<PlanLevel<'a>>,
}

/// The level of a mip chain of `count` levels, whose finest pixel covers `texel` mm, that a pixel covering `foot` mm looks up in.
pub fn level_for(texel: f64, foot: f64, count: usize) -> usize {
    ((foot / texel).log2().max(0.0).round() as usize).min(count.saturating_sub(1))
}

/// A texture ready to look up: what colour a point `(u, v)` of a face's plane, in mm, shows.
#[derive(Clone)]
pub struct Sampler {
    levels: Arc<Vec<Level>>,
    /// What the picture lets through, if it does (see [`Picture::alpha`]).
    alpha: Option<Arc<Vec<u8>>>,
    size: [f64; 2],
    turn: (f64, f64),
    offset: [f64; 2],
    /// Millimetres a pixel of the finest level covers.
    texel: f64,
    planar: bool,
    /// The material's colour, which shows where the picture lets it through.
    color: [u8; 3],
}

impl Sampler {
    /// `texture` over a material of `color`, of a picture `images` holds; none for one that can't be: a picture it hasn't.
    pub fn new(texture: &Texture, color: [u8; 3], images: &BTreeMap<String, Picture>) -> Option<Sampler> {
        let (levels, alpha) = match texture.kind {
            Kind::Image => {
                let picture = images.get(&texture.image)?;
                (Arc::clone(&picture.levels), picture.alpha.clone())
            }
            kind => (pattern(kind, color), None),
        };
        let base = levels.first()?;
        let texel = (texture.size[0] / base.w as f64).max(texture.size[1] / base.h as f64);
        let (s, c) = texture.angle.to_radians().sin_cos();
        (texel.is_finite() && texel > 0.0).then_some(Sampler {
            levels,
            alpha,
            size: texture.size,
            turn: (s, c),
            offset: texture.offset,
            texel,
            planar: texture.planar,
            color,
        })
    }

    /// Whether it is laid over the ground by where it is: a face is then looked up by its x and y, not across and up its own plane.
    pub fn planar(&self) -> bool {
        self.planar
    }

    /// The colour at `(u, v)` mm over its face, for a pixel that covers `foot` mm of it: the coarser level of the picture
    /// where a pixel covers more than one of its own.
    pub fn at(&self, u: f64, v: f64, foot: f64) -> Rgb {
        let (Rgb(r, g, b), a) = self.over_white(u, v, foot);
        if a >= 1.0 {
            return Rgb(r, g, b);
        }
        // Over white it is `c * a + 255 * (1 - a)`: over the material's colour `m`, that less `(1 - a) * (255 - m)`.
        let over = |c: u8, m: u8| (f64::from(c) - (1.0 - a) * f64::from(255 - m)).clamp(0.0, 255.0) as u8;
        Rgb(over(r, self.color[0]), over(g, self.color[1]), over(b, self.color[2]))
    }

    /// [`at`](Self::at) with the clear parts over white, and how much of the picture is there at the point (1 where nothing is clear).
    fn over_white(&self, u: f64, v: f64, foot: f64) -> (Rgb, f64) {
        let (x, y) = (u - self.offset[0], v - self.offset[1]);
        let (x, y) = (x * self.turn.1 + y * self.turn.0, y * self.turn.1 - x * self.turn.0);
        let (fu, fv) = (x / self.size[0], -y / self.size[1]);
        let Some(l) = self.levels.get(self.level_for(foot)) else { return (Rgb(128, 128, 128), 1.0) };
        let (fu, fv) = if self.planar {
            // Beyond the picture its edge goes on: held half a pixel in, where the blend of the four pixels round a point has none
            // from the far side.
            let (half_u, half_v) = (0.5 / l.w as f64, 0.5 / l.h as f64);
            (fu.clamp(half_u, 1.0 - half_u), fv.clamp(half_v, 1.0 - half_v))
        } else {
            (fu - fu.floor(), fv - fv.floor())
        };
        let [r, g, b] = l.bilinear(fu, fv);
        (Rgb(r.clamp(0.0, 255.0) as u8, g.clamp(0.0, 255.0) as u8, b.clamp(0.0, 255.0) as u8), l.bilinear_alpha(fu, fv))
    }

    /// How much of the picture is there at `(u, v)` mm over its face, 0 to 1: 1 where it lets nothing through, and everywhere for a
    /// picture that never does; blended between the four pixels round the point, held at the edge as the colours are.
    pub fn coverage(&self, u: f64, v: f64) -> f64 {
        let (Some(alpha), Some(base)) = (&self.alpha, self.levels.first()) else { return 1.0 };
        let (x, y) = (u - self.offset[0], v - self.offset[1]);
        let (x, y) = (x * self.turn.1 + y * self.turn.0, y * self.turn.1 - x * self.turn.0);
        let (fu, fv) = (x / self.size[0], -y / self.size[1]);
        let (w, h) = (base.w.max(1) as i64, base.h.max(1) as i64);
        let (fu, fv) = if self.planar {
            let (half_u, half_v) = (0.5 / w as f64, 0.5 / h as f64);
            (fu.clamp(half_u, 1.0 - half_u), fv.clamp(half_v, 1.0 - half_v))
        } else {
            (fu - fu.floor(), fv - fv.floor())
        };
        let (px, py) = (fu * w as f64 - 0.5, fv * h as f64 - 0.5);
        let (x0, y0) = (px.floor(), py.floor());
        let (tx, ty) = (px - x0, py - y0);
        let (x0, y0) = (x0 as i64, y0 as i64);
        let at = |x: i64, y: i64| {
            let (x, y) = (
                if self.planar { x.clamp(0, w - 1) } else { x.rem_euclid(w) },
                if self.planar { y.clamp(0, h - 1) } else { y.rem_euclid(h) },
            );
            alpha.get((y * w + x) as usize).map_or(1.0, |a| f64::from(*a) / 255.0)
        };
        let top = at(x0, y0) * (1.0 - tx) + at(x0 + 1, y0) * tx;
        let bottom = at(x0, y0 + 1) * (1.0 - tx) + at(x0 + 1, y0 + 1) * tx;
        (top * (1.0 - ty) + bottom * ty).clamp(0.0, 1.0)
    }

    /// A row of points `dx` mm apart along x, the first at `(x, y)`, for pixels that cover `foot` mm: `out[k]` is the colour there and how
    /// much of it is there, `[r, g, b, coverage]` with coverage 0 to 255. Each is what [`at`](Self::at) and [`coverage`](Self::coverage)
    /// give for its point; a picture laid over the ground and not turned (what a plan draws under its terrain) is done a row at a time
    /// (the level and the two rows of the picture it blends are found once), the rest point by point.
    pub fn row(&self, x: f64, dx: f64, y: f64, foot: f64, out: &mut [[u8; 4]]) {
        if !self.planar || self.turn != (0.0, 1.0) {
            for (k, px) in out.iter_mut().enumerate() {
                let p = x + k as f64 * dx;
                let (Rgb(r, g, b), _) = self.over_white(p, y, foot);
                *px = [r, g, b, (self.coverage(p, y) * 255.0).round().clamp(0.0, 255.0) as u8];
            }
            return;
        }
        let Some(level) = self.levels.get(self.level_for(foot)) else {
            out.fill([128, 128, 128, 255]);
            return;
        };
        // Down the picture: the two pixel rows blended, and how far between them.
        let across = |fraction: f64, n: usize| {
            let half = 0.5 / n.max(1) as f64;
            let held = fraction.clamp(half, 1.0 - half) * n as f64 - 0.5;
            let low = held.floor();
            (low as i64, held - low)
        };
        // Blended with eighths of 1/256, in whole numbers: the same as the point-by-point lookup to a unit of colour.
        let weight = |t: f64| (t * 256.0).round().clamp(0.0, 256.0) as u32;
        let down = -(y - self.offset[1]) / self.size[1];
        let (row_a, ty) = across(down, level.h);
        let wy = weight(ty);
        let (rows_a, rows_b) = (level.row(row_a).unwrap_or_default(), level.row(row_a + 1).unwrap_or_default());
        // What it lets through is blended on the finest level, whatever level the colours come from.
        let alpha = self.alpha.as_deref().zip(self.levels.first()).map(|(plane, base)| {
            let (low, ty) = across(down, base.h);
            let last = base.h.max(1) as i64 - 1;
            let row = |r: i64| plane.get(r.clamp(0, last) as usize * base.w..(r.clamp(0, last) as usize + 1) * base.w).unwrap_or_default();
            (row(low), row(low + 1), base.w, weight(ty))
        });
        for (k, px) in out.iter_mut().enumerate() {
            let fu = (x + k as f64 * dx - self.offset[0]) / self.size[0];
            let (x0, tx) = across(fu, level.w);
            let (wx, left, right) = (weight(tx), Level::column(x0, level.w) * 3, Level::column(x0 + 1, level.w) * 3);
            let blend = |a: u8, b: u8, c: u8, d: u8| {
                let top = u32::from(a) * (256 - wx) + u32::from(b) * wx;
                let bottom = u32::from(c) * (256 - wx) + u32::from(d) * wx;
                ((top * (256 - wy) + bottom * wy + 32768) >> 16) as u8
            };
            let mut pixel = [128, 128, 128, 255];
            if let (Some([a0, a1, a2]), Some([b0, b1, b2]), Some([c0, c1, c2]), Some([d0, d1, d2])) =
                (rows_a.get(left..left + 3), rows_a.get(right..right + 3), rows_b.get(left..left + 3), rows_b.get(right..right + 3))
            {
                pixel = [blend(*a0, *b0, *c0, *d0), blend(*a1, *b1, *c1, *d1), blend(*a2, *b2, *c2, *d2), 255];
            }
            if let Some((above, below, w, wy)) = &alpha {
                let (x0, tx) = across(fu, *w);
                let (wx, left, right) = (weight(tx), Level::column(x0, *w), Level::column(x0 + 1, *w));
                if let (Some(a), Some(b), Some(c), Some(d)) = (above.get(left), above.get(right), below.get(left), below.get(right)) {
                    let top = u32::from(*a) * (256 - wx) + u32::from(*b) * wx;
                    let bottom = u32::from(*c) * (256 - wx) + u32::from(*d) * wx;
                    pixel[3] = ((top * (256 - wy) + bottom * wy + 32768) >> 16) as u8;
                }
            }
            *px = pixel;
        }
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
            planar: self.planar,
            color: self.color,
            levels: self.levels.iter().map(|l| (l.w, l.h, l.rgb.as_slice(), l.alpha.as_deref())).collect(),
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
    fn a_picture_of_one_colour_says_which_and_one_of_many_does_not() {
        let pixmap =
            |rgba: Vec<u8>| tiny_skia::Pixmap::from_vec(rgba, tiny_skia::IntSize::from_wh(32, 32).unwrap()).unwrap().encode_png().unwrap();
        let flat = Picture::import("flat", &png(32, 32, [200, 100, 50, 255])).unwrap();
        assert_eq!(flat.flat(), Some([200, 100, 50]));
        assert_eq!(Picture::import("gradient", JPEG).unwrap().flat(), None);
        // A little noise round one colour is still one colour: what a JPEG of a flat tile comes back as.
        let noisy = pixmap((0..32 * 32).flat_map(|i| if i % 2 == 0 { [100, 100, 100, 255] } else { [104, 98, 101, 255] }).collect());
        assert_eq!(Picture::import("noisy", &noisy).unwrap().flat(), Some([102, 99, 101]));
        // One pixel in a thousand far from the rest is not, and neither are two plain colours.
        let mut spot: Vec<u8> = (0..32 * 32).flat_map(|_| [100, 100, 100, 255]).collect();
        spot[500 * 4..500 * 4 + 3].copy_from_slice(&[250, 100, 100]);
        assert_eq!(Picture::import("spot", &pixmap(spot)).unwrap().flat(), None);
        let halves = pixmap((0..32 * 32).flat_map(|i| if i % 32 < 16 { [255, 0, 0, 255] } else { [0, 0, 255, 255] }).collect());
        assert_eq!(Picture::import("halves", &halves).unwrap().flat(), None);
    }

    /// A picture of 4 by 2 pixels laid over the 4 m by 2 m ground whose west and north corner is at x = 0, y = 2000: opaque red in the
    /// west half, and nothing at all in the east half.
    fn half_clear() -> (Picture, Sampler) {
        let rgba: Vec<u8> = (0..8).flat_map(|k| if k % 4 < 2 { [255, 0, 0, 255] } else { [0, 0, 0, 0] }).collect();
        let png = tiny_skia::Pixmap::from_vec(rgba, tiny_skia::IntSize::from_wh(4, 2).unwrap()).unwrap().encode_png().unwrap();
        let picture = Picture::import("half clear", &png).unwrap();
        let id = picture.id();
        let images = BTreeMap::from([(id.clone(), picture.clone())]);
        let texture = Texture { kind: Kind::Image, size: [4000.0, 2000.0], angle: 0.0, offset: [0.0, 2000.0], image: id, planar: true };
        (picture, Sampler::new(&texture, [128, 128, 128], &images).unwrap())
    }

    #[test]
    fn a_picture_keeps_what_its_png_lets_through_so_a_plan_can_leave_it_clear_and_3d_shows_the_materials_colour_there() {
        let (picture, over) = half_clear();
        assert_eq!(picture.alpha().map(<[u8]>::len), Some(8), "a plane of alpha, a byte for a pixel");
        // The ground it covers: 1 = all there, 0 = none, half way across the edge between the two.
        let at = |x: f64| over.coverage(x, 1000.0);
        assert_eq!((at(500.0), at(3500.0)), (1.0, 0.0));
        assert!((at(2000.0) - 0.5).abs() < 1e-9, "half way, blended: {}", at(2000.0));
        // Beyond its edge the edge goes on (held, as the colours are).
        assert_eq!((over.coverage(-5000.0, 1000.0), over.coverage(9000.0, 1000.0)), (1.0, 0.0));
        // 3D shows the material's own colour where it lets the paper through, the photograph where it is there.
        assert_eq!(over.at(3500.0, 1000.0, 10.0), Rgb(128, 128, 128));
        assert_eq!(over.at(500.0, 1000.0, 10.0), Rgb(255, 0, 0));
        // Half way across the edge, the two blended.
        let Rgb(r, g, _) = over.at(2000.0, 1000.0, 10.0);
        assert!((i32::from(r) - 191).abs() <= 1 && (i32::from(g) - 64).abs() <= 1, "{r} {g}");
        // It goes into a project file and comes out whole: the PNG is kept with its alpha, and the plane is read back from it.
        let text = serde_json::to_string(&picture).unwrap();
        let back: Picture = serde_json::from_str(&text).unwrap();
        assert_eq!((back.alpha(), back == picture), (picture.alpha(), true));
        assert_eq!(back.id(), picture.id());
    }

    #[test]
    fn every_level_of_the_chain_has_its_own_alpha_and_a_picture_without_any_has_none() {
        let (picture, _) = half_clear();
        let alphas: Vec<Option<Vec<u8>>> = picture.levels.iter().map(|l| l.alpha.clone()).collect();
        assert_eq!(alphas.len(), 3, "4 x 2, 2 x 1, 1 x 1");
        assert_eq!(alphas[0].as_deref(), picture.alpha());
        // Two pixels of four clear one level down is half clear, and so on.
        assert_eq!(alphas[1].as_deref(), Some(&[255, 0][..]));
        assert_eq!(alphas[2].as_deref(), Some(&[128][..]));
        let opaque = Picture::import("opaque", &png(4, 4, [1, 2, 3, 255])).unwrap();
        assert!(opaque.levels.iter().all(|l| l.alpha.is_none()));
        assert!(Picture::import("jpeg", JPEG).unwrap().levels.iter().all(|l| l.alpha.is_none()));
        // A file keeps them.
        let back: Picture = serde_json::from_str(&serde_json::to_string(&picture).unwrap()).unwrap();
        assert!(back.levels.iter().zip(picture.levels.iter()).all(|(a, b)| a.alpha == b.alpha));
    }

    #[test]
    fn a_coarse_level_over_the_clear_part_is_the_materials_colour_too() {
        let (_, over) = half_clear();
        // A pixel covering the whole picture looks in the last level, half clear: half red, half the grey.
        let Rgb(r, g, _) = over.at(2000.0, 1000.0, 1e6);
        assert!((i32::from(r) - 191).abs() <= 1 && (i32::from(g) - 64).abs() <= 1, "{r} {g}");
        // And the plan reads the colours as ever, over white, with its coverage beside.
        let mut row = [[0u8; 4]; 1];
        over.row(3500.0, 1.0, 1000.0, 10.0, &mut row);
        assert_eq!(row[0], [255, 255, 255, 0]);
    }

    #[test]
    fn the_card_gets_the_alpha_of_each_level_and_the_materials_colour() {
        let (_, over) = half_clear();
        let plan = over.plan();
        assert_eq!(plan.color, [128, 128, 128]);
        assert_eq!(plan.levels.len(), 3);
        assert_eq!(plan.levels[0].3, Some(&[255, 255, 0, 0, 255, 255, 0, 0][..]));
        let opaque = Picture::import("opaque", &png(2, 2, [1, 2, 3, 255])).unwrap();
        let id = opaque.id();
        let images = BTreeMap::from([(id.clone(), opaque)]);
        let texture = Texture { kind: Kind::Image, size: [1000.0, 1000.0], angle: 0.0, offset: [0.0, 0.0], image: id, planar: true };
        let sampler = Sampler::new(&texture, [9, 9, 9], &images).unwrap();
        assert!(sampler.plan().levels.iter().all(|l| l.3.is_none()));
    }

    /// A picture of 16 by 12 pixels whose colours change everywhere and whose east third lets some through.
    fn busy() -> Picture {
        let rgba: Vec<u8> = (0..16 * 12)
            .flat_map(|k| {
                let (x, y) = (k % 16, k / 16);
                let a = if x >= 11 { 255 - (y * 20) as u8 } else { 255 };
                // Premultiplied: a colour no brighter than its alpha.
                let c = |v: u32| (v * u32::from(a) / 255) as u8;
                [c(x * 16), c(y * 20), c((x + y) * 8), a]
            })
            .collect();
        let png = tiny_skia::Pixmap::from_vec(rgba, tiny_skia::IntSize::from_wh(16, 12).unwrap()).unwrap().encode_png().unwrap();
        Picture::import("busy", &png).unwrap()
    }

    fn over(picture: &Picture, angle: f64, planar: bool) -> Sampler {
        let images = BTreeMap::from([(picture.id(), picture.clone())]);
        let texture =
            Texture { kind: Kind::Image, size: [16000.0, 12000.0], angle, offset: [2000.0, 15000.0], image: picture.id(), planar };
        Sampler::new(&texture, [128, 128, 128], &images).unwrap()
    }

    #[test]
    fn a_row_is_what_looking_up_each_of_its_points_gives_at_every_zoom_and_beyond_the_edges() {
        let picture = busy();
        for (what, sampler) in [
            ("laid over the ground", over(&picture, 0.0, true)),
            ("turned", over(&picture, 30.0, true)),
            ("across a face", over(&picture, 0.0, false)),
        ] {
            // Pixels of 5 mm (finer than the picture's), 1 m (about its own), and 40 m (many to a pixel of the coarser levels).
            for foot in [5.0, 1000.0, 3000.0, 40_000.0] {
                // Rows well inside the picture, along its edge, and beyond it; from before its west edge to after its east one.
                for y in [14_000.0, 9_000.0, 3_500.0, 3_000.0, 15_000.0, -2_000.0, 40_000.0] {
                    let (x0, dx) = (-3000.0, 137.0);
                    let mut row = vec![[0u8; 4]; 160];
                    sampler.row(x0, dx, y, foot, &mut row);
                    for (k, got) in row.iter().enumerate() {
                        let x = x0 + k as f64 * dx;
                        let (Rgb(r, g, b), _) = sampler.over_white(x, y, foot);
                        let want = [r, g, b, (sampler.coverage(x, y) * 255.0).round() as u8];
                        assert!(
                            got.iter().zip(want).all(|(a, b)| a.abs_diff(b) <= 1),
                            "{what}, a pixel of {foot} mm, row {y}, point {k} at x = {x}: {got:?} against {want:?}"
                        );
                    }
                }
            }
        }
        // A row of nothing, and a row of no pixels, are no trouble.
        over(&picture, 0.0, true).row(0.0, 1.0, 0.0, 1.0, &mut []);
    }

    #[test]
    fn a_row_of_hostile_numbers_is_filled_and_never_panics() {
        let picture = busy();
        let sampler = over(&picture, 0.0, true);
        for (x, dx, y, foot) in [
            (f64::NAN, 1.0, 0.0, 1.0),
            (0.0, f64::NAN, 0.0, 1.0),
            (0.0, f64::INFINITY, f64::NEG_INFINITY, 1.0),
            (1e300, 1e300, 1e300, 1e300),
            (-1e300, -1e300, 0.0, 0.0),
            (0.0, 1.0, 0.0, f64::NAN),
            (0.0, 0.0, 0.0, -1.0),
        ] {
            let mut row = vec![[7u8; 4]; 33];
            sampler.row(x, dx, y, foot, &mut row);
        }
    }

    #[test]
    fn a_picture_that_lets_nothing_through_has_no_alpha_and_is_kept_as_it_always_was() {
        for (what, picture) in [
            ("an opaque png", Picture::import("opaque", &png(8, 4, [10, 20, 30, 255])).unwrap()),
            ("a jpeg", Picture::import("jpeg", JPEG).unwrap()),
        ] {
            assert!(picture.alpha().is_none(), "{what}");
            let images = BTreeMap::from([(picture.id(), picture.clone())]);
            let texture =
                Texture { kind: Kind::Image, size: [1000.0, 1000.0], angle: 0.0, offset: [0.0, 1000.0], image: picture.id(), planar: true };
            let over = Sampler::new(&texture, [0, 0, 0], &images).unwrap();
            assert_eq!(over.coverage(500.0, 500.0), 1.0, "{what}: all there");
        }
        // The bytes kept for an opaque png are what they were before pictures kept alpha: the same png re-encoded opaque, so ids stay.
        let opaque = Picture::import("opaque", &png(8, 4, [10, 20, 30, 255])).unwrap();
        assert_eq!(*opaque.png, png(8, 4, [10, 20, 30, 255]));
    }

    #[test]
    fn a_picture_that_has_to_be_shrunk_loses_its_alpha_as_it_always_did() {
        // Twice as wide as a picture may be, with a clear end: it is halved, over white, and the plane would not fit it.
        let wide = MAX_SIDE as u32 * 2;
        let rgba: Vec<u8> = (0..wide * 2).flat_map(|k| if k % wide < wide / 2 { [9, 9, 9, 255] } else { [0, 0, 0, 0] }).collect();
        let png = tiny_skia::Pixmap::from_vec(rgba, tiny_skia::IntSize::from_wh(wide, 2).unwrap()).unwrap().encode_png().unwrap();
        let picture = Picture::import("wide", &png).unwrap();
        assert_eq!(picture.size().0, MAX_SIDE);
        assert!(picture.alpha().is_none());
    }

    /// A picture of 2 by 2 pixels: red and blue above, green and yellow under.
    fn quarters() -> Picture {
        let rgba: Vec<u8> = [[255, 0, 0, 255], [0, 0, 255, 255], [0, 255, 0, 255], [255, 255, 0, 255]].concat();
        let png = tiny_skia::Pixmap::from_vec(rgba, tiny_skia::IntSize::from_wh(2, 2).unwrap()).unwrap().encode_png().unwrap();
        Picture::import("quarters", &png).unwrap()
    }

    /// The picture as a texture over the 2 m square whose west and north corner is at x = 0, y = 2000.
    fn over_the_ground(planar: bool) -> Sampler {
        let picture = quarters();
        let id = picture.id();
        let images = BTreeMap::from([(id.clone(), picture)]);
        let texture = Texture { kind: Kind::Image, size: [2000.0, 2000.0], angle: 0.0, offset: [0.0, 2000.0], image: id, planar };
        Sampler::new(&texture, [128, 128, 128], &images).unwrap()
    }

    #[test]
    fn a_planar_texture_lies_over_its_ground_once_and_its_edge_goes_on_beyond_it() {
        let s = over_the_ground(true);
        let (red, blue, green, yellow) = (Rgb(255, 0, 0), Rgb(0, 0, 255), Rgb(0, 255, 0), Rgb(255, 255, 0));
        // North is up the picture and east across it.
        assert_eq!([s.at(500.0, 1500.0, 1.0), s.at(1500.0, 1500.0, 1.0)], [red, blue], "the north half");
        assert_eq!([s.at(500.0, 500.0, 1.0), s.at(1500.0, 500.0, 1.0)], [green, yellow], "the south half");
        // Beyond it, its edge goes on: nothing is repeated, and the far side does not bleed in.
        assert_eq!([s.at(-9000.0, 1500.0, 1.0), s.at(9000.0, 1500.0, 1.0)], [red, blue]);
        assert_eq!([s.at(500.0, 9000.0, 1.0), s.at(500.0, -9000.0, 1.0)], [red, green]);
        // And at its edge it is the picture's own colour, not a blend with the other side.
        assert_eq!([s.at(0.0, 1500.0, 1.0), s.at(2000.0, 1500.0, 1.0)], [red, blue]);
        assert_eq!([s.at(500.0, 2000.0, 1.0), s.at(500.0, 0.0, 1.0)], [red, green]);
        // A texture that is not planar repeats, as it always did.
        let wrapped = over_the_ground(false);
        assert_eq!([wrapped.at(-500.0, 1500.0, 1.0), wrapped.at(2500.0, 1500.0, 1.0)], [blue, red]);
        assert!(s.planar() && !wrapped.planar() && s.plan().planar && !wrapped.plan().planar);
    }

    #[test]
    fn a_planar_texture_is_a_picture_over_any_ground_and_only_a_picture() {
        let picture = quarters();
        let id = picture.id();
        let images = BTreeMap::from([(id.clone(), picture)]);
        let image = |size: f64, planar: bool| Texture { size: [size, size], planar, image: id.clone(), ..Texture::new(Kind::Image) };
        // Two kilometres across is the ground of a terrain; a tile that repeats is not so big.
        assert_eq!(image(2_000_000.0, true).check(&images), Ok(()));
        assert!(image(2_000_000.0, false).check(&images).is_err());
        assert!(image(MAX_COORD * 2.0, true).check(&images).is_err() && image(0.5, true).check(&images).is_err());
        // A pattern drawn in code has no ground to lie on.
        let brick = Texture { planar: true, ..Texture::new(Kind::Brick) };
        assert!(brick.check(&images).unwrap_err().contains("picture"));
    }

    #[test]
    fn planar_is_written_only_when_it_is_true_and_older_files_have_none() {
        assert!(!serde_json::to_string(&Texture::new(Kind::Brick)).unwrap().contains("planar"));
        let laid = Texture { planar: true, ..Texture::new(Kind::Image) };
        let text = serde_json::to_string(&laid).unwrap();
        assert!(text.contains(r#""planar":true"#), "{text}");
        assert_eq!(serde_json::from_str::<Texture>(&text).unwrap(), laid);
        assert!(!serde_json::from_str::<Texture>(r#"{"kind":"brick","size":[900,600]}"#).unwrap().planar);
    }

    #[test]
    fn the_pixels_of_a_png_or_jpeg_come_out_as_rgb_and_what_is_not_either_is_refused() {
        let (w, h, rgb) = decode_rgb(&png(4, 2, [10, 20, 30, 255])).unwrap();
        assert_eq!((w, h, rgb.len()), (4, 2, 4 * 2 * 3));
        assert_eq!(&rgb[..3], &[10, 20, 30]);
        let (w, h, rgb) = decode_rgb(JPEG).unwrap();
        assert_eq!((w, h, rgb.len()), (16, 8, 16 * 8 * 3));
        for (what, bytes) in
            [("empty", Vec::new()), ("a page", b"<html>429 Too Many Requests</html>".to_vec()), ("cut short", JPEG[..40].to_vec())]
        {
            assert!(decode_rgb(&bytes).is_err(), "{what}");
        }
        // A header that claims a picture the size of a country is refused by the limits, not by running out of memory.
        let mut huge = png(2, 2, [0, 0, 0, 255]);
        huge[16..20].copy_from_slice(&60_000u32.to_be_bytes());
        huge[20..24].copy_from_slice(&60_000u32.to_be_bytes());
        assert!(decode_rgb(&huge).is_err());
    }

    #[test]
    fn the_one_colour_of_raw_pixels_is_found_with_the_same_tolerance() {
        assert_eq!(flat_rgb(&[5, 5, 5, 5, 5, 5]), Some([5, 5, 5]));
        assert_eq!(flat_rgb(&[255, 255, 255, 250, 254, 253]), Some([253, 255, 254]));
        assert_eq!(flat_rgb(&[0, 0, 0, 255, 255, 255]), None);
        assert_eq!(flat_rgb(&[]), None);
        assert_eq!(flat_rgb(&[1, 2]), None, "less than a pixel is no picture");
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
