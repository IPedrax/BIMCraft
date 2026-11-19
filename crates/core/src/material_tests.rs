//! Materials as a front end drives them: the patterns of their surfaces and cuts, duplicates, RAL colours, parameters and
//! the cube they are previewed on.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom::P2;
use crate::render::{Prim, Rgb, View};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn held(s: &Session, name: &str) -> Value {
    serde_json::to_value(&s.model.materials[name]).unwrap()
}

/// A floor of `material` 6 by 4 metres, its plan's fine detail on, and the view of it.
fn floor_of(s: &mut Session, material: &str) -> (u64, View) {
    let layers = json!([{"function": "finish", "material": material, "thickness": 200}]);
    run(s, "type", json!({"category": "floor", "name": "Test floor", "layers": layers}));
    let boundary = json!([[0, 0], [6000, 0], [6000, 4000], [0, 4000]]);
    let f = run(s, "floor", json!({"boundary": boundary, "type": "Test floor"}))["id"].as_u64().unwrap();
    run(s, "detail", json!({"view": 100000, "detail": "fine"}));
    (f, View { angle: 0.0, scale: 0.2, cx: 3000.0, cy: 2000.0, width: 1200, height: 800 })
}

/// The colours what `owner` shows are drawn in, in the order they are drawn: its fills, and its thin lines.
fn inks(d: &crate::render::Drawing, owner: u64) -> (Vec<Rgb>, Vec<Rgb>) {
    let (mut fills, mut lines) = (Vec::new(), Vec::new());
    for (p, o) in d.prims.iter().zip(&d.owners) {
        if *o != Some(owner) {
            continue;
        }
        match p {
            Prim::Fill { color, .. } => fills.push(*color),
            Prim::Line { color, width, .. } if *width < d.width(crate::pen::Pen(4)) => lines.push(*color),
            _ => {}
        }
    }
    (fills, lines)
}

#[test]
fn a_material_holds_a_foreground_and_a_background_for_its_surface_and_its_cut() {
    let mut s = Session::new();
    let reply = run(
        &mut s,
        "material",
        json!({
            "name": "Clad",
            "surface": "diagonal", "surface_color": "#102030",
            "surface_background": "solid", "surface_background_color": [200, 210, 220],
            "cut": "crosshatch", "cut_color": "#405060",
            "cut_background": "grid", "cut_background_color": "#708090",
        }),
    );
    assert_eq!(reply["material"]["surface_color"], json!([16, 32, 48]));
    let m = held(&s, "Clad");
    assert_eq!(m["surface_background"], json!({"pattern": "solid", "color": [200, 210, 220]}));
    assert_eq!(m["cut_background"], json!({"pattern": "grid", "color": [112, 128, 144]}));
    // Taken off again, each by itself.
    run(&mut s, "material", json!({"name": "Clad", "surface_background": "none", "cut_color": null}));
    let m = held(&s, "Clad");
    assert!(m.get("surface_background").is_none() && m.get("cut_color").is_none(), "{m}");
    assert_eq!(m["cut_background"]["pattern"], "grid");
    // A pattern that isn't one, and a colour that isn't one, are refused and change nothing.
    for bad in [json!({"surface_background": "plaid"}), json!({"cut_background_color": "pink"}), json!({"surface_color": [1, 2]})] {
        let mut p = bad.clone();
        p["name"] = json!("Clad");
        assert!(s.execute("material", &p).is_err(), "{bad}");
    }
    assert_eq!(held(&s, "Clad"), m);
}

#[test]
fn a_misspelled_key_of_a_material_is_refused() {
    let mut s = Session::new();
    let e = s.execute("material", &json!({"name": "Oak", "surfce": "solid"})).unwrap_err();
    assert!(e.contains("surfce"), "{e}");
}

#[test]
fn a_floors_surface_shows_its_background_under_its_foreground_each_in_its_colour() {
    let mut s = Session::new();
    run(
        &mut s,
        "material",
        json!({"name": "Clad", "surface": "horizontal", "surface_color": "#ff0000", "surface_background": "solid", "surface_background_color": "#00ff00"}),
    );
    let (f, view) = floor_of(&mut s, "Clad");
    let d = crate::render::draw(&s.model, 100000, view);
    let (fills, lines) = inks(&d, f);
    assert!(fills.contains(&Rgb(0, 255, 0)), "the background's fill: {fills:?}");
    assert!(lines.contains(&Rgb(255, 0, 0)), "the foreground's lines: {lines:?}");
    // Under it: the fill comes before the first of the foreground's lines.
    let at = |want: &dyn Fn(&Prim) -> bool| d.prims.iter().zip(&d.owners).position(|(p, o)| *o == Some(f) && want(p)).unwrap();
    let fill = at(&|p| matches!(p, Prim::Fill { color, .. } if *color == Rgb(0, 255, 0)));
    let line = at(&|p| matches!(p, Prim::Line { color, .. } if *color == Rgb(255, 0, 0)));
    assert!(fill < line, "fill {fill}, line {line}");
}

