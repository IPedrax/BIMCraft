//! The Structure tab's elements, through the commands every front end drives: beams, structural columns, footings, strip
//! footings and structural floors.

use serde_json::{Value, json};

use crate::element_json;
use crate::engine::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn ids(r: &Value) -> Vec<u64> {
    r["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect()
}

fn fails(s: &mut Session, id: &str, p: Value) -> String {
    let before = s.model.clone();
    let e = s.execute(id, &p).expect_err(&format!("{id} {p} should fail"));
    assert_eq!(*s.model, *before, "a refused {id} changes nothing");
    e
}

#[test]
fn a_beam_runs_between_points_one_for_each_segment_its_top_at_the_level() {
    let mut s = Session::new();
    let r = run(&mut s, "beam", json!({"points": [[0, 0], [6000, 0], [6000, 4000]]}));
    let beams = ids(&r);
    assert_eq!(beams.len(), 2);
    let b = element_json(&s.model, beams[0]);
    assert_eq!((b["category"].as_str(), b["type"].as_str(), b["justify"].as_str()), (Some("beam"), Some("Beam 200 x 500"), Some("top")));
    assert_eq!((b["length"].as_f64(), b["width"].as_f64(), b["depth"].as_f64()), (Some(6000.0), Some(200.0), Some(500.0)));
    // Its top is at the level, so its centre line is half its depth under it.
    assert_eq!(b["axis"], json!([[0.0, 0.0, -250.0], [6000.0, 0.0, -250.0]]));
    assert_eq!(element_json(&s.model, beams[1])["length"], 4000.0);
    // Revit's shortcut for it; and closed, a ring.
    let ring = run(&mut s, "BM", json!({"points": [[0, 0], [1000, 0], [1000, 1000]], "closed": true}));
    assert_eq!(ids(&ring).len(), 3);
    assert_eq!(run(&mut s, "elements", json!({"category": "beams"}))["count"], 5);
}

#[test]
fn a_beam_sits_at_its_heights_slopes_and_says_which_system_it_is_in() {
    let mut s = Session::new();
    let r = run(
        &mut s,
        "beam",
        json!({"points": [[0, 0], [3000, 0]], "offset": 2800, "end_offset": 3400, "justify": "centre", "system": "Roof frame", "level": "Level 1"}),
    );
    let b = element_json(&s.model, ids(&r)[0]);
    assert_eq!(b["axis"], json!([[0.0, 0.0, 2800.0], [3000.0, 0.0, 3400.0]]));
    assert_eq!((b["system"].as_str(), b["justify"].as_str()), (Some("Roof frame"), Some("centre")));
    // `center` is accepted too, and the height of a run changes with `modify`.
    let id = ids(&run(&mut s, "beam", json!({"points": [[0, 1000], [3000, 1000]], "justify": "center"})))[0];
    run(&mut s, "modify", json!({"id": id, "offset": 500}));
    assert_eq!(element_json(&s.model, id)["axis"], json!([[0.0, 1000.0, 500.0], [3000.0, 1000.0, 500.0]]));
}

#[test]
fn a_beam_that_cannot_be_one_is_refused_and_nothing_changes() {
    let mut s = Session::new();
    let line = json!([[0, 0], [3000, 0]]);
    assert!(fails(&mut s, "beam", json!({"points": [[0, 0], [0, 0]]})).contains("too short"));
    assert!(fails(&mut s, "beam", json!({"points": [[0, 0]]})).contains("at least 2"));
    assert!(fails(&mut s, "beam", json!({"points": line, "type": "Nope"})).contains("type `Nope` doesn't exist"));
    assert!(fails(&mut s, "beam", json!({"points": line, "justify": "sideways"})).contains("justify"));
    assert!(fails(&mut s, "beam", json!({"points": line, "offset": 1e300})).contains("offset"));
    assert!(fails(&mut s, "beam", json!({"points": line, "offset": "high"})).contains("offset"));
    assert!(fails(&mut s, "beam", json!({"points": line, "system": "x".repeat(300)})).contains("system"));
    assert!(fails(&mut s, "beam", json!({"points": line, "level": "Nowhere"})).contains("Nowhere"));
    // What stands is a column: a beam slopes up to 75° and no more.
    let steep = fails(&mut s, "beam", json!({"points": [[0, 0], [100, 0]], "offset": 0, "end_offset": 5000}));
    assert!(steep.contains("slopes") && steep.contains("column"), "{steep}");
    // One end height is for one run.
    assert!(fails(&mut s, "beam", json!({"points": [[0, 0], [1000, 0], [2000, 0]], "end_offset": 100})).contains("end_offset"));
}

#[test]
fn run_types_are_made_changed_and_checked_with_type() {
    let mut s = Session::new();
    let made = run(&mut s, "type", json!({"category": "beam", "name": "IPE 200", "width": 100, "depth": 200, "material": "Metal"}));
    assert_eq!(made["type"], json!({"width": 100.0, "depth": 200.0, "round": false, "material": "Metal"}));
    let tube = run(&mut s, "type", json!({"category": "beams", "name": "Tube 100", "width": 100, "round": true}));
    assert_eq!(
        (tube["type"]["depth"].as_f64(), tube["type"]["round"].as_bool()),
        (Some(100.0), Some(true)),
        "round is as deep as it is wide"
    );
    let id = ids(&run(&mut s, "beam", json!({"points": [[0, 0], [3000, 0]], "type": "IPE 200"})))[0];
    // Changed, it changes everywhere it's used; what isn't said stays.
    run(&mut s, "type", json!({"category": "beam", "name": "IPE 200", "width": 150}));
    let b = element_json(&s.model, id);
    assert_eq!((b["width"].as_f64(), b["depth"].as_f64()), (Some(150.0), Some(200.0)));
    assert!(
        fails(&mut s, "type", json!({"category": "beam", "name": "Odd", "width": 100, "depth": 200, "material": "Unobtainium"}))
            .contains("library")
    );
    assert!(fails(&mut s, "type", json!({"category": "beam", "name": "Flat", "width": 100})).contains("depth"));
    assert!(fails(&mut s, "type", json!({"category": "beam", "name": "Zero", "width": 0, "depth": 10})).contains("above zero"));
    let e = fails(&mut s, "type", json!({"category": "duct", "name": "P", "width": 10}));
    assert!(e.contains("beam"), "the message lists what has types: {e}");
    // The strip footing's, and the columns' own.
    run(&mut s, "type", json!({"category": "stripfooting", "name": "Strip 800 x 300", "width": 800, "depth": 300}));
    run(&mut s, "type", json!({"category": "structural column", "name": "C 250 x 600", "width": 250, "depth": 600}));
    run(&mut s, "type", json!({"category": "footing", "name": "F 2000", "width": 2000, "depth": 2000}));
    assert!(s.model.types.structural_column.contains_key("C 250 x 600") && !s.model.types.column.contains_key("C 250 x 600"));
}

#[test]
fn structural_columns_and_footings_are_columns_with_a_category_and_types_of_their_own() {
    let mut s = Session::new();
    let c = run(&mut s, "structural_column", json!({"at": [1000, 2000]}))["id"].as_u64().unwrap();
    let e = element_json(&s.model, c);
    assert_eq!(
        (e["category"].as_str(), e["type"].as_str(), e["height"].as_f64()),
        (Some("structuralcolumn"), Some("Concrete 300 x 300"), Some(3000.0))
    );
    assert_eq!((e["width"].as_f64(), e["depth"].as_f64()), (Some(300.0), Some(300.0)));
    // The architectural column's types are not its types, nor its types theirs.
    let wrong = fails(&mut s, "structural_column", json!({"at": [0, 0], "type": "Rectangular 400 x 400"}));
    assert!(wrong.contains("Concrete 300 x 300"), "it lists its own: {wrong}");
    assert!(fails(&mut s, "column", json!({"at": [0, 0], "type": "Concrete 300 x 300"})).contains("Rectangular 400 x 400"));
    // A footing is 400 mm thick with its top at the level unless told otherwise.
    let f = run(&mut s, "footing", json!({"at": [1000, 2000]}))["id"].as_u64().unwrap();
    let e = element_json(&s.model, f);
    assert_eq!(
        (e["category"].as_str(), e["type"].as_str(), e["height"].as_f64(), e["base_offset"].as_f64()),
        (Some("footing"), Some("Footing 1200 x 1200"), Some(400.0), Some(-400.0))
    );
    let g = run(&mut s, "footing", json!({"at": [5000, 2000], "thickness": 600, "offset": -200, "angle": 30}))["id"].as_u64().unwrap();
    let e = element_json(&s.model, g);
    assert_eq!((e["height"].as_f64(), e["base_offset"].as_f64(), e["angle"].as_f64()), (Some(600.0), Some(-800.0), Some(30.0)));
    assert!(fails(&mut s, "footing", json!({"at": [0, 0], "thickness": 0})).contains("thickness"));
    assert!(fails(&mut s, "footing", json!({"at": [0, 0], "thickness": -5})).contains("thickness"));
    // Each is counted in its own category, however it is written.
    let n = |s: &mut Session, c: &str| run(s, "elements", json!({"category": c}))["count"].as_u64().unwrap();
    assert_eq!(
        (n(&mut s, "structural columns"), n(&mut s, "structural_column"), n(&mut s, "footings"), n(&mut s, "columns")),
        (1, 1, 2, 0)
    );
    // Radial dimensions and the rest still know only the architectural kind; CL is the column's, as in Revit.
    assert_eq!(
        run(&mut s, "CL", json!({"at": [9000, 0]}))["id"].as_u64().map(|id| element_json(&s.model, id)["category"].clone()),
        Some(json!("column"))
    );
}

#[test]
fn a_strip_footing_is_a_run_under_a_wall() {
    let mut s = Session::new();
    let r = run(&mut s, "strip_footing", json!({"points": [[0, 0], [5000, 0]]}));
    let e = element_json(&s.model, ids(&r)[0]);
    assert_eq!(
        (e["category"].as_str(), e["type"].as_str(), e["justify"].as_str()),
        (Some("stripfooting"), Some("Strip 600 x 300"), Some("top"))
    );
    // Its top is at the level.
    assert_eq!(e["axis"], json!([[0.0, 0.0, -150.0], [5000.0, 0.0, -150.0]]));
    assert!(fails(&mut s, "strip_footing", json!({"points": [[0, 0], [5000, 0]], "type": "Beam 200 x 500"})).contains("Strip 600 x 300"));
}

#[test]
fn a_structural_floor_is_a_floor_that_carries_load() {
    let mut s = Session::new();
    let square = json!([[0, 0], [5000, 0], [5000, 4000], [0, 4000]]);
    let plain = run(&mut s, "floor", json!({"boundary": square}))["id"].as_u64().unwrap();
    let loaded = run(&mut s, "floor", json!({"boundary": square, "structural": true}))["id"].as_u64().unwrap();
    assert!(element_json(&s.model, plain).get("structural").is_none(), "an ordinary floor says nothing of it");
    assert_eq!(element_json(&s.model, loaded)["structural"], true);
    assert!(fails(&mut s, "floor", json!({"boundary": square, "structural": "yes"})).contains("structural"));
    // A schedule can tell them apart.
    let table = run(&mut s, "schedule", json!({"category": "floors", "fields": ["Id", "Structural"]}));
    assert_eq!(table["rows"], json!([[plain as f64, false], [loaded as f64, true]]));
}

#[test]
fn runs_and_columns_of_the_structure_move_copy_rotate_mirror_and_delete() {
    let mut s = Session::new();
    let beam = ids(&run(&mut s, "beam", json!({"points": [[0, 0], [4000, 0]], "offset": 2800, "end_offset": 3000})))[0];
    let column = run(&mut s, "structural_column", json!({"at": [0, 0], "angle": 0}))["id"].as_u64().unwrap();
    let footing = run(&mut s, "footing", json!({"at": [0, 0]}))["id"].as_u64().unwrap();
    let axis = |s: &Session| element_json(&s.model, beam)["axis"].clone();
    let (start_z, end_z) = (axis(&s)[0][2].clone(), axis(&s)[1][2].clone());
    run(&mut s, "move", json!({"ids": [beam, column, footing], "by": [1000, 500]}));
    assert_eq!(axis(&s)[0], json!([1000.0, 500.0, start_z]), "heights stay");
    assert_eq!(element_json(&s.model, column)["at"], json!([1000.0, 500.0]));
    assert_eq!(element_json(&s.model, footing)["at"], json!([1000.0, 500.0]));
    run(&mut s, "rotate", json!({"ids": [beam, column], "about": [1000, 500], "angle": 90}));
    let a = axis(&s);
    assert!((a[1][0].as_f64().unwrap() - 1000.0).abs() < 1e-6 && (a[1][1].as_f64().unwrap() - 4500.0).abs() < 1e-6, "{a}");
    assert_eq!(element_json(&s.model, column)["angle"], 90.0);
    // A mirror keeps each end's height with its end.
    let m = run(&mut s, "mirror", json!({"ids": [beam], "axis": [[0, 0], [0, 1000]], "copy": true}));
    let copy = ids(&m)[0];
    let mirrored = element_json(&s.model, copy)["axis"].clone();
    assert_eq!((&mirrored[0][2], &mirrored[1][2]), (&start_z, &end_z));
    assert!((mirrored[0][0].as_f64().unwrap() + 1000.0).abs() < 1e-6, "{mirrored}");
    run(&mut s, "copy", json!({"ids": [beam, column, footing], "by": [0, -2000]}));
    assert_eq!(run(&mut s, "elements", json!({"category": "beam"}))["count"], 3);
    run(&mut s, "delete", json!({"ids": [beam, column, footing]}));
    assert_eq!(run(&mut s, "elements", json!({"category": "footing"}))["count"], 1);
    run(&mut s, "undo", json!({}));
    assert_eq!(run(&mut s, "elements", json!({"category": "footing"}))["count"], 2);
}

#[test]
fn a_project_with_structure_saves_and_opens_again() {
    let mut s = Session::new();
    run(&mut s, "beam", json!({"points": [[0, 0], [4000, 0]], "system": "Level 2 frame"}));
    run(&mut s, "structural_column", json!({"at": [0, 0]}));
    run(&mut s, "footing", json!({"at": [0, 0]}));
    run(&mut s, "strip_footing", json!({"points": [[0, 0], [4000, 0]]}));
    run(&mut s, "floor", json!({"boundary": [[0, 0], [4000, 0], [4000, 3000], [0, 3000]], "structural": true}));
    let path = std::env::temp_dir().join(format!("bimcraft-test-{}-structure.bimcraft", std::process::id()));
    run(&mut s, "save", json!({"path": path}));
    let mut t = Session::new();
    run(&mut t, "open", json!({"path": path}));
    let _ = std::fs::remove_file(&path);
    assert_eq!(*t.model, *s.model);
    // A project written before there was any structure has none of its fields, and opens.
    let mut old = serde_json::to_value(&*Session::new().model).unwrap();
    let types = old["types"].as_object_mut().unwrap();
    for gone in ["runs", "structural_column", "footing"] {
        types.remove(gone);
    }
    let m = crate::model::Model::from_json(&old.to_string()).unwrap();
    assert!(m.types.runs.is_empty() && m.check().is_ok());
}

// ---------- drawn ----------

use crate::geom::P2;
use crate::render::{self, Drawing, POCHE, Prim, View};

fn view() -> View {
    View { scale: 0.1, cx: 3000.0, cy: 0.0, width: 1000, height: 800 }
}

fn made(s: &mut Session, id: &str, p: Value) -> u64 {
    let r = run(s, id, p);
    r["id"].as_u64().or_else(|| ids(&r).first().copied()).unwrap()
}

/// What a drawing draws of element `id`.
fn of(d: &Drawing, id: u64) -> Vec<&Prim> {
    d.prims.iter().zip(&d.owners).filter(|(_, o)| **o == Some(id)).map(|(p, _)| p).collect()
}

/// The box the closed lines and fills of `id` take in the drawing, in model mm: from the south west corner to the north east.
fn extent(d: &Drawing, id: u64) -> (P2, P2) {
    let pts: Vec<P2> = of(d, id)
        .into_iter()
        .flat_map(|p| match p {
            Prim::Line { pts, .. } => pts.clone(),
            Prim::Fill { rings, .. } => rings.iter().flatten().copied().collect(),
            Prim::Image { .. } => Vec::new(),
        })
        .map(|q| d.view.mm(q))
        .collect();
    let lo = pts.iter().fold(P2(f64::MAX, f64::MAX), |a, q| P2(a.0.min(q.0), a.1.min(q.1)));
    let hi = pts.iter().fold(P2(f64::MIN, f64::MIN), |a, q| P2(a.0.max(q.0), a.1.max(q.1)));
    (P2((lo.0 * 10.0).round() / 10.0, (lo.1 * 10.0).round() / 10.0), P2((hi.0 * 10.0).round() / 10.0, (hi.1 * 10.0).round() / 10.0))
}

#[test]
fn a_plan_sees_the_beams_hanging_under_its_level_the_ones_it_cuts_and_the_ones_overhead() {
    let mut s = Session::new();
    // Hanging from Level 1: top at the level, 500 deep.
    let hung = made(&mut s, "beam", json!({"points": [[0, 0], [6000, 0]]}));
    // Over the cut plane, 1200 up, and under the level above: overhead.
    let over = made(&mut s, "beam", json!({"points": [[0, 1000], [6000, 1000]], "offset": 2800}));
    // Through the cut plane; and one far over the view range's top, which the plan doesn't see.
    let through = made(&mut s, "beam", json!({"points": [[0, 2000], [6000, 2000]], "offset": 1500}));
    let high = made(&mut s, "beam", json!({"points": [[0, 4000], [6000, 4000]], "offset": 6000}));
    let d = render::draw(&s.model, 100000, view());
    // Hung: one outline of its footprint, 200 wide, projected (thin, solid).
    let lines = of(&d, hung);
    assert!(
        matches!(lines.as_slice(), [Prim::Line { closed: true, dash: None, width, .. }, Prim::Line { closed: false, dash: Some(_), .. }] if *width < 2.0),
        "{} prims",
        lines.len()
    );
    assert_eq!(extent(&d, hung), (P2(0.0, -100.0), P2(6000.0, 100.0)));
    // Overhead: dashed.
    let overhead = of(&d, over);
    assert!(overhead.iter().all(|p| matches!(p, Prim::Line { dash: Some(_), .. })), "dashed");
    assert_eq!(overhead.iter().filter(|p| matches!(p, Prim::Line { closed: true, .. })).count(), 1, "its outline, and its centre line");
    assert_eq!(overhead.len(), 2);
    // Cut: a heavy outline and poché.
    let cut = of(&d, through);
    assert!(cut.iter().any(|p| matches!(p, Prim::Fill { color, .. } if *color == POCHE)), "{} prims", cut.len());
    assert!(cut.iter().any(|p| matches!(p, Prim::Line { width, dash: None, .. } if *width >= 3.0)));
    assert!(cut.iter().all(|p| !matches!(p, Prim::Line { dash: Some(_), .. })), "a beam that is cut is not also overhead");
    assert!(of(&d, high).is_empty());
    // The plan above sees none of them; the level above is at 3000.
    let up = render::draw(&s.model, 100001, view());
    for id in [hung, through] {
        assert!(of(&up, id).is_empty(), "{id} is under the plan above's view range");
    }
}

#[test]
fn the_plan_above_sees_the_beams_hanging_under_it_but_not_the_walls_of_the_storey_below() {
    let mut s = Session::new();
    // Walls up to Level 2, and a beam of Level 1 hung from it: its top flush with the plan above's bottom.
    let wall = ids(&run(&mut s, "wall", json!({"points": [[0, 0], [6000, 0]]})))[0];
    let beam = made(&mut s, "beam", json!({"points": [[0, 1000], [6000, 1000]], "offset": 3000}));
    let d = render::draw(&s.model, 100001, view());
    assert!(of(&d, wall).is_empty(), "a wall's top at the view's bottom is under the plan");
    assert!(!of(&d, beam).is_empty(), "a beam's is the level it hangs from");
    // Lower, its top is under the view's depth: not seen.
    run(&mut s, "modify", json!({"id": beam, "offset": 2900}));
    assert!(of(&render::draw(&s.model, 100001, view()), beam).is_empty());
}

#[test]
fn a_plan_sees_footings_under_its_level_and_structural_columns_where_columns_are_seen() {
    let mut s = Session::new();
    let footing = made(&mut s, "footing", json!({"at": [1000, 1000], "type": "Footing 1500 x 1500"}));
    let column = made(&mut s, "structural_column", json!({"at": [1000, 1000]}));
    let strip = made(&mut s, "strip_footing", json!({"points": [[3000, 0], [3000, 4000]]}));
    let d = render::draw(&s.model, 100000, view());
    assert_eq!(extent(&d, footing), (P2(250.0, 250.0), P2(1750.0, 1750.0)));
    assert_eq!(extent(&d, column), (P2(850.0, 850.0), P2(1150.0, 1150.0)));
    assert!(of(&d, column).iter().any(|p| matches!(p, Prim::Fill { color, .. } if *color == POCHE)), "a column is cut by the plane");
    assert_eq!(extent(&d, strip), (P2(2700.0, 0.0), P2(3300.0, 4000.0)));
    // A footing's sketch is drawn as the plan would draw it.
    let sketch = render::sketch(&s.model, view(), 999, &s.model.elements[&footing]);
    assert!(sketch.owners.iter().all(|o| *o == Some(999)) && !sketch.prims.is_empty());
    let sketch = render::sketch(&s.model, view(), 999, &s.model.elements[&strip]);
    assert_eq!(extent(&sketch, 999), (P2(2700.0, 0.0), P2(3300.0, 4000.0)));
}

#[test]
fn a_section_cuts_a_beam_across_its_section() {
    let mut s = Session::new();
    let beam = made(&mut s, "beam", json!({"points": [[0, 0], [6000, 0]], "offset": 2800}));
    let section = made(&mut s, "section", json!({"start": [3000, -2000], "end": [3000, 2000]}));
    let d = render::section(&s.model, section, View { scale: 0.1, cx: 2000.0, cy: 1500.0, width: 1000, height: 800 });
    let (lo, hi) = extent(&d, beam);
    // 200 wide, across the line at 2000 along it; 500 deep, its top at 2800.
    assert_eq!((lo, hi), (P2(1900.0, 2300.0), P2(2100.0, 2800.0)));
    assert!(of(&d, beam).iter().any(|p| matches!(p, Prim::Fill { color, .. } if *color == POCHE)));
}

#[test]
fn the_3d_view_has_a_solid_for_a_beam_a_round_one_and_the_columns_and_footings() {
    let mut s = Session::new();
    let beam = made(&mut s, "beam", json!({"points": [[0, 0], [6000, 0]], "offset": 2800}));
    let tube = made(&mut s, "beam", json!({"points": [[0, 2000], [6000, 2000]], "type": "Steel Tube 150", "justify": "centre"}));
    let column = made(&mut s, "structural_column", json!({"at": [0, 0]}));
    let footing = made(&mut s, "footing", json!({"at": [0, 0]}));
    let solids = crate::view3d::solids(&s.model);
    let of = |id: u64| solids.iter().filter(|x| x.owner() == id).collect::<Vec<_>>();
    assert_eq!(of(beam).iter().map(|x| x.heights()).collect::<Vec<_>>(), vec![(2300.0, 2800.0)]);
    assert!(matches!(of(beam).as_slice(), [crate::view3d::Solid::Prism { .. }]));
    assert!(matches!(of(tube).as_slice(), [crate::view3d::Solid::Mesh { round: true, .. }]));
    assert_eq!(of(column)[0].heights(), (0.0, 3000.0));
    assert_eq!(of(footing)[0].heights(), (-400.0, 0.0));
}

#[test]
fn points_picked_snap_to_a_beams_ends_and_middle() {
    use crate::snap::{Kind, Reach, Snaps, snap};
    let mut s = Session::new();
    made(&mut s, "beam", json!({"points": [[0, 0], [6000, 0]], "offset": 2800}));
    made(&mut s, "structural_column", json!({"at": [3000, 3000]}));
    let reach = Reach { point: 100.0, line: 60.0 };
    let at = |p: P2| {
        let f = snap(&s.model, 100000, p, reach, &[], false, &Snaps::default());
        (f.at, f.kind)
    };
    assert_eq!(at(P2(6040.0, 30.0)), (P2(6000.0, 0.0), Kind::Endpoint));
    assert_eq!(at(P2(3040.0, 30.0)), (P2(3000.0, 0.0), Kind::Midpoint));
    assert_eq!(at(P2(3030.0, 3020.0)), (P2(3000.0, 3000.0), Kind::Centre));
}

#[test]
fn every_category_of_the_structure_has_a_schedule_and_hides_by_category() {
    let mut s = Session::new();
    let beam = made(&mut s, "beam", json!({"points": [[0, 0], [6000, 0]], "system": "Frame A"}));
    made(&mut s, "beam", json!({"points": [[0, 3000], [3000, 3000]], "system": "Frame B", "type": "Beam 150 x 400"}));
    made(&mut s, "structural_column", json!({"at": [0, 0]}));
    made(&mut s, "footing", json!({"at": [0, 0], "thickness": 500}));
    made(&mut s, "strip_footing", json!({"points": [[0, 0], [6000, 0]]}));
    let table = |s: &mut Session, p: Value| run(s, "schedule", p);
    let r = table(&mut s, json!({"name": "Beam Schedule", "category": "beams"}));
    assert_eq!(r["columns"], json!(["Id", "Type", "Level", "Length", "Width", "Depth"]));
    assert_eq!(r["rows"].as_array().unwrap().len(), 2);
    assert_eq!(r["rows"][0][3], 6000.0);
    // By system, as one row for each.
    let r = table(
        &mut s,
        json!({"name": "Beam Schedule", "fields": ["System", "Count", {"name": "Length", "total": true}], "sort": ["System"], "grouped": true}),
    );
    assert_eq!(r["rows"], json!([["Frame A", 1.0, 6000.0], ["Frame B", 1.0, 3000.0]]));
    assert_eq!(r["totals"], json!([null, null, 9000.0]));
    let r = table(&mut s, json!({"name": "Footing Schedule", "category": "footings"}));
    assert_eq!(
        (r["columns"].clone(), r["rows"][0][5].clone()),
        (json!(["Id", "Type", "Level", "Width", "Depth", "Thickness"]), json!(500.0))
    );
    let r = table(&mut s, json!({"name": "Structural Column Schedule", "category": "structural columns"}));
    assert_eq!(r["columns"], json!(["Id", "Type", "Level", "Width", "Depth", "Height"]));
    let r = table(&mut s, json!({"name": "Strip Footing Schedule", "category": "stripfootings"}));
    assert_eq!(r["rows"][0][3], 6000.0);
    // Hide the category in the plan: the beams go; the footings stay.
    let view = view();
    let shown = |s: &Session| render::draw(&s.model, 100000, view).owners.iter().filter(|o| **o == Some(beam)).count();
    assert!(shown(&s) > 0);
    run(&mut s, "graphics", json!({"view": "Level 1", "hide": ["beam"]}));
    assert_eq!(shown(&s), 0);
    run(&mut s, "graphics", json!({"view": "Level 1", "hide": ["structuralcolumn", "footing", "stripfooting"]}));
}

// ---------- IFC ----------

/// The IFC4 file of the project, and the entities of one kind in it: `#n=NAME(…);`.
fn ifc(s: &mut Session) -> String {
    let path = std::env::temp_dir().join(format!("bimcraft-test-{}-{}.ifc", std::process::id(), s.model.uid));
    run(s, "save", json!({"path": path}));
    let text = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(path);
    text
}

fn entities<'a>(file: &'a str, name: &str) -> Vec<&'a str> {
    let head = format!("={name}(");
    file.lines().filter(|l| l.contains(&head)).collect()
}

