//! OBJ and glTF read: objects and their materials, units and up turned to millimetres with z up, a node's placement, and what
//! the reader leaves out said, never a panic.

use serde_json::json;

use crate::mesh_in::{gltf, obj};

const CUBE_OBJ: &str = "\
# a 1 x 1 x 2 box, y up, two materials
mtllib box.mtl
o Box
v 0 0 0
v 1 0 0
v 1 0 1
v 0 0 1
v 0 2 0
v 1 2 0
v 1 2 1
v 0 2 1
usemtl Red
f 1 2 3 4
f 5 8 7 6
usemtl Tint
f 1 5 6 2
f -7 -3 -2 -6
f 3 7 8 4
f 1/1/1 4/2/1 8/3/1 5/4/1
l 1 2
o Empty
";

const BOX_MTL: &str = "newmtl Red\nKd 1 0 0\nnewmtl Tint\nKd 0.5 0.5 1.0\nd 0.25\n";

#[test]
fn an_obj_comes_in_by_object_and_material_in_millimetres_with_z_up() {
    let r = obj(CUBE_OBJ, 1000.0, true, |name| (name == "box.mtl").then(|| BOX_MTL.to_string())).unwrap();
    assert_eq!(r.objects.len(), 1, "an object with no faces is no object");
    let o = &r.objects[0];
    assert_eq!(o.name, "Box");
    assert_eq!(o.triangles(), 12, "six quads, two triangles each");
    let red = o.parts.iter().find(|p| p.material.name == "Red").unwrap();
    assert_eq!(red.material.rgba, Some([255, 0, 0, 255]));
    let glass = o.parts.iter().find(|p| p.material.name == "Tint").unwrap();
    assert_eq!(glass.material.rgba, Some([128, 128, 255, 64]));
    // y up, 2 units tall: 2000 mm tall in z; the file's +z comes toward the viewer, south.
    let pts: Vec<[f64; 3]> = o.parts.iter().flat_map(|p| p.tris.iter().flatten().copied()).collect();
    let max = |k: usize| pts.iter().map(|p| p[k]).fold(f64::MIN, f64::max);
    let min = |k: usize| pts.iter().map(|p| p[k]).fold(f64::MAX, f64::min);
    assert_eq!((min(0), max(0)), (0.0, 1000.0));
    assert_eq!((min(1), max(1)), (-1000.0, 0.0));
    assert_eq!((min(2), max(2)), (0.0, 2000.0));
    assert_eq!(r.left_out.get("lines and points"), Some(&1));
}

#[test]
fn an_obj_with_z_up_keeps_its_axes_and_a_missing_library_or_vertex_is_said() {
    let text = "mtllib gone.mtl\nv 0 0 0\nv 1000 0 0\nv 0 0 1000\nf 1 2 3\nf 1 2 9\n";
    let r = obj(text, 1.0, false, |_| None).unwrap();
    let t = r.objects[0].parts[0].tris[0];
    assert_eq!(t, [[0.0, 0.0, 0.0], [1000.0, 0.0, 0.0], [0.0, 0.0, 1000.0]]);
    assert_eq!(r.objects[0].parts[0].material.rgba, None);
    assert_eq!(r.left_out.get("material libraries not found"), Some(&1));
    assert_eq!(r.left_out.get("faces naming a vertex the file doesn't have"), Some(&1));
}

#[test]
fn hostile_obj_text_is_read_or_refused_never_a_panic() {
    for text in [
        "",
        "f",
        "f 1 2 3",
        "v nan nan nan\nv 1 1 1\nv 2 2 2\nf 1 2 3",
        "v 1e300 0 0\nv 0 1 0\nv 0 0 1\nf 1 2 3",
        "f -1 -2 -3",
        "v 0 0 0\nf 1 1 1",
    ] {
        let r = obj(text, 1.0, true, |_| None).unwrap();
        assert!(r.objects.is_empty(), "{text:?}: {:?}", r.objects);
    }
    assert!(obj("v 0 0 0", 0.0, true, |_| None).is_err());
    assert!(obj("v 0 0 0", f64::NAN, true, |_| None).is_err());
}

