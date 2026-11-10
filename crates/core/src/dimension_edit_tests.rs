//! Typing values into dimensions: which reference stays, resizing one wall, equal chains, editing a dimension's
//! references, and the text that beats an equal chain's numbers.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::{DimKind, Element, Id, Model};

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

fn err(s: &mut Session, c: &str, p: Value) -> String {
    s.execute(c, &p).err().unwrap_or_else(|| panic!("{c} {p} was accepted"))
}

fn wall(s: &mut Session, points: Value) -> Id {
    run(s, "wall", json!({"points": points}))["ids"][0].as_u64().unwrap_or_default()
}

fn dimension(s: &mut Session, refs: Value, at: Value) -> Id {
    run(s, "dimension", json!({"refs": refs, "at": at}))["id"].as_u64().unwrap_or_default()
}

fn dim_of(s: &Session, id: Id) -> crate::model::Dimension {
    match s.model.elements.get(id) {
        Some(Element::Dimension(d)) => d.clone(),
        other => panic!("{id} is {other:?}"),
    }
}

fn values(s: &Session, id: Id) -> Vec<f64> {
    s.model.dimension_values(&dim_of(s, id))
}

fn wall_start(s: &Session, id: Id) -> f64 {
    s.model.wall(id).map_or(f64::NAN, |w| w.start.0)
}

fn wall_end(s: &Session, id: Id) -> f64 {
    s.model.wall(id).map_or(f64::NAN, |w| w.end.0)
}

/// Four walls running north, apart: their centre lines at the `xs`, and a dimension across them below.
fn chain(s: &mut Session, xs: &[f64]) -> (Vec<Id>, Id) {
    let walls: Vec<Id> = xs.iter().map(|&x| wall(s, json!([[x, 0], [x, 4000]]))).collect();
    let refs: Vec<Value> = walls.iter().map(|&w| json!({"id": w})).collect();
    let d = dimension(s, json!(refs), json!([(xs[0] + xs[xs.len() - 1]) / 2.0, -1000]));
    (walls, d)
}

#[test]
fn dimension_value_moves_the_first_reference_with_hold_end() {
    let mut s = Session::new();
    let a = wall(&mut s, json!([[0, 0], [0, 4000]]));
    let b = wall(&mut s, json!([[2000, 0], [2000, 4000]]));
    let d = dimension(&mut s, json!([{"id": a}, {"id": b}]), json!([1000, -1000]));
    let out = run(&mut s, "dimension_value", json!({"id": d, "value": 3000, "hold": "end"}));
    assert_eq!((wall_start(&s, a), wall_start(&s, b)), (-1000.0, 2000.0), "the first wall moved");
    assert_eq!(out["moved"], json!(a));
    assert_eq!(out["values"], json!([3000.0]));
    assert_eq!(values(&s, d), vec![3000.0]);
}

#[test]
fn dimension_value_moves_the_reference_named_by_hold() {
    let mut s = Session::new();
    let a = wall(&mut s, json!([[0, 0], [0, 4000]]));
    let b = wall(&mut s, json!([[2000, 0], [2000, 4000]]));
    let d = dimension(&mut s, json!([{"id": a}, {"id": b}]), json!([1000, -1000]));
    let out = run(&mut s, "dimension_value", json!({"id": d, "value": 3000, "hold": {"id": a}}));
    assert_eq!((wall_start(&s, a), wall_start(&s, b)), (0.0, 3000.0), "the second wall moved");
    assert_eq!(out["moved"], json!(b));
    assert_eq!(values(&s, d), vec![3000.0]);
}

#[test]
fn dimension_value_refuses_a_hold_that_isnt_an_end_of_the_segment() {
    let mut s = Session::new();
    let a = wall(&mut s, json!([[0, 0], [0, 4000]]));
    let b = wall(&mut s, json!([[2000, 0], [2000, 4000]]));
    let other = wall(&mut s, json!([[5000, 0], [5000, 4000]]));
    let d = dimension(&mut s, json!([{"id": a}, {"id": b}]), json!([1000, -1000]));
    let e = err(&mut s, "dimension_value", json!({"id": d, "value": 3000, "hold": {"id": other}}));
    assert!(e.contains("hold"), "{e}");
    assert_eq!((wall_start(&s, a), wall_start(&s, b), wall_start(&s, other)), (0.0, 2000.0, 5000.0), "as it was");
}

