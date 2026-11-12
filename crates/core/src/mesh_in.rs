//! Triangle meshes read from OBJ and glTF, from their public specifications (Wavefront's OBJ and MTL as the format's own
//! documentation gives them, and Khronos' glTF 2.0): each object of a file, its triangles grouped by the material they are
//! made of, with that material's name and colour, in millimetres with z up. What the reader can't read is left out and
//! counted (lines and points, textures, a compressed mesh, an object past the triangles a family may hold), never a reason to
//! refuse the file. This module knows no element: `import_mesh` makes families and components of what it reads.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::family::MAX_TRIANGLES;
use crate::view3d::P3;

/// Triangles by material name, as an object gathers them.
type ByMaterial = BTreeMap<String, Vec<[P3; 3]>>;
/// A material's colour, when the file gives one, and its triangles.
type Colored = (Option<[u8; 4]>, Vec<[P3; 3]>);

/// The largest file read, bytes: an OBJ, a glTF's JSON, a GLB or a buffer.
pub const MAX_BYTES: usize = 256 << 20;
/// The most objects one file may bring in, and the most vertices one may hold.
pub const MAX_OBJECTS: usize = 2_000;
const MAX_VERTICES: usize = 8_000_000;
/// The farthest a vertex may lie from the file's origin, mm.
const MAX_COORD: f64 = 1e9;
/// How deep a glTF's nodes may nest, and how many times nodes may be visited in all (a node held twice is drawn twice).
const MAX_DEPTH: usize = 64;
const MAX_VISITS: usize = 100_000;

/// A material as the file names it: its name and its colour, red, green, blue and opacity.
#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    pub name: String,
    pub rgba: Option<[u8; 4]>,
}

/// Triangles of one material.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub material: Material,
    pub tris: Vec<[P3; 3]>,
}

/// One object of the file: an OBJ's `o` or `g`, a glTF's node with a mesh.
#[derive(Clone, Debug, PartialEq)]
pub struct Object {
    pub name: String,
    pub parts: Vec<Part>,
}

impl Object {
    pub fn triangles(&self) -> usize {
        self.parts.iter().map(|p| p.tris.len()).sum()
    }
}

/// A file read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Read {
    pub objects: Vec<Object>,
    /// What was left out, by what it was, with its count.
    pub left_out: BTreeMap<String, usize>,
}

impl Read {
    fn leave(&mut self, what: &str) {
        *self.left_out.entry(what.into()).or_default() += 1;
    }

    /// Objects past [`MAX_TRIANGLES`] split into pieces that fit a family, named on (`Name 2`, `Name 3`…), each piece whole
    /// parts while they fit and a part too large cut into runs of its triangles; objects past [`MAX_OBJECTS`] left out; empty
    /// ones dropped.
    fn settle(mut self) -> Read {
        let mut kept = Vec::new();
        for o in std::mem::take(&mut self.objects) {
            if o.triangles() == 0 {
                continue;
            }
            for piece in pieces(o) {
                if kept.len() >= MAX_OBJECTS {
                    self.leave(&format!("objects past the first {MAX_OBJECTS}"));
                } else {
                    kept.push(piece);
                }
            }
        }
        self.objects = kept;
        self
    }
}

/// `o` as objects of at most [`MAX_TRIANGLES`] each: itself when it fits.
fn pieces(o: Object) -> Vec<Object> {
    if o.triangles() <= MAX_TRIANGLES {
        return vec![o];
    }
    let mut out: Vec<Object> = Vec::new();
    let mut now = Object { name: o.name.clone(), parts: Vec::new() };
    for part in o.parts {
        for run in part.tris.chunks(MAX_TRIANGLES) {
            if now.triangles() + run.len() > MAX_TRIANGLES {
                let n = out.len() + 2;
                out.push(std::mem::replace(&mut now, Object { name: clip(&format!("{} {n}", o.name)), parts: Vec::new() }));
            }
            now.parts.push(Part { material: part.material.clone(), tris: run.to_vec() });
        }
    }
    out.push(now);
    out.retain(|p| p.triangles() > 0);
    out
}

