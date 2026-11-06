//! `cut_fill`: what pads and proposed ground take out of the terrain and put in; and `place_on_terrain`: things stood on the ground.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::Element;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn refused(s: &mut Session, id: &str, p: Value) -> String {
    s.execute(id, &p).expect_err(&format!("{id} {p} should be refused"))
}

fn rect(x0: i32, y0: i32, x1: i32, y1: i32) -> Value {
    json!([[x0, y0], [x1, y0], [x1, y1], [x0, y1]])
}

/// Terrain over `n` x `n` points `step` mm apart, the height `f(x, y)` of each.
fn terrain(s: &mut Session, n: usize, step: f64, f: impl Fn(f64, f64) -> f64) -> u64 {
    let mut points: Vec<Value> = Vec::new();
    for i in 0..n {
        for j in 0..n {
            let (x, y) = (i as f64 * step, j as f64 * step);
            points.push(json!([x, y, f(x, y)]));
        }
    }
    run(s, "toposurface", json!({"points": points, "interval": 0}))["id"].as_u64().unwrap()
}

fn near(a: &Value, b: f64) -> bool {
    a.as_f64().is_some_and(|a| (a - b).abs() < 1e-6)
}

#[test]
fn the_pads_of_a_project_are_totalled_against_the_ground() {
    let mut s = Session::new();
    terrain(&mut s, 3, 5000.0, |_, _| 1000.0);
    // 16 m2 half a metre down: 8 m3 cut. 4 m2 half a metre up: 2 m3 fill.
    let a = run(&mut s, "pad", json!({"boundary": rect(3000, 3000, 7000, 7000), "offset": 500, "name": "House"}))["id"].as_u64().unwrap();
    let b = run(&mut s, "pad", json!({"boundary": rect(0, 8000, 2000, 10000), "offset": 1500, "name": "Shed"}))["id"].as_u64().unwrap();
    let r = run(&mut s, "cut_fill", json!({}));
    assert_eq!(r["method"], "pads");
    let t = &r["totals"];
    assert!(near(&t["cut_m3"], 8.0) && near(&t["fill_m3"], 2.0) && near(&t["net_m3"], -6.0), "{t}");
    let pads = r["pads"].as_array().unwrap();
    assert_eq!(pads.len(), 2);
    let of = |id: u64| pads.iter().find(|p| p["id"] == id).unwrap();
    assert!(near(&of(a)["cut_m3"], 8.0) && near(&of(a)["fill_m3"], 0.0) && of(a)["name"] == "House");
    assert!(near(&of(b)["fill_m3"], 2.0) && near(&of(b)["covered_m2"], 4.0));
    // A pad asked for by itself, and what the listing says of it.
    let r = run(&mut s, "cut_fill", json!({"pads": [b]}));
    assert!(near(&r["totals"]["fill_m3"], 2.0) && r["pads"].as_array().unwrap().len() == 1);
    // More than one ground: the one named.
    let high = terrain(&mut s, 3, 5000.0, |_, _| 2000.0);
    let r = run(&mut s, "cut_fill", json!({"pads": [a], "terrain": high}));
    assert!(near(&r["totals"]["cut_m3"], 16.0 * 1.5), "{r}");
}

#[test]
fn what_cannot_be_totalled_is_refused() {
    let mut s = Session::new();
    assert!(refused(&mut s, "cut_fill", json!({})).contains("pad"));
    run(&mut s, "pad", json!({"boundary": rect(0, 0, 4000, 4000)}));
    assert!(refused(&mut s, "cut_fill", json!({})).contains("terrain"));
    terrain(&mut s, 3, 5000.0, |_, _| 1000.0);
    for (p, says) in [
        (json!({"pads": [999_999]}), "pad"),
        (json!({"pads": "all"}), "pads"),
        (json!({"terrain": 100000}), "toposurface"),
        (json!({"existing": 100000, "proposed": 100000}), "toposurface"),
        (json!({"existing": 100002}), "proposed"),
        (json!({"pads": [100002], "existing": 100003, "proposed": 100003}), "not both"),
        (json!({"cell": -1}), "cell"),
        (json!({"colour": 1}), "colour"),
    ] {
        assert!(refused(&mut s, "cut_fill", p.clone()).contains(says), "{p}");
    }
}

#[test]
fn two_surfaces_are_compared_where_both_are() {
    let mut s = Session::new();
    let existing = terrain(&mut s, 11, 1000.0, |x, _| x / 10.0);
    let proposed = terrain(&mut s, 3, 5000.0, |_, _| 500.0);
    // The ground rises 0 to 1 m over 10 m; proposed is flat at 0.5 m: half cut, half fill, 12.5 m3 each.
    let r = run(&mut s, "cut_fill", json!({"existing": existing, "proposed": proposed, "cell": 100}));
    assert_eq!(r["method"], "grid");
    let (cut, fill) = (r["cut_m3"].as_f64().unwrap(), r["fill_m3"].as_f64().unwrap());
    assert!((cut - 12.5).abs() < 0.125 && (fill - 12.5).abs() < 0.125, "{r}");
    assert!(near(&r["area_m2"], 100.0) && r["cells"] == 10_000, "{r}");
    // Its own cell when none is said, and a cell too fine refused.
    let r = run(&mut s, "cut_fill", json!({"existing": existing, "proposed": proposed}));
    assert!((r["cut_m3"].as_f64().unwrap() - 12.5).abs() < 0.5, "{r}");
    assert!(refused(&mut s, "cut_fill", json!({"existing": existing, "proposed": proposed, "cell": 1})).contains("cell"));
}