#[test]
fn a_beam_is_an_ifc_beam_swept_along_its_axis_in_its_material() {
    let mut s = Session::new();
    made(&mut s, "beam", json!({"points": [[0, 0], [6000, 0]], "offset": 2800, "system": "Frame A"}));
    let file = ifc(&mut s);
    let beam = entities(&file, "IFCBEAM");
    assert_eq!(beam.len(), 1, "{file}");
    assert!(beam[0].contains("'Beam 200 x 500'") && beam[0].ends_with(".BEAM.);"), "{}", beam[0]);
    // 200 across, 500 deep, 6000 long; placed at the start of its centre line, 250 under its top.
    assert_eq!(entities(&file, "IFCRECTANGLEPROFILEDEF").len(), 1);
    assert!(entities(&file, "IFCRECTANGLEPROFILEDEF")[0].ends_with(",200.,500.);"));
    assert!(entities(&file, "IFCEXTRUDEDAREASOLID")[0].ends_with(",6000.);"));
    assert!(file.contains("IFCCARTESIANPOINT((0.,0.,2550.))"), "{file}");
    // Along east; its section's x across it, to the north.
    assert!(file.contains("IFCDIRECTION((1.,0.,0.))") && file.contains("IFCDIRECTION((0.,1.,0.))"));
    // Of concrete, load bearing, in a system.
    assert!(file.contains("IFCMATERIAL('Concrete'"));
    assert_eq!(entities(&file, "IFCRELASSOCIATESMATERIAL").len(), 1);
    assert!(
        file.contains("IFCPROPERTYSET(")
            && file.contains("'Pset_BeamCommon'")
            && file.contains("IFCPROPERTYSINGLEVALUE('LoadBearing',$,IFCBOOLEAN(.T.),$)")
    );
    // In the system it is in: an IfcSystem it is assigned to, and its justification in a property set of its own.
    assert!(file.contains("'BIMCraft_Run'") && file.contains("IFCTEXT('top')"));
    assert!(entities(&file, "IFCSYSTEM")[0].contains("'Frame A'") && entities(&file, "IFCRELASSIGNSTOGROUP").len() == 1);
}

