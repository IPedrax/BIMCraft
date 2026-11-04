//! CAD imports through the commands every front end drives: a DXF comes in, is placed as an element on a level, goes where the
//! element goes, is saved with the project, and is drawn and snapped to in its plans.

use std::path::PathBuf;

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::Model;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn fails(s: &mut Session, id: &str, p: Value) -> String {
    match s.execute(id, &p) {
        Ok(v) => panic!("{id} {p} was taken: {v}"),
        Err(e) => e,
    }
}

/// A folder of this test's own.
fn dir(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bimcraft-cad-{}-{test}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A file in this test's folder, written.
fn file(test: &str, name: &str, text: &str) -> String {
    let path = dir(test).join(name);
    std::fs::write(&path, text).unwrap();
    path.to_string_lossy().into_owned()
}

/// Pairs as a DXF writes them: a code line, then its value.
fn pairs(list: &[(i32, &str)]) -> String {
    list.iter().map(|(c, v)| format!("{c}\n{v}\n")).collect()
}

/// A DXF of `units` ($INSUNITS) with these layers (name, colour, off) and entities.
fn dxf(units: &str, layers: &[(&str, &str, bool)], entities: &str) -> String {
    let header = pairs(&[(9, "$INSUNITS"), (70, units)]);
    let mut table = pairs(&[(0, "TABLE"), (2, "LAYER")]);
    for (name, color, off) in layers {
        let color = if *off { format!("-{color}") } else { color.to_string() };
        table += &pairs(&[(0, "LAYER"), (2, name), (70, "0"), (62, &color), (6, "Continuous")]);
    }
    table += &pairs(&[(0, "ENDTAB")]);
    format!(
        "0\nSECTION\n2\nHEADER\n{header}0\nENDSEC\n0\nSECTION\n2\nTABLES\n{table}0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{entities}0\nENDSEC\n0\nEOF\n"
    )
}

fn line(layer: &str, a: (f64, f64), b: (f64, f64)) -> String {
    let (ax, ay, bx, by) = (a.0.to_string(), a.1.to_string(), b.0.to_string(), b.1.to_string());
    pairs(&[(0, "LINE"), (8, layer), (10, &ax), (20, &ay), (11, &bx), (21, &by)])
}

/// A plan of a room 10 m by 6 m drawn in metres, its walls on WALLS (red), its furniture on FURN (off), a note on TEXT.
fn plan() -> String {
    let walls: String = [((0.0, 0.0), (10.0, 0.0)), ((10.0, 0.0), (10.0, 6.0)), ((10.0, 6.0), (0.0, 6.0)), ((0.0, 6.0), (0.0, 0.0))]
        .iter()
        .map(|(a, b)| line("WALLS", *a, *b))
        .collect();
    let furn = line("FURN", (1.0, 1.0), (2.0, 1.0));
    let note = pairs(&[(0, "TEXT"), (8, "TEXT"), (10, "1"), (20, "5"), (40, "0.25"), (1, "Kitchen")]);
    dxf("6", &[("WALLS", "1", false), ("FURN", "3", true), ("TEXT", "7", false)], &(walls + &furn + &note))
}

fn one(s: &mut Session) -> Value {
    let all = run(s, "elements", json!({"category": "cadimport"}));
    assert_eq!(all["count"], 1, "{all}");
    all["elements"][0].clone()
}

fn near(v: &Value, want: [[f64; 2]; 2]) -> bool {
    (0..2).all(|i| (0..2).all(|k| v[i][k].as_f64().is_some_and(|x| (x - want[i][k]).abs() < 1e-6)))
}

#[test]
fn a_drawing_comes_in_in_its_files_unit_its_middle_on_the_origin_with_its_layers_off_hidden() {
    let mut s = Session::new();
    let r = run(&mut s, "import_cad", json!({"path": file("in", "ground floor.dxf", &plan())}));
    assert_eq!(
        (r["cad"].as_str(), r["source"].as_str(), r["unit"].as_str()),
        (Some("ground floor"), Some("ground floor.dxf"), Some("m")),
        "{r}"
    );
    assert_eq!(
        (r["scale"].as_f64(), r["entities"].as_u64(), r["layers"].as_u64(), r["far"].as_u64()),
        (Some(1000.0), Some(6), Some(3), Some(0))
    );
    // Metres, so 10 m by 6 m, its middle on the origin.
    assert!(near(&r["extents"], [[-5000.0, -3000.0], [5000.0, 3000.0]]), "{r}");
    let e = one(&mut s);
    assert_eq!(
        (e["id"].as_u64(), e["cad"].as_str(), &e["hidden"], e["layers"].as_u64()),
        (r["id"].as_u64(), Some("ground floor"), &json!(["FURN"]), Some(3)),
        "{e}"
    );
    assert_eq!(e["level"], json!(s.model.guid(s.model.levels()[0].0)));
    assert!(e.get("view").is_none() && e.get("colors").is_none(), "every plan of its level, its colours as they are: {e}");
}

#[test]
fn units_scale_position_angle_layers_and_colours_are_the_users_to_say() {
    let mut s = Session::new();
    let path = file("say", "plan.dxf", &plan());
    // In centimetres instead, its origin on (1000, 2000), turned a quarter.
    let r = run(&mut s, "import_cad", json!({"path": path, "units": "cm", "position": "origin", "at": [1000, 2000], "angle": 90}));
    assert!(near(&r["extents"], [[940.0, 2000.0], [1000.0, 2100.0]]), "{r}");
    assert_eq!(r["unit"], "cm (its file says m)", "the unit it was read in");
    // A scale of its own, all its layers, in black.
    let r = run(&mut s, "import_cad", json!({"cad": "plan", "scale": 2, "position": "origin", "layers": "all", "colors": "black"}));
    assert!(near(&r["extents"], [[0.0, 0.0], [20.0, 12.0]]), "{r}");
    assert_eq!(r["unit"], "a scale of its own (its file says m)");
    let e = &run(&mut s, "elements", json!({"category": "cadimport"}))["elements"][1];
    assert_eq!((e.get("hidden"), e["colors"].as_str()), (None, Some("black")), "{e}");
    // A list of the layers to show hides the rest.
    let r = run(&mut s, "import_cad", json!({"cad": "plan", "layers": ["WALLS"]}));
    let e = crate::element_json(&s.model, r["id"].as_u64().unwrap());
    assert_eq!(e["hidden"], json!(["FURN", "TEXT"]));
    for (p, says) in [
        (json!({"cad": "plan", "units": "furlong"}), "units"),
        (json!({"cad": "plan", "units": "mm", "scale": 2}), "not both"),
        (json!({"cad": "plan", "scale": 0}), "scale"),
        (json!({"cad": "plan", "position": "left"}), "position"),
        (json!({"cad": "plan", "colors": "pink"}), "colors"),
        (json!({"cad": "plan", "layers": ["NOPE"]}), "no layer `NOPE`"),
        (json!({"cad": "plan", "layers": 5}), "layers"),
        (json!({"cad": "plan", "link": true}), "link"),
        (json!({"cad": "nope"}), "holds no CAD drawing named `nope`"),
        (json!({"path": path, "cad": "plan"}), "not both"),
        (json!({}), "give `path`"),
        (json!({"path": path}), "already holds a CAD drawing named `plan`"),
    ] {
        let e = fails(&mut s, "import_cad", p.clone());
        assert!(e.contains(says), "{p}: {e}");
    }
}

#[test]
fn a_dwg_says_to_save_it_as_dxf_and_a_file_that_isnt_one_says_why() {
    let mut s = Session::new();
    let e = fails(&mut s, "import_cad", json!({"path": file("dwg", "plan.dwg", "AC1032")}));
    assert!(e.contains("DWG") && e.contains("save it as DXF"), "{e}");
    let e = fails(&mut s, "import_cad", json!({"path": file("dwg", "plan.pdf", "%PDF")}));
    assert!(e.contains("reads DXF"), "{e}");
    let e = fails(&mut s, "import_cad", json!({"path": file("dwg", "broken.dxf", "0\nSECTION\n2\nENTITIES\nhello\n")}));
    assert!(e.contains("broken.dxf") && e.contains("group code"), "{e}");
    let e = fails(&mut s, "import_cad", json!({"path": file("dwg", "empty.dxf", &dxf("4", &[], ""))}));
    assert!(e.contains("draws nothing"), "{e}");
    let e = fails(&mut s, "import_cad", json!({"path": dir("dwg").join("nowhere.dxf")}));
    assert!(e.contains("nowhere.dxf"), "{e}");
    assert!(s.model.cads.is_empty());
}

/// A DWG reader as the apps hand a session one: here one that gives `plan()`'s DXF for a file that begins as a DWG does, with
/// a note, and says why for any other.
fn dwg_reader() -> crate::cad::DwgReader {
    std::sync::Arc::new(|path: &std::path::Path| {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        if !bytes.starts_with(b"AC10") {
            return Err("it isn't a DWG file".into());
        }
        Ok((plan().into_bytes(), vec!["2 of the DWG's object records couldn't be read and were left out".into()]))
    })
}

#[test]
fn a_dwg_comes_in_through_the_reader_a_session_is_given_and_is_held_as_the_dxf_it_became() {
    let mut s = Session::new();
    s.dwg = Some(dwg_reader());
    let path = file("dwg-reader", "site.dwg", "AC1032 a drawing");
    let r = run(&mut s, "import_cad", json!({"path": path, "link": true, "layers": "all"}));
    assert_eq!((r["cad"].as_str(), r["source"].as_str(), r["unit"].as_str()), (Some("site"), Some("site.dwg"), Some("m")), "{r}");
    assert_eq!(r["notes"], json!(["2 of the DWG's object records couldn't be read and were left out"]), "{r}");
    assert_eq!(s.model.cads["site"].dxf().entities.len(), 6);
    // Reloaded through it too; a file it can't read says why, and the drawing keeps what it had.
    assert_eq!(run(&mut s, "reload_cad", json!({"cad": "site"}))["changed"], false);
    std::fs::write(&path, "not a drawing").unwrap();
    let e = fails(&mut s, "reload_cad", json!({"cad": "site"}));
    assert!(e.contains("site.dwg") && e.contains("isn't a DWG"), "{e}");
    assert_eq!(s.model.cads["site"].dxf().entities.len(), 6);
    // A project file holds the DXF, so one opened where no DWG reader is still has the drawing; the link says why it wasn't
    // read again.
    let saved = dir("dwg-reader").join("p.bim");
    run(&mut s, "save", json!({"path": saved}));
    let mut plain = Session::new();
    let r = run(&mut plain, "open", json!({"path": saved}));
    assert_eq!(plain.model.cads["site"].dxf().entities.len(), 6);
    let why = r["cads_not_reloaded"].to_string();
    assert!(why.contains("site") && why.contains("save it as DXF"), "{r}");
}

#[test]
fn current_view_only_is_one_plan_of_its_level() {
    let mut s = Session::new();
    let path = file("view", "plan.dxf", &plan());
    let level = s.model.levels()[0].0;
    let r = run(&mut s, "import_cad", json!({"path": path, "view": level}));
    let e = crate::element_json(&s.model, r["id"].as_u64().unwrap());
    assert_eq!(e["view"], json!(s.model.guid(level)));
    // Level 2's plan, while saying level 1, is refused.
    let upper = s.model.levels()[1].0;
    let e = fails(&mut s, "import_cad", json!({"cad": "plan", "level": level, "view": upper}));
    assert!(e.contains("another level"), "{e}");
    // A plan view of a level names its level too.
    let pv = run(&mut s, "duplicate_view", json!({"view": upper}))["id"].as_u64().unwrap();
    let r = run(&mut s, "import_cad", json!({"cad": "plan", "view": pv}));
    let e = crate::element_json(&s.model, r["id"].as_u64().unwrap());
    assert_eq!((e["view"].clone(), e["level"].clone()), (json!(s.model.guid(pv)), json!(s.model.guid(upper))));
}

#[test]
fn an_import_moves_turns_copies_and_refuses_to_mirror() {
    let mut s = Session::new();
    let id = run(&mut s, "import_cad", json!({"path": file("move", "plan.dxf", &plan())}))["id"].as_u64().unwrap();
    run(&mut s, "move", json!({"ids": [id], "by": [100, 200]}));
    run(&mut s, "rotate", json!({"ids": [id], "about": [100, 200], "angle": 90}));
    let e = one(&mut s);
    assert_eq!((&e["at"], e["angle"].as_f64()), (&json!([100.0, 200.0]), Some(90.0)), "{e}");
    assert!(near(&e["extents"], [[-2900.0, -4800.0], [3100.0, 5200.0]]), "{e}");
    let err = fails(&mut s, "mirror", json!({"ids": [id], "axis": [[0, 0], [0, 1000]]}));
    assert!(err.contains("can't be mirrored"), "{err}");
    let copies = run(&mut s, "copy", json!({"ids": [id], "by": [20000, 0]}));
    assert_eq!(run(&mut s, "elements", json!({"category": "cadimport"}))["count"], 2, "{copies}");
    assert_eq!(s.model.cads.len(), 1, "both place the one drawing");
}

#[test]
fn a_drawing_goes_when_nothing_places_it_and_undo_brings_it_back() {
    let mut s = Session::new();
    let id = run(&mut s, "import_cad", json!({"path": file("delete", "plan.dxf", &plan())}))["id"].as_u64().unwrap();
    let again = run(&mut s, "import_cad", json!({"cad": "plan", "at": [20000, 0]}))["id"].as_u64().unwrap();
    run(&mut s, "delete", json!({"ids": [id]}));
    assert_eq!(s.model.cads.len(), 1, "one still places it");
    run(&mut s, "delete", json!({"ids": [again]}));
    assert!(s.model.cads.is_empty());
    run(&mut s, "undo", json!({}));
    assert_eq!(s.model.cads.len(), 1);
    run(&mut s, "undo", json!({"count": 2}));
    run(&mut s, "undo", json!({}));
    assert!(s.model.cads.is_empty(), "before it came in");
}

#[test]
fn a_drawing_is_saved_with_the_project_and_opens_as_it_was_and_insert_brings_it() {
    let mut s = Session::new();
    let path = file("save", "plan.dxf", &plan());
    let id = run(&mut s, "import_cad", json!({"path": path, "angle": 30, "at": [100, 200], "colors": "invert"}))["id"].as_u64().unwrap();
    let before = (one(&mut s), s.model.cads.clone());
    let project = dir("save").join("project.bimcraft");
    run(&mut s, "save", json!({"path": project}));
    let text = std::fs::read_to_string(&project).unwrap();
    let saved: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["format"], json!(crate::model::FORMAT));
    assert_eq!(crate::model::FORMAT, 15);
    assert!(saved["cads"]["plan"]["dxf"].is_string() && saved["cads"]["plan"]["source"] == "plan.dxf", "{}", saved["cads"]);
    assert!(saved["library"].to_string().contains("\"cad\""), "the drawing is a library entry: {}", saved["library"]);
    let mut t = Session::new();
    run(&mut t, "open", json!({"path": project}));
    assert_eq!((one(&mut t), t.model.cads.clone()), before);
    // Insert from File brings the drawing with its import.
    let mut u = Session::new();
    run(&mut u, "insert", json!({"path": project, "ids": [id]}));
    assert_eq!((one(&mut u)["cad"].clone(), u.model.cads.get("plan")), (json!("plan"), s.model.cads.get("plan")));
}

