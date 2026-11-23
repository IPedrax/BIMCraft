//! The Asset Manager through the commands every front end drives: `assets` lists what the project draws with and where each
//! is used.

use serde_json::{Value, json};

use crate::engine::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// The row of `name` in tab `kind`.
fn row(v: &Value, kind: &str, name: &str) -> Value {
    v[kind]
        .as_array()
        .unwrap_or_else(|| panic!("no {kind}: {v}"))
        .iter()
        .find(|r| r["name"] == name)
        .cloned()
        .unwrap_or_else(|| panic!("no {name} in {kind}"))
}

/// A PNG of 2 × 1 px.
fn png(path: &std::path::Path) {
    let rgba = vec![200, 0, 0, 255, 0, 0, 200, 255];
    let png = tiny_skia::Pixmap::from_vec(rgba, tiny_skia::IntSize::from_wh(2, 1).unwrap()).unwrap().encode_png().unwrap();
    std::fs::write(path, png).unwrap();
}

#[test]
fn every_kind_is_listed_with_its_id_and_guid() {
    let mut s = Session::new();
    let v = run(&mut s, "assets", json!({}));
    for kind in crate::assets::KINDS {
        assert!(v[kind].is_array(), "{kind}: {v}");
    }
    let thin = row(&v, "line_styles", "Thin Lines");
    assert!(thin["id"].as_u64().is_some_and(|id| id >= 900_000), "a library id: {thin}");
    assert!(thin["guid"].is_string(), "{thin}");
    let only = run(&mut s, "assets", json!({"kind": "line_patterns"}));
    assert_eq!(only.as_object().unwrap().len(), 1, "{only}");
    let e = s.execute("assets", &json!({"kind": "textures"})).unwrap_err();
    assert!(e.contains(crate::assets::KIND_NAMES), "{e}");
}

#[test]
fn a_material_is_used_by_type_layers_and_painted_faces() {
    let mut s = Session::new();
    run(&mut s, "material", json!({"name": "Oak", "color": "#a0522d"}));
    run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]]}));
    let wall = *s.model.elements.keys().last().unwrap();
    run(&mut s, "paint", json!({"id": wall, "face": "exterior", "material": "Oak"}));
    let v = run(&mut s, "assets", json!({"kind": "materials"}));
    let oak = row(&v, "materials", "Oak");
    assert_eq!(oak["uses"], 1, "{oak}");
    assert_eq!(oak["used_by"][0], json!({"kind": "element", "id": wall, "category": "wall"}));
    // A starter wall type's layers are made of a starter material.
    let layers = s.model.types.wall.values().flat_map(|t| t.layers.iter());
    let material = layers.map(|l| l.material.clone()).find(|m| !m.is_empty()).expect("a starter wall type's layer of a material");
    let m = row(&v, "materials", &material);
    assert!(m["used_by"].as_array().unwrap().iter().any(|u| u["kind"] == "type" && u["category"] == "wall"), "{m}");
}

#[test]
fn patterns_and_styles_name_their_users() {
    let mut s = Session::new();
    run(&mut s, "line_pattern", json!({"name": "Dash Dot", "segments": [{"dash": 6}, {"space": 2}, "dot", {"space": 2}]}));
    run(&mut s, "line_style", json!({"name": "Site", "pen": 3, "pattern": "Dash Dot"}));
    let line =
        run(&mut s, "detail_line", json!({"view": "Level 1", "points": [[0, 0], [1000, 0]], "style": "Site"}))["id"].as_u64().unwrap();
    let v = run(&mut s, "assets", json!({}));
    assert_eq!(row(&v, "line_patterns", "Dash Dot")["used_by"], json!([{"kind": "line_style", "name": "Site"}]));
    assert_eq!(row(&v, "line_styles", "Site")["used_by"], json!([{"kind": "element", "id": line, "category": "line"}]));
    // A starter fill type's pattern names the type.
    let brick = row(&v, "fill_types", "Brick");
    let pattern = s.model.region_types["Brick"].foreground.clone().unwrap();
    assert!(row(&v, "fill_patterns", &pattern)["used_by"].as_array().unwrap().contains(&json!({"kind": "fill_type", "name": "Brick"})));
    assert!(brick["guid"].is_string());
}