/// A glTF of one triangle (0,0,0), (1,0,0), (0,1,0) in a buffer, its node moved and named as given.
fn triangle_gltf(node: serde_json::Value, buffer_uri: Option<&str>) -> (serde_json::Value, Vec<u8>) {
    let mut bin: Vec<u8> = Vec::new();
    for v in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        bin.extend(v.to_le_bytes());
    }
    for i in [0u16, 1, 2, 0] {
        bin.extend(i.to_le_bytes());
    }
    let mut buffer = json!({"byteLength": bin.len()});
    if let Some(uri) = buffer_uri {
        buffer["uri"] = json!(uri);
    }
    let doc = json!({
        "asset": {"version": "2.0"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [node],
        "meshes": [{"name": "Tri", "primitives": [{"attributes": {"POSITION": 0}, "indices": 1, "material": 0}]}],
        "materials": [{"name": "Brick", "pbrMetallicRoughness": {"baseColorFactor": [1.0, 0.215_860_5, 0.0, 1.0]}}],
        "buffers": [buffer],
        "bufferViews": [{"buffer": 0, "byteLength": 36}, {"buffer": 0, "byteOffset": 36, "byteLength": 6}],
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0, 0, 0], "max": [1, 1, 0]},
            {"bufferView": 1, "componentType": 5123, "count": 3, "type": "SCALAR"}
        ]
    });
    (doc, bin)
}

/// `doc` and `bin` as a GLB.
fn glb(doc: &serde_json::Value, bin: &[u8]) -> Vec<u8> {
    let mut json = serde_json::to_vec(doc).unwrap();
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    let mut bin = bin.to_vec();
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let mut out = b"glTF".to_vec();
    out.extend(2u32.to_le_bytes());
    out.extend(((12 + 8 + json.len() + 8 + bin.len()) as u32).to_le_bytes());
    out.extend((json.len() as u32).to_le_bytes());
    out.extend(0x4E4F_534Au32.to_le_bytes());
    out.extend(json);
    out.extend((bin.len() as u32).to_le_bytes());
    out.extend(0x004E_4942u32.to_le_bytes());
    out.extend(bin);
    out
}

#[test]
fn a_glb_node_comes_in_placed_in_millimetres_with_z_up_and_its_material_in_srgb() {
    let (doc, bin) = triangle_gltf(json!({"name": "Chair", "mesh": 0, "translation": [2.0, 0.0, 0.0]}), None);
    let r = gltf(&glb(&doc, &bin), |_| None).unwrap();
    assert_eq!(r.objects.len(), 1);
    let o = &r.objects[0];
    assert_eq!(o.name, "Chair");
    let part = &o.parts[0];
    assert_eq!(part.material.name, "Brick");
    assert_eq!(part.material.rgba, Some([255, 128, 0, 255]), "the factor is linear; the colour is sRGB");
    // Moved 2 m east; the file's y up is our z.
    assert_eq!(part.tris, vec![[[2000.0, 0.0, 0.0], [3000.0, 0.0, 0.0], [2000.0, 0.0, 1000.0]]]);
    assert!(r.left_out.is_empty(), "{:?}", r.left_out);
}