#[test]
fn a_file_of_5_opens_and_what_a_file_holds_of_a_drawing_is_checked_as_it_is_read() {
    // A file of 5 or 6 holds no drawing: it opens, and is this build's format once read.
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]]}));
    let mut v: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
    for older in [5, 6] {
        v["format"] = json!(older);
        let (m, left_out) = Model::read(&v.to_string()).unwrap();
        assert_eq!((m.format, left_out.len()), (crate::model::FORMAT, 0));
    }
    // A drawing whose data isn't base64, isn't zlib, or isn't a DXF is refused, saying whose it is.
    run(&mut s, "import_cad", json!({"path": file("read", "plan.dxf", &plan())}));
    let good: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
    for (data, says) in [
        (json!("!!!"), "base64"),
        (json!("aGVsbG8="), "zlib"),
        (json!(crate::commands::base64(&crate::cloud::deflate(b"0\nEOF\n"))), "ENTITIES"),
    ] {
        let mut bad = good.clone();
        bad["cads"]["plan"]["dxf"] = data;
        let e = Model::read(&bad.to_string()).unwrap_err();
        assert!(e.contains(says), "{e}");
    }
    // More drawings than a project holds is refused before the extra one is unpacked.
    let mut many = good.clone();
    for i in 0..crate::cad::MAX_CADS {
        many["cads"][format!("copy {i}")] = good["cads"]["plan"].clone();
    }
    let e = Model::read(&many.to_string()).unwrap_err();
    assert!(e.contains("CAD drawings") && e.contains(&crate::cad::MAX_CADS.to_string()), "{e}");
    // An import of a drawing the project doesn't hold is refused by the check.
    let mut orphan = good.clone();
    orphan["cads"] = json!({});
    let (m, _) = Model::read(&orphan.to_string()).unwrap();
    let e = m.check().unwrap_err();
    assert!(e.contains("no CAD drawing named `plan`"), "{e}");
}