#[test]
fn dimension_value_resizes_a_free_wall_symmetrically() {
    let mut s = Session::new();
    let w = wall(&mut s, json!([[0, 0], [4000, 0]]));
    let d = dimension(&mut s, json!([{"id": w, "point": "start"}, {"id": w, "point": "end"}]), json!([2000, -1000]));
    let out = run(&mut s, "dimension_value", json!({"id": d, "value": 6000}));
    assert_eq!((wall_start(&s, w), wall_end(&s, w)), (-1000.0, 5000.0), "each end moved 1000");
    assert_eq!(out["values"], json!([6000.0]));
}

#[test]
fn dimension_value_resizes_a_wall_joined_at_one_end() {
    let mut s = Session::new();
    let w = wall(&mut s, json!([[0, 0], [4000, 0]]));
    let _other = wall(&mut s, json!([[0, 0], [0, 3000]]));
    let d = dimension(&mut s, json!([{"id": w, "point": "start"}, {"id": w, "point": "end"}]), json!([2000, -1000]));
    run(&mut s, "dimension_value", json!({"id": d, "value": 6000}));
    assert_eq!((wall_start(&s, w), wall_end(&s, w)), (0.0, 6000.0), "the free end moved alone");
}

#[test]
fn dimension_value_resizes_a_wall_joined_at_both_ends() {
    let mut s = Session::new();
    let w = wall(&mut s, json!([[0, 0], [4000, 0]]));
    wall(&mut s, json!([[0, 0], [0, 3000]]));
    wall(&mut s, json!([[4000, 0], [4000, 3000]]));
    let d = dimension(&mut s, json!([{"id": w, "point": "start"}, {"id": w, "point": "end"}]), json!([2000, -1000]));
    run(&mut s, "dimension_value", json!({"id": d, "value": 6000}));
    assert_eq!((wall_start(&s, w), wall_end(&s, w)), (-1000.0, 5000.0), "both ends moved half");
    assert_eq!(values(&s, d), vec![6000.0]);
}

#[test]
fn dimension_value_refuses_the_two_faces_of_one_wall() {
    let mut s = Session::new();
    let w = wall(&mut s, json!([[0, 0], [4000, 0]]));
    let d = dimension(&mut s, json!([{"id": w, "face": "left"}, {"id": w, "face": "right"}]), json!([2000, -1000]));
    let before = s.model.wall(w).cloned();
    let e = err(&mut s, "dimension_value", json!({"id": d, "value": 6000}));
    assert!(e.contains("thickness") && e.contains("Wall Type"), "{e}");
    assert_eq!(s.model.wall(w).cloned(), before, "the wall is as it was");
}

#[test]
fn dimension_value_refuses_a_door_width() {
    let mut s = Session::new();
    wall(&mut s, json!([[0, 0], [6000, 0]]));
    let door = run(&mut s, "door", json!({"at": [3000, 0]}))["id"].as_u64().unwrap_or_default();
    let d = dimension(&mut s, json!([{"id": door, "face": "start"}, {"id": door, "face": "end"}]), json!([3000, -1000]));
    let e = err(&mut s, "dimension_value", json!({"id": d, "value": 1200}));
    assert!(e.contains("type"), "{e}");
}

#[test]
fn pressing_equal_keeps_the_ends_and_spreads_inner_references_evenly() {
    let mut s = Session::new();
    let (w, d) = chain(&mut s, &[0.0, 2000.0, 5000.0]);
    run(&mut s, "dimension_constraint", json!({"id": d, "equal": true}));
    assert_eq!((wall_start(&s, w[0]), wall_start(&s, w[1]), wall_start(&s, w[2])), (0.0, 2500.0, 5000.0));
    assert_eq!(values(&s, d), vec![2500.0, 2500.0]);
    // Already even: nothing moves.
    run(&mut s, "dimension_constraint", json!({"id": d, "equal": true}));
    assert_eq!(wall_start(&s, w[1]), 2500.0);
}