#[test]
fn a_sloped_round_beam_has_a_circle_for_a_profile_and_aims_along_its_axis() {
    let mut s = Session::new();
    made(
        &mut s,
        "beam",
        json!({"points": [[0, 0], [3000, 0]], "offset": 0, "end_offset": 4000, "justify": "centre", "type": "Steel Tube 150"}),
    );
    let file = ifc(&mut s);
    assert!(entities(&file, "IFCCIRCLEPROFILEDEF")[0].ends_with(",75.);"));
    assert!(entities(&file, "IFCEXTRUDEDAREASOLID")[0].ends_with(",5000.);"));
    // Aimed along (3, 0, 4) / 5.
    assert!(file.contains("IFCDIRECTION((0.6,0.,0.8))"), "{file}");
}

#[test]
fn footings_and_strip_footings_are_ifc_footings_and_structural_columns_are_columns_that_carry_load() {
    let mut s = Session::new();
    made(&mut s, "footing", json!({"at": [0, 0]}));
    made(&mut s, "strip_footing", json!({"points": [[0, 0], [4000, 0]]}));
    made(&mut s, "structural_column", json!({"at": [0, 0], "type": "Concrete Round 400"}));
    made(&mut s, "column", json!({"at": [5000, 0]}));
    let file = ifc(&mut s);
    let footings = entities(&file, "IFCFOOTING");
    assert_eq!(footings.len(), 2);
    assert_eq!(footings.iter().filter(|f| f.ends_with(".PAD_FOOTING.);")).count(), 1);
    assert_eq!(footings.iter().filter(|f| f.ends_with(".STRIP_FOOTING.);")).count(), 1);
    assert_eq!(entities(&file, "IFCCOLUMN").len(), 2);
    // The pad is 1200 square, 400 thick, its top at the level: it starts 400 under it.
    assert!(entities(&file, "IFCRECTANGLEPROFILEDEF").iter().any(|r| r.ends_with(",1200.,1200.);")));
    assert!(file.contains("IFCCARTESIANPOINT((0.,0.,-400.))"));
    assert!(entities(&file, "IFCCIRCLEPROFILEDEF").iter().any(|r| r.ends_with(",200.);")));
    // Only what is structural says it carries load: the strip footing, the footing and the one column, not the other.
    for pset in ["Pset_FootingCommon", "Pset_ColumnCommon"] {
        assert!(file.contains(&format!("'{pset}'")), "{pset}");
    }
    assert_eq!(file.matches("'Pset_ColumnCommon'").count(), 1);
    assert_eq!(file.matches("'Pset_FootingCommon'").count(), 2);
}

