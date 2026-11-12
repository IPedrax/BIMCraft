//! A plan saved as a DXF, read back by our own DXF reader (the one Import CAD uses): in millimetres, where the model is,
//! each category on its layer, cut walls filled, dashes kept as a linetype and weights as line weights.

use std::path::PathBuf;

use serde_json::{Value, json};

use crate::dxf::{self, Shape};
use crate::dxf_out::lineweight;
use crate::engine::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// A file in a folder of this test's own.
fn path(test: &str, name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bimcraft-dxf-out-{}-{test}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

#[test]
fn a_line_weight_on_paper_is_the_nearest_standard_one() {
    assert_eq!(lineweight(0.0), 0);
    assert_eq!(lineweight(0.18), 18);
    assert_eq!(lineweight(0.34), 35);
    assert_eq!(lineweight(0.7), 70);
    assert_eq!(lineweight(5.0), 211);
    assert_eq!(lineweight(f64::NAN), 0);
    assert_eq!(lineweight(-1.0), 0);
}

#[test]
fn a_plan_saved_as_dxf_reads_back_in_millimetres_where_the_model_is_each_category_on_its_layer() {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [6000, 0], [6000, 4000]]}));
    run(&mut s, "door", json!({"at": [2000, 0]}));
    let file = path("plan", "plan.dxf");
    let reply = run(&mut s, "save", json!({"path": file}));
    assert_eq!(reply["format"], "DXF");
    assert!(reply["entities"].as_u64().unwrap() > 0, "{reply}");
    // The project itself is not saved to it: the DXF is an export.
    assert_eq!(s.path, None);

    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("AC1018"), "the 2004 format, for true colours");
    let d = dxf::read_text(&text).unwrap();
    assert_eq!(d.unit, "mm");
    assert!(d.left_out.is_empty(), "everything written is read: {:?}", d.left_out);
    let names: Vec<&str> = d.layers.iter().map(|l| l.name.as_str()).collect();
    assert!(names.contains(&"wall-cut"), "{names:?}");
    assert!(names.iter().any(|n| n.starts_with("door")), "{names:?}");
    for layer in reply["layers"].as_array().unwrap() {
        assert!(names.contains(&layer.as_str().unwrap()), "{layer} is in the LAYER table");
    }

    // The walls' cut lines lie on the walls, in model millimetres: the box round them is the walls' own.
    let cut: Vec<&dxf::Entity> = d.entities.iter().filter(|e| e.layer == "wall-cut").collect();
    let b = dxf::union(cut.iter().map(|e| e.shape.bounds())).unwrap();
    // The walls are 200 mm thick about their lines, and end square at their ends.
    let near = |a: f64, b: f64| (a - b).abs() < 0.01;
    assert!(near(b[0].0, 0.0) && near(b[1].0, 6100.0) && near(b[0].1, -100.0) && near(b[1].1, 4000.0), "{b:?}");
    // Cut walls are filled through, and their outlines are heavier than what is seen beyond.
    assert!(cut.iter().any(|e| matches!(e.shape, Shape::Fill { solid: true, .. })), "the poché is a solid hatch");
    let heaviest = |layer: &str| d.entities.iter().filter(|e| e.layer == layer).filter_map(|e| e.weight).fold(0.0, f64::max);
    assert!(heaviest("wall-cut") > 0.0);
    // The door's swing is dashed or solid as the plan draws it; any dash comes with its linetype.
    for e in &d.entities {
        if let Some(lt) = &e.linetype {
            assert!(d.linetypes.iter().any(|l| &l.name == lt && !l.dashes.is_empty()), "{lt} is defined");
        }
    }
}

#[test]
fn a_view_names_the_level_whose_plan_is_written() {
    let mut s = Session::new();
    run(&mut s, "level", json!({"name": "Upper", "elevation": 3000}));
    run(&mut s, "wall", json!({"points": [[0, 0], [3000, 0]], "level": "Upper"}));
    let lower = path("levels", "lower.dxf");
    let upper = path("levels", "upper.dxf");
    run(&mut s, "save", json!({"path": lower}));
    run(&mut s, "save", json!({"path": upper, "view": "Upper"}));
    let walls = |p: &PathBuf| {
        let d = dxf::read_text(&std::fs::read_to_string(p).unwrap()).unwrap();
        d.entities.iter().filter(|e| e.layer.starts_with("wall")).count()
    };
    assert_eq!(walls(&lower), 0);
    assert!(walls(&upper) > 0);
}