#[test]
fn dimension_value_with_equal_sets_every_segment_and_anchors_the_first_end() {
    let mut s = Session::new();
    let (w, d) = chain(&mut s, &[0.0, 2000.0, 5000.0, 9000.0]);
    run(&mut s, "dimension_constraint", json!({"id": d, "equal": true}));
    let out = run(&mut s, "dimension_value", json!({"id": d, "value": 4000, "hold": "start"}));
    assert_eq!(values(&s, d), vec![4000.0, 4000.0, 4000.0]);
    assert_eq!((wall_start(&s, w[0]), wall_start(&s, w[3])), (0.0, 12000.0), "the first end stayed, the last moved");
    assert_eq!(out["by"], json!(3000.0));
}

#[test]
fn dimension_value_with_equal_anchors_the_last_end() {
    let mut s = Session::new();
    let (w, d) = chain(&mut s, &[0.0, 2000.0, 5000.0, 9000.0]);
    run(&mut s, "dimension_constraint", json!({"id": d, "equal": true}));
    run(&mut s, "dimension_value", json!({"id": d, "value": 4000, "hold": "end"}));
    assert_eq!(values(&s, d), vec![4000.0, 4000.0, 4000.0]);
    assert_eq!((wall_start(&s, w[0]), wall_start(&s, w[3])), (-3000.0, 9000.0), "the last end stayed, the first moved");
}

#[test]
fn dimension_value_with_equal_both_ends() {
    let mut s = Session::new();
    let (w, d) = chain(&mut s, &[0.0, 2000.0, 5000.0]);
    run(&mut s, "dimension_constraint", json!({"id": d, "equal": true}));
    run(&mut s, "dimension_value", json!({"id": d, "value": 2500, "hold": "both"}));
    assert_eq!((wall_start(&s, w[0]), wall_start(&s, w[1]), wall_start(&s, w[2])), (0.0, 2500.0, 5000.0));
    assert_eq!(values(&s, d), vec![2500.0, 2500.0]);
    // A value that would move both ends is refused, and the model is as it was.
    let e = err(&mut s, "dimension_value", json!({"id": d, "value": 3000, "hold": "both"}));
    assert!(e.contains("both ends"), "{e}");
    assert_eq!(values(&s, d), vec![2500.0, 2500.0]);
}

#[test]
fn dimension_value_with_equal_and_locks_updates_every_lock() {
    let mut s = Session::new();
    let (w, d) = chain(&mut s, &[0.0, 2000.0, 5000.0]);
    run(&mut s, "dimension_constraint", json!({"id": d, "equal": true}));
    run(&mut s, "dimension_constraint", json!({"id": d, "locked": [0, 1]}));
    run(&mut s, "dimension_value", json!({"id": d, "value": 4000, "hold": "start"}));
    assert_eq!(values(&s, d), vec![4000.0, 4000.0]);
    assert!(dim_of(&s, d).locked.iter().all(|l| (l.length - 4000.0).abs() < 0.01));
    assert_eq!((wall_start(&s, w[0]), wall_start(&s, w[2])), (0.0, 8000.0));
}

#[test]
fn dimension_value_with_equal_refuses_an_inner_hold() {
    let mut s = Session::new();
    let (_, d) = chain(&mut s, &[0.0, 2000.0, 5000.0]);
    run(&mut s, "dimension_constraint", json!({"id": d, "equal": true}));
    let inner = dim_of(&s, d).refs[1];
    let e = err(&mut s, "dimension_value", json!({"id": d, "value": 4000, "hold": inner}));
    assert!(e.contains("ends"), "{e}");
}

#[test]
fn editing_refs_remaps_locks_and_texts() {
    let mut s = Session::new();
    let (w, d) = chain(&mut s, &[0.0, 2000.0, 5000.0, 9000.0]);
    run(&mut s, "dimension_constraint", json!({"id": d, "locked": [1]}));
    run(&mut s, "dimension_text", json!({"id": d, "segment": 2, "replace": "A"}));
    // Drop the first reference: the old segment 1 (walls 2 and 3) is the new segment 0, and the old text's (3 and 4), 1.
    let out = run(&mut s, "dimension", json!({"id": d, "refs": [{"id": w[1]}, {"id": w[2]}, {"id": w[3]}]}));
    assert_eq!(out["values"], json!([3000.0, 4000.0]));
    let edited = dim_of(&s, d);
    assert_eq!(edited.locked.iter().map(|l| l.segment).collect::<Vec<_>>(), vec![0]);
    assert_eq!(edited.texts.iter().map(|t| t.segment).collect::<Vec<_>>(), vec![1]);
    assert_eq!(edited.refs.len(), 3);
}