#[test]
fn a_structural_floor_says_it_carries_load() {
    let mut s = Session::new();
    let square = json!([[0, 0], [5000, 0], [5000, 4000], [0, 4000]]);
    run(&mut s, "floor", json!({"boundary": square}));
    assert!(!ifc(&mut s).contains("Pset_SlabCommon"));
    run(&mut s, "floor", json!({"boundary": square, "structural": true}));
    let file = ifc(&mut s);
    assert_eq!(file.matches("'Pset_SlabCommon'").count(), 1);
    assert_eq!(entities(&file, "IFCSLAB").len(), 2);
}

// ---------- IFC, back in ----------

/// What an element is and where, to the tenth of a millimetre, whatever the id and the type's name.
fn described(m: &crate::model::Model, category: &str) -> Vec<String> {
    let fine = |v: &Value| match v {
        Value::Number(x) => format!("{:.1}", x.as_f64().unwrap_or(f64::NAN) + 0.0),
        Value::Array(a) => format!("{a:?}"),
        other => other.to_string(),
    };
    let keys: &[&str] = match category {
        "beam" | "stripfooting" => &["width", "depth", "round", "length"],
        "floor" => &["structural", "area_m2"],
        // A column's section reads back with its sides the other way round and turned a quarter: the same section.
        "footing" => &["at", "round", "height", "base_offset", "top_level"],
        _ => &["at", "round", "height", "base_offset"],
    };
    let mut out: Vec<String> = m
        .elements
        .iter()
        .filter(|(_, e)| e.category() == category)
        .map(|(id, _)| {
            let e = element_json(m, *id);
            let mut parts: Vec<String> = keys.iter().map(|k| format!("{k}={}", fine(&e[*k]))).collect();
            if !matches!(category, "beam" | "stripfooting" | "floor") {
                let (w, d) = (e["width"].as_f64().unwrap(), e["depth"].as_f64().unwrap());
                parts.push(format!("section={:.1}x{:.1}", w.min(d), w.max(d)));
            }
            if let Some(axis) = e["axis"].as_array() {
                let round = |p: &Value| {
                    p.as_array().unwrap().iter().map(|x| format!("{:.1}", x.as_f64().unwrap() + 0.0)).collect::<Vec<_>>().join(",")
                };
                // Which end is the start is the file's to say: a beam drawn west comes back drawn east.
                let mut ends = [round(&axis[0]), round(&axis[1])];
                ends.sort();
                parts.push(format!("axis={}->{}", ends[0], ends[1]));
            }
            parts.join(" ")
        })
        .collect();
    out.sort();
    out
}