#[test]
fn layers_are_hidden_and_shown_everywhere_or_in_one_view() {
    let mut s = Session::new();
    let id = run(&mut s, "import_cad", json!({"path": file("layers", "plan.dxf", &plan())}))["id"].as_u64().unwrap();
    let shown = |r: &Value| -> Vec<(String, bool)> {
        r["layers"].as_array().unwrap().iter().map(|l| (l["name"].as_str().unwrap().to_string(), l["shown"].as_bool().unwrap())).collect()
    };
    let r = run(&mut s, "cad_layers", json!({"id": id}));
    assert_eq!(shown(&r), [("WALLS".into(), true), ("FURN".into(), false), ("TEXT".into(), true)]);
    assert_eq!(r["layers"][0]["color"], json!([255, 0, 0]));
    let r = run(&mut s, "cad_layers", json!({"id": id, "hide": ["TEXT"], "show": ["FURN"]}));
    assert_eq!(shown(&r), [("WALLS".into(), true), ("FURN".into(), true), ("TEXT".into(), false)]);
    // In one view: its graphics, by the drawing's name.
    let level = s.model.levels()[0].0;
    let r = run(&mut s, "cad_layers", json!({"id": id, "view": level, "hide": ["WALLS"]}));
    assert_eq!(r["layers"][0]["shown_in_view"], false);
    assert_eq!(r["layers"][1]["shown_in_view"], true);
    let g = crate::graphics::graphics_of(&s.model, level).unwrap();
    assert_eq!(g.layers.get("plan").map(|l| l.iter().cloned().collect::<Vec<_>>()), Some(vec!["WALLS".to_string()]));
    run(&mut s, "undo", json!({}));
    assert!(crate::graphics::graphics_of(&s.model, level).unwrap().layers.is_empty());
    // Visibility/Graphics says the same, and its reply lists them.
    let r = run(&mut s, "graphics", json!({"view": level, "layers": {"plan": ["FURN", "TEXT"]}}));
    assert_eq!(r["layers"], json!({"plan": ["FURN", "TEXT"]}));
    // A file holds the drawing by its GUID, and opens with it named again.
    let text = s.model.to_json().unwrap();
    let guid = s.model.library.guid(&crate::library::Kind::Cad, "plan").unwrap().to_string();
    assert!(text.contains(&format!("\"{guid}\":")), "keyed by GUID");
    let (m, _) = Model::read(&text).unwrap();
    assert_eq!(crate::graphics::graphics_of(&m, level).unwrap().layers.keys().collect::<Vec<_>>(), ["plan"]);
    let r = run(&mut s, "graphics", json!({"view": level, "layers": {"plan": null}}));
    assert_eq!(r["layers"], json!({}));
    for (p, says) in [
        (json!({"view": level, "layers": {"plan": ["NOPE"]}}), "NOPE"),
        (json!({"view": level, "layers": {"other": ["WALLS"]}}), "other"),
        (json!({"view": level, "layers": {"plan": [5]}}), "by name"),
        (json!({"view": level, "layers": 5}), "`layers`"),
    ] {
        let e = fails(&mut s, "graphics", p.clone());
        assert!(e.contains(says), "{p}: {e}");
    }
    for (p, says) in [
        (json!({"id": id, "hide": ["NOPE"]}), "no layer `NOPE`"),
        (json!({"id": id, "hide": ["FURN"], "show": ["FURN"]}), "in `hide` and in `show`"),
        (json!({"id": level}), "isn't a CAD import"),
        (json!({}), "`id`"),
    ] {
        let e = fails(&mut s, "cad_layers", p.clone());
        assert!(e.contains(says), "{p}: {e}");
    }
    // The drawing gone, no view hides its layers.
    run(&mut s, "graphics", json!({"view": level, "layers": {"plan": ["FURN"]}}));
    run(&mut s, "delete", json!({"ids": [id]}));
    assert!(crate::graphics::graphics_of(&s.model, level).unwrap().layers.is_empty());
}

