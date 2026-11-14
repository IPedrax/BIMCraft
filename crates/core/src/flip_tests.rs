//! What a door or a window can flip, and the `flip` command turning them.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom::P2;
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

fn flips_of(s: &Session, id: u64) -> (bool, bool) {
    match &s.model.elements[&id] {
        Element::Door(o) | Element::Window(o) => (o.flip_hand, o.flip_facing),
        _ => panic!("no opening {id}"),
    }
}

fn ends(s: &Session, id: u64) -> (P2, P2) {
    let Some(Element::Wall(w)) = s.model.elements.get(id) else { panic!("no wall {id}") };
    (w.start, w.end)
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

#[test]
fn flip_cycles_a_swing_door_through_four_states_one_undo_each() {
    let mut s = Session::new();
    let w = wall(&mut s);
    let door = run(&mut s, "door", json!({"wall": w, "offset": 2000}))["id"].as_u64().unwrap();
    for want in [(true, false), (false, true), (true, true), (false, false)] {
        let steps = s.history().len();
        let r = run(&mut s, "flip", json!({"ids": [door], "cycle": "auto"}));
        assert_eq!(flips_of(&s, door), want);
        assert_eq!(r["states"][door.to_string()], json!({"hand": want.0, "facing": want.1}));
        assert_eq!(s.history().len(), steps + 1, "one press is one Undo step");
        run(&mut s, "undo", json!({}));
        assert_ne!(flips_of(&s, door), want, "one undo takes back just this press");
        run(&mut s, "redo", json!({}));
        assert_eq!(flips_of(&s, door), want);
    }
    // The hand alone, as Revit turns a door while placing it.
    run(&mut s, "flip", json!({"ids": [door], "cycle": "hand"}));
    assert_eq!(flips_of(&s, door), (true, false));
    run(&mut s, "flip", json!({"ids": [door], "cycle": "hand"}));
    assert_eq!(flips_of(&s, door), (false, true));
}

#[test]
fn flip_turns_a_window_and_refuses_a_flag_it_does_not_have() {
    let mut s = Session::new();
    let w = wall(&mut s);
    let window = run(&mut s, "window", json!({"wall": w, "offset": 2000}))["id"].as_u64().unwrap();
    run(&mut s, "flip", json!({"ids": [window], "facing": true}));
    assert_eq!(flips_of(&s, window), (false, true));
    run(&mut s, "flip", json!({"ids": [window], "cycle": "auto"}));
    assert_eq!(flips_of(&s, window), (false, false));
    let e = fails(&mut s, "flip", json!({"ids": [window], "hand": true}));
    assert!(e.contains("can't flip `hand`") && e.contains("`facing`"), "{e}");
    let e = fails(&mut s, "flip", json!({"ids": [window]}));
    assert!(e.contains("give `hand`, `facing` or `cycle`"), "{e}");
    // A sliding door flips nothing: `auto` leaves it as it is, and asking for its hand is refused.
    let sliding = run(&mut s, "door", json!({"wall": w, "offset": 5000, "family": "Sliding Door"}))["id"].as_u64().unwrap();
    run(&mut s, "flip", json!({"ids": [sliding], "cycle": "auto"}));
    assert_eq!(flips_of(&s, sliding), (false, false));
    let e = fails(&mut s, "flip", json!({"ids": [sliding], "cycle": "hand"}));
    assert!(e.contains("can't flip `hand`") && e.contains("nothing"), "{e}");
    let column = run(&mut s, "column", json!({"at": [4000, 2000]}))["id"].as_u64().unwrap();
    let e = fails(&mut s, "flip", json!({"ids": [column]}));
    assert!(e.contains("isn't a wall, a door or a window"), "{e}");
}

#[test]
fn flip_turns_a_wall_and_a_door_in_one_step_and_carries_the_doors_it_hosts() {
    let mut s = Session::new();
    let w = wall(&mut s);
    let door = run(&mut s, "door", json!({"wall": w, "offset": 2000, "flip_hand": true}))["id"].as_u64().unwrap();
    let carried = run(&mut s, "door", json!({"wall": w, "offset": 6000}))["id"].as_u64().unwrap();
    let steps = s.history().len();
    run(&mut s, "flip", json!({"ids": [w, door], "cycle": "auto"}));
    assert_eq!(ends(&s, w), (P2(8000.0, 0.0), P2(0.0, 0.0)));
    // The listed door turns once, and the wall moves it without turning it; the other follows the wall round.
    assert_eq!(flips_of(&s, door), (false, true));
    assert_eq!(flips_of(&s, carried), (true, true));
    let offset = |s: &Session, id: u64| match &s.model.elements[&id] {
        Element::Door(o) | Element::Window(o) => o.offset,
        _ => panic!("no opening {id}"),
    };
    assert_eq!([offset(&s, door), offset(&s, carried)], [6000.0, 2000.0]);
    assert_eq!(s.history().len(), steps + 1, "walls and openings turn in one Undo step");
    run(&mut s, "undo", json!({}));
    assert_eq!(ends(&s, w), (P2(0.0, 0.0), P2(8000.0, 0.0)));
    assert_eq!(flips_of(&s, door), (true, false));
    assert_eq!(flips_of(&s, carried), (false, false));
}

#[test]
fn flip_refuses_a_pinned_opening_and_ignores_opening_flags_on_walls() {
    let mut s = Session::new();
    let w = wall(&mut s);
    let door = run(&mut s, "door", json!({"wall": w, "offset": 2000}))["id"].as_u64().unwrap();
    run(&mut s, "pin", json!({"ids": [door]}));
    let e = fails(&mut s, "flip", json!({"ids": [door], "cycle": "auto"}));
    assert!(e.contains("pinned"), "{e}");
    run(&mut s, "unpin", json!({"ids": [door]}));
    // Walls ignore the openings' flags: one call turns the wall and leaves the doors to their own rules.
    run(&mut s, "flip", json!({"ids": [w], "hand": true, "facing": true}));
    assert_eq!(ends(&s, w), (P2(8000.0, 0.0), P2(0.0, 0.0)));
    assert_eq!(flips_of(&s, door), (true, true), "the wall carries its door round");
    let e = fails(&mut s, "flip", json!({"ids": [door], "cycle": "hand", "hand": true}));
    assert!(e.contains("neither `hand` nor `facing`"), "{e}");
}