/// `text` cut to what a name may hold.
fn clip(text: &str) -> String {
    text.trim().chars().take(crate::model::MAX_TEXT).collect()
}

/// Whether every coordinate of `t` is finite and in range, and it has an area.
fn fits(t: &[P3; 3]) -> bool {
    let ok = t.iter().flatten().all(|v| v.is_finite() && v.abs() <= MAX_COORD);
    let (a, b, c) = (t[0], t[1], t[2]);
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    ok && n.iter().map(|x| x * x).sum::<f64>() > 1e-12
}

/// A point of a file whose up is `y` (glTF's, and most OBJ writers'), in mm with z up: x east, -z north.
fn y_up(p: P3, mm: f64) -> P3 {
    [p[0] * mm, -p[2] * mm, p[1] * mm]
}

/// An OBJ's text, in a unit `mm` millimetres long, its up `y` or `z`; `mtl` gives the text of a material library it names.
pub fn obj(text: &str, mm: f64, up_is_y: bool, mtl: impl Fn(&str) -> Option<String>) -> Result<Read, String> {
    if text.len() > MAX_BYTES {
        return Err(format!("the file is larger than {} MB", MAX_BYTES >> 20));
    }
    if !(mm.is_finite() && mm > 0.0) {
        return Err("the unit must be a positive length".into());
    }
    let mut read = Read::default();
    let mut colors: BTreeMap<String, [u8; 4]> = BTreeMap::new();
    let mut vertices: Vec<P3> = Vec::new();
    let mut name = String::from("Object");
    let mut material = String::new();
    // Objects in the order they came, each its parts by material.
    let mut objects: Vec<(String, ByMaterial)> = Vec::new();
    let place = |p: P3| if up_is_y { y_up(p, mm) } else { p.map(|v| v * mm) };
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or_default().trim();
        let mut words = line.split_whitespace();
        let Some(key) = words.next() else { continue };
        match key {
            "v" => {
                let xyz: Vec<f64> = words.take(3).filter_map(|w| w.parse().ok()).collect();
                if vertices.len() >= MAX_VERTICES {
                    return Err(format!("the file has more than {MAX_VERTICES} vertices"));
                }
                // A vertex that can't be read keeps its place in the count, so later faces still find theirs.
                vertices.push(match xyz[..] {
                    [x, y, z] => place([x, y, z]),
                    _ => [f64::NAN; 3],
                });
            }
            "o" | "g" => {
                let n = line.get(1..).unwrap_or_default().trim();
                if !n.is_empty() {
                    name = clip(n);
                }
            }
            "usemtl" => material = clip(line.get(6..).unwrap_or_default().trim()),
            "mtllib" => {
                for lib in line.get(6..).unwrap_or_default().split_whitespace() {
                    match mtl(lib) {
                        Some(t) => colors.extend(mtl_colors(&t)),
                        None => read.leave("material libraries not found"),
                    }
                }
            }
            "f" => {
                let n = vertices.len() as i64;
                let corners: Vec<Option<P3>> = words
                    .map(|w| {
                        let i: i64 = w.split('/').next()?.parse().ok()?;
                        let i = if i < 0 { n + i } else { i - 1 };
                        vertices.get(usize::try_from(i).ok()?).copied()
                    })
                    .collect();
                let Some(face) = corners.into_iter().collect::<Option<Vec<P3>>>().filter(|f| f.len() >= 3) else {
                    read.leave("faces naming a vertex the file doesn't have");
                    continue;
                };
                let at = match objects.last_mut() {
                    Some((last, parts)) if *last == name => parts,
                    _ => {
                        objects.push((name.clone(), BTreeMap::new()));
                        match objects.last_mut() {
                            Some((_, parts)) => parts,
                            None => continue,
                        }
                    }
                };
                let tris = at.entry(material.clone()).or_default();
                // A fan: OBJ's faces are flat and, as writers make them, convex.
                for i in 1..face.len() - 1 {
                    let t = [face[0], face[i], face[i + 1]];
                    if fits(&t) {
                        tris.push(t);
                    }
                }
            }
            "l" | "p" => read.leave("lines and points"),
            "curv" | "surf" | "curv2" => read.leave("curves and surfaces"),
            _ => {}
        }
    }
    for (name, parts) in objects {
        let parts = parts
            .into_iter()
            .filter(|(_, t)| !t.is_empty())
            .map(|(m, tris)| Part { material: Material { rgba: colors.get(&m).copied(), name: m }, tris })
            .collect();
        read.objects.push(Object { name, parts });
    }
    Ok(read.settle())
}