#[test]
fn a_linked_drawing_is_read_again_by_reload_and_by_opening_the_project() {
    let mut s = Session::new();
    let path = file("link", "plan.dxf", &plan());
    run(&mut s, "import_cad", json!({"path": path, "link": true}));
    assert_eq!(one(&mut s)["linked"].as_str(), Some(path.as_str()));
    assert_eq!(run(&mut s, "reload_cad", json!({"cad": "plan"}))["changed"], false);
    // The file changes: one more line.
    let more = plan().replace("0\nENDSEC\n0\nEOF", &(line("WALLS", (0.0, 0.0), (10.0, 6.0)) + "0\nENDSEC\n0\nEOF"));
    std::fs::write(&path, &more).unwrap();
    let r = run(&mut s, "reload_cad", json!({"cad": "plan"}));
    assert_eq!((r["changed"].as_bool(), r["entities"].as_u64()), (Some(true), Some(7)));
    run(&mut s, "undo", json!({}));
    assert_eq!(s.model.cads["plan"].dxf().entities.len(), 6, "undo takes the reload back");
    // Saved, then the file changes again: opening reads it.
    let project = dir("link").join("project.bimcraft");
    run(&mut s, "save", json!({"path": project}));
    let mut t = Session::new();
    run(&mut t, "open", json!({"path": project}));
    assert_eq!(t.model.cads["plan"].dxf().entities.len(), 7);
    // Its file gone, the project keeps what it holds and says so.
    std::fs::remove_file(&path).unwrap();
    let r = run(&mut t, "open", json!({"path": project}));
    assert_eq!(t.model.cads["plan"].dxf().entities.len(), 6, "what the project file holds");
    assert_eq!(r["cads_not_reloaded"][0]["cad"], "plan", "{r}");
    // An imported (not linked) drawing has no file to read again.
    let mut u = Session::new();
    run(&mut u, "import_cad", json!({"path": file("link", "other.dxf", &plan())}));
    let e = fails(&mut u, "reload_cad", json!({"cad": "other"}));
    assert!(e.contains("isn't linked"), "{e}");
}

