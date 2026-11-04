//! glTF 2.0 binary (.glb) export of the 3D scene, for Blender and other viewers.
//!
//! What the 3D view draws is what the file holds: the triangles of the [`Scene`] (the ones the processor's picture and the card's
//! mesh are made from), as the default 3D view shows them, so its phase, its design options and what it hides are followed and a
//! section box is not. One node and one mesh for each element, the node named by the element's id, with its GUID and category in
//! `extras`; a mesh has a primitive for each colour the element is shaded in. A surface becomes a PBR material of its base
//! colour (the scene's sRGB colour, in linear light as glTF takes it), blended where the scene sees through it, and double
//! sided, since the scene draws both sides of everything. Textures, edges, levels and point clouds are not written.
//!
//! BIMCraft is z up in millimetres and glTF y up in metres: a point `(x, y, z)` mm is written at `(x, z, -y)` thousandths, a
//! turn about the east axis and not a mirror, so winding and handedness are kept. A .glb is a 12-byte header, a JSON chunk and a
//! binary chunk, each 4-byte aligned; nothing here needs a glTF library.

use std::collections::{BTreeMap, HashMap};

use serde_json::{Value, json};

use crate::model::{Id, Model};
use crate::render::Rgb;
use crate::view3d::{Facet, P3, Scene, dot};

/// The most triangles a file holds: the card's limit for a mesh, which also keeps a file (at most 84 bytes a triangle) well under
/// the size a project file may be.
pub const MAX_TRIS: usize = crate::view3d::gpu::MAX_TRIS;

/// What an export wrote.
pub struct Export {
    /// The .glb file.
    pub bytes: Vec<u8>,
    /// How many elements it holds, a node each.
    pub nodes: usize,
    /// How many triangles it holds. Those with no area are not written.
    pub triangles: usize,
}

/// Whether a scene of `n` triangles can be written, and if not, why.
fn check_count(n: usize) -> Result<(), String> {
    if n == 0 {
        return Err(
            "the default 3D view shows nothing to export: draw something, or show again what Visibility/Graphics hides there".into()
        );
    }
    if n > MAX_TRIS {
        return Err(format!(
            "the default 3D view shows {n} triangles, more than a glTF file is given ({MAX_TRIS}); hide some of the model there (Visibility/Graphics, a phase or a design option) and export again"
        ));
    }
    Ok(())
}

/// The scene of `m` that the default 3D view shows, as a .glb.
pub fn export(m: &Model) -> Result<Export, String> {
    let scene = Scene::new(m);
    let look = crate::graphics::Look::of_3d(m);
    check_count(scene.shown(&look))?;
    let mut by_owner: BTreeMap<Id, Vec<Facet>> = BTreeMap::new();
    for f in scene.facets(&look) {
        by_owner.entry(f.owner).or_default().push(f);
    }
    let mut file = File::default();
    for (id, facets) in by_owner {
        file.element(m, id, &facets);
    }
    if file.triangles == 0 {
        return Err("the default 3D view shows only surfaces with no area, nothing to export".into());
    }
    let (nodes, triangles) = (file.nodes.len(), file.triangles);
    Ok(Export { bytes: file.finish()?, nodes, triangles })
}

/// A point in glTF's axes and metres: `(x, y, z)` is written at `(x, z, -y)`, a quarter turn about x; `k` scales it.
fn turned(p: P3, k: f64) -> [f32; 3] {
    // `+ 0.0` makes a negative zero a zero.
    [(p[0] * k + 0.0) as f32, (p[2] * k + 0.0) as f32, (-p[1] * k + 0.0) as f32]
}