#[test]
fn a_picture_says_which_materials_its_texture_is() {
    let dir = std::env::temp_dir().join(format!("bimcraft-assets-{}-picture", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("oak.png");
    png(&path);
    let mut s = Session::new();
    run(&mut s, "material", json!({"name": "Oak", "texture": {"kind": "image", "path": path.to_string_lossy()}}));
    let v = run(&mut s, "assets", json!({"kind": "pictures"}));
    let pictures = v["pictures"].as_array().unwrap();
    // The starter materials hold pictures of their own (#536): Oak's is the one more.
    let oak = pictures.iter().find(|p| p["used_by"] == json!([{"kind": "material", "name": "Oak"}])).unwrap_or_else(|| panic!("{v}"));
    assert_eq!(pictures.len(), Session::new().model.images.len() + 1, "{v}");
    assert_eq!(oak["size"], json!([2, 1]));
}

/// A folder of this test's own, empty.
fn dir(test: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("bimcraft-bima-{}-{test}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A project with a material of a picture and a fill pattern, a fill type and a line style of a pattern of its own.
fn office(dir: &std::path::Path) -> Session {
    let picture = dir.join("oak.png");
    png(&picture);
    let mut s = Session::new();
    run(&mut s, "fill_pattern", json!({"name": "Planks", "target": "model", "lines": [{"angle": 0, "spacing": 150}]}));
    run(
        &mut s,
        "material",
        json!({"name": "Oak", "color": "#a0522d", "cut": "Planks", "texture": {"kind": "image", "path": picture.to_string_lossy()}}),
    );
    run(&mut s, "filled_region_type", json!({"name": "Decking", "foreground": "Planks"}));
    run(&mut s, "line_pattern", json!({"name": "Fence", "segments": [{"dash": 4}, {"space": 1}]}));
    run(&mut s, "line_style", json!({"name": "Boundary", "pen": 2, "pattern": "Fence"}));
    s
}

#[test]
fn a_bima_takes_what_an_asset_needs_and_brings_it_back_with_its_guid() {
    let dir = dir("round");
    let mut a = office(&dir);
    let path = dir.join("Office.bima").to_string_lossy().into_owned();
    let v = run(&mut a, "export_assets", json!({"path": path, "materials": ["Oak"], "line_styles": ["Boundary"]}));
    assert_eq!((v["materials"].clone(), v["fill_patterns"].clone(), v["pictures"].clone()), (json!(1), json!(1), json!(1)), "{v}");
    assert_eq!((v["line_styles"].clone(), v["line_patterns"].clone(), v["fill_types"].clone()), (json!(1), json!(1), json!(0)), "{v}");
    // It's a zip of the JSON and the picture's PNG, as the project holds it.
    let bytes = std::fs::read(&path).unwrap();
    let files = crate::bimc::unzip_files(&bytes, 1 << 26).unwrap();
    let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
    let id = a.model.materials["Oak"].texture.as_ref().unwrap().image.clone();
    assert_eq!(names, vec!["assets.json".to_string(), format!("images/{id}.png")]);
    assert_eq!(files[1].1, a.model.images[&id].file());

    let mut b = Session::new();
    let v = run(&mut b, "import_assets", json!({"path": path}));
    assert_eq!(v["left_out"], json!([]), "{v}");
    assert_eq!(b.model.materials["Oak"], a.model.materials["Oak"]);
    assert_eq!(b.model.fill_patterns["Planks"], a.model.fill_patterns["Planks"]);
    assert_eq!(b.model.line_styles["Boundary"], a.model.line_styles["Boundary"]);
    assert_eq!(b.model.images, a.model.images, "the picture's pixels, byte for byte");
    use crate::library::Kind;
    for (kind, name) in
        [(Kind::Material, "Oak"), (Kind::FillPattern, "Planks"), (Kind::LineStyle, "Boundary"), (Kind::Picture, id.as_str())]
    {
        assert_eq!(b.model.library.guid(&kind, name), a.model.library.guid(&kind, name), "{kind:?} {name} keeps its GUID");
    }
    assert!(!b.model.region_types.contains_key("Decking"), "what wasn't chosen stayed out");
}

#[test]
fn a_name_the_project_has_is_skipped_replaced_or_renamed() {
    let dir = dir("existing");
    let mut a = office(&dir);
    let path = dir.join("Office.bima").to_string_lossy().into_owned();
    run(&mut a, "export_assets", json!({"path": path}));

    let ours = || {
        let mut s = Session::new();
        run(&mut s, "fill_pattern", json!({"name": "Planks", "target": "drafting", "lines": [{"angle": 45, "spacing": 3}]}));
        run(&mut s, "material", json!({"name": "Oak", "color": "#000000"}));
        s
    };
    let mut s = ours();
    let v = run(&mut s, "import_assets", json!({"path": path, "materials": ["Oak"]}));
    assert!(v["skipped"].as_array().unwrap().contains(&json!({"kind": "materials", "name": "Oak"})), "{v}");
    assert_eq!(s.model.materials["Oak"].color, [0, 0, 0], "skip keeps the project's");

    let mut s = ours();
    run(&mut s, "import_assets", json!({"path": path, "materials": ["Oak"], "existing": "replace"}));
    assert_eq!(s.model.materials["Oak"].color, [0xa0, 0x52, 0x2d]);
    assert_eq!(s.model.fill_patterns["Planks"], a.model.fill_patterns["Planks"]);

    let mut s = ours();
    let v = run(&mut s, "import_assets", json!({"path": path, "materials": ["Oak"], "existing": "rename"}));
    assert!(v["renamed"].as_array().unwrap().contains(&json!({"kind": "fill_patterns", "from": "Planks", "to": "Planks 2"})), "{v}");
    assert_eq!(s.model.materials["Oak"].color, [0, 0, 0]);
    let oak2 = &s.model.materials["Oak 2"];
    assert_eq!(oak2.cut_pattern.as_deref(), Some("Planks 2"), "what came with it names it by its new name");
    assert_eq!(s.model.fill_patterns["Planks"].lines.len(), 1);
}

#[test]
fn a_hostile_or_newer_assets_file_is_refused_without_harm() {
    let dir = dir("hostile");
    let mut s = Session::new();
    let before = s.model.clone();
    let bad = |name: &str, bytes: &[u8]| {
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p.to_string_lossy().into_owned()
    };
    let not_zip = bad("text.bima", b"not a zip");
    let newer = crate::bimc::zip_files(&[("assets.json", br#"{"format": 999}"#, true)]).unwrap();
    let newer = bad("newer.bima", &newer);
    let lying = crate::bimc::zip_files(&[
        (
            "assets.json",
            br#"{"format": 1, "pictures": {"x": {"name": "x", "file": "images/x.png"}}, "materials": {"M": {"value": {"colour": 3}}}}"#,
            true,
        ),
        ("images/x.png", b"\x89PNG\r\n\x1a\nbroken", false),
    ])
    .unwrap();
    let lying = bad("lying.bima", &lying);
    let mut damaged = crate::bimc::zip_files(&[("assets.json", br#"{"format": 1}"#, true)]).unwrap();
    let n = damaged.len();
    damaged[n / 3] ^= 0xff;
    let damaged = bad("damaged.bima", &damaged);
    for path in [&not_zip, &newer, &damaged] {
        let e = s.execute("import_assets", &json!({"path": path})).unwrap_err();
        assert!(e.starts_with("import_assets"), "{path}: {e}");
    }
    let e = s.execute("import_assets", &json!({"path": newer})).unwrap_err();
    assert!(e.contains("newer"), "{e}");
    assert_eq!(*s.model, *before);
    // What can't be read is left out, and said.
    let v = run(&mut s, "import_assets", json!({"path": lying}));
    let left: Vec<&str> = v["left_out"].as_array().unwrap().iter().map(|l| l["kind"].as_str().unwrap()).collect();
    assert_eq!(left, vec!["pictures", "materials"], "{v}");
    assert!(!s.model.materials.contains_key("M"));
}

#[test]
fn an_asset_is_deleted_unless_something_uses_it() {
    let mut s = Session::new();
    run(&mut s, "line_pattern", json!({"name": "Fence", "segments": [{"dash": 4}, {"space": 1}]}));
    run(&mut s, "line_style", json!({"name": "Boundary", "pen": 2, "pattern": "Fence"}));
    let e = s.execute("delete_assets", &json!({"line_patterns": ["Fence"]})).unwrap_err();
    assert!(e.contains("line style Boundary"), "{e}");
    // With its user, both go.
    let v = run(&mut s, "delete_assets", json!({"line_patterns": ["Fence"], "line_styles": ["Boundary"]}));
    assert_eq!(v["deleted"].as_array().unwrap().len(), 2, "{v}");
    assert!(!s.model.line_patterns.contains_key("Fence") && !s.model.line_styles.contains_key("Boundary"));
    let v = run(&mut s, "assets", json!({"kind": "line_patterns"}));
    assert!(!v["line_patterns"].as_array().unwrap().iter().any(|r| r["name"] == "Fence"), "{v}");
    run(&mut s, "undo", json!({}));
    assert!(s.model.line_patterns.contains_key("Fence"), "undo brings them back");
}