#[test]
fn a_gltf_reads_its_buffer_from_a_file_beside_it_or_a_data_uri_and_a_mirrored_node_faces_out() {
    let (doc, bin) = triangle_gltf(json!({"mesh": 0, "scale": [-1.0, 1.0, 1.0]}), Some("tri%20data.bin"));
    let text = serde_json::to_vec(&doc).unwrap();
    let r = gltf(&text, |name| (name == "tri data.bin").then(|| bin.clone())).unwrap();
    let t = r.objects[0].parts[0].tris[0];
    assert_eq!(r.objects[0].name, "Tri", "a node without a name takes its mesh's");
    // Mirrored in x, its winding turned back: (0,0,0), (0,0,1000), (-1000,0,0).
    assert_eq!(t, [[0.0, 0.0, 0.0], [0.0, 0.0, 1000.0], [-1000.0, 0.0, 0.0]]);
    let b64 = {
        const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut s = String::new();
        for c in bin.chunks(3) {
            let n = c.iter().enumerate().fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
            for i in 0..4 {
                s.push(if i <= c.len() { char::from(A[(n >> (18 - 6 * i) & 63) as usize]) } else { '=' });
            }
        }
        s
    };
    let (doc, _) = triangle_gltf(json!({"mesh": 0}), Some(&format!("data:application/octet-stream;base64,{b64}")));
    let r = gltf(&serde_json::to_vec(&doc).unwrap(), |_| None).unwrap();
    assert_eq!(r.objects[0].triangles(), 1);
}

#[test]
fn a_compressed_gltf_is_refused_with_why_and_hostile_bytes_never_panic() {
    let (mut doc, bin) = triangle_gltf(json!({"mesh": 0}), None);
    doc["extensionsRequired"] = json!(["KHR_draco_mesh_compression"]);
    let e = gltf(&glb(&doc, &bin), |_| None).unwrap_err();
    assert!(e.contains("compressed"), "{e}");
    let (doc, bin) = triangle_gltf(json!({"mesh": 0}), None);
    let good = glb(&doc, &bin);
    for cut in [0, 4, 12, 20, good.len() / 2, good.len() - 1] {
        let _ = gltf(&good[..cut], |_| None);
    }
    for i in (0..good.len()).step_by(7) {
        let mut bad = good.clone();
        bad[i] ^= 0xFF;
        let _ = gltf(&bad, |_| None);
    }
    // Offsets, lengths and strides as large as JSON holds: left out, not added past what a number holds.
    for (view, key, v) in [(0, "byteOffset", u64::MAX), (0, "byteLength", u64::MAX), (0, "byteStride", u64::MAX), (1, "byteOffset", u64::MAX - 8)] {
        let (mut doc, bin) = triangle_gltf(json!({"mesh": 0}), None);
        doc["bufferViews"][view][key] = json!(v);
        doc["accessors"][view]["byteOffset"] = json!(16);
        let _ = gltf(&glb(&doc, &bin), |_| None);
    }
    // A node holding itself is read once, not for ever.
    let (mut doc, bin) = triangle_gltf(json!({"mesh": 0, "children": [0]}), None);
    doc["scenes"] = json!([{"nodes": [0]}]);
    let r = gltf(&glb(&doc, &bin), |_| None).unwrap();
    assert!(r.objects.len() <= 65);
}