#[test]
fn editing_refs_drops_locks_of_a_removed_pair() {
    let mut s = Session::new();
    let (w, d) = chain(&mut s, &[0.0, 2000.0, 5000.0, 9000.0]);
    run(&mut s, "dimension_constraint", json!({"id": d, "locked": [0, 1]}));
    run(&mut s, "dimension", json!({"id": d, "refs": [{"id": w[1]}, {"id": w[2]}, {"id": w[3]}]}));
    let edited = dim_of(&s, d);
    assert_eq!(edited.locked.iter().map(|l| l.segment).collect::<Vec<_>>(), vec![0], "the removed pair's lock is gone");
}

#[test]
fn editing_refs_refuses_an_alignment_lock() {
    let mut s = Session::new();
    let a = wall(&mut s, json!([[0, 0], [0, 4000]]));
    let b = wall(&mut s, json!([[500, 6000], [500, 9000]]));
    let out = run(&mut s, "align", json!({"id": b, "reference": [[0, 0], [0, 4000]], "line": [[500, 6000], [500, 9000]], "lock": true}));
    let lock = out["lock"].as_u64().unwrap_or_default();
    let e = err(&mut s, "dimension", json!({"id": lock, "refs": [{"id": a}, {"id": b}]}));
    assert!(e.contains("alignment"), "{e}");
    assert!(matches!(s.model.elements.get(lock), Some(Element::Dimension(d)) if d.kind == DimKind::Aligned));
}

#[test]
fn editing_refs_refuses_fewer_than_two_refs_and_duplicates() {
    let mut s = Session::new();
    let (w, d) = chain(&mut s, &[0.0, 2000.0, 5000.0]);
    let e = err(&mut s, "dimension", json!({"id": d, "refs": [{"id": w[0]}]}));
    assert!(e.contains("two"), "{e}");
    let e = err(&mut s, "dimension", json!({"id": d, "refs": [{"id": w[0]}, {"id": w[0]}]}));
    assert!(e.contains("duplicate"), "{e}");
    assert_eq!(dim_of(&s, d).refs.len(), 3, "as it was");
}

#[test]
fn editing_refs_keeps_equal_and_its_segments() {
    let mut s = Session::new();
    let (w, d) = chain(&mut s, &[0.0, 2000.0, 5000.0]);
    run(&mut s, "dimension_constraint", json!({"id": d, "equal": true}));
    let e = err(&mut s, "dimension", json!({"id": d, "refs": [{"id": w[0]}, {"id": w[1]}]}));
    assert!(e.contains("two segments"), "{e}");
    run(&mut s, "dimension", json!({"id": d, "refs": [{"id": w[0]}, {"id": w[1]}, {"id": w[2]}]}));
    assert!(dim_of(&s, d).equal);
    assert_eq!(values(&s, d), vec![2500.0, 2500.0]);
}

#[test]
fn a_segment_text_override_replaces_the_number() {
    let mut s = Session::new();
    let (_, d) = chain(&mut s, &[0.0, 2000.0, 5000.0]);
    run(&mut s, "dimension_constraint", json!({"id": d, "equal": true}));
    let labels = crate::commands::element_json(&s.model, d)["labels"].clone();
    assert_eq!(labels, json!(["2500", "2500"]), "an equal chain still shows its lengths");
    let out = run(&mut s, "dimension_text", json!({"id": d, "segment": 0, "replace": "X"}));
    assert_eq!(out["labels"], json!(["X", "2500"]), "the replacement beats the number");
    // Clearing it brings back the true number.
    let out = run(&mut s, "dimension_text", json!({"id": d, "segment": 0, "replace": ""}));
    assert_eq!(out["labels"], json!(["2500", "2500"]));
    run(&mut s, "dimension_constraint", json!({"id": d, "equal": false}));
    let labels = crate::commands::element_json(&s.model, d)["labels"].clone();
    assert_eq!(labels, json!(["2500", "2500"]));
}