fn mass_base(s: &Session, id: u64) -> f64 {
    match s.model.elements.get(id) {
        Some(Element::Mass(m)) => m.base_offset,
        _ => panic!("{id} is not a mass"),
    }
}

#[test]
fn things_are_stood_on_the_ground_at_the_lowest_under_them() {
    let mut s = Session::new();
    // Ground z = x / 10: 0 to 1 m over 10 m.
    terrain(&mut s, 11, 1000.0, |x, _| x / 10.0);
    let mass = run(&mut s, "mass", json!({"boundary": rect(6000, 2000, 8000, 4000), "height": 3000}))["id"].as_u64().unwrap();
    let column = run(&mut s, "column", json!({"at": [5000, 5000]}))["id"].as_u64().unwrap();
    let table = run(&mut s, "component", json!({"family": "Table", "type": "1600 x 900", "at": [4000, 5000]}))["id"].as_u64().unwrap();
    let wall = run(&mut s, "wall", json!({"points": [[2000, 8000], [8000, 8000]]}))["ids"][0].as_u64().unwrap();
    let r = run(&mut s, "place_on_terrain", json!({"ids": [mass, column, table, wall]}));
    assert_eq!(r["moved"].as_array().unwrap().len(), 4, "{r}");
    assert!((mass_base(&s, mass) - 600.0).abs() < 1.0, "the lowest ground under its west side: {}", mass_base(&s, mass));
    let c = run(&mut s, "elements", json!({"category": "column"}));
    assert!(c["elements"][0]["base_offset"].as_f64().is_some_and(|v| (v - 500.0).abs() < 150.0), "{c}");
    let _ = column;
    let t = run(&mut s, "elements", json!({"category": "component"}));
    assert!(t["elements"][0]["offset"].as_f64().is_some_and(|v| (v - 400.0).abs() < 1.0), "{t}");
    let w = run(&mut s, "elements", json!({"category": "wall"}));
    assert!(w["elements"][0]["base_offset"].as_f64().is_some_and(|v| (v - 200.0).abs() < 1.0), "its west end: {w}");
    // One undo step puts them all back.
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(mass_base(&s, mass), 0.0);
}

#[test]
fn an_embedment_goes_below_the_ground_and_what_is_off_the_ground_is_left() {
    let mut s = Session::new();
    terrain(&mut s, 11, 1000.0, |x, _| x / 10.0);
    let near = run(&mut s, "mass", json!({"boundary": rect(6000, 2000, 8000, 4000), "height": 3000}))["id"].as_u64().unwrap();
    let off = run(&mut s, "mass", json!({"boundary": rect(50000, 2000, 52000, 4000), "height": 3000}))["id"].as_u64().unwrap();
    let r = run(&mut s, "place_on_terrain", json!({"ids": [near, off], "offset": -300}));
    assert!((mass_base(&s, near) - 300.0).abs() < 1.0, "300 below the ground: {}", mass_base(&s, near));
    assert_eq!(mass_base(&s, off), 0.0);
    assert_eq!((r["moved"].as_array().unwrap().len(), r["skipped"].as_array().unwrap().len()), (1, 1), "{r}");
    assert!(r["skipped"][0]["why"].as_str().unwrap().contains("terrain"));
    // Only what is off it: nothing to do, which is an error.
    assert!(refused(&mut s, "place_on_terrain", json!({"ids": [off]})).contains("terrain"));
}

#[test]
fn what_cannot_be_stood_on_the_ground_is_refused() {
    let mut s = Session::new();
    assert!(refused(&mut s, "place_on_terrain", json!({"ids": [100000]})).contains("terrain"), "no ground yet");
    terrain(&mut s, 3, 5000.0, |_, _| 1000.0);
    let pad = run(&mut s, "pad", json!({"boundary": rect(0, 0, 4000, 4000)}))["id"].as_u64().unwrap();
    for (p, says) in [
        (json!({}), "ids"),
        (json!({"ids": []}), "ids"),
        (json!({"ids": "all"}), "ids"),
        (json!({"ids": [pad]}), "can't be stood"),
        (json!({"ids": [999_999]}), "999999"),
        (json!({"ids": [pad], "offset": "x"}), "offset"),
        (json!({"ids": [pad], "terrain": 100000}), "toposurface"),
        (json!({"ids": [pad], "colour": 1}), "colour"),
    ] {
        assert!(refused(&mut s, "place_on_terrain", p.clone()).contains(says), "{p}");
    }
}