#[test]
fn a_dwg_needs_the_apps_writer_and_without_one_the_reply_says_to_save_as_dxf() {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [3000, 0]]}));
    let file = path("dwg", "plan.dwg");
    let e = s.execute("save", &json!({"path": file})).unwrap_err();
    assert!(e.contains(".dxf"), "{e}");
    assert!(!file.exists());
    // With one, it writes what the writer makes of the DXF.
    s.dwg_out = Some(std::sync::Arc::new(|dxf: &str| Ok(format!("DWG of {} bytes", dxf.len()).into_bytes())));
    assert_eq!(run(&mut s, "save", json!({"path": file}))["format"], "DWG");
    assert!(std::fs::read_to_string(&file).unwrap().starts_with("DWG of "));
}

#[test]
fn an_imported_drawing_goes_out_on_its_own_layers_with_its_text_as_text_and_no_empty_lines() {
    use crate::cad_tests::{dxf, file, line, pairs};
    let walls: String = [((0.0, 0.0), (10.0, 0.0)), ((10.0, 0.0), (10.0, 6.0)), ((10.0, 6.0), (0.0, 6.0)), ((0.0, 6.0), (0.0, 0.0))]
        .iter()
        .map(|(a, b)| line("WALLS", *a, *b))
        .collect();
    // A line of no length, which a reader takes as an empty entity, and a layer that's off.
    let empty = line("WALLS", (3.0, 3.0), (3.0, 3.0));
    let furn = line("FURN", (1.0, 1.0), (2.0, 1.0));
    let note = pairs(&[(0, "TEXT"), (8, "NOTES"), (10, "1"), (20, "5"), (40, "0.25"), (1, "Cozinha é aqui")]);
    let text = dxf("6", &[("WALLS", "1", false), ("FURN", "3", true), ("NOTES", "7", false)], &(walls + &empty + &furn + &note));
    let mut s = Session::new();
    run(&mut s, "import_cad", json!({"path": file("dxf-out", "ground floor.dxf", &text)}));
    let out = path("imported", "plan.dxf");
    let reply = run(&mut s, "save", json!({"path": out}));
    let layers: Vec<&str> = reply["layers"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert!(layers.contains(&"WALLS") && layers.contains(&"NOTES"), "the drawing's own layers: {layers:?}");
    assert!(!layers.iter().any(|l| l.starts_with("cadimport") || *l == "FURN"), "none of the import's own, nor one that's off: {layers:?}");

    let d = dxf::read_text(&std::fs::read_to_string(&out).unwrap()).unwrap();
    assert!(d.left_out.is_empty(), "{:?}", d.left_out);
    let on = |layer: &str| d.entities.iter().filter(|e| e.layer == layer).collect::<Vec<_>>();
    // Metres placed in millimetres, the drawing's middle on the origin; the line of no length left out.
    let walls = on("WALLS");
    assert_eq!(walls.len(), 4, "{walls:?}");
    let b = dxf::union(walls.iter().map(|e| e.shape.bounds())).unwrap();
    let near = |a: f64, b: f64| (a - b).abs() < 0.01;
    assert!(near(b[0].0, -5000.0) && near(b[0].1, -3000.0) && near(b[1].0, 5000.0) && near(b[1].1, 3000.0), "{b:?}");
    // The note is one TEXT, where it was and as tall, its accent kept.
    let notes = on("NOTES");
    assert_eq!(notes.len(), 1, "lettering as text, not the lines of our font: {notes:?}");
    let Shape::Text { at, height, text, .. } = &notes[0].shape else { panic!("a TEXT: {:?}", notes[0]) };
    assert!(near(at.0, -4000.0) && near(at.1, 2000.0) && near(*height, 250.0), "{at:?} {height}");
    assert_eq!(text, "Cozinha é aqui");
    // Nothing anywhere is an empty line.
    for e in &d.entities {
        if let Shape::Polyline { pts, .. } = &e.shape {
            assert!(pts.windows(2).any(|w| w[0] != w[1]), "{e:?}");
        }
    }
}