#[test]
fn query_names_the_entity_nearest_a_point() {
    let mut s = Session::new();
    let id = run(&mut s, "import_cad", json!({"path": file("query", "plan.dxf", &plan()), "position": "origin"}))["id"].as_u64().unwrap();
    let r = run(&mut s, "cad_query", json!({"id": id, "at": [5000, 30]}));
    assert_eq!(
        (r["kind"].as_str(), r["layer"].as_str(), &r["color"], r["distance"].as_f64()),
        (Some("line"), Some("WALLS"), &json!([255, 0, 0]), Some(30.0)),
        "{r}"
    );
    // FURN is hidden: its line isn't found, the wall's is further off.
    assert_eq!(run(&mut s, "cad_query", json!({"id": id, "at": [1500, 1000], "reach": 50}))["found"], false);
    assert_eq!(run(&mut s, "cad_query", json!({"id": id, "at": [1000, 5000]}))["kind"], "text");
}

#[test]
fn stray_entities_far_off_are_counted_and_left_out_of_its_extents() {
    // The bulk: a 10 m by 6 m room of many short lines, and one line a hundred kilometres away.
    let mut entities: String = (0..40).map(|i| line("WALLS", (f64::from(i) * 0.25, 0.0), (f64::from(i) * 0.25 + 0.25, 6.0))).collect();
    entities += &line("JUNK", (100_000.0, 100_000.0), (100_001.0, 100_000.0));
    let mut s = Session::new();
    let r = run(&mut s, "import_cad", json!({"path": file("far", "plan.dxf", &dxf("6", &[], &entities))}));
    assert_eq!(r["far"], 1, "{r}");
    assert!(r["warning"].as_str().is_some_and(|w| w.contains("far from the rest")), "{r}");
    assert!(near(&r["extents"], [[-5000.0, -3000.0], [5000.0, 3000.0]]), "the bulk's, centred: {r}");
    // So placed far away, what's past the model's reach is refused.
    let e = fails(&mut s, "import_cad", json!({"cad": "plan", "at": [1e9, 0]}));
    assert!(e.contains("place it nearer"), "{e}");
}