#[test]
fn a_solid_surface_fills_however_far_out_the_view_is() {
    let mut s = Session::new();
    run(&mut s, "material", json!({"name": "Paint", "surface": "solid", "surface_color": "#3366cc"}));
    let (f, mut view) = floor_of(&mut s, "Paint");
    view.scale = 0.02;
    let d = crate::render::draw(&s.model, 100000, view);
    assert!(inks(&d, f).0.contains(&Rgb(0x33, 0x66, 0xcc)));
}

#[test]
fn a_cut_layer_shows_its_background_and_its_foreground_each_in_its_colour() {
    let mut s = Session::new();
    run(
        &mut s,
        "material",
        json!({"name": "Clad", "cut": "diagonal", "cut_color": "#ff0000", "cut_background": "solid", "cut_background_color": "#00ff00"}),
    );
    let layers = json!([{"function": "structure", "material": "Clad", "thickness": 300}]);
    run(&mut s, "type", json!({"category": "wall", "name": "Clad 300", "layers": layers}));
    let w = run(&mut s, "wall", json!({"points": [[0, 0], [6000, 0]], "type": "Clad 300"}))["ids"][0].as_u64().unwrap();
    run(&mut s, "detail", json!({"view": 100000, "detail": "fine"}));
    let view = View { angle: 0.0, scale: 0.2, cx: 3000.0, cy: 0.0, width: 1200, height: 800 };
    let d = crate::render::draw(&s.model, 100000, view);
    let (fills, lines) = inks(&d, w);
    assert!(fills.contains(&Rgb(0, 255, 0)), "{fills:?}");
    assert!(lines.contains(&Rgb(255, 0, 0)), "{lines:?}");
}

#[test]
fn a_material_without_the_new_patterns_draws_as_it_did() {
    let mut s = Session::new();
    run(&mut s, "material", json!({"name": "Plain", "surface": "horizontal"}));
    let (f, view) = floor_of(&mut s, "Plain");
    let d = crate::render::draw(&s.model, 100000, view);
    let (fills, lines) = inks(&d, f);
    assert!(fills.is_empty(), "{fills:?}");
    assert!(lines.iter().all(|c| *c == crate::render::FAINT), "{lines:?}");
}

#[test]
fn a_fill_pattern_a_background_shows_is_kept_and_renamed_with_it() {
    let mut s = Session::new();
    let name = "Clad Hatch";
    run(&mut s, "fill_pattern", json!({"name": name, "lines": [{"angle": 30, "spacing": 2}]}));
    run(&mut s, "material", json!({"name": "Clad", "cut_background": name}));
    assert_eq!(held(&s, "Clad")["cut_background"]["fill"], json!(name));
    let e = s.execute("fill_pattern", &json!({"name": name, "delete": true})).unwrap_err();
    assert!(e.contains("Clad"), "{e}");
    run(&mut s, "fill_pattern", json!({"name": name, "rename": "Renamed Pattern"}));
    assert_eq!(held(&s, "Clad")["cut_background"]["fill"], "Renamed Pattern");
}

#[test]
fn duplicate_copies_a_material_under_a_new_name() {
    let mut s = Session::new();
    run(&mut s, "material", json!({"name": "Oak", "color": "#a0703c", "surface": "diagonal", "parameters": {"Mark": "M1"}}));
    let reply = run(&mut s, "material", json!({"name": "Oak", "duplicate": "Oak dark"}));
    assert_eq!(reply["name"], "Oak dark");
    assert_eq!(held(&s, "Oak dark"), held(&s, "Oak"));
    // Its own: changing the copy leaves the first as it was.
    run(&mut s, "material", json!({"name": "Oak dark", "color": "#402010"}));
    assert_eq!(held(&s, "Oak")["color"], json!([160, 112, 60]));
    // It has a GUID of its own.
    let kind = crate::library::Kind::Material;
    assert_ne!(s.model.library.guid(&kind, "Oak"), s.model.library.guid(&kind, "Oak dark"));
    for (p, says) in [
        (json!({"name": "Oak", "duplicate": "Oak dark"}), "already"),
        (json!({"name": "Nothing", "duplicate": "Other"}), "Nothing"),
        (json!({"name": "Oak", "duplicate": "  "}), "name"),
    ] {
        let e = s.execute("material", &p).unwrap_err();
        assert!(e.contains(says), "{p}: {e}");
    }
}

