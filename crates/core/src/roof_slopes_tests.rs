//! A slope for each edge of a roof by footprint (`slopes`, file format 15), through the commands every front end drives.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom::P2;
use crate::model::{Element, Roof};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn fails(s: &mut Session, id: &str, p: Value) -> String {
    let before = s.model.clone();
    let e = s.execute(id, &p).expect_err(&format!("{id} {p} should fail"));
    assert_eq!(*s.model, *before, "a refused {id} changes nothing");
    e
}

/// An 8 by 6 m rectangle: its edges are south (0), east (1), north (2) and west (3).
fn rect() -> Value {
    json!([[0, 0], [8000, 0], [8000, 6000], [0, 6000]])
}

fn roof_of(s: &Session, id: u64) -> &Roof {
    let Some(Element::Roof(r)) = s.model.elements.get(id) else { panic!("no roof {id}") };
    r
}

/// How steep each face of a roof is, as rise over run, flattest first.
fn pitches(s: &Session, id: u64) -> Vec<f64> {
    let mut g: Vec<f64> = s.model.roof_faces(roof_of(s, id)).unwrap().iter().map(|(_, plane)| plane.g.length()).collect();
    g.sort_by(f64::total_cmp);
    g
}

/// How high roof `id`'s top is over (`x`, `y`), mm above Level 1.
fn top(s: &Session, id: u64, x: f64, y: f64) -> f64 {
    crate::view3d::tops_at(&s.model, P2(x, y)).get(&id).copied().unwrap_or(f64::NAN)
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

fn tan(degrees: f64) -> f64 {
    degrees.to_radians().tan()
}

#[test]
fn a_rectangle_with_two_edges_at_30_degrees_and_two_at_45_has_faces_at_those_pitches() {
    let mut s = Session::new();
    // South and north at 30°, east and west at 45°.
    let id = run(&mut s, "roof", json!({"boundary": rect(), "slope": 30, "slopes": [30, 45, 30, 45]}))["id"].as_u64().unwrap();
    let g = pitches(&s, id);
    let want = [tan(30.0), tan(30.0), tan(45.0), tan(45.0)];
    assert!(g.len() == 4 && g.iter().zip(want).all(|(a, b)| near(*a, b)), "{g:?}");
    // The eaves are level, so the slopes meet along their hips: the south and west faces are one height where they meet.
    let faces = s.model.roof_faces(roof_of(&s, id)).unwrap();
    let plane = |along_y: bool| {
        faces.iter().map(|(_, p)| *p).find(|p| {
            (p.g.length() - tan(if along_y { 30.0 } else { 45.0 })).abs() < 1e-9 && (p.g.1 > 1e-9) == along_y && (p.g.0 > 1e-9) != along_y
        })
    };
    let (south, west) = (plane(true).unwrap(), plane(false).unwrap());
    let hip = P2(500.0, 500.0 * tan(45.0) / tan(30.0));
    assert!(near(south.at(hip), west.at(hip)) && near(south.at(hip), 3000.0 + 500.0), "{} {}", south.at(hip), west.at(hip));
    // The ridge, where the 30° slopes meet, 3 m in from both of them.
    let ridge = 3000.0 + 3000.0 * tan(30.0) + 300.0 / 30f64.to_radians().cos();
    assert!(near(top(&s, id, 4000.0, 3000.0), ridge), "{}", top(&s, id, 4000.0, 3000.0));
    // And near the west end, 500 mm from the eave, the 45° slope.
    assert!(near(top(&s, id, 500.0, 3000.0), 3000.0 + 500.0 + 300.0 / 45f64.to_radians().cos()));
}

#[test]
fn a_roofs_slopes_are_changed_with_modify_and_zero_is_the_roofs_own() {
    let mut s = Session::new();
    let id = run(&mut s, "roof", json!({"boundary": rect(), "slope": 30}))["id"].as_u64().unwrap();
    assert!(roof_of(&s, id).slopes.is_empty(), "every edge at `slope`");
    let flat = pitches(&s, id);
    assert!(flat.iter().all(|g| near(*g, tan(30.0))), "{flat:?}");
    // Zero for an edge is the roof's slope, so a list of zeros is the roof as it was.
    run(&mut s, "modify", json!({"id": id, "slopes": [0, 0, 0, 0]}));
    assert_eq!(pitches(&s, id), flat);
    run(&mut s, "modify", json!({"id": id, "slopes": [0, 45, 0, 45]}));
    let g = pitches(&s, id);
    assert!(near(g[0], tan(30.0)) && near(g[3], tan(45.0)), "{g:?}");
    // Edges with none of their own follow the roof when its slope changes.
    run(&mut s, "modify", json!({"id": id, "slope": 20}));
    let g = pitches(&s, id);
    assert!(near(g[0], tan(20.0)) && near(g[1], tan(20.0)) && near(g[2], tan(45.0)), "{g:?}");
    // An empty list is every edge at `slope` again.
    run(&mut s, "modify", json!({"id": id, "slopes": []}));
    assert!(pitches(&s, id).iter().all(|g| near(*g, tan(20.0))));
}

#[test]
fn a_gable_edge_stays_upright_whatever_its_slope() {
    let mut s = Session::new();
    for slopes in [json!([]), json!([30, 60, 30, 60]), json!([0, 10, 0, 74])] {
        let id = run(&mut s, "roof", json!({"boundary": rect(), "slope": 30, "slopes": slopes, "gables": [1, 3]}))["id"].as_u64().unwrap();
        // Two slopes only, the 30° ones; the ridge runs out to the upright ends.
        let g = pitches(&s, id);
        assert!(g.len() == 2 && g.iter().all(|g| near(*g, tan(30.0))), "{slopes}: {g:?}");
        let ridge = 3000.0 + 3000.0 * tan(30.0) + 300.0 / 30f64.to_radians().cos();
        assert!(near(top(&s, id, 7999.0, 3000.0), ridge), "{slopes}");
    }
}

#[test]
fn a_list_of_the_wrong_length_or_a_slope_out_of_range_is_refused_saying_which_edge() {
    let mut s = Session::new();
    let e = fails(&mut s, "roof", json!({"boundary": rect(), "slope": 30, "slopes": [30, 45, 30]}));
    assert!(e.contains("slopes") && e.contains("3") && e.contains("4 edges"), "{e}");
    let id = run(&mut s, "roof", json!({"boundary": rect(), "slope": 30}))["id"].as_u64().unwrap();
    let e = fails(&mut s, "modify", json!({"id": id, "slopes": [30, 45, 30, 45, 30]}));
    assert!(e.contains("slopes") && e.contains("5") && e.contains("4 edges"), "{e}");
    let e = fails(&mut s, "modify", json!({"id": id, "slopes": [30, 80, 30, 30]}));
    assert!(e.contains("edge 1") && e.contains("75"), "{e}");
    let e = fails(&mut s, "modify", json!({"id": id, "slopes": [30, 30, -5, 30]}));
    assert!(e.contains("edge 2"), "{e}");
    let e = fails(&mut s, "modify", json!({"id": id, "slopes": [1e300, 30, 30, 30]}));
    assert!(e.contains("edge 0"), "{e}");
    let e = fails(&mut s, "modify", json!({"id": id, "slopes": "steep"}));
    assert!(e.contains("invalid type") && e.contains("sequence"), "{e}");
    // An extrusion has no footprint edges to give slopes to.
    let e = fails(
        &mut s,
        "roof",
        json!({"extrusion": {"line": [[0, 0], [4000, 0]], "profile": [[0, 0], [2000, 1000], [4000, 0]], "to": 3000}, "slopes": [30]}),
    );
    assert!(e.contains("slopes") && e.contains("extrusion"), "{e}");
}

#[test]
fn edges_are_numbered_on_through_the_courtyards() {
    let mut s = Session::new();
    let court = json!([[[2000, 2000], [6000, 2000], [6000, 4000], [2000, 4000]]]);
    let e = fails(&mut s, "roof", json!({"boundary": rect(), "courtyards": court, "slope": 30, "slopes": [30, 30, 30, 30]}));
    assert!(e.contains("8 edges"), "{e}");
    // The courtyard's south edge, number 4, at 45°.
    let id = run(&mut s, "roof", json!({"boundary": rect(), "courtyards": court, "slope": 30, "slopes": [0, 0, 0, 0, 45, 0, 0, 0]}))["id"]
        .as_u64()
        .unwrap();
    let g = pitches(&s, id);
    assert_eq!(g.len(), 8);
    assert_eq!(g.iter().filter(|g| near(**g, tan(45.0))).count(), 1, "{g:?}");
    assert_eq!(g.iter().filter(|g| near(**g, tan(30.0))).count(), 7, "{g:?}");
}

#[test]
fn a_roof_with_slopes_is_saved_and_opened_in_the_current_format() {
    let mut s = Session::new();
    let id = run(&mut s, "roof", json!({"boundary": rect(), "slope": 30, "slopes": [30, 45, 30, 45]}))["id"].as_u64().unwrap();
    let mut v: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
    assert_eq!(v["format"], crate::model::FORMAT);
    let guid = s.model.guid(id).to_string();
    let saved = v["elements"].as_object().unwrap().values().find(|e| e["guid"] == json!(guid)).unwrap().clone();
    assert_eq!(saved["slopes"], json!([30.0, 45.0, 30.0, 45.0]));
    // Read as it was written, with nothing left out.
    let (m, left_out) = crate::model::Model::read(&v.to_string()).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(m.elements.get(id), s.model.elements.get(id));
    // A roof of an older file has none, and every edge slopes at the roof's.
    let key = v["elements"].as_object().unwrap().iter().find(|(_, e)| e["guid"] == json!(guid)).map(|(k, _)| k.clone()).unwrap();
    v["elements"][&key].as_object_mut().unwrap().remove("slopes");
    v["format"] = json!(14);
    let (m, left_out) = crate::model::Model::read(&v.to_string()).unwrap();
    assert!(left_out.is_empty() && m.format == crate::model::FORMAT, "{left_out:?}");
    let Some(Element::Roof(r)) = m.elements.get(id) else { panic!() };
    assert!(r.slopes.is_empty());
}

#[test]
fn editing_a_roofs_boundary_keeps_the_slope_of_an_edge_that_stays_or_is_cut_in_two() {
    let mut s = Session::new();
    let id = run(&mut s, "roof", json!({"boundary": rect(), "slope": 30, "slopes": [30, 45, 30, 45]}))["id"].as_u64().unwrap();
    // Edit Boundary hands the lines back with the slope each edge has.
    let now = run(&mut s, "edit_boundary", json!({"id": id}));
    let given: Vec<Option<f64>> = now["sketch"].as_array().unwrap().iter().map(|l| l.get("slope").and_then(Value::as_f64)).collect();
    assert_eq!(given, [Some(30.0), Some(45.0), Some(30.0), Some(45.0)], "{now}");
    // The north edge cut in two lines: both halves keep its slope, and the east and west edges theirs.
    let lines = json!([
        [[0, 0], [8000, 0]],
        [[8000, 0], [8000, 6000]],
        [[8000, 6000], [4000, 6000]],
        [[4000, 6000], [0, 6000]],
        [[0, 6000], [0, 0]]
    ]);
    run(&mut s, "edit_boundary", json!({"id": id, "sketch": lines}));
    assert_eq!(roof_of(&s, id).slopes, [30.0, 45.0, 30.0, 30.0, 45.0]);
    // The south edge moved: a new edge is the roof's, the north edge stays.
    run(&mut s, "undo", json!({}));
    let lines = json!([[[0, -1000], [8000, -1000]], [[8000, -1000], [8000, 6000]], [[8000, 6000], [0, 6000]], [[0, 6000], [0, -1000]]]);
    run(&mut s, "edit_boundary", json!({"id": id, "sketch": lines}));
    assert_eq!(roof_of(&s, id).slopes, [0.0, 0.0, 30.0, 0.0]);
}

#[test]
fn a_sketch_line_gives_its_edges_a_slope_and_none_of_it_is_kept_in_the_roofs_sketch() {
    let mut s = Session::new();
    let line = |a: [i32; 2], b: [i32; 2], slope: Value| json!({"start": a, "end": b, "slope": slope});
    // A new roof drawn as lines: the east and west lines at 45°; a line with no slope is the roof's.
    let lines = json!([
        {"start": [0, 0], "end": [8000, 0]},
        line([8000, 0], [8000, 6000], json!(45)),
        {"start": [8000, 6000], "end": [0, 6000]},
        line([0, 6000], [0, 0], json!(45)),
    ]);
    let id = run(&mut s, "roof", json!({"sketch": lines, "slope": 30}))["id"].as_u64().unwrap();
    assert_eq!(roof_of(&s, id).slopes, [0.0, 45.0, 0.0, 45.0]);
    assert!(roof_of(&s, id).sketch.iter().all(|l| l.slope.is_none()), "the roof's slopes are its own: {:?}", roof_of(&s, id).sketch);
    // Edited: the east line 50°, the west line sent back to the roof's with a slope of 0, which is not "no slope given".
    let lines = json!([
        {"start": [0, 0], "end": [8000, 0]},
        line([8000, 0], [8000, 6000], json!(50)),
        {"start": [8000, 6000], "end": [0, 6000]},
        line([0, 6000], [0, 0], json!(0)),
    ]);
    run(&mut s, "edit_boundary", json!({"id": id, "sketch": lines}));
    assert_eq!(roof_of(&s, id).slopes, [0.0, 50.0, 0.0, 0.0]);
    // Lines that give no slope leave the edges theirs.
    let plain = json!([[[0, 0], [8000, 0]], [[8000, 0], [8000, 6000]], [[8000, 6000], [0, 6000]], [[0, 6000], [0, 0]]]);
    run(&mut s, "edit_boundary", json!({"id": id, "sketch": plain}));
    assert_eq!(roof_of(&s, id).slopes, [0.0, 50.0, 0.0, 0.0]);
    // A slope out of range is refused, saying which edge.
    let e = fails(
        &mut s,
        "edit_boundary",
        json!({"id": id, "sketch": [line([0, 0], [8000, 0], json!(90)), {"start": [8000, 0], "end": [8000, 6000]}, {"start": [8000, 6000], "end": [0, 6000]}, {"start": [0, 6000], "end": [0, 0]}]}),
    );
    assert!(e.contains("edge 0") && e.contains("75"), "{e}");
    // Only a roof has edges to slope: a floor's sketch is refused a line with one.
    let e = fails(&mut s, "floor", json!({"sketch": lines}));
    assert!(e.contains("slope") && e.contains("roof"), "{e}");
}