// ---------- plans ----------

use crate::geom::P2;
use crate::render::{self, Rgb};
use crate::snap::{Kind, Reach, Snaps, snap};

/// How many pixels of a drawing's PNG are `colour`'s.
fn drawn(d: &render::Drawing, colour: impl Fn(Rgb) -> bool) -> usize {
    let pm = tiny_skia::Pixmap::decode_png(&render::png(d).unwrap()).unwrap();
    pm.pixels().iter().map(|p| p.demultiply()).filter(|p| colour(Rgb(p.red(), p.green(), p.blue()))).count()
}

/// Red, or a thin red line's pixels on the paper: the finest pen is less than a pixel wide in a plan fitted to the drawing.
fn reddish(c: Rgb) -> bool {
    c.0 > 200 && i32::from(c.0) - i32::from(c.1) > 40 && c.1.abs_diff(c.2) < 20
}

fn greenish(c: Rgb) -> bool {
    c.1 > 200 && i32::from(c.1) - i32::from(c.0) > 40 && c.0.abs_diff(c.2) < 20
}

fn levels(s: &Session) -> (u64, u64) {
    let l = s.model.levels();
    (l[0].0, l[1].0)
}

#[test]
fn a_plan_of_its_level_draws_it_in_its_colours_and_picks_it_as_one() {
    let mut s = Session::new();
    let id = run(&mut s, "import_cad", json!({"path": file("draw", "plan.dxf", &plan()), "position": "origin"}))["id"].as_u64().unwrap();
    let (one, two) = levels(&s);
    let d = render::plan(&s.model, one, 600, 600);
    let reds = drawn(&d, reddish);
    assert!(reds > 300, "the red walls: {reds}");
    assert_eq!(drawn(&d, greenish), 0, "FURN is off in its file");
    // Picked where its lines are, as one element; nothing between them.
    assert_eq!(d.pick(d.view.px(P2(5000.0, 0.0)), 3.0), Some(id));
    assert_eq!(d.pick(d.view.px(P2(5000.0, 3000.0)), 3.0), None);
    // Its note is lettered.
    assert!(d.owners.iter().zip(&d.parts).any(|(o, p)| *o == Some(id) && *p == render::Part::Text), "the note");
    // Level 2's plan doesn't show it.
    assert!(!render::plan(&s.model, two, 600, 600).owners.contains(&Some(id)));
    // All its layers: the furniture too.
    run(&mut s, "cad_layers", json!({"id": id, "show": ["FURN"]}));
    assert!(drawn(&render::plan(&s.model, one, 600, 600), greenish) > 10);
    // In black, no colour at all.
    run(&mut s, "modify", json!({"id": id, "colors": "black"}));
    let d = render::plan(&s.model, one, 600, 600);
    assert_eq!((drawn(&d, reddish), drawn(&d, greenish)), (0, 0));
    // Inverted, red is cyan.
    run(&mut s, "modify", json!({"id": id, "colors": "invert"}));
    let cyan = |c: Rgb| c.2 > 200 && c.1 > 200 && i32::from(c.2) - i32::from(c.0) > 40;
    assert!(drawn(&render::plan(&s.model, one, 600, 600), cyan) > 300);
}

