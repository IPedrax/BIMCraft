//! What a door or a window can flip, and the `flip` command turning them.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::{Element, Flips, OpeningType, Operation};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn fails(s: &mut Session, id: &str, p: Value) -> String {
    let before = s.model.clone();
    let e = s.execute(id, &p).expect_err(&format!("{id} {p} should fail"));
    assert_eq!(*s.model, *before, "a refused {id} changes nothing");
    e
}

/// A wall from [0, 0] to [8000, 0], its exterior side north.
fn wall(s: &mut Session) -> u64 {
    run(s, "wall", json!({"points": [[0, 0], [8000, 0]]}))["ids"][0].as_u64().unwrap()
}

#[test]
fn opening_flips_says_what_a_swing_door_a_sliding_door_and_a_window_can_turn() {
    let mut s = Session::new();
    let w = wall(&mut s);
    let door = run(&mut s, "door", json!({"wall": w, "offset": 1000}))["id"].as_u64().unwrap();
    let sliding = run(&mut s, "door", json!({"wall": w, "offset": 3000, "family": "Sliding Door"}))["id"].as_u64().unwrap();
    let window = run(&mut s, "window", json!({"wall": w, "offset": 5000}))["id"].as_u64().unwrap();
    let flips = |s: &Session, id: u64| s.model.opening_flips(&s.model.elements[&id]);
    assert_eq!(flips(&s, door), Some(Flips { hand: true, facing: true }));
    assert_eq!(flips(&s, sliding), Some(Flips::default()));
    assert_eq!(flips(&s, window), Some(Flips { hand: false, facing: true }));
    assert_eq!(s.model.opening_flips(&s.model.elements[&w]), None);
    let column = run(&mut s, "column", json!({"at": [4000, 2000]}))["id"].as_u64().unwrap();
    assert_eq!(s.model.opening_flips(&s.model.elements[&column]), None);
}

#[test]
fn a_door_that_names_no_operation_swings_and_a_folding_one_flips_nothing() {
    let mut s = Session::new();
    let w = wall(&mut s);
    let m = s.model_mut();
    m.types.door.insert("Plain 900".into(), OpeningType { width: 900.0, height: 2100.0, sill: 0.0, operation: None });
    m.types.door.insert("Folding 900".into(), OpeningType { width: 900.0, height: 2100.0, sill: 0.0, operation: Some(Operation::Folding) });
    let plain = run(&mut s, "door", json!({"wall": w, "offset": 2000, "type": "Plain 900"}))["id"].as_u64().unwrap();
    let folding = run(&mut s, "door", json!({"wall": w, "offset": 5000, "type": "Folding 900"}))["id"].as_u64().unwrap();
    let flips = |s: &Session, id: u64| s.model.opening_flips(&s.model.elements[&id]);
    assert_eq!(flips(&s, plain), Some(Flips { hand: true, facing: true }));
    assert_eq!(flips(&s, folding), Some(Flips::default()));
    let Element::Door(o) = &s.model.elements[&plain] else { panic!("door") };
    assert_eq!((o.flip_hand, o.flip_facing), (false, false));
}