#[test]
fn every_ral_colour_has_a_code_a_name_and_a_colour_near_what_it_is_known_by() {
    let all = crate::ral::ALL;
    assert_eq!(all.len(), 213);
    let mut codes: Vec<u16> = all.iter().map(|c| c.code).collect();
    codes.dedup();
    assert_eq!(codes.len(), all.len(), "in order, each once");
    assert!(all.iter().all(|c| (1000..=9023).contains(&c.code) && !c.name.is_empty()));
    let rgb = |code: u16| all.iter().find(|c| c.code == code).unwrap().rgb();
    let near = |a: [u8; 3], b: [u8; 3]| a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 14);
    // Reds are red, blues blue, a white white and a black black.
    assert!(near(rgb(9016), [241, 240, 234]), "traffic white {:?}", rgb(9016));
    assert!(near(rgb(9005), [14, 14, 16]), "jet black {:?}", rgb(9005));
    let [r, g, b] = rgb(3020);
    assert!(r > 150 && g < 50 && b < 50, "traffic red {:?}", rgb(3020));
    let [r, g, b] = rgb(5015);
    assert!(b > 130 && r < 50 && g > 70, "sky blue {:?}", rgb(5015));
}

/// A file of the user's collections, and a CSV beside it, for this test alone.
fn scratch(test: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let folder = std::env::temp_dir().join(format!("bimcraft-colors-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    (folder.join("kept.json"), folder.join("table.csv"))
}

#[test]
fn standard_colours_are_found_by_code_or_by_name() {
    let (kept, _) = scratch("find");
    let mut s = Session::new();
    let mut list = |p: Value| {
        let mut p = p;
        p["path"] = json!(kept);
        run(&mut s, "standard_color", p)
    };
    let codes = |reply: &Value| -> Vec<String> {
        reply["colors"].as_array().unwrap().iter().map(|c| c["code"].as_str().unwrap().to_string()).collect()
    };
    assert_eq!(codes(&list(json!({}))).len(), 213);
    assert_eq!(codes(&list(json!({"search": "3020"}))), ["RAL 3020"]);
    assert_eq!(codes(&list(json!({"search": "ral3020"}))), ["RAL 3020"]);
    assert!(codes(&list(json!({"search": "TRAFFIC RED"}))).contains(&"RAL 3020".to_string()));
    assert!(codes(&list(json!({"search": "nothing like it"}))).is_empty());
    assert_eq!(codes(&list(json!({"collection": "css", "search": "rebecca"}))), ["CSS rebeccapurple"]);
    // Each says where its colours are from, and which there are.
    let reply = list(json!({}));
    assert!(reply["source"].as_str().unwrap().contains("Color library"));
    assert_eq!(reply["collections"].as_array().unwrap().len(), 2);
    assert!(list(json!({"collection": "css"}))["source"].as_str().unwrap().contains("W3C"));
    assert!(s.model.materials.keys().all(|n| !n.starts_with("RAL")));
}

#[test]
fn a_standard_colour_makes_a_solid_material_named_for_it() {
    let (kept, _) = scratch("make");
    let mut s = Session::new();
    let reply = run(&mut s, "standard_color", json!({"code": "RAL 3020", "path": kept}));
    assert_eq!((reply["name"].as_str(), reply["created"].as_bool()), (Some("RAL 3020 Traffic red"), Some(true)));
    let want = crate::ral::ALL.iter().find(|c| c.code == 3020).unwrap().rgb();
    let m = &s.model.materials["RAL 3020 Traffic red"];
    assert_eq!((m.color, m.surface, m.cut), (want, crate::model::Pattern::Solid, crate::model::Pattern::Solid));
    assert_eq!((m.surface_color, m.cut_color), (Some(want), Some(want)));
    // Asked again it is the one there is, however the code is written, and what was done to it stays.
    run(&mut s, "material", json!({"name": "RAL 3020 Traffic red", "transparency": 20}));
    let reply = run(&mut s, "standard_color", json!({"code": 3020, "path": kept}));
    assert_eq!(reply["created"], false);
    assert_eq!(s.model.materials["RAL 3020 Traffic red"].transparency, 20);
    // A CSS colour has a name for a code.
    let reply = run(&mut s, "standard_color", json!({"collection": "CSS", "code": "tomato", "path": kept}));
    assert_eq!(reply["name"], "CSS tomato");
    assert_eq!(s.model.materials["CSS tomato"].color, [255, 99, 71]);
    for bad in [
        json!({"code": "RAL 1"}),
        json!({"code": "red"}),
        json!({"code": 12.5}),
        json!({"code": true}),
        json!({"code": 1000, "collection": "None"}),
    ] {
        let mut p = bad.clone();
        p["path"] = json!(kept);
        assert!(s.execute("standard_color", &p).is_err(), "{bad}");
    }
}

#[test]
fn a_table_of_colours_is_imported_for_every_project_and_taken_away_again() {
    let (kept, csv) = scratch("import");
    std::fs::write(&csv, "Code,Name,Colour\n18 E 53,Poppy,#c8102e\n\"10 B 15\",\"Sand, light\",#e0d0b0\n").unwrap();
    let mut s = Session::new();
    let reply = run(
        &mut s,
        "standard_color",
        json!({"action": "import", "collection": "Office", "file": csv, "source": "Our licensed table", "path": kept}),
    );
    assert_eq!(reply["count"], 2);
    // In another session too: it is the user's, not the project's.
    let mut other = Session::new();
    let reply = run(&mut other, "standard_color", json!({"collection": "office", "path": kept}));
    assert_eq!((reply["count"].as_u64(), reply["source"].as_str()), (Some(2), Some("Our licensed table")));
    assert_eq!(reply["colors"][1]["name"], "Sand, light");
    let reply = run(&mut other, "standard_color", json!({"collection": "Office", "code": "18e53", "path": kept}));
    assert_eq!(reply["name"], "Office 18 E 53 Poppy");
    assert_eq!(other.model.materials["Office 18 E 53 Poppy"].color, [0xc8, 0x10, 0x2e]);
    assert!(!serde_json::to_string(&*other.model).unwrap().contains("Our licensed table"));
    // Kept unless told to replace it, BIMCraft's own names are its own, and only the user's go.
    let again = json!({"action": "import", "collection": "Office", "file": csv, "path": kept});
    assert!(s.execute("standard_color", &again).unwrap_err().contains("replace"));
    let own = json!({"action": "import", "collection": "ral", "file": csv, "path": kept});
    assert!(s.execute("standard_color", &own).is_err());
    assert!(s.execute("standard_color", &json!({"action": "remove", "collection": "RAL", "path": kept})).is_err());
    run(&mut s, "standard_color", json!({"action": "remove", "collection": "Office", "path": kept}));
    assert!(s.execute("standard_color", &json!({"collection": "Office", "path": kept})).is_err());
}

#[test]
fn a_table_that_is_not_one_is_refused_by_its_row() {
    use crate::standard_colors::parse_csv;
    assert_eq!(parse_csv("1,One,10,20,30").unwrap()[0].rgb, [10, 20, 30]);
    for (text, says) in [
        ("", "no colours"),
        ("a,b\nc,d\n", "row 2"),
        ("1,One,#ff0000\n2,Two,#gg0000\n", "row 2"),
        ("Code,Name,Colour\n1,One,300,0,0\n", "row 2"),
        ("1,One,#ff0000\n1,Again,#00ff00\n", "already"),
        (",One,#ff0000\n", "code"),
    ] {
        let e = parse_csv(text).map(|c| format!("{c:?}")).unwrap_err();
        assert!(e.contains(says), "{text:?}: {e}");
    }
}

#[test]
fn the_css_colours_are_the_specifications() {
    let css = crate::standard_colors::CSS;
    assert!(css.windows(2).all(|w| w[0].0 < w[1].0), "by name, each once");
    let of = |n: &str| css.iter().find(|(x, _)| *x == n).unwrap().1;
    assert_eq!((of("rebeccapurple"), of("cornflowerblue"), of("grey")), ([102, 51, 153], [100, 149, 237], of("gray")));
}

#[test]
fn materials_have_built_in_parameters() {
    let mut s = Session::new();
    run(
        &mut s,
        "material",
        json!({"name": "Oak", "parameters": {"Mark": "M-01", "Description": "Oiled oak", "Keywords": "timber, floor", "Cost": 42.5}}),
    );
    let m = held(&s, "Oak");
    assert_eq!(m["parameters"]["Mark"], "M-01");
    assert_eq!(m["parameters"]["Cost"], 42.5);
    // Cleared by null or by nothing typed.
    run(&mut s, "material", json!({"name": "Oak", "parameters": {"Mark": null, "Keywords": ""}}));
    let m = held(&s, "Oak");
    assert!(m["parameters"].get("Mark").is_none() && m["parameters"].get("Keywords").is_none(), "{m}");
    // Each holds what it says it holds.
    for bad in [json!({"Cost": "cheap"}), json!({"Cost": -1}), json!({"Mark": 3}), json!({"Nonsense": "x"})] {
        let e = s.execute("material", &json!({"name": "Oak", "parameters": bad})).unwrap_err();
        assert!(e.contains("parameter") || e.contains("Cost") || e.contains("Mark"), "{bad}: {e}");
    }
    // A parameter of the project's can't be for materials under a built-in's name.
    let e = s.execute("parameter", &json!({"name": "Mark", "categories": ["material"]})).unwrap_err();
    assert!(e.contains("Mark"), "{e}");
}

#[test]
fn project_parameters_can_be_for_materials() {
    let mut s = Session::new();
    run(&mut s, "parameter", json!({"name": "Fire Rating", "kind": "text", "categories": ["materials"]}));
    assert_eq!(s.model.parameters["Fire Rating"].categories, ["material"]);
    run(&mut s, "parameter", json!({"name": "Recycled", "kind": "yesno", "categories": ["material", "wall"]}));
    run(&mut s, "material", json!({"name": "Oak", "parameters": {"Fire Rating": "D-s2", "Recycled": true}}));
    assert_eq!(held(&s, "Oak")["parameters"]["Fire Rating"], "D-s2");
    // Of its kind, and only one that is for materials.
    assert!(s.execute("material", &json!({"name": "Oak", "parameters": {"Recycled": "maybe"}})).is_err());
    run(&mut s, "parameter", json!({"name": "Wall only", "categories": ["wall"]}));
    assert!(s.execute("material", &json!({"name": "Oak", "parameters": {"Wall only": "x"}})).is_err());
    // Materials have no types.
    let e = s.execute("parameter", &json!({"name": "By type", "categories": ["material"], "scope": "type"})).unwrap_err();
    assert!(e.contains("material"), "{e}");
    // Its kind stays while a material holds a value of it.
    assert!(s.execute("parameter", &json!({"name": "Fire Rating", "kind": "number"})).is_err());
    // Renamed, the values follow; off the category or deleted, they go.
    run(&mut s, "parameter", json!({"name": "Fire Rating", "rename": "Reaction to Fire"}));
    assert_eq!(held(&s, "Oak")["parameters"]["Reaction to Fire"], "D-s2");
    run(&mut s, "parameter", json!({"name": "Recycled", "categories": ["wall"]}));
    assert!(held(&s, "Oak")["parameters"].get("Recycled").is_none());
    run(&mut s, "parameter", json!({"name": "Reaction to Fire", "delete": true}));
    assert!(held(&s, "Oak").get("parameters").is_none());
}

#[test]
fn merged_parameters_bring_a_materials_values_along() {
    let mut s = Session::new();
    run(&mut s, "parameter", json!({"name": "Rating", "categories": ["material"]}));
    run(&mut s, "parameter", json!({"name": "Grade", "categories": ["material"]}));
    run(&mut s, "material", json!({"name": "Oak", "parameters": {"Rating": "A"}}));
    run(&mut s, "parameter_merge", json!({"from": "Rating", "into": "Grade"}));
    assert_eq!(held(&s, "Oak")["parameters"], json!({"Grade": "A"}));
}

#[test]
fn a_material_saved_and_read_keeps_what_it_holds() {
    let mut s = Session::new();
    run(&mut s, "parameter", json!({"name": "Rating", "categories": ["material"]}));
    run(
        &mut s,
        "material",
        json!({"name": "Oak", "surface_background": "solid", "surface_background_color": "#aabbcc", "parameters": {"Rating": "A", "Mark": "M1"}}),
    );
    let text = serde_json::to_string(&*s.model).unwrap();
    let (back, left_out) = crate::model::Model::read(&text).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(back.materials["Oak"], s.model.materials["Oak"]);
}

#[test]
fn a_value_of_a_parameter_the_file_has_not_is_left_out_on_reading() {
    let mut s = Session::new();
    run(&mut s, "parameter", json!({"name": "Rating", "categories": ["material"]}));
    run(&mut s, "material", json!({"name": "Oak", "parameters": {"Rating": "A", "Mark": "M1"}}));
    let mut v = serde_json::to_value(&*s.model).unwrap();
    v["parameters"].as_object_mut().unwrap().remove("Rating");
    let (back, _) = crate::model::Model::read(&v.to_string()).unwrap();
    assert_eq!(serde_json::to_value(&back.materials["Oak"].parameters).unwrap(), json!({"Mark": "M1"}));
    back.check().unwrap();
}

#[test]
fn a_favorite_with_a_parameter_this_project_has_not_comes_in_without_it() {
    let file = std::env::temp_dir().join(format!("bimcraft-material-favorites-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&file);
    let mut s = Session::new();
    run(&mut s, "parameter", json!({"name": "Rating", "categories": ["material"]}));
    run(&mut s, "material", json!({"name": "Oak", "parameters": {"Rating": "A", "Mark": "M1"}}));
    run(&mut s, "favorite", json!({"action": "save", "name": "Oak", "path": file}));
    let mut other = Session::new();
    run(&mut other, "favorite", json!({"action": "load", "name": "Oak", "path": file}));
    assert_eq!(serde_json::to_value(&other.model.materials["Oak"].parameters).unwrap(), json!({"Mark": "M1"}));
    let _ = std::fs::remove_file(file);
}

#[test]
fn a_jpeg_or_a_png_becomes_a_materials_texture() {
    let jpeg = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/gradient.jpg");
    let mut s = Session::new();
    run(&mut s, "material", json!({"name": "Photo", "texture": {"kind": "image", "path": jpeg}}));
    let t = s.model.materials["Photo"].texture.clone().unwrap();
    assert_eq!(t.kind, crate::texture::Kind::Image);
    assert!(s.model.images.contains_key(&t.image));
    let not_a_picture = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
    assert!(s.execute("material", &json!({"name": "Photo", "texture": {"kind": "image", "path": not_a_picture}})).is_err());
}

#[test]
fn the_cube_shows_a_materials_colour_on_three_shaded_faces_and_nothing_round_it() {
    let m = crate::model::Material { color: [200, 40, 40], ..Default::default() };
    let side = 96;
    let cube = crate::materials::cube(&m, &Default::default(), side, crate::materials::CUBE_VIEW);
    assert_eq!(cube.len(), side * side * 4);
    let at = |x: usize, y: usize| -> [u8; 4] { cube[(y * side + x) * 4..][..4].try_into().unwrap() };
    assert_eq!(at(0, 0)[3], 0, "the corner is clear");
    // Three faces meet at the middle: the top, and the two sides, each lit differently, all of the material's hue.
    let (top, left, right) = (at(side / 2, side / 5), at(side / 3, side * 3 / 5), at(side * 2 / 3, side * 3 / 5));
    for c in [top, left, right] {
        assert_eq!(c[3], 255, "{c:?}");
        assert!(c[0] > c[1] && c[1] == c[2], "{c:?}");
    }
    assert!(top[0] != left[0] && left[0] != right[0], "{top:?} {left:?} {right:?}");
}

#[test]
fn the_cube_is_seen_through_as_a_transparent_material_is_and_shows_its_texture() {
    let solid = crate::model::Material { color: [200, 40, 40], ..Default::default() };
    let clear = crate::model::Material { transparency: 70, ..solid.clone() };
    let side = 96;
    let view = crate::materials::CUBE_VIEW;
    let images = Default::default();
    let middle = |c: &[u8]| -> [u8; 4] { c[(side / 2 * side + side / 3) * 4..][..4].try_into().unwrap() };
    let (a, b) = (crate::materials::cube(&solid, &images, side, view), crate::materials::cube(&clear, &images, side, view));
    assert!(middle(&b)[3] < middle(&a)[3], "{:?} {:?}", middle(&a), middle(&b));
    // A brick texture differs from the plain colour across a face.
    let brick = crate::model::Material { texture: Some(crate::texture::Texture::new(crate::texture::Kind::Brick)), ..solid.clone() };
    let c = crate::materials::cube(&brick, &images, side, view);
    let mut seen = std::collections::BTreeSet::new();
    for x in side / 4..side / 2 {
        seen.insert(c[(side * 3 / 5 * side + x) * 4]);
    }
    assert!(seen.len() > 3, "{seen:?}");
}

#[test]
fn the_cube_turns_and_takes_any_view_without_breaking() {
    let m = crate::model::Material::default();
    let images = Default::default();
    let a = crate::materials::cube(&m, &images, 48, crate::materials::CUBE_VIEW);
    let b = crate::materials::cube(&m, &images, 48, [1.2, 0.3]);
    assert_ne!(a, b);
    for view in [[f64::NAN, 0.0], [f64::INFINITY, f64::NEG_INFINITY], [1e300, -1e300]] {
        assert_eq!(crate::materials::cube(&m, &images, 8, view).len(), 8 * 8 * 4);
    }
    assert!(crate::materials::cube(&m, &images, 0, crate::materials::CUBE_VIEW).is_empty());
    // Held to a size a window can show.
    assert_eq!(crate::materials::cube(&m, &images, 1 << 20, crate::materials::CUBE_VIEW).len(), 1024 * 1024 * 4);
}

#[test]
fn a_pattern_has_a_swatch_to_show_in_a_list() {
    use crate::model::Pattern;
    let (w, h) = (48.0, 18.0);
    for p in Pattern::ALL {
        let s = crate::render::pattern_swatch(p, w, h);
        match p {
            Pattern::None => assert!(!s.solid && s.lines.is_empty()),
            Pattern::Solid => assert!(s.solid && s.lines.is_empty()),
            _ => assert!(!s.solid && !s.lines.is_empty(), "{p:?}"),
        }
        let inside = |q: P2| (-0.5..=w + 0.5).contains(&q.0) && (-0.5..=h + 0.5).contains(&q.1);
        assert!(s.lines.iter().flatten().all(|q| inside(*q)), "{p:?}: {:?}", s.lines);
    }
    assert_ne!(crate::render::pattern_swatch(Pattern::Diagonal, w, h), crate::render::pattern_swatch(Pattern::Grid, w, h));
}

/// A PNG of `w` by `h` pixels of one colour, written for test `test`, and its path.
fn png_file(test: &str, w: u32, h: u32, color: [u8; 4]) -> std::path::PathBuf {
    let folder = std::env::temp_dir().join(format!("bimcraft-image-fill-{test}-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let rgba: Vec<u8> = (0..w * h).flat_map(|_| color).collect();
    let png = tiny_skia::Pixmap::from_vec(rgba, tiny_skia::IntSize::from_wh(w, h).unwrap()).unwrap().encode_png().unwrap();
    let path = folder.join("fill.png");
    std::fs::write(&path, png).unwrap();
    path
}

/// The pictures `owner` is drawn with, and what colours show in them.
fn pictures_of(d: &crate::render::Drawing, owner: u64) -> Vec<Vec<Rgb>> {
    d.prims
        .iter()
        .zip(&d.owners)
        .filter(|(_, o)| **o == Some(owner))
        .filter_map(|(p, _)| match p {
            Prim::Image { rgba, .. } => Some(rgba.as_chunks::<4>().0.iter().filter(|p| p[3] > 0).map(|p| Rgb(p[0], p[1], p[2])).collect()),
            _ => None,
        })
        .collect()
}

#[test]
fn an_image_fill_is_taken_in_small_and_shows_in_place_of_a_surfaces_patterns() {
    let file = png_file("surface", 1200, 600, [10, 120, 200, 255]);
    let mut s = Session::new();
    let fill = json!({"path": file, "angle": 30, "offset": [100, 0]});
    run(&mut s, "material", json!({"name": "Photo", "surface": "horizontal", "surface_color": "#ff0000", "surface_image": fill}));
    let t = s.model.materials["Photo"].surface_image.clone().unwrap();
    let (w, h) = s.model.images[&t.image].size();
    assert!(w <= crate::texture::FILL_MAX_SIDE && h <= crate::texture::FILL_MAX_SIDE, "kept at {w} by {h}");
    assert_eq!((t.size, t.angle, t.offset), ([1000.0, 500.0], 30.0, [100.0, 0.0]), "a metre across, as tall as the picture");
    let (f, view) = floor_of(&mut s, "Photo");
    let d = crate::render::draw(&s.model, 100000, view);
    let pictures = pictures_of(&d, f);
    assert_eq!(pictures.len(), 1, "one picture over the floor");
    assert!(pictures[0].iter().all(|c| *c == Rgb(10, 120, 200)) && pictures[0].len() > 1000, "of the picture's colour");
    assert!(!inks(&d, f).1.contains(&Rgb(255, 0, 0)), "and no foreground lines");
    // Taken away, the patterns are back and the picture goes with it.
    let before = Session::new().model.images.len();
    run(&mut s, "material", json!({"name": "Photo", "surface_image": null}));
    assert_eq!(s.model.images.len(), before);
    let d = crate::render::draw(&s.model, 100000, view);
    assert!(pictures_of(&d, f).is_empty() && inks(&d, f).1.contains(&Rgb(255, 0, 0)));
}

#[test]
fn a_cut_layer_shows_its_image_fill() {
    let file = png_file("cut", 64, 64, [200, 40, 40, 255]);
    let mut s = Session::new();
    run(&mut s, "material", json!({"name": "Stone", "cut": "diagonal", "cut_image": {"path": file, "size": 400}}));
    assert_eq!(s.model.materials["Stone"].cut_image.as_ref().unwrap().size, [400.0, 400.0]);
    let layers = json!([{"function": "structure", "material": "Stone", "thickness": 300}]);
    run(&mut s, "type", json!({"category": "wall", "name": "Stone 300", "layers": layers}));
    let w = run(&mut s, "wall", json!({"points": [[0, 0], [6000, 0]], "type": "Stone 300"}))["ids"][0].as_u64().unwrap();
    run(&mut s, "detail", json!({"view": 100000, "detail": "fine"}));
    let view = View { angle: 0.0, scale: 0.2, cx: 3000.0, cy: 0.0, width: 1200, height: 800 };
    let d = crate::render::draw(&s.model, 100000, view);
    let pictures = pictures_of(&d, w);
    assert!(pictures.iter().any(|p| p.contains(&Rgb(200, 40, 40))), "{}", pictures.len());
    // Its picture is one of the project's, so the cut of another material may show it by its id.
    let id = s.model.materials["Stone"].cut_image.as_ref().unwrap().image.clone();
    run(&mut s, "material", json!({"name": "Other", "surface_image": {"image": id}}));
}

#[test]
fn an_image_fill_that_is_not_one_is_refused() {
    let file = png_file("refused", 8, 8, [0, 0, 0, 255]);
    let mut s = Session::new();
    let before = s.model.images.clone();
    let not_a_picture = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
    for bad in [
        json!({"cut_image": {"path": not_a_picture}}),
        json!({"cut_image": {"kind": "brick"}}),
        json!({"cut_image": {"image": "img-nothing"}}),
        json!({"surface_image": {}}),
        json!({"surface_image": {"path": file, "size": [0, 10]}}),
        json!({"surface_image": "photo.png"}),
    ] {
        let mut p = bad.clone();
        p["name"] = json!("Photo");
        assert!(s.execute("material", &p).is_err(), "{bad}");
    }
    assert_eq!(s.model.images, before, "nothing came in");
    // A file larger than an image fill takes.
    let big = file.with_file_name("big.png");
    std::fs::write(&big, vec![0_u8; crate::texture::FILL_MAX_BYTES + 1]).unwrap();
    let e = s.execute("material", &json!({"name": "Photo", "cut_image": {"path": big}})).unwrap_err();
    assert!(e.contains("8 MB"), "{e}");
}

#[test]
fn a_view_past_its_image_fill_budget_shows_the_rest_in_their_average_colour() {
    let file = png_file("budget", 16, 16, [30, 160, 60, 255]);
    let mut s = Session::new();
    run(&mut s, "material", json!({"name": "Grass", "surface_image": {"path": file}}));
    let (f, _) = floor_of(&mut s, "Grass");
    // A view bigger than the budget holds.
    let side = (crate::render::MAX_IMAGE_FILL as f64).sqrt() as u32 + 200;
    let view = View { angle: 0.0, scale: f64::from(side) / 4000.0, cx: 3000.0, cy: 2000.0, width: side * 2, height: side };
    let d = crate::render::draw(&s.model, 100000, view);
    assert!(pictures_of(&d, f).is_empty());
    assert!(inks(&d, f).0.contains(&Rgb(30, 160, 60)), "filled in its average colour");
    assert!(d.notes.iter().any(|n| n.contains("image fills")), "{:?}", d.notes);
}

#[test]
fn a_material_with_image_fills_saved_and_read_keeps_them() {
    let file = png_file("saved", 32, 16, [90, 90, 20, 255]);
    let mut s = Session::new();
    let before = s.model.images.len();
    run(&mut s, "material", json!({"name": "Photo", "cut_image": {"path": file, "angle": 45}, "surface_image": {"path": file}}));
    let text = serde_json::to_string(&s.model).unwrap();
    let back = crate::model::Model::read(&text).unwrap().0;
    assert_eq!(back.materials["Photo"], s.model.materials["Photo"]);
    assert_eq!(back.images.len(), before + 1, "one picture for both");
}

#[test]
fn the_starter_library_has_ground_and_finishes_shown_by_a_picture() {
    let s = Session::new();
    for name in ["Grass", "Gravel", "Wood", "Paving", "Water"] {
        let m = &s.model.materials[name];
        let fill = m.surface_image.as_ref().unwrap_or_else(|| panic!("{name} has an image fill"));
        let (w, h) = s.model.images[&fill.image].size();
        assert!(w.max(h) <= 128 && w.min(h) >= 64, "{name}: {w} by {h}");
        assert_eq!(m.texture.as_ref(), Some(fill), "{name}: the same picture in 3D");
    }
    assert_eq!(s.model.materials["Water"].transparency, 50, "water is half see-through");
    assert_eq!(s.model.materials["Grass"].transparency, 0);
    // Held as the JPEGs they ship as: a new project stays small.
    let text = serde_json::to_string(&s.model.images).unwrap();
    assert!(text.len() < 32_000, "{} bytes", text.len());
    let back = crate::model::Model::read(&serde_json::to_string(&s.model).unwrap()).unwrap().0;
    assert_eq!(back.images, s.model.images);
}