#[test]
fn a_view_hides_its_layers_or_all_of_it_and_current_view_only_is_that_view() {
    let mut s = Session::new();
    let path = file("hide", "plan.dxf", &plan());
    let id = run(&mut s, "import_cad", json!({"path": path, "position": "origin"}))["id"].as_u64().unwrap();
    let (one, _) = levels(&s);
    let other = run(&mut s, "duplicate_view", json!({"view": one}))["id"].as_u64().unwrap();
    // Level 1's plan hides WALLS; the plan view of the same level still shows them.
    run(&mut s, "cad_layers", json!({"id": id, "view": one, "hide": ["WALLS"]}));
    assert_eq!(drawn(&render::plan(&s.model, one, 600, 600), reddish), 0);
    assert!(drawn(&render::plan(&s.model, other, 600, 600), reddish) > 300);
    // The category hidden in a view, as Revit's Imported Categories.
    run(&mut s, "graphics", json!({"view": other, "categories": {"cadimport": {"hidden": true}}}));
    assert!(!render::plan(&s.model, other, 600, 600).owners.contains(&Some(id)));
    // Current view only: in that plan, not in the other plan of its level.
    let mut t = Session::new();
    let (one, _) = levels(&t);
    let other = run(&mut t, "duplicate_view", json!({"view": one}))["id"].as_u64().unwrap();
    let id = run(&mut t, "import_cad", json!({"path": path, "view": other}))["id"].as_u64().unwrap();
    assert!(render::plan(&t.model, other, 600, 600).owners.contains(&Some(id)));
    assert!(!render::plan(&t.model, one, 600, 600).owners.contains(&Some(id)));
    // It goes with its view, and its drawing with it.
    run(&mut t, "delete", json!({"ids": [other]}));
    assert!(t.model.elements.get(id).is_none() && t.model.cads.is_empty());
}

#[test]
fn a_hatch_fills_text_is_aligned_and_a_dashed_layer_draws_dashed() {
    // A solid square, a note centred on a point, and a dashed line, in mm.
    let solid = pairs(&[
        (0, "SOLID"),
        (8, "FILL"),
        (62, "5"),
        (10, "0"),
        (20, "0"),
        (11, "1000"),
        (21, "0"),
        (12, "0"),
        (22, "1000"),
        (13, "1000"),
        (23, "1000"),
    ]);
    let note =
        pairs(&[(0, "TEXT"), (8, "0"), (10, "0"), (20, "0"), (11, "3000"), (21, "500"), (40, "200"), (72, "1"), (73, "2"), (1, "MID")]);
    let dashed = pairs(&[(0, "LINE"), (8, "0"), (6, "DASHED"), (10, "0"), (20, "-2000"), (11, "4000"), (21, "-2000")]);
    let ltype = pairs(&[(0, "TABLE"), (2, "LTYPE"), (0, "LTYPE"), (2, "DASHED"), (49, "200"), (49, "-100"), (0, "ENDTAB")]);
    let text = dxf("4", &[], &(solid + &note + &dashed)).replace("0\nSECTION\n2\nTABLES\n", &format!("0\nSECTION\n2\nTABLES\n{ltype}"));
    let mut s = Session::new();
    let id = run(&mut s, "import_cad", json!({"path": file("hatch", "detail.dxf", &text), "position": "origin"}))["id"].as_u64().unwrap();
    let (one, _) = levels(&s);
    let d = render::plan(&s.model, one, 800, 800);
    let blue = |c: Rgb| c.2 > 200 && c.0 < 60 && c.1 < 60;
    // The square is a metre a side: filled through.
    let side = 1000.0 * d.view.scale;
    assert!(drawn(&d, blue) as f64 > side * side * 0.8, "{} of {}", drawn(&d, blue), side * side);
    // The note's middle is on its point: its strokes lie either side of it.
    let (x, y) = d.view.px(P2(3000.0, 500.0));
    let strokes: Vec<(f32, f32)> = d
        .prims
        .iter()
        .zip(&d.parts)
        .zip(&d.owners)
        .filter(|((_, part), o)| **part == render::Part::Text && **o == Some(id))
        .flat_map(|((p, _), _)| p.points())
        .collect();
    let (lo, hi) = strokes.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p.0), hi.max(p.0)));
    assert!(lo < x && hi > x && ((lo + hi) / 2.0 - x).abs() < 3.0, "{lo} {hi} about {x}");
    let (top, bottom) = strokes.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p.1), hi.max(p.1)));
    assert!(((top + bottom) / 2.0 - y).abs() < 3.0, "{top} {bottom} about {y}");
    // The dashed line carries its dashes, scaled to the view.
    let dash = d.prims.iter().find_map(|p| match p {
        render::Prim::Line { dash: Some(dash), pts, .. } if pts.len() == 2 => Some(dash.clone()),
        _ => None,
    });
    let k = d.view.scale as f32;
    assert!(
        dash.as_deref().is_some_and(|d| d.len() == 2 && (d[0] - 200.0 * k).abs() < 0.01 && (d[1] - 100.0 * k).abs() < 0.01),
        "{dash:?}"
    );
}