fn the_structure(s: &mut Session) {
    run(s, "beam", json!({"points": [[0, 0], [6000, 0]], "offset": 3000}));
    run(
        s,
        "beam",
        json!({"points": [[0, 3000], [4000, 3000]], "offset": 2800, "end_offset": 3400, "justify": "centre", "type": "Beam 150 x 400"}),
    );
    run(
        s,
        "beam",
        json!({"points": [[500, 5000], [3500, 4000]], "type": "Steel Tube 150", "offset": 3000, "end_offset": 3900, "justify": "centre"}),
    );
    run(s, "footing", json!({"at": [0, 0], "thickness": 500, "angle": 20, "type": "Footing 1500 x 1500"}));
    run(s, "footing", json!({"at": [6000, 0]}));
    run(s, "strip_footing", json!({"points": [[0, 0], [6000, 0], [6000, 5000]], "offset": -200}));
    run(s, "structural_column", json!({"at": [4000, 3000], "type": "Concrete Round 400"}));
    run(s, "structural_column", json!({"at": [5000, 3000], "type": "Concrete 200 x 500", "angle": 30}));
    run(s, "column", json!({"at": [8000, 3000]}));
    // Types of their own, which a project read from a file has none of until it brings them; one beam drawn west; a footing
    // hanging from the level above.
    run(s, "type", json!({"category": "beam", "name": "B 250 x 600", "width": 250, "depth": 600, "material": "Concrete"}));
    run(s, "type", json!({"category": "stripfooting", "name": "S 900 x 350", "width": 900, "depth": 350}));
    run(s, "type", json!({"category": "structuralcolumn", "name": "C 250 x 600", "width": 250, "depth": 600}));
    run(s, "type", json!({"category": "footing", "name": "F 1800", "width": 1800, "depth": 1800}));
    run(s, "beam", json!({"points": [[6000, 6500], [0, 6500]], "type": "B 250 x 600", "offset": 3000}));
    // And one running north-north-west, whose direction the body's points alone cannot sign.
    run(s, "beam", json!({"points": [[7500, 500], [6500, 4500]], "offset": 3000}));
    run(s, "strip_footing", json!({"points": [[0, 5500], [3000, 5500]], "type": "S 900 x 350"}));
    run(s, "structural_column", json!({"at": [1000, 4000], "type": "C 250 x 600"}));
    run(s, "footing", json!({"at": [3000, 4000], "type": "F 1800", "offset": 3000, "thickness": 400}));
    let square = json!([[0, 0], [6000, 0], [6000, 5000], [0, 5000]]);
    run(s, "floor", json!({"boundary": square, "structural": true, "offset": 3000}));
    run(s, "floor", json!({"boundary": [[7000, 0], [9000, 0], [9000, 2000], [7000, 2000]]}));
}