/// A colour's channel from sRGB, as glTF's factors are, in linear light.
fn linear(c: u8) -> f64 {
    let c = f64::from(c) / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// The triangles of one element in one material.
#[derive(Default)]
struct Primitive {
    positions: Vec<f32>,
    normals: Vec<f32>,
    indices: Vec<u32>,
    /// A vertex's place, by its position and normal: corners that agree are one vertex.
    shared: HashMap<[u32; 6], u32>,
}

impl Primitive {
    fn corner(&mut self, position: [f32; 3], normal: [f32; 3]) {
        let key = [position[0], position[1], position[2], normal[0], normal[1], normal[2]].map(f32::to_bits);
        let at = *self.shared.entry(key).or_insert_with(|| {
            self.positions.extend(position);
            self.normals.extend(normal);
            (self.positions.len() / 3 - 1) as u32
        });
        self.indices.push(at);
    }
}

/// The file as it is put together: the binary chunk's three runs (positions, normals, indices), and what the JSON lists.
#[derive(Default)]
struct File {
    positions: Vec<u8>,
    normals: Vec<u8>,
    indices: Vec<u8>,
    accessors: Vec<Value>,
    materials: Vec<Value>,
    material_of: HashMap<([u8; 3], u8), usize>,
    meshes: Vec<Value>,
    nodes: Vec<Value>,
    triangles: usize,
}

/// The buffer views the runs are: positions, normals, indices.
const POSITIONS: usize = 0;
const NORMALS: usize = 1;
const INDICES: usize = 2;
const FLOAT: u32 = 5126;
const UNSIGNED_INT: u32 = 5125;
const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;

impl File {
    /// The material of a surface of `color` and `clearness` percent, made once.
    fn material(&mut self, Rgb(r, g, b): Rgb, clearness: u8) -> usize {
        if let Some(i) = self.material_of.get(&([r, g, b], clearness)) {
            return *i;
        }
        let alpha = 1.0 - f64::from(clearness) / 100.0;
        let mut name = format!("#{r:02x}{g:02x}{b:02x}");
        if clearness > 0 {
            name.push_str(&format!(" {clearness}% clear"));
        }
        let mut material = json!({
            "name": name,
            "pbrMetallicRoughness": {"baseColorFactor": [linear(r), linear(g), linear(b), alpha], "metallicFactor": 0.0, "roughnessFactor": 0.9},
            "doubleSided": true,
        });
        if clearness > 0 {
            material["alphaMode"] = json!("BLEND");
        }
        self.materials.push(material);
        self.material_of.insert(([r, g, b], clearness), self.materials.len() - 1);
        self.materials.len() - 1
    }

    fn accessor(&mut self, view: usize, offset: usize, kind: &str, component: u32, count: usize) -> usize {
        self.accessors.push(json!({"bufferView": view, "byteOffset": offset, "componentType": component, "count": count, "type": kind}));
        self.accessors.len() - 1
    }

    /// Element `id` as a mesh and a node.
    fn element(&mut self, m: &Model, id: Id, facets: &[Facet]) {
        let mut parts: BTreeMap<([u8; 3], u8), Primitive> = BTreeMap::new();
        for f in facets {
            // A triangle with no area, or a corner out of range, draws nothing; and a normal of nothing is not a normal.
            let flat = f.corners.iter().flatten().all(|v| v.is_finite()) && f.normals.iter().all(|n| dot(*n, *n) > 0.5);
            if !flat {
                continue;
            }
            let Rgb(r, g, b) = f.color;
            let part = parts.entry(([r, g, b], f.clearness)).or_default();
            for (p, n) in f.corners.iter().zip(&f.normals) {
                part.corner(turned(*p, 0.001), turned(*n, 1.0));
            }
        }
        let mut primitives = Vec::new();
        for ((rgb, clearness), part) in parts {
            let material = self.material(Rgb(rgb[0], rgb[1], rgb[2]), clearness);
            let (at, normal_at, index_at) = (self.positions.len(), self.normals.len(), self.indices.len());
            self.positions.extend(part.positions.iter().flat_map(|v| v.to_le_bytes()));
            self.normals.extend(part.normals.iter().flat_map(|v| v.to_le_bytes()));
            self.indices.extend(part.indices.iter().flat_map(|v| v.to_le_bytes()));
            let count = part.positions.len() / 3;
            let (position, normal, indices) = (
                self.accessor(POSITIONS, at, "VEC3", FLOAT, count),
                self.accessor(NORMALS, normal_at, "VEC3", FLOAT, count),
                self.accessor(INDICES, index_at, "SCALAR", UNSIGNED_INT, part.indices.len()),
            );
            // The bounds a position accessor must state, of the numbers as they are written.
            let bound = |pick: fn(f32, f32) -> f32, start: f32| -> Vec<f64> {
                (0..3).map(|k| f64::from(part.positions.iter().skip(k).step_by(3).copied().fold(start, pick))).collect()
            };
            if let Some(a) = self.accessors.get_mut(position) {
                a["min"] = json!(bound(f32::min, f32::INFINITY));
                a["max"] = json!(bound(f32::max, f32::NEG_INFINITY));
            }
            self.triangles += part.indices.len() / 3;
            primitives
                .push(json!({"attributes": {"POSITION": position, "NORMAL": normal}, "indices": indices, "material": material, "mode": 4}));
        }
        if primitives.is_empty() {
            return;
        }
        let mut extras = json!({"id": id, "guid": m.guid(id).to_string(), "category": m.elements.get(id).map_or("", |e| e.category())});
        if crate::link::is_linked(id) {
            extras["linked"] = json!(true);
        }
        let name = id.to_string();
        self.meshes.push(json!({"name": name, "primitives": primitives}));
        self.nodes.push(json!({"name": name, "mesh": self.meshes.len() - 1, "extras": extras}));
    }

    /// The .glb: the header, the JSON chunk and the binary chunk.
    fn finish(self) -> Result<Vec<u8>, String> {
        let mut bin = self.positions;
        let (normals_at, indices_at) = (bin.len(), bin.len() + self.normals.len());
        bin.extend(&self.normals);
        bin.extend(&self.indices);
        let view = |start: usize, end: usize, target: u32| {
            let mut v = json!({"buffer": 0, "byteOffset": start, "byteLength": end - start, "target": target});
            if target == ARRAY_BUFFER {
                // Tightly packed, which the stride says outright as several accessors read the one view.
                v["byteStride"] = json!(12);
            }
            v
        };
        let document = json!({
            "asset": {
                "version": "2.0",
                "generator": concat!("BIMCraft ", env!("CARGO_PKG_VERSION")),
                "extras": {"units": "metres", "up": "+Y, BIMCraft's +Z", "source": "the default 3D view"},
            },
            "scene": 0,
            "scenes": [{"nodes": (0..self.nodes.len()).collect::<Vec<_>>()}],
            "nodes": self.nodes,
            "meshes": self.meshes,
            "materials": self.materials,
            "accessors": self.accessors,
            "bufferViews": [
                view(0, normals_at, ARRAY_BUFFER),
                view(normals_at, indices_at, ARRAY_BUFFER),
                view(indices_at, bin.len(), ELEMENT_ARRAY_BUFFER),
            ],
            "buffers": [{"byteLength": bin.len()}],
        });
        let mut json = serde_json::to_vec(&document).map_err(|e| format!("glTF: {e}"))?;
        json.resize(json.len().next_multiple_of(4), b' ');
        bin.resize(bin.len().next_multiple_of(4), 0);
        let total = 12 + 8 + json.len() + 8 + bin.len();
        let too_big = || format!("glTF: {total} bytes, more than a glb holds (4 GiB)");
        let (total32, json32, bin32) = (
            u32::try_from(total).map_err(|_| too_big())?,
            u32::try_from(json.len()).map_err(|_| too_big())?,
            u32::try_from(bin.len()).map_err(|_| too_big())?,
        );
        let mut out = Vec::with_capacity(total);
        out.extend(b"glTF");
        out.extend(2u32.to_le_bytes());
        out.extend(total32.to_le_bytes());
        out.extend(json32.to_le_bytes());
        out.extend(b"JSON");
        out.extend(json);
        out.extend(bin32.to_le_bytes());
        out.extend(b"BIN\0");
        out.extend(bin);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Session;
    use serde_json::{Value, json};

    /// A .glb taken apart: its JSON, and its binary chunk. Fails the test where the framing is wrong.
    fn parts(bytes: &[u8]) -> (Value, Vec<u8>) {
        let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        assert_eq!(&bytes[..4], b"glTF");
        assert_eq!(word(4), 2, "version");
        assert_eq!(word(8), bytes.len(), "the header's length is the file's");
        let (json_len, json_type) = (word(12), &bytes[16..20]);
        assert_eq!(json_type, b"JSON");
        assert_eq!(json_len % 4, 0, "the JSON chunk is 4-byte aligned");
        let json = &bytes[20..20 + json_len];
        assert!(json.ends_with(b"}") || json.ends_with(b" "), "padded with spaces");
        let bin_at = 20 + json_len;
        let (bin_len, bin_type) = (word(bin_at), &bytes[bin_at + 4..bin_at + 8]);
        assert_eq!(bin_type, b"BIN\0");
        assert_eq!(bin_len % 4, 0, "the binary chunk is 4-byte aligned");
        assert_eq!(bin_at + 8 + bin_len, bytes.len(), "two chunks and nothing after");
        (serde_json::from_slice(json).unwrap(), bytes[bin_at + 8..].to_vec())
    }

    /// Mass `boundary` and `height` in a fresh project: a straight prism of 12 triangles.
    fn boxes(sizes: &[(f64, f64, f64)]) -> (Session, Vec<u64>) {
        let mut s = Session::new();
        let ids = sizes
            .iter()
            .enumerate()
            .map(|(k, (w, d, h))| {
                let x = k as f64 * 10_000.0;
                let boundary = json!([[x, 0.0], [x + w, 0.0], [x + w, *d], [x, *d]]);
                s.execute("mass", &json!({"boundary": boundary, "height": h})).unwrap()["id"].as_u64().unwrap()
            })
            .collect();
        (s, ids)
    }

    fn floats(bin: &[u8], json: &Value, accessor: u64) -> Vec<f32> {
        let a = &json["accessors"][accessor as usize];
        let view = &json["bufferViews"][a["bufferView"].as_u64().unwrap() as usize];
        let at = view["byteOffset"].as_u64().unwrap_or(0) as usize + a["byteOffset"].as_u64().unwrap_or(0) as usize;
        let n = a["count"].as_u64().unwrap() as usize * if a["type"] == "VEC3" { 3 } else { 1 };
        bin[at..at + 4 * n].chunks(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect()
    }

    fn indices(bin: &[u8], json: &Value, accessor: u64) -> Vec<u32> {
        let a = &json["accessors"][accessor as usize];
        assert_eq!(a["componentType"], 5125, "unsigned int");
        let view = &json["bufferViews"][a["bufferView"].as_u64().unwrap() as usize];
        let at = view["byteOffset"].as_u64().unwrap_or(0) as usize + a["byteOffset"].as_u64().unwrap_or(0) as usize;
        let n = a["count"].as_u64().unwrap() as usize;
        bin[at..at + 4 * n].chunks(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect()
    }

    #[test]
    fn a_glb_has_its_header_and_two_aligned_chunks() {
        let (s, _) = boxes(&[(4000.0, 2000.0, 3000.0)]);
        let out = export(&s.model).unwrap();
        let (json, bin) = parts(&out.bytes);
        assert_eq!(json["asset"]["version"], "2.0");
        assert_eq!(json["buffers"][0]["byteLength"].as_u64().unwrap() as usize, bin.len(), "no padding in the buffer's own length");
    }

    #[test]
    fn each_element_is_a_node_named_by_its_id_with_its_guid_and_category() {
        let (s, ids) = boxes(&[(4000.0, 2000.0, 3000.0), (1000.0, 1000.0, 1000.0)]);
        let out = export(&s.model).unwrap();
        let (json, bin) = parts(&out.bytes);
        let nodes = json["nodes"].as_array().unwrap();
        assert_eq!(nodes.len(), 2);
        assert_eq!(json["meshes"].as_array().unwrap().len(), 2);
        assert_eq!(json["scenes"][0]["nodes"], json!([0, 1]));
        for (node, id) in nodes.iter().zip(&ids) {
            assert_eq!(node["name"], id.to_string());
            assert_eq!(node["extras"]["id"], *id);
            assert_eq!(node["extras"]["guid"], s.model.guid(*id).to_string());
            assert_eq!(node["extras"]["category"], "mass");
        }
        // 12 triangles a box, and the file counts what it holds.
        let held: usize = json["meshes"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|m| m["primitives"].as_array().unwrap())
            .map(|p| indices(&bin, &json, p["indices"].as_u64().unwrap()).len() / 3)
            .sum();
        assert_eq!((held, out.triangles, out.nodes), (24, 24, 2));
    }

    #[test]
    fn a_known_box_comes_out_in_metres_with_y_up_and_turned_not_mirrored() {
        // 4 m east, 2 m north, 3 m up, from the origin.
        let (s, _) = boxes(&[(4000.0, 2000.0, 3000.0)]);
        let (json, bin) = parts(&export(&s.model).unwrap().bytes);
        let p = &json["meshes"][0]["primitives"][0];
        let at = floats(&bin, &json, p["attributes"]["POSITION"].as_u64().unwrap());
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for v in at.chunks(3) {
            for k in 0..3 {
                lo[k] = lo[k].min(v[k]);
                hi[k] = hi[k].max(v[k]);
            }
        }
        // East stays x; up is y; north, away from the viewer who looks down -z, is -z.
        assert_eq!((lo, hi), ([0.0, 0.0, -2.0], [4.0, 3.0, 0.0]));
        let a = &json["accessors"][p["attributes"]["POSITION"].as_u64().unwrap() as usize];
        assert_eq!((&a["min"], &a["max"]), (&json!([0.0, 0.0, -2.0]), &json!([4.0, 3.0, 0.0])));
        // A rotation keeps the winding: the triangles still enclose a positive volume, 4 x 2 x 3 m.
        let idx = indices(&bin, &json, p["indices"].as_u64().unwrap());
        let v = |i: u32| -> [f64; 3] { std::array::from_fn(|k| f64::from(at[3 * i as usize + k])) };
        let volume: f64 = idx
            .chunks(3)
            .map(|t| {
                let (a, b, c) = (v(t[0]), v(t[1]), v(t[2]));
                (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0])) / 6.0
            })
            .sum();
        assert!((volume - 24.0).abs() < 1e-4, "{volume}");
        // Normals are unit and point out of the box: up on its top.
        let normals = floats(&bin, &json, p["attributes"]["NORMAL"].as_u64().unwrap());
        assert!(normals.chunks(3).all(|n| (n.iter().map(|x| x * x).sum::<f32>().sqrt() - 1.0).abs() < 1e-5));
        assert!(normals.chunks(3).any(|n| n == [0.0, 1.0, 0.0]), "a face looks up");
    }

    #[test]
    fn a_surface_takes_its_colour_in_linear_light_and_what_the_view_sees_through_blends() {
        let (mut s, ids) = boxes(&[(1000.0, 1000.0, 1000.0), (1000.0, 1000.0, 1000.0)]);
        // A mass is seen through by default (60%); the second's view override makes it 80%.
        s.execute("graphics", &json!({"view": "{3D}", "elements": {ids[1].to_string(): {"transparency": 80}}})).unwrap();
        let (json, _) = parts(&export(&s.model).unwrap().bytes);
        let material = |node: usize| {
            let m = json["meshes"][node]["primitives"][0]["material"].as_u64().unwrap() as usize;
            json["materials"][m].clone()
        };
        let factor = |m: &Value| m["pbrMetallicRoughness"]["baseColorFactor"].as_array().unwrap().clone();
        let (first, second) = (factor(&material(0)), factor(&material(1)));
        // The mass is (158, 190, 224) in the scene, an sRGB colour; the file holds it linear.
        let back = |c: &Value| (c.as_f64().unwrap().powf(1.0 / 2.4) * 1.055 - 0.055) * 255.0;
        for (c, want) in first.iter().zip([158.0, 190.0, 224.0]) {
            assert!((back(c) - want).abs() < 0.6, "{first:?}");
        }
        assert!((first[3].as_f64().unwrap() - 0.4).abs() < 1e-9 && (second[3].as_f64().unwrap() - 0.2).abs() < 1e-9);
        assert!([material(0), material(1)].iter().all(|m| m["alphaMode"] == "BLEND"));
        // The scene draws both sides of every surface, so the file does.
        assert!(json["materials"].as_array().unwrap().iter().all(|m| m["doubleSided"] == true));
    }

    #[test]
    fn what_the_view_does_not_see_through_stays_opaque() {
        let mut s = Session::new();
        s.execute("floor", &json!({"boundary": [[0, 0], [4000, 0], [4000, 3000], [0, 3000]]})).unwrap();
        let (json, _) = parts(&export(&s.model).unwrap().bytes);
        let materials = json["materials"].as_array().unwrap();
        assert!(!materials.is_empty());
        for m in materials {
            assert_eq!(m["pbrMetallicRoughness"]["baseColorFactor"][3], 1.0, "{m}");
            assert_eq!(m.get("alphaMode"), None, "opaque is glTF's default");
        }
    }

    #[test]
    fn what_the_default_3d_view_hides_is_left_out() {
        let (mut s, ids) = boxes(&[(1000.0, 1000.0, 1000.0), (2000.0, 1000.0, 1000.0)]);
        s.execute("graphics", &json!({"view": "{3D}", "hide_elements": [ids[0]]})).unwrap();
        let out = export(&s.model).unwrap();
        let (json, _) = parts(&out.bytes);
        assert_eq!(json["nodes"].as_array().unwrap().len(), 1);
        assert_eq!(json["nodes"][0]["name"], ids[1].to_string());
        assert_eq!(out.nodes, 1);
        // Everything hidden is nothing to write, and the error says so.
        s.execute("graphics", &json!({"view": "{3D}", "hide_elements": [ids[0], ids[1]]})).unwrap();
        let e = export(&s.model).err().unwrap();
        assert!(e.contains("nothing"), "{e}");
    }

    #[test]
    fn the_default_3d_views_phase_and_design_options_decide_what_is_written() {
        let named = |s: &Session| -> Vec<String> {
            let (json, _) = parts(&export(&s.model).unwrap().bytes);
            json["nodes"].as_array().unwrap().iter().map(|n| n["name"].as_str().unwrap().to_string()).collect()
        };
        // Two masses in the options of a set: the primary's shows until the view chooses the other.
        let (mut s, ids) = boxes(&[(1000.0, 1000.0, 1000.0), (1000.0, 1000.0, 1000.0)]);
        s.execute("option", &json!({"set": "Facade", "options": ["Brick", "Glass"]})).unwrap();
        s.execute("modify", &json!({"ids": [ids[0]], "params": {"Design Option": "Facade: Brick"}})).unwrap();
        s.execute("modify", &json!({"ids": [ids[1]], "params": {"Design Option": "Facade: Glass"}})).unwrap();
        assert_eq!(named(&s), [ids[0].to_string()]);
        s.execute("graphics", &json!({"view": "{3D}", "options": {"Facade": "Glass"}})).unwrap();
        assert_eq!(named(&s), [ids[1].to_string()]);
        // A mass made in the second phase is not in a 3D view of the first.
        let mut s = Session::new();
        let first = s.execute("mass", &json!({"boundary": [[0, 0], [1000, 0], [1000, 1000], [0, 1000]], "height": 1000})).unwrap()["id"]
            .as_u64()
            .unwrap();
        s.execute("phase", &json!({"now": "New Construction"})).unwrap();
        let later =
            s.execute("mass", &json!({"boundary": [[3000, 0], [4000, 0], [4000, 1000], [3000, 1000]], "height": 1000})).unwrap()["id"]
                .as_u64()
                .unwrap();
        assert_eq!(named(&s), [first.to_string(), later.to_string()]);
        s.execute("graphics", &json!({"view": "{3D}", "phase": "Existing"})).unwrap();
        assert_eq!(named(&s), [first.to_string()]);
    }

    #[test]
    fn the_sample_house_writes_a_node_for_each_element_the_scene_draws() {
        let mut s = Session::new();
        s.script(crate::sample::SAMPLE).unwrap();
        let out = export(&s.model).unwrap();
        let (json, bin) = parts(&out.bytes);
        let scene = crate::view3d::Scene::new(&s.model);
        let look = crate::graphics::Look::of_3d(&s.model);
        let owners: std::collections::BTreeSet<u64> = scene.facets(&look).map(|f| f.owner).collect();
        let named: std::collections::BTreeSet<u64> =
            json["nodes"].as_array().unwrap().iter().map(|n| n["name"].as_str().unwrap().parse().unwrap()).collect();
        assert_eq!(named, owners);
        assert!(out.triangles > 100 && out.triangles <= scene.shown(&look), "{}", out.triangles);
        // Every index names a vertex of its own primitive.
        for p in json["meshes"].as_array().unwrap().iter().flat_map(|m| m["primitives"].as_array().unwrap()) {
            let count = json["accessors"][p["attributes"]["POSITION"].as_u64().unwrap() as usize]["count"].as_u64().unwrap() as u32;
            assert!(indices(&bin, &json, p["indices"].as_u64().unwrap()).iter().all(|i| *i < count));
        }
    }

    #[test]
    fn a_scene_past_the_triangle_cap_is_refused_with_how_many_there_are() {
        let e = check_count(MAX_TRIS + 1).unwrap_err();
        assert!(e.contains(&(MAX_TRIS + 1).to_string()) && e.contains(&MAX_TRIS.to_string()), "{e}");
        assert!(check_count(MAX_TRIS).is_ok());
        assert!(check_count(0).unwrap_err().contains("nothing"));
    }

    #[test]
    fn saving_as_glb_writes_the_file_through_the_command() {
        let (mut s, _) = boxes(&[(4000.0, 2000.0, 3000.0)]);
        let dir = std::env::temp_dir().join(format!("bimcraft-gltf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("scene.GLB");
        let r = s.execute("save", &json!({"path": path})).unwrap();
        assert_eq!((r["format"].as_str(), r["triangles"].as_u64(), r["elements"].as_u64()), (Some("glTF 2.0"), Some(12), Some(1)));
        let bytes = std::fs::read(&path).unwrap();
        parts(&bytes);
        assert_eq!(s.path, None, "an export is not the project's file");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