/// The colours an MTL's materials give: `Kd` (diffuse, 0 to 1) and `d` (opacity) or `Tr` (its opposite).
fn mtl_colors(text: &str) -> BTreeMap<String, [u8; 4]> {
    let byte = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let mut out = BTreeMap::new();
    let mut name: Option<String> = None;
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or_default().trim();
        let mut words = line.split_whitespace();
        let key = words.next().unwrap_or_default();
        let nums: Vec<f64> = words.filter_map(|w| w.parse().ok()).filter(|v: &f64| v.is_finite()).collect();
        match (key, &name) {
            ("newmtl", _) => {
                let n = clip(line.get(6..).unwrap_or_default().trim());
                out.entry(n.clone()).or_insert([204, 204, 204, 255]);
                name = Some(n);
            }
            ("Kd", Some(n)) if nums.len() >= 3 => {
                if let Some(c) = out.get_mut(n) {
                    (c[0], c[1], c[2]) = (byte(nums[0]), byte(nums[1]), byte(nums[2]));
                }
            }
            ("d", Some(n)) if !nums.is_empty() => {
                if let Some(c) = out.get_mut(n) {
                    c[3] = byte(nums[0]);
                }
            }
            ("Tr", Some(n)) if !nums.is_empty() => {
                if let Some(c) = out.get_mut(n) {
                    c[3] = byte(1.0 - nums[0]);
                }
            }
            _ => {}
        }
    }
    out
}