#[test]
fn the_structure_comes_back_from_ifc() {
    let mut s = Session::new();
    the_structure(&mut s);
    let path = std::env::temp_dir().join(format!("bimcraft-test-{}-structure-round.ifc", std::process::id()));
    run(&mut s, "save", json!({"path": path}));
    let mut t = Session::new();
    let r = run(&mut t, "open", json!({"path": path}));
    let _ = std::fs::remove_file(&path);
    assert_eq!(r["skipped"], json!([]), "{r}");
    for category in ["beam", "stripfooting", "structuralcolumn", "footing", "column", "floor"] {
        let (before, after) = (described(&s.model, category), described(&t.model, category));
        assert!(!before.is_empty(), "{category}");
        assert_eq!(after, before, "{category}");
    }
    // A beam comes back running east, whichever way it was drawn.
    for (_, e) in t.model.elements.iter().filter(|(_, e)| e.category() == "beam") {
        let Some(r) = e.run() else { continue };
        assert!(r.start.0 <= r.end.0, "{:?} to {:?}", r.start, r.end);
    }
    // The types they were of came with them, in the maps of their own.
    assert!(t.model.types.structural_column.contains_key("C 250 x 600") && t.model.types.footing.contains_key("F 1800"));
    assert!(t.model.types.runs["beam"].contains_key("B 250 x 600") && t.model.types.runs["stripfooting"].contains_key("S 900 x 350"));
    // An architectural column stays one; a floor says whether it is structural.
    assert_eq!(described(&t.model, "column").len(), 1);
    // And the project they come into holds what they need.
    assert!(t.model.check().is_ok());
}

