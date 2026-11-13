//! The model as a viewable: glTF 2.0 binary (GLB, Khronos's open format for 3D on the web, ISO/IEC 12113:2022) made for a
//! web viewer (`web/viewer`), where [`crate::gltf`] makes one for Blender. It holds the surfaces the 3D view shades, as a web
//! viewer draws them. Each element is a node under one root, named for its category and id, holding its id, GUID, category
//! and properties (what `elements` gives) in its `extras`, so a viewer picks and lists elements as the desktop does; the
//! scene's `extras` lists the levels and how the default 3D view opens. glTF is in metres with y up: x stays east, y (north)
//! becomes -z, and the root's translation is the middle of the model.
//!
//! A shape is stored once. Each element's triangles are told in a frame of its own (from its first corner, along the corner
//! farthest from it, then the one farthest from that line), so the copies of a family, however they are turned or tilted,
//! come out the same and share one mesh; each element's node places it. The vertices are stored small, as Khronos's
//! KHR_mesh_quantization lets them be: a position in 16 bits across its mesh's box (the node's matrix scales it back),
//! a normal in 8 bits, and the indices in 16 bits where a mesh has few enough vertices. Colours are per vertex, in linear
//! light, as glTF defines them; what the view sees through is a second, blended primitive. Textures are left out.

use std::collections::{BTreeMap, HashMap};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::LazyLock;

use serde_json::{Map, Value, json};

use crate::graphics::Look;
use crate::model::{Element, Id, Model};
use crate::view3d::Scene;
use crate::view3d::gpu::{Mesh, Vertex};

/// glTF's component types and buffer view targets, by their numbers in the specification.
const BYTE: u32 = 5120;
const UNSIGNED_BYTE: u32 = 5121;
const UNSIGNED_SHORT: u32 = 5123;
const UNSIGNED_INT: u32 = 5125;
const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;
const QUANTIZATION: &str = "KHR_mesh_quantization";

/// Two corners closer than this (m) count as equally far when a frame is chosen, so every copy of a shape picks the same.
const TIE: f64 = 1e-5;
/// How far (m) a copy's corner may lie from the stored shape's and still be that shape, beyond the 16-bit grid's few steps:
/// copies differ by thousandths of a millimetre, so comparing on a grid instead would split them where a value falls
/// either side of a step.
const SAME: f64 = 5e-5;
/// Grid steps of the same, and of a normal's (in 1/127).
const SAME_STEPS: f64 = 3.0;
const SAME_NORMAL: i16 = 6;

/// A viewable: the file's bytes, and how many elements, triangles and distinct shapes it holds.
pub struct Glb {
    pub bytes: Vec<u8>,
    pub elements: usize,
    pub triangles: usize,
    pub shapes: usize,
}

type V3 = [f64; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn times(a: V3, k: f64) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn length(a: V3) -> f64 {
    dot(a, a).sqrt()
}

/// A point or a direction of the model (mm, z up) in glTF's axes: x east, y up, z south.
fn axes(v: [f32; 3], scale: f64) -> V3 {
    [f64::from(v[0]) * scale, f64::from(v[2]) * scale, -f64::from(v[1]) * scale]
}

/// A colour byte as a model holds it and a screen shows it (sRGB), in linear light, as a byte: glTF defines `COLOR_0` as linear,
/// so a viewer encodes it for the screen, and a byte stored as shown would be lifted a second time and come out washed out.
/// Eight bits hold dark colours coarsely: a byte comes back within 6 of what it was under 13, within 4 under 30, 3 under 60 and
/// 1 from there on. A triangle has one colour, so there is no ramp for that to band.
fn linear(shown: u8) -> u8 {
    static BYTES: LazyLock<[u8; 256]> = LazyLock::new(|| {
        std::array::from_fn(|i| {
            let s = i as f64 / 255.0;
            let l = if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) };
            (l * 255.0).round() as u8
        })
    });
    BYTES[usize::from(shown)]
}

/// One corner of an element's triangles as the scene gives it: where and facing as the mesh has them (mm from its origin, the
/// model's axes, single precision), and its colour. The export holds one for each corner of each triangle (tens of millions
/// for a large building), so it keeps them as they came, 28 bytes, and reads them in glTF's axes ([`Corner::p`],
/// [`Corner::n`]) to the same bits as when it kept them so, at 56.
struct Corner {
    position: [f32; 3],
    normal: [f32; 3],
    c: [u8; 4],
}

impl Corner {
    fn new(v: &Vertex, alpha: u8) -> Corner {
        let [r, g, b, _] = v.color.to_le_bytes();
        Corner { position: v.position, normal: v.normal, c: [linear(r), linear(g), linear(b), alpha] }
    }

    /// Where it is: m, glTF's axes, from the root.
    fn p(&self) -> V3 {
        axes(self.position, 0.001)
    }