/// A glTF 2.0 file, `.gltf` (JSON) or `.glb` (binary), in metres with y up as the format says: what its default scene's nodes
/// draw, each node with a mesh an object, in millimetres with z up. `resource` gives the bytes of a buffer the file names by
/// path; one in a data URI it reads itself.
pub fn gltf(bytes: &[u8], resource: impl Fn(&str) -> Option<Vec<u8>>) -> Result<Read, String> {
    if bytes.len() > MAX_BYTES {
        return Err(format!("the file is larger than {} MB", MAX_BYTES >> 20));
    }
    let (json, bin) = glb(bytes)?;
    let doc: Value = serde_json::from_slice(json).map_err(|e| format!("the glTF's JSON can't be read: {e}"))?;
    let mut read = Read::default();
    for ext in doc["extensionsRequired"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        if ext == "KHR_draco_mesh_compression" || ext == "EXT_meshopt_compression" {
            return Err(format!("its meshes are compressed ({ext}), which this reader can't read; export it uncompressed"));
        }
    }
    // The buffers, read once.
    let buffers: Vec<Option<Vec<u8>>> = doc["buffers"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(i, b)| match b["uri"].as_str() {
            None if i == 0 => bin.map(<[u8]>::to_vec),
            None => None,
            Some(uri) if uri.starts_with("data:") => uri.split_once(";base64,").and_then(|(_, data)| crate::texture::unbase64(data)),
            Some(uri) => resource(&percent_decoded(uri)).filter(|b| b.len() <= MAX_BYTES),
        })
        .collect();
    let g = Gltf { doc: &doc, buffers: &buffers };
    let scene = doc["scene"].as_u64().unwrap_or(0) as usize;
    let roots: Vec<usize> = match doc["scenes"][scene]["nodes"].as_array() {
        Some(nodes) => nodes.iter().filter_map(|n| n.as_u64()).map(|n| n as usize).collect(),
        // No scene: every node no other node holds.
        None => {
            let held: Vec<u64> = doc["nodes"]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|n| n["children"].as_array().into_iter().flatten())
                .filter_map(Value::as_u64)
                .collect();
            (0..doc["nodes"].as_array().map_or(0, Vec::len)).filter(|i| !held.contains(&(*i as u64))).collect()
        }
    };
    let mut stack: Vec<(usize, [f64; 16], usize)> = roots.into_iter().rev().map(|n| (n, IDENTITY, 0)).collect();
    let mut vertices = 0usize;
    let mut visits = 0usize;
    while let Some((n, parent, depth)) = stack.pop() {
        visits += 1;
        if visits > MAX_VISITS {
            read.leave(&format!("nodes past the first {MAX_VISITS} visited"));
            break;
        }
        let node = &doc["nodes"][n];
        if node.is_null() || depth > MAX_DEPTH {
            read.leave("nodes nested too deep or not defined");
            continue;
        }
        let m = mul(&parent, &local(node));
        for c in node["children"].as_array().into_iter().flatten().rev().filter_map(Value::as_u64) {
            stack.push((c as usize, m, depth + 1));
        }
        let Some(mesh) = node["mesh"].as_u64() else { continue };
        let mesh = &doc["meshes"][mesh as usize];
        let name = node["name"].as_str().or(mesh["name"].as_str()).map(clip).unwrap_or_else(|| format!("Node {n}"));
        let mut parts: BTreeMap<String, Colored> = BTreeMap::new();
        for prim in mesh["primitives"].as_array().into_iter().flatten() {
            let mode = prim["mode"].as_u64().unwrap_or(4);
            if !(4..=6).contains(&mode) {
                read.leave("lines and points");
                continue;
            }
            let Some(points) = prim["attributes"]["POSITION"].as_u64().and_then(|a| g.vec3(a as usize)) else {
                read.leave("primitives whose positions can't be read");
                continue;
            };
            vertices += points.len();
            if vertices > MAX_VERTICES {
                return Err(format!("the file has more than {MAX_VERTICES} vertices"));
            }
            let index: Vec<usize> = match prim["indices"].as_u64() {
                Some(a) => match g.scalars(a as usize) {
                    Some(i) => i,
                    None => {
                        read.leave("primitives whose indices can't be read");
                        continue;
                    }
                },
                None => (0..points.len()).collect(),
            };
            let corner = |i: usize| index.get(i).and_then(|v| points.get(*v)).map(|p| y_up(transform(&m, *p), 1000.0));
            let triples: Vec<[usize; 3]> = match mode {
                4 => (0..index.len() / 3).map(|t| [3 * t, 3 * t + 1, 3 * t + 2]).collect(),
                5 => (0..index.len().saturating_sub(2)).map(|t| if t % 2 == 0 { [t, t + 1, t + 2] } else { [t + 1, t, t + 2] }).collect(),
                _ => (1..index.len().saturating_sub(1)).map(|t| [0, t, t + 1]).collect(),
            };
            let (material, rgba) = g.material(prim["material"].as_u64());
            let tris = &mut parts.entry(material).or_insert((rgba, Vec::new())).1;
            for [a, b, c] in triples {
                if let (Some(a), Some(b), Some(c)) = (corner(a), corner(b), corner(c)) {
                    // A node mirrored turns its triangles inside out; turned back, they face out again.
                    let t = if det3(&m) < 0.0 { [a, c, b] } else { [a, b, c] };
                    if fits(&t) {
                        tris.push(t);
                    }
                }
            }
            if prim["material"]
                .as_u64()
                .is_some_and(|i| !doc["materials"][i as usize]["pbrMetallicRoughness"]["baseColorTexture"].is_null())
            {
                read.leave("textures");
            }
        }
        let parts = parts
            .into_iter()
            .filter(|(_, (_, t))| !t.is_empty())
            .map(|(name, (rgba, tris))| Part { material: Material { name, rgba }, tris })
            .collect();
        read.objects.push(Object { name, parts });
    }
    Ok(read.settle())
}