#[test]
fn a_plan_snaps_to_an_imports_ends_middles_crossings_and_centres_so_walls_can_be_traced() {
    let mut s = Session::new();
    let circle = pairs(&[(0, "CIRCLE"), (8, "WALLS"), (10, "20"), (20, "3"), (40, "0.5")]);
    let text = plan().replace("0\nENDSEC\n0\nEOF", &(circle + &line("WALLS", (2.0, -1.0), (2.0, 7.0)) + "0\nENDSEC\n0\nEOF"));
    run(&mut s, "import_cad", json!({"path": file("snap", "plan.dxf", &text), "position": "origin", "at": [1000, 1000]}));
    let (one, two) = levels(&s);
    let reach = Reach { point: 100.0, line: 100.0 };
    let at = |s: &Session, level: u64, p: P2| snap(&s.model, level, p, reach, &[], false, &Snaps::default());
    let f = at(&s, one, P2(1030.0, 1020.0));
    assert_eq!((f.kind, f.at), (Kind::Endpoint, P2(1000.0, 1000.0)), "a corner: {f:?}");
    let f = at(&s, one, P2(6020.0, 990.0));
    assert_eq!((f.kind, f.at), (Kind::Midpoint, P2(6000.0, 1000.0)), "{f:?}");
    let f = at(&s, one, P2(3020.0, 7010.0));
    assert_eq!((f.kind, f.at), (Kind::Intersection, P2(3000.0, 7000.0)), "the cross line and the north wall: {f:?}");
    let f = at(&s, one, P2(21_020.0, 4_030.0));
    assert_eq!((f.kind, f.at), (Kind::Centre, P2(21_000.0, 4_000.0)), "{f:?}");
    assert_eq!(at(&s, two, P2(1030.0, 1020.0)).kind, Kind::Free, "Level 2 has no import");
    // FURN, off in its file, isn't snapped to; a layer hidden isn't either.
    assert_eq!(at(&s, one, P2(2030.0, 2010.0)).kind, Kind::Free, "FURN's end");
    let id = run(&mut s, "elements", json!({"category": "cadimport"}))["elements"][0]["id"].clone();
    run(&mut s, "cad_layers", json!({"id": id, "hide": ["WALLS"]}));
    assert_eq!(at(&s, one, P2(1030.0, 1020.0)).kind, Kind::Free);
    run(&mut s, "cad_layers", json!({"id": id, "show": ["WALLS"]}));
    // A wall traced from corner to corner of the drawing lies on its line.
    let (a, b) = (at(&s, one, P2(1020.0, 1010.0)).at, at(&s, one, P2(10_990.0, 1020.0)).at);
    run(&mut s, "wall", json!({"points": [[a.0, a.1], [b.0, b.1]]}));
    let w = &run(&mut s, "elements", json!({"category": "wall"}))["elements"][0];
    assert_eq!((&w["start"], &w["end"]), (&json!([1000.0, 1000.0]), &json!([11000.0, 1000.0])));
}

#[test]
fn a_plan_fits_to_its_imports_as_to_the_rest_it_shows() {
    let mut s = Session::new();
    let id = run(&mut s, "import_cad", json!({"path": file("fit", "plan.dxf", &plan()), "at": [80_000, -40_000]}))["id"].as_u64().unwrap();
    let (one, two) = levels(&s);
    let d = render::plan(&s.model, one, 600, 600);
    assert!(drawn(&d, reddish) > 300, "the plan is fitted to the drawing, far from the origin");
    assert!(d.pick(d.view.px(P2(80_000.0, -43_000.0)), 3.0) == Some(id), "its south wall is on screen");
    // A plan of another level, or one an import isn't in, isn't fitted to it.
    assert!(render::plan_shows(&s.model, two).is_empty());
    let other = run(&mut s, "duplicate_view", json!({"view": one}))["id"].as_u64().unwrap();
    let mut t = Session::new();
    let (one, _) = levels(&t);
    let other_t = run(&mut t, "duplicate_view", json!({"view": one}))["id"].as_u64().unwrap();
    run(&mut t, "import_cad", json!({"path": file("fit", "plan.dxf", &plan()), "view": other_t, "at": [80_000, 0]}));
    assert!(render::plan_shows(&t.model, one).is_empty());
    assert!(!render::plan_shows(&t.model, other_t).is_empty());
    assert!(!render::plan_shows(&s.model, other).is_empty());
}
