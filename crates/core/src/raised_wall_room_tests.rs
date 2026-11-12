//! A high wall's joins must not change the rooms below it. These journeys use the command interface shared by front ends.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::{Element, Id, Model};

fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|e| panic!("{command} {params}: {e}"))
}

fn hall(s: &mut Session, measure: &str) {
    run(
        s,
        "wall",
        json!({"points": [[0, 0], [0, 8000], [20000, 8000], [20000, 0], [10000, 0]],
        "height": 7200, "location": "exterior-finish"}),
    );
    let gable = run(s, "wall", json!({"points": [[10000, 0], [0, 0]], "height": 6000, "location": "exterior-finish"}))["ids"][0].clone();
    let roof = run(
        s,
        "roof",
        json!({"boundary": [[-500, -500], [10500, -500], [10500, 8500], [-500, 8500]],
        "gables": [0, 1, 2], "slope": 10, "offset": 6000, "overhang": 0}),
    )["id"]
        .clone();
    run(s, "modify", json!({"id": gable, "attach_top": roof}));
    run(s, "room_area", json!({"at": measure}));
    run(s, "room", json!({"point": [5000, 4000]}));
}

fn partition(s: &mut Session, base: f64) -> Id {
    run(
        s,
        "wall",
        json!({"points": [[10000, 8000], [10000, 0]], "base_offset": base, "height": 3000,
        "location": "exterior-finish"}),
    )["ids"][0]
        .as_u64()
        .unwrap()
}

fn room(s: &mut Session) -> Value {
    run(s, "elements", json!({"category": "room"}))["elements"][0].clone()
}

fn assert_enclosed(s: &mut Session, expected: f64) {
    let listed = room(s);
    assert_eq!(listed["enclosed"], true, "{listed}");
    assert_eq!(listed["area_m2"], expected, "{listed}");
    // A new model read has no incremental lookup. It must give the same boundary as the live session.
    let (fresh, left_out) = Model::read(&s.model.to_json().unwrap()).unwrap();
    assert!(left_out.is_empty());
    let r = fresh.elements.values().find_map(|e| if let Element::Room(r) = e { Some(r) } else { None }).unwrap();
    let region = fresh.room_region(r).unwrap();
    assert!((region.area() / 1_000_000.0 - expected).abs() < 0.006);
}

fn raised_wall_edits(measure: &str, area: f64) {
    let mut s = Session::new();
    hall(&mut s, measure);
    assert_enclosed(&mut s, area);
    let raised = partition(&mut s, 7200.0);
    assert_enclosed(&mut s, area);
    run(&mut s, "move", json!({"ids": [raised], "by": [2000, 0]}));
    assert_enclosed(&mut s, area);
    run(&mut s, "move", json!({"ids": [raised], "by": [-2000, 0]}));
    assert_enclosed(&mut s, area);
    run(&mut s, "delete", json!({"ids": [raised]}));
    assert_enclosed(&mut s, area);
    run(&mut s, "undo", json!({}));
    assert_enclosed(&mut s, area);
    run(&mut s, "redo", json!({}));
    assert_enclosed(&mut s, area);
}

#[test]
fn raised_wall_edits_preserve_the_room_below_at_centre_lines() {
    raised_wall_edits("centre", 154.44);
}

#[test]
fn raised_wall_edits_preserve_the_room_below_at_finish_faces() {
    raised_wall_edits("finish", 148.96);
}

#[test]
fn a_wall_reaching_the_computation_height_still_divides_the_hall() {
    let mut s = Session::new();
    hall(&mut s, "centre");
    let wall = partition(&mut s, 0.0);
    assert_enclosed(&mut s, 76.44);
    run(&mut s, "modify", json!({"id": wall, "base_offset": 7200}));
    assert_enclosed(&mut s, 154.44);
    run(&mut s, "modify", json!({"id": wall, "base_offset": 0}));
    assert_enclosed(&mut s, 76.44);
    run(&mut s, "modify", json!({"id": wall, "room_bounding": false}));
    assert_enclosed(&mut s, 154.44);
}

#[test]
fn joins_follow_the_levels_computation_height_when_it_changes() {
    let mut s = Session::new();
    hall(&mut s, "centre");
    let wall = partition(&mut s, 0.0);
    run(&mut s, "modify", json!({"id": wall, "height": 1000}));
    assert_enclosed(&mut s, 154.44);
    let level = s.model.levels()[0].0;
    run(&mut s, "modify", json!({"id": level, "computation_height": 500}));
    assert_enclosed(&mut s, 76.44);
    run(&mut s, "modify", json!({"id": level, "computation_height": 1200}));
    assert_enclosed(&mut s, 154.44);
}