/// A GLB's JSON chunk and its binary chunk, or a `.gltf`'s text as its JSON.
fn glb(bytes: &[u8]) -> Result<(&[u8], Option<&[u8]>), String> {
    if !bytes.starts_with(b"glTF") {
        return Ok((bytes, None));
    }
    let word = |at: usize| bytes.get(at..at + 4).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes).map(|v| v as usize);
    if word(4) != Some(2) {
        return Err("only glTF 2.0 binaries are read".into());
    }
    let mut at = 12;
    let (mut json, mut bin) = (None, None);
    while let (Some(len), Some(kind)) = (word(at), word(at + 4)) {
        let body = bytes.get(at + 8..(at + 8).saturating_add(len)).ok_or("the GLB is cut short")?;
        match kind {
            0x4E4F_534A => json = json.or(Some(body)),
            0x004E_4942 => bin = bin.or(Some(body)),
            _ => {}
        }
        at = at + 8 + len;
    }
    Ok((json.ok_or("the GLB has no JSON chunk")?, bin))
}

/// `%20` and its kind turned back into the bytes they stand for.
fn percent_decoded(uri: &str) -> String {
    let b = uri.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = b.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok());
        match (b[i], hex) {
            (b'%', Some(v)) => {
                out.push(v);
                i += 3;
            }
            (c, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

const IDENTITY: [f64; 16] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

/// A node's own matrix, column-major: its `matrix`, or its translation, rotation and scale.
fn local(node: &Value) -> [f64; 16] {
    let nums = |v: &Value, n: usize| -> Option<Vec<f64>> {
        let a: Vec<f64> = v.as_array()?.iter().filter_map(Value::as_f64).filter(|x| x.is_finite()).collect();
        (a.len() == n).then_some(a)
    };
    if let Some(m) = nums(&node["matrix"], 16) {
        let mut out = IDENTITY;
        out.copy_from_slice(&m);
        return out;
    }
    let t = nums(&node["translation"], 3).unwrap_or_else(|| vec![0.0; 3]);
    let r = nums(&node["rotation"], 4).unwrap_or_else(|| vec![0.0, 0.0, 0.0, 1.0]);
    let s = nums(&node["scale"], 3).unwrap_or_else(|| vec![1.0; 3]);
    let (x, y, z, w) = (r[0], r[1], r[2], r[3]);
    let rot = [
        1.0 - 2.0 * (y * y + z * z),
        2.0 * (x * y + z * w),
        2.0 * (x * z - y * w),
        2.0 * (x * y - z * w),
        1.0 - 2.0 * (x * x + z * z),
        2.0 * (y * z + x * w),
        2.0 * (x * z + y * w),
        2.0 * (y * z - x * w),
        1.0 - 2.0 * (x * x + y * y),
    ];
    [
        rot[0] * s[0],
        rot[1] * s[0],
        rot[2] * s[0],
        0.0,
        rot[3] * s[1],
        rot[4] * s[1],
        rot[5] * s[1],
        0.0,
        rot[6] * s[2],
        rot[7] * s[2],
        rot[8] * s[2],
        0.0,
        t[0],
        t[1],
        t[2],
        1.0,
    ]
}

fn mul(a: &[f64; 16], b: &[f64; 16]) -> [f64; 16] {
    let mut out = [0.0; 16];
    for c in 0..4 {
        for r in 0..4 {
            out[c * 4 + r] = (0..4).map(|k| a[k * 4 + r] * b[c * 4 + k]).sum();
        }
    }
    out
}

fn transform(m: &[f64; 16], p: P3) -> P3 {
    [0, 1, 2].map(|r| m[r] * p[0] + m[4 + r] * p[1] + m[8 + r] * p[2] + m[12 + r])
}

/// The determinant of a matrix's turning part: negative when it mirrors.
fn det3(m: &[f64; 16]) -> f64 {
    m[0] * (m[5] * m[10] - m[9] * m[6]) - m[4] * (m[1] * m[10] - m[9] * m[2]) + m[8] * (m[1] * m[6] - m[5] * m[2])
}

/// A glTF document and its buffers, for its accessors.
struct Gltf<'a> {
    doc: &'a Value,
    buffers: &'a [Option<Vec<u8>>],
}

impl Gltf<'_> {
    /// The bytes of accessor `a`'s elements, each `size` bytes of components: its view's slice, stride and count.
    fn elements(&self, a: usize, components: usize) -> Option<(Vec<&[u8]>, u64)> {
        let acc = &self.doc["accessors"][a];
        if !acc["sparse"].is_null() {
            return None;
        }
        let kind = acc["componentType"].as_u64()?;
        let width = match kind {
            5120 | 5121 => 1,
            5122 | 5123 => 2,
            5125 | 5126 => 4,
            _ => return None,
        };
        let count = usize::try_from(acc["count"].as_u64()?).ok()?;
        let view = &self.doc["bufferViews"][usize::try_from(acc["bufferView"].as_u64()?).ok()?];
        let buffer = self.buffers.get(usize::try_from(view["buffer"].as_u64()?).ok()?)?.as_deref()?;
        // Every offset and length is the file's: added and multiplied checked, so one past what a number holds is no element.
        let start = usize::try_from(view["byteOffset"].as_u64().unwrap_or(0).checked_add(acc["byteOffset"].as_u64().unwrap_or(0))?).ok()?;
        let size = width * components;
        let stride = usize::try_from(view["byteStride"].as_u64().unwrap_or(size as u64)).ok()?.max(size);
        let end = usize::try_from(view["byteOffset"].as_u64().unwrap_or(0).checked_add(view["byteLength"].as_u64()?)?).ok()?;
        let view_bytes = buffer.get(..end.min(buffer.len()))?;
        let mut out = Vec::with_capacity(count.min(MAX_VERTICES));
        for i in 0..count {
            let at = i.checked_mul(stride)?.checked_add(start)?;
            out.push(view_bytes.get(at..at.checked_add(size)?)?);
        }
        Some((out, kind))
    }

    /// A VEC3 of floats accessor, as points.
    fn vec3(&self, a: usize) -> Option<Vec<P3>> {
        let (els, kind) = self.elements(a, 3)?;
        if kind != 5126 || self.doc["accessors"][a]["type"] != "VEC3" {
            return None;
        }
        let f = |b: &[u8], i: usize| b.get(i * 4..i * 4 + 4).and_then(|x| x.try_into().ok()).map(f32::from_le_bytes).map(f64::from);
        els.into_iter().map(|b| Some([f(b, 0)?, f(b, 1)?, f(b, 2)?])).collect()
    }

    /// A SCALAR accessor of unsigned integers, as indices.
    fn scalars(&self, a: usize) -> Option<Vec<usize>> {
        let (els, kind) = self.elements(a, 1)?;
        els.into_iter()
            .map(|b| match kind {
                5121 => b.first().map(|v| usize::from(*v)),
                5123 => b.try_into().ok().map(u16::from_le_bytes).map(usize::from),
                5125 => b.try_into().ok().map(u32::from_le_bytes).and_then(|v| usize::try_from(v).ok()),
                _ => None,
            })
            .collect()
    }

    /// A material's name and its base colour (sRGB as the file's factor is linear: turned to sRGB bytes).
    fn material(&self, m: Option<u64>) -> (String, Option<[u8; 4]>) {
        let Some(i) = m else { return ("Default".into(), None) };
        let mat = &self.doc["materials"][i as usize];
        let name = mat["name"].as_str().map(clip).unwrap_or_else(|| format!("Material {i}"));
        let f: Vec<f64> =
            mat["pbrMetallicRoughness"]["baseColorFactor"].as_array().into_iter().flatten().filter_map(Value::as_f64).collect();
        let srgb = |v: f64| {
            let v = v.clamp(0.0, 1.0);
            let s = if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
            (s * 255.0).round() as u8
        };
        let rgba = (f.len() == 4).then(|| [srgb(f[0]), srgb(f[1]), srgb(f[2]), (f[3].clamp(0.0, 1.0) * 255.0).round() as u8]);
        (name, rgba.or(Some([255, 255, 255, 255])))
    }
}
