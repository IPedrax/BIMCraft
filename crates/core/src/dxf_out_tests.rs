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