/// The model's own check finds nothing wrong after every command these tests ran.
#[test]
fn every_edit_leaves_a_model_that_checks() {
    let mut s = Session::new();
    let _: &Model = &s.model;
    let (_, d) = chain(&mut s, &[0.0, 2000.0, 5000.0, 9000.0]);
    run(&mut s, "dimension_constraint", json!({"id": d, "equal": true}));
    run(&mut s, "dimension_value", json!({"id": d, "value": 3000, "hold": "start"}));
    run(&mut s, "dimension_text", json!({"id": d, "segment": 1, "prefix": "~"}));
    assert_eq!(s.model.check(), Ok(()));
}

#[test]
fn wall_length_resizes_a_free_wall_evenly() {
    let mut s = Session::new();
    let w = wall(&mut s, json!([[0, 0], [4000, 0]]));
    let out = run(&mut s, "wall_length", json!({"id": w, "value": 6000}));
    assert_eq!((wall_start(&s, w), wall_end(&s, w)), (-1000.0, 5000.0));
    assert_eq!(out["length"], json!(6000.0));
}

#[test]
fn wall_length_moves_only_the_free_end_when_one_is_joined() {
    let mut s = Session::new();
    let w = wall(&mut s, json!([[0, 0], [4000, 0]]));
    wall(&mut s, json!([[0, 0], [0, 3000]]));
    run(&mut s, "wall_length", json!({"id": w, "value": 6000}));
    assert_eq!((wall_start(&s, w), wall_end(&s, w)), (0.0, 6000.0));
}

#[test]
fn wall_length_moves_both_joined_ends_evenly() {
    let mut s = Session::new();
    let w = wall(&mut s, json!([[0, 0], [4000, 0]]));
    wall(&mut s, json!([[0, 0], [0, 3000]]));
    wall(&mut s, json!([[4000, 0], [4000, 3000]]));
    run(&mut s, "wall_length", json!({"id": w, "value": 6000}));
    assert_eq!((wall_start(&s, w), wall_end(&s, w)), (-1000.0, 5000.0));
}

#[test]
fn deleting_a_segment_merges_the_references_either_side_of_it() {
    let mut s = Session::new();
    let (_, d) = chain(&mut s, &[0.0, 2000.0, 5000.0, 9000.0]);
    let out = run(&mut s, "dimension_segment", json!({"id": d, "segment": 1}));
    assert_eq!(out["values"], json!([2000.0, 7000.0]));
    assert_eq!(dim_of(&s, d).refs.len(), 3);
}

#[test]
fn deleting_the_last_segment_of_a_chain_takes_its_first_reference() {
    let mut s = Session::new();
    let (_, d) = chain(&mut s, &[0.0, 2000.0, 5000.0, 9000.0]);
    let out = run(&mut s, "dimension_segment", json!({"id": d, "segment": 2}));
    assert_eq!(out["values"], json!([2000.0, 7000.0]));
    assert_eq!(dim_of(&s, d).refs.len(), 3);
}

#[test]
fn deleting_a_single_segment_dimension_takes_the_dimension_away() {
    let mut s = Session::new();
    let a = wall(&mut s, json!([[0, 0], [0, 4000]]));
    let b = wall(&mut s, json!([[2000, 0], [2000, 4000]]));
    let d = dimension(&mut s, json!([{"id": a}, {"id": b}]), json!([1000, -1000]));
    run(&mut s, "dimension_segment", json!({"id": d, "segment": 0}));
    assert!(s.model.elements.get(d).is_none());
}

#[test]
fn deleting_a_segment_of_an_equal_chain_turns_its_equality_off_when_one_is_left() {
    let mut s = Session::new();
    let (_, d) = chain(&mut s, &[0.0, 2000.0, 5000.0]);
    run(&mut s, "dimension_constraint", json!({"id": d, "equal": true}));
    run(&mut s, "dimension_segment", json!({"id": d, "segment": 0}));
    let edited = dim_of(&s, d);
    assert_eq!(edited.refs.len(), 2);
    assert!(!edited.equal);
    assert_eq!(values(&s, d), vec![5000.0]);
}