#[test]
fn hostile_parameters_to_the_structure_are_refused_without_a_panic_or_a_broken_model() {
    let hostile = [
        json!(null),
        json!([]),
        json!("x"),
        json!({"points": "x"}),
        json!({"points": [[0, 0]]}),
        json!({"points": [[0, 0], [1e308, 1e308]]}),
        json!({"points": [[0, 0], [-1e300, 0]], "closed": true}),
        json!({"points": [[0, 0], [1000, 0]], "offset": 1e300}),
        json!({"points": [[0, 0], [1000, 0]], "offset": f64::MAX, "end_offset": -f64::MAX}),
        json!({"points": [[0, 0], [1000, 0]], "offset": [], "end_offset": {}}),
        json!({"points": [[0, 0], [1000, 0]], "justify": 5}),
        json!({"points": [[0, 0], [1000, 0]], "justify": "sideways"}),
        json!({"points": [[0, 0], [1000, 0]], "system": 5}),
        json!({"points": [[0, 0], [1000, 0]], "type": 5}),
        json!({"points": [[0, 0], [1000, 0]], "level": 1e300}),
        json!({"points": [[0, 0], [1000, 0]], "offset": 0, "end_offset": 1e9}),
        json!({"points": [[0, 0], [0, 0]], "offset": 0, "end_offset": 1000}),
        json!({"at": [1e300, -1e300]}),
        json!({"at": [0, 0], "thickness": 1e300}),
        json!({"at": [0, 0], "thickness": -1}),
        json!({"at": [0, 0], "thickness": "x", "offset": 1e300}),
        json!({"at": [0, 0], "type": "Nothing", "angle": 1e300}),
        json!({"at": [0, 0], "height": 1e300, "base_offset": -1e300}),
        json!({"category": "beam", "name": "", "width": 1}),
        json!({"category": "beam", "name": "B", "width": 1e300, "depth": 1}),
        json!({"category": "beam", "name": "B", "width": 100, "depth": -1}),
        json!({"category": "beam", "name": "B", "width": 100, "round": "yes"}),
        json!({"category": "beam", "name": "B", "width": 100, "depth": 100, "material": 5}),
        json!({"category": "stripfooting", "name": "S", "width": f64::NAN}),
        json!({"category": "footing", "name": "F", "width": 0}),
        json!({"boundary": [[0, 0], [1, 0], [0, 1]], "structural": []}),
        json!({"boundary": [[0, 0], [1000, 0], [0, 1000]], "structural": true, "type": "Nothing"}),
    ];
    for id in ["beam", "strip_footing", "structural_column", "footing", "type", "floor", "modify"] {
        for p in &hostile {
            let mut s = Session::new();
            the_structure(&mut s);
            let refused = s.execute(id, p);
            if let Err(e) = &refused {
                assert!(!e.contains("internal error"), "{id} {p}: {e}");
            }
            assert_eq!(s.model.check(), Ok(()), "{id} {p} left a broken model");
        }
    }
    // A beam changed into something it can't be is refused, whatever the field.
    let mut s = Session::new();
    the_structure(&mut s);
    let beam = s.model.elements.iter().find(|(_, e)| e.category() == "beam").map(|(id, _)| *id).unwrap();
    for change in [
        json!({"end_offset": 1e9}),
        json!({"end": [0, 0], "start": [0, 0]}),
        json!({"type": "Nope"}),
        json!({"justify": "up"}),
        json!({"offset": 1e300}),
    ] {
        let mut p = change.clone();
        p["id"] = json!(beam);
        let before = s.model.clone();
        assert!(s.execute("modify", &p).is_err(), "{p}");
        assert_eq!(*s.model, *before);
    }
}