    /// Which way it faces, in glTF's axes.
    fn n(&self) -> V3 {
        axes(self.normal, 1.0)
    }

    /// A corner at `p` facing `n`, both as glTF has them, as near as single precision holds them.
    #[cfg(test)]
    fn at(p: V3, n: V3, c: [u8; 4]) -> Corner {
        let model = |v: V3, scale: f64| [(v[0] / scale) as f32, (-v[2] / scale) as f32, (v[1] / scale) as f32];
        Corner { position: model(p, 0.001), normal: model(n, 1.0), c }
    }
}

/// Where an element's shape is told from: its first corner, and three axes at right angles, right-handed.
struct Frame {
    origin: V3,
    x: V3,
    y: V3,
    z: V3,
}

impl Frame {
    /// The frame of `corners`: from the first, x toward the farthest from it, y toward the farthest from that line. Of
    /// corners about as far, the first is taken, so copies of a shape, which list their corners alike, take the same.
    fn of(corners: &[&Corner]) -> Frame {
        let identity = |origin| Frame { origin, x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0], z: [0.0, 0.0, 1.0] };
        let Some(first) = corners.first() else { return identity([0.0; 3]) };
        let o = first.p();
        let pick = |f: &dyn Fn(&Corner) -> f64| {
            let best = corners.iter().map(|c| f(c)).fold(0.0, f64::max);
            corners.iter().find(|c| f(c) >= best - TIE).map_or(o, |c| c.p())
        };
        let far = sub(pick(&|c| length(sub(c.p(), o))), o);
        if length(far) < TIE {
            return identity(o);
        }
        let x = times(far, 1.0 / length(far));
        let off = |p: V3| {
            let d = sub(p, o);
            sub(d, times(x, dot(d, x)))
        };
        let mut y = off(pick(&|c| length(off(c.p()))));
        if length(y) < TIE {
            // A shape on a line: any square turn of x will do.
            y = if x[0].abs() < 0.9 { cross(x, [1.0, 0.0, 0.0]) } else { cross(x, [0.0, 1.0, 0.0]) };
        }
        let y = times(y, 1.0 / length(y));
        Frame { origin: o, x, y, z: cross(x, y) }
    }
    fn local(&self, p: V3) -> V3 {
        let d = sub(p, self.origin);
        [dot(d, self.x), dot(d, self.y), dot(d, self.z)]
    }
    fn turn(&self, n: V3) -> V3 {
        [dot(n, self.x), dot(n, self.y), dot(n, self.z)]
    }
}

/// What every copy of a shape has alike, so only shapes in the same group are compared corner by corner: how many corners
/// each side has, their colours in order, and the shape's size in its frame to the centimetre.
type Group = (usize, usize, u64, [i64; 3]);