/// A folder of this test's own.
fn dir(test: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("bimcraft-mesh-{}-{test}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn import_mesh_makes_a_generic_family_of_each_object_placed_where_the_file_has_it_with_its_colours() {
    use crate::engine::Session;
    use crate::model::Element;
    let d = dir("obj");
    // The box moved 3 m east and 1 m up in the file (y up).
    let moved: String = CUBE_OBJ
        .lines()
        .map(|l| match l.strip_prefix("v ") {
            Some(v) => {
                let n: Vec<f64> = v.split(' ').map(|x| x.parse().unwrap()).collect();
                format!("v {} {} {}\n", n[0] + 3.0, n[1] + 1.0, n[2])
            }
            None => format!("{l}\n"),
        })
        .collect();
    std::fs::write(d.join("box.obj"), moved).unwrap();
    std::fs::write(d.join("box.mtl"), BOX_MTL).unwrap();
    let mut s = Session::new();
    let r = s.execute("import_mesh", &json!({"path": d.join("box.obj"), "at": [100, 200]})).unwrap();
    assert_eq!(r["families"], json!(["Box"]));
    let id = r["components"][0].as_u64().unwrap();
    let Some(Element::Component(c)) = s.model.elements.get(id) else { panic!("a component") };
    // Its middle where the file has it, on the file's origin moved to `at`; its base 1 m over the level.
    assert_eq!((c.at.0, c.at.1, c.offset), (100.0 + 3500.0, 200.0 - 500.0, 1000.0));
    let f = &s.model.families["Box"];
    assert_eq!(f.category, crate::family::Category::Generic);
    assert_eq!(f.forms.len(), 2);
    let zs: Vec<f64> = f.forms.iter().flat_map(|form| form.mesh.iter().flatten().map(|p| p[2])).collect();
    assert_eq!(zs.iter().copied().fold(f64::MAX, f64::min), 0.0, "the family stands on its base");
    assert_eq!(s.model.materials["Red"].color, [255, 0, 0]);
    assert_eq!(s.model.materials["Tint"].transparency, 75);
    // One undo takes it all back.
    s.execute("undo", &json!({})).unwrap();
    assert!(!s.model.families.contains_key("Box"));
    // Again: a second family of the name is numbered.
    s.execute("redo", &json!({})).unwrap();
    let r = s.execute("import_mesh", &json!({"path": d.join("box.obj")})).unwrap();
    assert_eq!(r["families"], json!(["Box (2)"]));
    // It goes out to IFC as a proxy with its body.
    let ifc = crate::ifc::export(&s.model);
    assert_eq!(ifc.matches("IFCBUILDINGELEMENTPROXY(").count(), 2, "{}", ifc.len());
}

#[test]
fn import_mesh_refuses_what_it_cant_read_and_a_material_file_outside_its_folder() {
    use crate::engine::Session;
    let d = dir("refuse");
    let mut s = Session::new();
    std::fs::write(d.join("a.stl"), "solid").unwrap();
    assert!(s.execute("import_mesh", &json!({"path": d.join("a.stl")})).unwrap_err().contains(".obj"));
    std::fs::write(d.join("empty.obj"), "# nothing\n").unwrap();
    assert!(s.execute("import_mesh", &json!({"path": d.join("empty.obj")})).unwrap_err().contains("no triangles"));
    std::fs::write(d.join("t.obj"), "mtllib ../secret.mtl\nusemtl X\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n").unwrap();
    let r = s.execute("import_mesh", &json!({"path": d.join("t.obj"), "units": "mm", "up": "z"})).unwrap();
    assert_eq!(r["left_out"]["material libraries not found"], 1);
    assert!(s.execute("import_mesh", &json!({"path": d.join("t.obj"), "up": "x"})).is_err());
    let (doc, bin) = triangle_gltf(json!({"mesh": 0}), None);
    std::fs::write(d.join("t.glb"), glb(&doc, &bin)).unwrap();
    assert!(s.execute("import_mesh", &json!({"path": d.join("t.glb"), "units": "mm"})).is_err(), "a glTF's unit is the file's");
    assert_eq!(s.execute("import_mesh", &json!({"path": d.join("t.glb")})).unwrap()["families"], json!(["Tri"]));
}

#[test]
fn an_object_past_what_a_family_holds_comes_in_as_pieces_that_do() {
    let n = crate::family::MAX_TRIANGLES + 10;
    let mut text = String::from("o Big\nusemtl A\n");
    for i in 0..n {
        let x = i as f64;
        text += &format!("v {x} 0 0\nv {} 0 0\nv {x} 1 0\nf -3 -2 -1\n", x + 1.0);
    }
    let r = obj(&text, 1.0, false, |_| None).unwrap();
    assert_eq!(r.objects.len(), 2);
    assert_eq!((r.objects[0].name.as_str(), r.objects[1].name.as_str()), ("Big", "Big 2"));
    assert_eq!(r.objects.iter().map(|o| o.triangles()).sum::<usize>(), n, "nothing lost");
    assert!(r.objects.iter().all(|o| o.triangles() <= crate::family::MAX_TRIANGLES));
}