#[test]
fn what_a_tool_previews_is_the_element_its_command_makes() {
    let mut s = Session::new();
    let beam = json!({"points": [[0, 0], [4000, 0]], "offset": 200, "justify": "centre", "system": "S"});
    let sketched = crate::runs_of(&s.model, &beam, "beam").unwrap();
    let id = made(&mut s, "beam", beam);
    assert_eq!(sketched, vec![s.model.elements[&id].clone()]);
    let strip = json!({"points": [[0, 2000], [4000, 2000]]});
    let sketched = crate::runs_of(&s.model, &strip, "stripfooting").unwrap();
    let id = made(&mut s, "strip_footing", strip);
    assert_eq!(sketched, vec![s.model.elements[&id].clone()]);
    for (category, command, p) in [
        ("structuralcolumn", "structural_column", json!({"at": [1000, 1000], "height": 2500})),
        ("footing", "footing", json!({"at": [1000, 1000], "thickness": 300, "offset": -100})),
        ("column", "column", json!({"at": [3000, 1000]})),
    ] {
        let sketched = crate::stood_of(&s.model, &p, category).unwrap();
        let id = made(&mut s, command, p);
        assert_eq!(sketched, s.model.elements[&id], "{command}");
    }
    // And it says why when the command would refuse.
    assert!(crate::runs_of(&s.model, &json!({"points": [[0, 0]]}), "beam").unwrap_err().contains("at least 2"));
    assert!(crate::stood_of(&s.model, &json!({"at": [0, 0], "type": "Nope"}), "footing").is_ok(), "the type is the model's to check");
}

#[test]
fn a_project_whose_run_type_is_round_and_not_as_deep_as_it_is_wide_is_refused() {
    let s = Session::new();
    let mut m = (*s.model).clone();
    assert_eq!(m.check(), Ok(()));
    let t = crate::model::RunType { width: 100.0, depth: 200.0, round: true, material: String::new() };
    m.types.runs.entry("beam".into()).or_default().insert("Odd".into(), t);
    assert!(m.check().unwrap_err().contains("round section"), "{:?}", m.check());
    // Or of a category that has no runs, or a material the library hasn't.
    let mut m = (*s.model).clone();
    m.types.runs.insert("duct".into(), Default::default());
    assert!(m.check().unwrap_err().contains("no run types"));
    let mut m = (*s.model).clone();
    if let Some(t) = m.types.runs.get_mut("beam").and_then(|t| t.get_mut("Beam 200 x 500")) {
        t.material = "Unobtainium".into();
    }
    assert!(m.check().unwrap_err().contains("library"));
}