fn group(sides: &[Vec<Corner>; 2], frame: &Frame) -> Group {
    let mut colours = DefaultHasher::new();
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for c in sides.iter().flatten() {
        c.c.hash(&mut colours);
        let p = frame.local(c.p());
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let size = [0, 1, 2].map(|k| ((hi[k] - lo[k]).max(0.0) * 100.0).round() as i64);
    (sides[0].len(), sides[1].len(), colours.finish(), size)
}

/// A corner on a shape's grid, in `frame`: its position in 16-bit steps across the box from `lo` (not yet rounded), and
/// its normal as stored, as it stands on that squashed grid (scaled by the box, made unit again), so the node's scale
/// turns it back to the true one.
fn on_grid(frame: &Frame, lo: V3, size: V3, c: &Corner) -> (V3, [i8; 3]) {
    let p = frame.local(c.p());
    let q = [0, 1, 2].map(|k| (p[k] - lo[k]) / size[k] * 65535.0);
    let n = frame.turn(c.n());
    let s = [n[0] * size[0], n[1] * size[1], n[2] * size[2]];
    let s = if length(s) > 1e-12 { times(s, 1.0 / length(s)) } else { n };
    (q, s.map(|v| (v * 127.0).round().clamp(-127.0, 127.0) as i8))
}

/// A vertex as it is stored: its position, normal and colour.
type StoredVertex = ([u16; 3], [i8; 3], [u8; 4]);

/// One primitive's vertices as they are stored, each once, and its triangles by them.
#[derive(Default)]
struct Part {
    positions: Vec<[u16; 3]>,
    normals: Vec<[i8; 3]>,
    colours: Vec<[u8; 4]>,
    indices: Vec<u32>,
    seen: HashMap<StoredVertex, u32>,
}

impl Part {
    fn push(&mut self, position: [u16; 3], normal: [i8; 3], colour: [u8; 4]) -> Result<(), String> {
        let next = u32::try_from(self.positions.len()).map_err(|_| "more vertices than a glTF index holds".to_string())?;
        let index = *self.seen.entry((position, normal, colour)).or_insert(next);
        if index == next {
            self.positions.push(position);
            self.normals.push(normal);
            self.colours.push(colour);
        }
        self.indices.push(index);
        Ok(())
    }
}

/// A shape stored once: its mesh, the box its 16-bit positions span in its frame (m), and each corner as stored.
struct Stored {
    mesh: usize,
    lo: V3,
    size: V3,
    corners: Vec<([u16; 3], [i8; 3])>,
}

impl Stored {
    /// Whether `sides`, told in `frame`, is this shape: each corner within a few grid steps of the stored one (or `SAME`),
    /// facing about as it does. The group has already matched their counts and colours.
    fn fits(&self, sides: &[Vec<Corner>; 2], frame: &Frame) -> bool {
        sides.iter().flatten().zip(&self.corners).all(|(c, (q0, n0))| {
            let (q, n) = on_grid(frame, self.lo, self.size, c);
            (0..3).all(|k| (q[k] - f64::from(q0[k])).abs() <= (SAME / self.size[k] * 65535.0).max(SAME_STEPS))
                && (0..3).all(|k| (i16::from(n[k]) - i16::from(n0[k])).abs() <= SAME_NORMAL)
        })
    }
}

/// The binary chunk being built, and the views and accessors into it.
#[derive(Default)]
struct Writer {
    bin: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}

impl Writer {
    /// `bytes` as a view of their own, aligned to 4 bytes as accessors need, its elements `stride` apart if given; its index.
    fn view(&mut self, bytes: &[u8], target: Option<u32>, stride: Option<usize>) -> usize {
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
        let mut v = json!({"buffer": 0, "byteOffset": self.bin.len(), "byteLength": bytes.len()});
        if let Some(t) = target {
            v["target"] = json!(t);
        }
        if let Some(s) = stride {
            v["byteStride"] = json!(s);
        }
        self.views.push(v);
        self.bin.extend_from_slice(bytes);
        self.views.len() - 1
    }

    fn accessor(&mut self, a: Value) -> usize {
        self.accessors.push(a);
        self.accessors.len() - 1
    }

    /// A part's attributes and indices as accessors: the primitive that draws it with `material`. Each attribute's
    /// elements start on 4 bytes, as glTF asks: a position's three 16-bit numbers take 8, a normal's three bytes take 4.
    fn primitive(&mut self, part: &Part, material: usize) -> Value {
        let (mut lo, mut hi) = ([u16::MAX; 3], [0u16; 3]);
        for p in &part.positions {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        let count = part.positions.len();
        let bytes: Vec<u8> = part.positions.iter().flat_map(|p| p.iter().flat_map(|v| v.to_le_bytes()).chain([0, 0])).collect();
        let at = self.view(&bytes, Some(ARRAY_BUFFER), Some(8));
        let position = self.accessor(json!({"bufferView": at, "byteOffset": 0, "componentType": UNSIGNED_SHORT, "normalized": true,
            "count": count, "type": "VEC3", "min": lo, "max": hi}));
        let bytes: Vec<u8> = part.normals.iter().flat_map(|n| n.iter().map(|v| v.to_le_bytes()[0]).chain([0])).collect();
        let at = self.view(&bytes, Some(ARRAY_BUFFER), Some(4));
        let normal = self.accessor(
            json!({"bufferView": at, "byteOffset": 0, "componentType": BYTE, "normalized": true, "count": count, "type": "VEC3"}),
        );
        let at = self.view(&part.colours.concat(), Some(ARRAY_BUFFER), None);
        let colour = self.accessor(
            json!({"bufferView": at, "byteOffset": 0, "componentType": UNSIGNED_BYTE, "normalized": true, "count": count, "type": "VEC4"}),
        );
        let (bytes, kind): (Vec<u8>, u32) = if count <= usize::from(u16::MAX) {
            (part.indices.iter().flat_map(|i| (*i as u16).to_le_bytes()).collect(), UNSIGNED_SHORT)
        } else {
            (part.indices.iter().flat_map(|i| i.to_le_bytes()).collect(), UNSIGNED_INT)
        };
        let at = self.view(&bytes, Some(ELEMENT_ARRAY_BUFFER), None);
        let indices =
            self.accessor(json!({"bufferView": at, "byteOffset": 0, "componentType": kind, "count": part.indices.len(), "type": "SCALAR"}));
        json!({"attributes": {"POSITION": position, "NORMAL": normal, "COLOR_0": colour}, "indices": indices, "material": material})
    }

    /// A shape's mesh: its corners in `frame`, on a 16-bit grid across their box (`on_grid`).
    fn shape(&mut self, sides: &[Vec<Corner>; 2], frame: &Frame, meshes: &mut Vec<Value>) -> Result<Stored, String> {
        let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
        for c in sides.iter().flatten() {
            let p = frame.local(c.p());
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        let size = [0, 1, 2].map(|k| (hi[k] - lo[k]).max(1e-6));
        let (mut primitives, mut corners) = (Vec::new(), Vec::new());
        for (material, side) in sides.iter().enumerate() {
            if side.is_empty() {
                continue;
            }
            let mut part = Part::default();
            for c in side {
                let (q, n) = on_grid(frame, lo, size, c);
                let q = q.map(|v| v.round().clamp(0.0, 65535.0) as u16);
                part.push(q, n, c.c)?;
                corners.push((q, n));
            }
            primitives.push(self.primitive(&part, material));
        }
        meshes.push(json!({"primitives": primitives}));
        Ok(Stored { mesh: meshes.len() - 1, lo, size, corners })
    }
}

/// `chunk` padded to 4 bytes with `pad`, after its length and its type.
fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], mut data: Vec<u8>, pad: u8) -> Result<(), String> {
    while !data.len().is_multiple_of(4) {
        data.push(pad);
    }
    let len = u32::try_from(data.len()).map_err(|_| "more than the 4 GB a GLB file holds".to_string())?;
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(&data);
    Ok(())
}

/// The node's matrix (column by column, as glTF lists it): the stored grid scaled to its box, then set in the frame.
fn matrix(frame: &Frame, s: &Stored) -> Vec<f64> {
    let at = [0, 1, 2].into_iter().fold(frame.origin, |p, k| {
        let axis = [frame.x, frame.y, frame.z][k];
        [p[0] + axis[0] * s.lo[k], p[1] + axis[1] * s.lo[k], p[2] + axis[2] * s.lo[k]]
    });
    let col = |a: V3, k: f64| [a[0] * k, a[1] * k, a[2] * k, 0.0];
    [col(frame.x, s.size[0]), col(frame.y, s.size[1]), col(frame.z, s.size[2]), [at[0], at[1], at[2], 1.0]].concat()
}

/// `m` as a viewable: what the default 3D view shows, its phase, its design options and what it hides (as a view with no
/// graphics of its own until they're changed: its last phase, its sets' primary options), but not its section box, as GLB;
/// with what its links build when `m` is [`crate::link::seen`].
pub fn export(m: &Model) -> Result<Glb, String> {
    write(m, &Built::from(m, &Scene::new(m))?)
}

/// The 3D view's surfaces, as the export reads them: each element's corners, opaque and seen through apart, in the order the
/// mesh holds them, told from `origin` (mm, the model's axes).
struct Built {
    origin: crate::view3d::P3,
    elements: BTreeMap<Id, [Vec<Corner>; 2]>,
}

impl Built {
    /// From the middle of what `scene` draws. The mesh goes before the corners are taken, but for its vertices, and each
    /// element's corners are given just their room, so a large model is not held twice.
    fn from(m: &Model, scene: &Scene) -> Result<Built, String> {
        let look = Look::of_3d(m).with_section_box(None);
        // Every element's own triangles, the copies' too: a viewable shares its shapes by what it finds equal, whatever made them.
        let mut mesh = Mesh::flat(scene, &look)?;
        let (vertices, owners, origin) = (std::mem::take(&mut mesh.vertices), std::mem::take(&mut mesh.owners), mesh.origin);
        drop(mesh);
        // Each vertex's element, and how far it is seen through.
        let side = |v: &Vertex| {
            let owner = (v.owner as usize).checked_sub(1).and_then(|k| owners.get(k))?;
            Some((owner.id, owner.transparency))
        };
        let mut counts: HashMap<Id, [usize; 2]> = HashMap::new();
        for (id, transparency) in vertices.iter().filter_map(side) {
            counts.entry(id).or_default()[usize::from(transparency > 0)] += 1;
        }
        let mut elements: BTreeMap<Id, [Vec<Corner>; 2]> =
            counts.into_iter().map(|(id, [opaque, clear])| (id, [Vec::with_capacity(opaque), Vec::with_capacity(clear)])).collect();
        for v in &vertices {
            let Some((id, transparency)) = side(v) else { continue };
            let alpha = (255 - u32::from(transparency.min(100)) * 255 / 100) as u8;
            elements.entry(id).or_default()[usize::from(transparency > 0)].push(Corner::new(v, alpha));
        }
        Ok(Built { origin, elements })
    }
}

/// How the default 3D view opens, for a viewer to open the same way: in parallel projection or in perspective.
fn default_view(m: &Model) -> Value {
    let parallel = match m.default_3d().and_then(|v| m.elements.get(v)) {
        Some(Element::View3D(v)) => v.lens().is_none(),
        _ => true,
    };
    json!({"parallel": parallel})
}

/// The viewable of what `built` holds.
fn write(m: &Model, built: &Built) -> Result<Glb, String> {
    let elements = &built.elements;
    let mut w = Writer::default();
    let (mut meshes, mut nodes, mut children) = (Vec::new(), vec![Value::Null], Vec::new());
    let mut stored: HashMap<Group, Vec<Stored>> = HashMap::new();
    for (id, sides) in elements {
        let frame = Frame::of(&sides.iter().flatten().collect::<Vec<_>>());
        let same = stored.entry(group(sides, &frame)).or_default();
        let k = match same.iter().position(|s| s.fits(sides, &frame)) {
            Some(k) => k,
            None => {
                same.push(w.shape(sides, &frame, &mut meshes)?);
                same.len() - 1
            }
        };
        let Some(s) = same.get(k) else { continue };
        let category = m.elements.get(*id).map_or("", |e| e.category());
        let extras = json!({"id": id, "guid": m.guid(*id), "category": category, "properties": crate::element_json(m, *id)});
        nodes.push(json!({"name": format!("{category} {id}"), "mesh": s.mesh, "matrix": matrix(&frame, s), "extras": extras}));
        children.push(nodes.len() - 1);
    }
    let o = axes(built.origin.map(|v| v as f32), 0.001);
    let mut root = Map::new();
    root.insert("name".into(), json!(m.name));
    root.insert("translation".into(), json!(o));
    if !children.is_empty() {
        root.insert("children".into(), json!(children));
    }
    nodes[0] = Value::Object(root);
    let levels: Vec<Value> =
        m.levels().iter().map(|(id, l)| json!({"id": id, "guid": m.guid(*id), "name": l.name, "elevation": l.elevation})).collect();
    let material = |name: &str, blend: bool| {
        let mut x = json!({"name": name, "doubleSided": true, "pbrMetallicRoughness": {"metallicFactor": 0.0, "roughnessFactor": 0.9}});
        if blend {
            x["alphaMode"] = json!("BLEND");
        }
        x
    };
    let mut doc = json!({
        "asset": {"version": "2.0", "generator": format!("BIMCraft {}", env!("CARGO_PKG_VERSION"))},
        "scene": 0,
        "scenes": [{"nodes": [0], "extras": {"levels": levels, "view_3d": default_view(m), "units": "metres, y up; levels' elevations in mm"}}],
        "nodes": nodes,
        "materials": [material("opaque", false), material("seen through", true)],
    });
    if !meshes.is_empty() {
        doc["extensionsUsed"] = json!([QUANTIZATION]);
        doc["extensionsRequired"] = json!([QUANTIZATION]);
        doc["meshes"] = json!(meshes);
    }
    if !w.bin.is_empty() {
        doc["buffers"] = json!([{"byteLength": w.bin.len()}]);
        doc["bufferViews"] = json!(w.views);
    }
    if !w.accessors.is_empty() {
        doc["accessors"] = json!(w.accessors);
    }
    let text = serde_json::to_vec(&doc).map_err(|e| e.to_string())?;
    let mut bytes = Vec::with_capacity(28 + text.len() + w.bin.len());
    bytes.extend_from_slice(b"glTF");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    chunk(&mut bytes, b"JSON", text, b' ')?;
    if !w.bin.is_empty() {
        chunk(&mut bytes, b"BIN\0", w.bin, 0)?;
    }
    let len = u32::try_from(bytes.len()).map_err(|_| "more than the 4 GB a GLB file holds".to_string())?;
    bytes[8..12].copy_from_slice(&len.to_le_bytes());
    let triangles = elements.values().flat_map(|sides| sides.iter()).map(|side| side.len() / 3).sum();
    Ok(Glb { bytes, elements: elements.len(), triangles, shapes: meshes.len() })
}

/// A GLB read back as a viewer reads it, for tests: its JSON, and each element's corners (m, from the root, glTF's axes),
/// opaque then seen through, its stored positions scaled to their box and placed by its node.
#[cfg(test)]
pub(crate) fn decode(bytes: &[u8]) -> (Value, BTreeMap<u64, Vec<V3>>) {
    let word = |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) as usize;
    let json_len = word(12);
    let doc: Value = serde_json::from_slice(&bytes[20..20 + json_len]).unwrap();
    let bin = &bytes[20 + json_len + 8..];
    let n = |v: &Value| v.as_u64().unwrap() as usize;
    let read = |a: usize, i: usize, k: usize| -> f64 {
        let acc = &doc["accessors"][a];
        let view = &doc["bufferViews"][n(&acc["bufferView"])];
        let ty = n(&acc["componentType"]);
        let size = match ty {
            5120 | 5121 => 1,
            5122 | 5123 => 2,
            _ => 4,
        };
        let count = match acc["type"].as_str().unwrap() {
            "VEC3" => 3,
            "VEC4" => 4,
            _ => 1,
        };
        let stride = view["byteStride"].as_u64().map_or(size * count, |s| s as usize);
        let at = n(&view["byteOffset"]) + n(&acc["byteOffset"]) + i * stride + k * size;
        let raw = &bin[at..at + size];
        let normalized = acc["normalized"].as_bool().unwrap_or(false);
        match ty {
            5120 => {
                let v = f64::from(raw[0] as i8);
                if normalized { (v / 127.0).max(-1.0) } else { v }
            }
            5121 => {
                let v = f64::from(raw[0]);
                if normalized { v / 255.0 } else { v }
            }
            5123 => {
                let v = f64::from(u16::from_le_bytes([raw[0], raw[1]]));
                if normalized { v / 65535.0 } else { v }
            }
            5125 => f64::from(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]])),
            _ => f64::from(f32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]])),
        }
    };
    let mut out = BTreeMap::new();
    for node in doc["nodes"].as_array().unwrap() {
        let Some(mesh) = node["mesh"].as_u64() else { continue };
        let m: Vec<f64> = node["matrix"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        let mut corners = Vec::new();
        for p in doc["meshes"][mesh as usize]["primitives"].as_array().unwrap() {
            let (pos, ix) = (n(&p["attributes"]["POSITION"]), n(&p["indices"]));
            for i in 0..n(&doc["accessors"][ix]["count"]) {
                let v = read(ix, i, 0) as usize;
                let l = [read(pos, v, 0), read(pos, v, 1), read(pos, v, 2)];
                corners.push([0, 1, 2].map(|r| m[r] * l[0] + m[4 + r] * l[1] + m[8 + r] * l[2] + m[12 + r]));
            }
        }
        out.insert(node["extras"]["id"].as_u64().unwrap(), corners);
    }
    (doc, out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Session;

    fn run(s: &mut Session, cmd: &str, p: Value) -> Value {
        s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd}: {e}"))
    }

    /// The scene's corners of each element, as the export reads them, before anything is shared or made small.
    fn scene_corners(m: &Model) -> BTreeMap<u64, Vec<V3>> {
        let scene = Scene::new(m);
        let mesh = Mesh::flat(&scene, &Look::with(m, &crate::graphics::Graphics::default())).unwrap();
        let mut out: BTreeMap<u64, [Vec<V3>; 2]> = BTreeMap::new();
        for v in &mesh.vertices {
            let Some(owner) = (v.owner as usize).checked_sub(1).and_then(|k| mesh.owners.get(k)) else { continue };
            out.entry(owner.id).or_default()[usize::from(owner.transparency > 0)].push(axes(v.position, 0.001));
        }
        out.into_iter().map(|(id, [a, b])| (id, [a, b].concat())).collect()
    }

    /// The colours of an element's corners as a saved file stores them: its `COLOR_0`, red, green, blue and alpha bytes.
    fn stored_colours(bytes: &[u8], id: u64) -> Vec<[u8; 4]> {
        let (doc, _) = decode(bytes);
        let json_len = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
        let bin = &bytes[20 + json_len + 8..];
        let n = |v: &Value| v.as_u64().unwrap() as usize;
        let node = doc["nodes"].as_array().unwrap().iter().find(|x| x["extras"]["id"] == json!(id)).unwrap();
        let mut out = Vec::new();
        for p in doc["meshes"][n(&node["mesh"])]["primitives"].as_array().unwrap() {
            let accessor = &doc["accessors"][n(&p["attributes"]["COLOR_0"])];
            assert_eq!(accessor["componentType"], json!(UNSIGNED_BYTE));
            assert_eq!(accessor["normalized"], json!(true));
            assert_eq!(accessor["type"], json!("VEC4"));
            let view = &doc["bufferViews"][n(&accessor["bufferView"])];
            let at = n(&view["byteOffset"]) + n(&accessor["byteOffset"]);
            let raw = &bin[at..at + 4 * n(&accessor["count"])];
            out.extend_from_slice(raw.as_chunks::<4>().0);
        }
        out
    }

    #[test]
    fn linear_bytes_are_the_specifications_values() {
        // The sRGB formula's values, worked by hand: mid grey in light is 188 as it is shown.
        for (shown, stored) in [(0, 0), (10, 1), (62, 12), (84, 23), (176, 111), (188, 128), (255, 255)] {
            assert_eq!(linear(shown), stored, "{shown}");
        }
        assert!((0..255).all(|v| linear(v) <= linear(v + 1)), "darker stays darker");
    }

    #[test]
    fn a_files_colours_are_stored_in_linear_light_and_its_alpha_as_it_was() {
        let mut s = Session::new();
        run(&mut s, "material", json!({"name": "Brick", "color": "#b0543e"}));
        let wall = run(&mut s, "wall", json!({"points": [[0, 0], [6000, 0]], "type": "Exterior - 300mm"}))["ids"][0].as_u64().unwrap();
        let bytes = export(&s.model).unwrap().bytes;
        let stored = stored_colours(&bytes, wall);
        assert!(stored.contains(&[111, 23, 12, 255]), "the brick (176, 84, 62) as linear light: {stored:?}");
        assert!(!stored.iter().any(|c| c[..3] == [176, 84, 62]), "and not as it is shown: {stored:?}");
        // Seen through: the same colour, and the alpha the material's transparency makes, untouched.
        run(&mut s, "material", json!({"name": "Brick", "transparency": 40}));
        let bytes = export(&s.model).unwrap().bytes;
        let stored = stored_colours(&bytes, wall);
        assert!(stored.contains(&[111, 23, 12, 153]), "alpha is no colour: {stored:?}");
    }

    #[test]
    fn copies_of_a_shape_share_one_mesh_and_every_element_comes_back_where_it_was() {
        let mut s = Session::new();
        run(&mut s, "wall", json!({"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true}));
        let bed = |s: &mut Session, p: Value| {
            let mut p = p;
            p["family"] = json!("Bed");
            run(s, "component", p)["id"].as_u64().unwrap()
        };
        let same = [
            bed(&mut s, json!({"at": [2000, 2000]})),
            bed(&mut s, json!({"at": [5000, 2500], "angle": 37})),
            bed(&mut s, json!({"at": [3000, 4000], "angle": 90, "offset": 450})),
        ];
        let mirrored = bed(&mut s, json!({"at": [6000, 4500], "angle": 200, "mirrored": true}));
        let narrow = bed(&mut s, json!({"type": "900 x 2000", "at": [1500, 4500]}));
        let glb = export(&s.model).unwrap();
        let (doc, decoded) = decode(&glb.bytes);
        assert_eq!(doc["extensionsRequired"], json!([QUANTIZATION]));
        let node = |id: u64| doc["nodes"].as_array().unwrap().iter().find(|n| n["extras"]["id"] == json!(id)).unwrap().clone();
        let mesh = |id: u64| node(id)["mesh"].as_u64().unwrap();
        assert!(same.iter().all(|id| mesh(*id) == mesh(same[0])), "the three beds of one type, turned and raised, are one mesh");
        assert_ne!(mesh(narrow), mesh(same[0]), "another type is another shape");
        assert!(glb.shapes < glb.elements, "{} shapes for {} elements", glb.shapes, glb.elements);
        assert_eq!(doc["meshes"].as_array().unwrap().len(), glb.shapes);
        // Every element, the mirrored bed and the walls too, comes back where the scene has it, within the 16-bit grid.
        let scene = scene_corners(&s.model);
        assert_eq!(decoded.keys().collect::<Vec<_>>(), scene.keys().collect::<Vec<_>>());
        assert!(decoded.contains_key(&mirrored));
        for (id, want) in &scene {
            let got = &decoded[id];
            assert_eq!(got.len(), want.len(), "element {id}'s corners");
            let (lo, hi) = want.iter().fold(([f64::MAX; 3], [f64::MIN; 3]), |(lo, hi), p| {
                ([0, 1, 2].map(|k| lo[k].min(p[k])), [0, 1, 2].map(|k| hi[k].max(p[k])))
            });
            // The grid's step is the shape's box over 65535, and its box in its own frame is at most the diagonal of this one.
            let step = length(sub(hi, lo)) / 65535.0 + 1e-5;
            for (g, w) in got.iter().zip(want) {
                assert!(length(sub(*g, *w)) <= 2.0 * step, "element {id}: {g:?} for {w:?}, step {step}");
            }
        }
    }

    #[test]
    fn a_copy_a_few_thousandths_of_a_millimetre_off_is_the_same_shape_and_a_millimetre_off_is_not() {
        // A wedge's corners, then the same turned about a tilted axis, moved, and shaken by up to 0.002 mm.
        let shape: Vec<V3> = vec![[0.0, 0.0, 0.0], [1.2, 0.0, 0.0], [0.0, 0.7, 0.0], [1.2, 0.0, 0.0], [0.0, 0.7, 0.0], [0.4, 0.3, 0.25]];
        let (a, b) = (0.6_f64, 0.35_f64);
        let turn = |p: V3| {
            let p = [p[0] * a.cos() - p[1] * a.sin(), p[0] * a.sin() + p[1] * a.cos(), p[2]];
            [p[0], p[1] * b.cos() - p[2] * b.sin(), p[1] * b.sin() + p[2] * b.cos()]
        };
        let corners = |ps: &[V3]| -> [Vec<Corner>; 2] {
            [ps.iter().map(|p| Corner::at(*p, [0.0, 0.0, 1.0], [200, 180, 160, 255])).collect(), Vec::new()]
        };
        let original = corners(&shape);
        let shaken: Vec<V3> = shape
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let q = turn(*p);
                let jiggle = ((i * 7919) % 5) as f64 * 5e-7 - 1e-6;
                [q[0] + 3.0 + jiggle, q[1] - 2.0 - jiggle, q[2] + 1.5 + jiggle]
            })
            .collect();
        let copy = corners(&shaken);
        // The copy's normals turn with it.
        let mut copy = copy;
        let n = turn([0.0, 0.0, 1.0]);
        copy[0].iter_mut().for_each(|c| *c = Corner::at(c.p(), n, c.c));
        let frame = |s: &[Vec<Corner>; 2]| Frame::of(&s.iter().flatten().collect::<Vec<_>>());
        let (f0, f1) = (frame(&original), frame(&copy));
        assert_eq!(group(&original, &f0), group(&copy, &f1));
        let mut meshes = Vec::new();
        let stored = Writer::default().shape(&original, &f0, &mut meshes).unwrap();
        assert!(stored.fits(&copy, &f1), "the shaken copy is the same shape");
        let mut moved = shaken.clone();
        moved[5][2] += 0.001;
        let mut other = corners(&moved);
        other[0].iter_mut().for_each(|c| *c = Corner::at(c.p(), n, c.c));
        assert!(!stored.fits(&other, &frame(&other)), "a corner a millimetre off is another shape");
    }

    #[test]
    fn the_default_3d_views_projection_rides_in_the_file() {
        let mut s = Session::new();
        s.script(crate::sample::SAMPLE).unwrap();
        let view = |s: &Session| decode(&export(&s.model).unwrap().bytes).0["scenes"][0]["extras"]["view_3d"].clone();
        // As the desktop opens it: parallel.
        assert_eq!(view(&s), json!({"parallel": true}));
        s.execute("perspective", &json!({"on": true})).unwrap();
        assert_eq!(view(&s), json!({"parallel": false}));
    }

    #[test]
    fn save_writes_the_viewable_with_viewer_and_the_scene_for_blender_without_it() {
        let mut s = Session::new();
        s.script(crate::sample::SAMPLE).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let (viewable, blender, plan) = (dir.path().join("v.glb"), dir.path().join("b.glb"), dir.path().join("p.svg"));
        let said = run(&mut s, "save", json!({"path": viewable, "viewer": true}));
        assert_eq!(said["format"], json!("glTF 2.0, for the web viewer"));
        assert!(said["shapes"].as_u64().unwrap() < said["elements"].as_u64().unwrap(), "{said}");
        let (doc, _) = decode(&std::fs::read(&viewable).unwrap());
        assert_eq!(doc["extensionsRequired"], json!([QUANTIZATION]));
        assert!(!doc["scenes"][0]["extras"]["levels"].as_array().unwrap().is_empty(), "the levels ride along");
        let node = doc["nodes"].as_array().unwrap().iter().find(|n| n["extras"]["category"] == json!("wall")).unwrap();
        assert!(node["extras"]["properties"].is_object(), "{node}");
        // Without it, the scene for Blender as before.
        let said = run(&mut s, "save", json!({"path": blender}));
        assert_eq!(said["format"], json!("glTF 2.0"));
        let bytes = std::fs::read(&blender).unwrap();
        let len = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
        let doc: Value = serde_json::from_slice(&bytes[20..20 + len]).unwrap();
        assert!(doc.get("extensionsRequired").is_none(), "no extension a Blender import must know");
        // Only a .glb is a viewable.
        let refused = s.execute("save", &json!({"path": plan, "viewer": true})).unwrap_err();
        assert!(refused.contains("ending in .glb"), "{refused}");
        assert!(!plan.exists());
    }

    #[test]
    fn a_frame_is_square_and_right_handed_and_a_flat_or_empty_shape_still_has_one() {
        let c = |p: V3| Corner::at(p, [0.0, 0.0, 1.0], [0; 4]);
        let tilted = [c([1.0, 1.0, 1.0]), c([3.0, 2.0, 1.5]), c([1.0, 4.0, 0.0])];
        let f = Frame::of(&tilted.iter().collect::<Vec<_>>());
        for (a, b) in [(f.x, f.y), (f.y, f.z), (f.z, f.x)] {
            assert!(dot(a, b).abs() < 1e-12);
        }
        assert!((dot(cross(f.x, f.y), f.z) - 1.0).abs() < 1e-12);
        let point = [c([2.0, 2.0, 2.0]), c([2.0, 2.0, 2.0])];
        let f = Frame::of(&point.iter().collect::<Vec<_>>());
        assert_eq!((f.origin, f.x, f.z), ([2.0, 2.0, 2.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]));
        let line = [c([0.0, 0.0, 0.0]), c([1.0, 0.0, 0.0])];
        let f = Frame::of(&line.iter().collect::<Vec<_>>());
        assert!(length(f.y) > 0.99 && dot(f.x, f.y).abs() < 1e-12);
        assert_eq!(Frame::of(&[]).origin, [0.0; 3]);
    }
}
