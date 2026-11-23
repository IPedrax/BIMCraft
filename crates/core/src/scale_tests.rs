//! Scale (RE) through the `scale` command: annotation, CAD drawings, images and PDFs `factor` times their size about a point
//! that stays; text, tags, groups, families and model elements left out, each with why.

use serde_json::{Value, json};

use crate::commands::element_json;
use crate::engine::Session;
use crate::model::{Element, Id};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn fails(s: &mut Session, id: &str, p: Value) -> String {
    match s.execute(id, &p) {
        Ok(v) => panic!("{id} {p} was taken: {v}"),
        Err(e) => e,
    }
}

fn id(v: &Value) -> Id {
    v["id"].as_u64().unwrap_or_else(|| panic!("no id in {v}")) as Id
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

/// A folder of this test's own, empty.
fn dir(test: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("bimcraft-scale-{}-{test}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A DXF in millimetres of one line from (0, 0) to (1000, 0).
fn one_line() -> &'static str {
    "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n\
     0\nLINE\n8\n0\n10\n0\n20\n0\n11\n1000\n21\n0\n0\nENDSEC\n0\nEOF\n"
}

/// A PNG 40 px by 20, grey.
fn png(path: &std::path::Path) {
    let rgba: Vec<u8> = std::iter::repeat_n([128u8, 128, 128, 255], 40 * 20).flatten().collect();
    let pm = tiny_skia::Pixmap::from_vec(rgba, tiny_skia::IntSize::from_wh(40, 20).unwrap()).unwrap();
    std::fs::write(path, pm.encode_png().unwrap()).unwrap();
}

#[test]
fn lines_regions_and_circles_go_twice_as_big_about_the_point_that_stays() {
    let mut s = Session::new();
    let line = id(&run(&mut s, "detail_line", json!({"points": [[1000, 1000], [3000, 1000]], "arcs": [90]})));
    let circle = id(&run(&mut s, "detail_line", json!({"centre": [2000, 3000], "radius": 500})));
    let region =
        id(&run(&mut s, "filled_region", json!({"sketch": [[[0, 0], [1000, 0]], [[1000, 0], [1000, 1000]], [[1000, 1000], [0, 0]]]})));
    let r = run(&mut s, "scale_elements", json!({"ids": [line, circle, region], "about": [1000, 1000], "factor": 2}));
    assert_eq!((r["scaled"].clone(), r["left_out"].clone()), (json!([line, circle, region]), json!([])), "{r}");
    let Some(Element::DetailLine(l)) = s.model.elements.get(line) else { panic!("no line") };
    let crate::curves::Curve::Poly { points, arcs, .. } = &l.curve else { panic!("{:?}", l.curve) };
    // Its start stays; its end twice as far from it, the arc turning as far.
    assert_eq!((points.clone(), arcs.clone()), (vec![crate::geom::P2(1000.0, 1000.0), crate::geom::P2(5000.0, 1000.0)], vec![90.0]));
    let Some(Element::DetailLine(c)) = s.model.elements.get(circle) else { panic!("no circle") };
    assert_eq!(c.curve, crate::curves::Curve::Circle { centre: crate::geom::P2(3000.0, 5000.0), radius: 1000.0 });
    let Some(Element::FilledRegion(f)) = s.model.elements.get(region) else { panic!("no region") };
    let ends: Vec<_> = f.sketch.iter().map(|l| l.curve.points()[0]).collect();
    assert_eq!(ends, [crate::geom::P2(-1000.0, -1000.0), crate::geom::P2(1000.0, -1000.0), crate::geom::P2(1000.0, 1000.0)]);
    // One step to take back.
    run(&mut s, "undo", json!({}));
    let Some(Element::DetailLine(c)) = s.model.elements.get(circle) else { panic!("no circle") };
    assert_eq!(c.curve, crate::curves::Curve::Circle { centre: crate::geom::P2(2000.0, 3000.0), radius: 500.0 });
}

#[test]
fn a_cad_drawing_and_an_image_grow_about_the_point_and_a_dimension_reads_the_new_length() {
    let dir = dir("cad-image");
    let (dxf, picture) = (dir.join("line.dxf"), dir.join("site.png"));
    std::fs::write(&dxf, one_line()).unwrap();
    png(&picture);
    let mut s = Session::new();
    let cad = id(&run(&mut s, "import_cad", json!({"path": dxf.to_string_lossy(), "at": [0, 0], "position": "origin", "scale": 1})));
    let image = id(&run(&mut s, "image", json!({"path": picture.to_string_lossy(), "at": [2000, 0], "width": 4000})));
    let line = id(&run(&mut s, "detail_line", json!({"points": [[0, 5000], [1000, 5000]]})));
    let dim =
        id(&run(&mut s, "dimension", json!({"refs": [{"id": line, "point": "start"}, {"id": line, "point": "end"}], "at": [500, 6000]})));
    run(&mut s, "scale_elements", json!({"ids": [cad, image, line, dim], "about": [0, 0], "factor": 0.5}));
    let Some(Element::CadImport(ci)) = s.model.elements.get(cad) else { panic!("no import") };
    assert!(near(ci.scale, 0.5) && ci.at == crate::geom::P2(0.0, 0.0), "{ci:?}");
    let Some(Element::Image(im)) = s.model.elements.get(image) else { panic!("no image") };
    assert!(near(im.width, 2000.0) && im.at == crate::geom::P2(1000.0, 0.0), "{im:?}");
    assert_eq!(im.height(&s.model), Some(1000.0), "as tall as its pixels say");
    // The dimension measures the line half as long, its own line halfway to the point.
    let d = element_json(&s.model, dim);
    assert_eq!(d["values"], json!([500.0]), "{d}");
    let Some(Element::Dimension(dm)) = s.model.elements.get(dim) else { panic!("no dimension") };
    assert_eq!(dm.at, crate::geom::P2(250.0, 3000.0));
}

#[test]
fn text_and_model_elements_keep_their_size_and_say_why_and_nothing_scalable_is_refused() {
    let mut s = Session::new();
    let line = id(&run(&mut s, "detail_line", json!({"points": [[0, 0], [1000, 0]]})));
    let note = id(&run(&mut s, "text", json!({"text": "Kitchen", "at": [0, 2000]})));
    let wall = run(&mut s, "wall", json!({"points": [[0, -3000], [4000, -3000]]}))["ids"][0].as_u64().unwrap() as Id;
    let before = (s.model.elements.get(note).cloned(), s.model.elements.get(wall).cloned());
    let r = run(&mut s, "scale_elements", json!({"ids": [line, note, wall], "about": [0, 0], "factor": 3}));
    assert_eq!(r["scaled"], json!([line]), "{r}");
    let why: Vec<(u64, &str)> =
        r["left_out"].as_array().unwrap().iter().map(|l| (l["id"].as_u64().unwrap(), l["why"].as_str().unwrap())).collect();
    assert_eq!(why.iter().map(|w| w.0).collect::<Vec<_>>(), [note as u64, wall as u64], "{r}");
    assert!(why[0].1.contains("text keeps its size") && why[1].1.contains("model element"), "{why:?}");
    assert_eq!((s.model.elements.get(note).cloned(), s.model.elements.get(wall).cloned()), before, "left where they were, as big");
    // Only what keeps its size: refused, with why.
    let e = fails(&mut s, "scale_elements", json!({"ids": [note, wall], "about": [0, 0], "factor": 3}));
    assert!(e.contains("nothing selected is scaled") && e.contains("text"), "{e}");
}

#[test]
fn a_factor_out_of_range_and_a_pinned_line_are_refused_and_nothing_changes() {
    let mut s = Session::new();
    let line = id(&run(&mut s, "detail_line", json!({"points": [[0, 0], [1000, 0]]})));
    let before = s.model.elements.get(line).cloned();
    for k in [json!(0), json!(-2), json!(0.0001), json!(5000), json!("big"), json!(null)] {
        let e = fails(&mut s, "scale_elements", json!({"ids": [line], "about": [0, 0], "factor": k}));
        assert!(e.starts_with("scale_elements: `factor`"), "{k}: {e}");
    }
    run(&mut s, "pin", json!({"ids": [line]}));
    let e = fails(&mut s, "scale_elements", json!({"ids": [line], "about": [0, 0], "factor": 2}));
    assert!(e.contains("pinned"), "{e}");
    assert_eq!(s.model.elements.get(line).cloned(), before);
}

#[test]
fn a_sections_detail_line_is_scaled_in_the_sections_own_terms() {
    let mut s = Session::new();
    let section = id(&run(&mut s, "section", json!({"start": [0, 0], "end": [10000, 0]})));
    let line = id(&run(&mut s, "detail_line", json!({"view": section, "points": [[1000, 500], [2000, 500]]})));
    run(&mut s, "scale_elements", json!({"ids": [line], "about": [1000, 0], "factor": 2}));
    let Some(Element::DetailLine(l)) = s.model.elements.get(line) else { panic!("no line") };
    assert_eq!(l.curve.points(), [crate::geom::P2(1000.0, 1000.0), crate::geom::P2(3000.0, 1000.0)]);
}
