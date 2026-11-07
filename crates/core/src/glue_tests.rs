//! A strip footing stuck to the wall it carries: the condition it takes (the same line, its top on the wall's
//! base), how it rides its wall in plan and in height, and how it lets go — of its own accord, when the geometry
//! stops making the condition, or because a wall's end dragged changes the footing by hand.

use crate::engine::Session;
use crate::geom::P2;
use crate::model::Run;
use serde_json::json;

fn session(script: &str) -> Session {
    let mut s = Session::new();
    s.script(script).unwrap();
    s
}

/// A 200 mm wall east from the origin on Level 1, and a strip footing under it with its top on its base.
fn under() -> (Session, u64, u64) {
    let s = session(
        r#"wall {"points": [[0, 0], [6000, 0]]}
           strip_footing {"points": [[0, 0], [6000, 0]]}"#,
    );
    let wall = s.model.elements.iter().find(|(_, e)| e.category() == "wall").map(|(id, _)| *id).unwrap();
    let footing = s.model.elements.iter().find(|(_, e)| e.category() == "stripfooting").map(|(id, _)| *id).unwrap();
    (s, wall, footing)
}

fn run(s: &Session, id: u64) -> Run {
    s.model.run_of(id).cloned().unwrap()
}

fn wall(s: &Session, id: u64) -> crate::model::Wall {
    s.model.wall(id).cloned().unwrap()
}

/// The height of the footing's section's top, as the model draws it.
fn top(s: &Session, id: u64) -> f64 {
    crate::runs::Geom::of(&s.model, &run(s, id), s.model.run_type(s.model.elements.get(id).unwrap()).unwrap()).z_span().1
}

#[test]
fn a_footing_on_a_walls_line_with_its_top_on_the_walls_base_keeps_its_top_there_as_the_wall_rises() {
    let (mut s, wall_id, footing) = under();
    assert_eq!(top(&s, footing), s.model.base_z(&wall(&s, wall_id)), "the condition holds as drawn");
    // The wall's base a metre up: the footing rises with it, its top on the new base.
    s.execute("modify", &json!({"id": wall_id, "base_offset": 1000.0})).unwrap();
    assert_eq!(s.model.base_z(&wall(&s, wall_id)), 1000.0);
    assert_eq!(top(&s, footing), 1000.0, "the footing's top is on the wall's base");
    assert_eq!(run(&s, footing).justify, crate::model::Justify::Top, "its line stays a top's");
    assert_eq!(run(&s, footing).offset, 1000.0, "offset set for its top to be there");
    // Down again, and under the level.
    s.execute("modify", &json!({"id": wall_id, "base_offset": -500.0})).unwrap();
    assert_eq!(top(&s, footing), -500.0);
    // Its justify kept, a footing centred on its line is lifted by half its depth instead.
    s.execute("modify", &json!({"id": footing, "justify": "centre", "offset": -650.0})).unwrap();
    assert_eq!(top(&s, footing), -500.0, "still on the base, as drawn");
    s.execute("modify", &json!({"id": wall_id, "base_offset": 0.0})).unwrap();
    assert_eq!(top(&s, footing), 0.0, "a centre's top is half its depth over its offset");
    assert_eq!(run(&s, footing).offset, -150.0);
}

#[test]
fn a_glued_footing_moves_and_turns_with_its_wall_in_plan() {
    let (mut s, wall_id, footing) = under();
    // Moved: it goes the same way, the condition still holds.
    s.execute("move", &json!({"ids": [wall_id], "by": [2000.0, 1500.0]})).unwrap();
    assert_eq!((run(&s, footing).start, run(&s, footing).end), (P2(2000.0, 1500.0), P2(8000.0, 1500.0)));
    // Turned about its start: the footing turns about the same point.
    s.execute("rotate", &json!({"ids": [wall_id], "about": [2000, 1500], "angle": 90})).unwrap();
    let (a, b) = (run(&s, footing).start, run(&s, footing).end);
    assert!((a.dist(P2(2000.0, 1500.0))).abs() < 1e-6, "{a:?}");
    assert!((b.dist(P2(2000.0, 7500.0))).abs() < 1e-6, "{b:?}");
    // Its top is still the wall's base after all of it.
    assert_eq!(top(&s, footing), s.model.base_z(&wall(&s, wall_id)));
    // Mirrored: across the y axis, riding too.
    s.execute("mirror", &json!({"ids": [wall_id], "axis": [[0, 0], [0, 1000]]})).unwrap();
    let (a, b) = (run(&s, footing).start, run(&s, footing).end);
    assert!((a.dist(P2(-2000.0, 1500.0))).abs() < 1e-6 && (b.dist(P2(-2000.0, 7500.0))).abs() < 1e-6, "{a:?} {b:?}");
}

#[test]
fn the_glue_goes_and_comes_as_the_geometry_does() {
    let (mut s, wall_id, footing) = under();
    // The footing moved by hand a metre north: their lines no longer bear on one another. It lets go of the wall
    // and stays where it is put; the wall's next change of base doesn't carry it.
    s.execute("move", &json!({"ids": [footing], "by": [0.0, 1000.0]})).unwrap();
    s.execute("modify", &json!({"id": wall_id, "base_offset": 700.0})).unwrap();
    assert_eq!(run(&s, footing).start, P2(0.0, 1000.0), "its own move stands");
    assert_eq!(top(&s, footing), 0.0, "no longer following the wall");
    // Put back on the wall's line with its top on the wall's base: glued again, and it follows once more.
    s.execute("move", &json!({"ids": [footing], "by": [0.0, -1000.0]})).unwrap();
    s.execute("modify", &json!({"id": footing, "offset": 700.0})).unwrap();
    s.execute("modify", &json!({"id": wall_id, "base_offset": 1500.0})).unwrap();
    assert_eq!(top(&s, footing), 1500.0, "glued again");
    // Moved back and forth as one: the wall with it, the condition kept all the while.
    s.execute("move", &json!({"ids": [wall_id, footing], "by": [1000.0, 0.0]})).unwrap();
    assert_eq!(run(&s, footing).start, P2(1000.0, 0.0), "moved once, not twice");
    assert_eq!(top(&s, footing), 1500.0);
}

#[test]
fn a_footing_changed_by_hand_lets_go_of_its_wall() {
    let (mut s, wall_id, footing) = under();
    // Off its line by hand: it is no footing of that wall's, and the wall's base leaves it alone.
    s.execute("modify", &json!({"id": footing, "start": [0, 300]})).unwrap();
    s.execute("modify", &json!({"id": wall_id, "base_offset": 800.0})).unwrap();
    assert_eq!(top(&s, footing), 0.0, "its own change stands");
    // Back on the wall's line, top on its base: glued again.
    s.execute("modify", &json!({"id": footing, "start": [0, 0], "offset": 800.0})).unwrap();
    s.execute("modify", &json!({"id": wall_id, "base_offset": 1600.0})).unwrap();
    assert_eq!(top(&s, footing), 1600.0);
}

#[test]
fn dragging_a_walls_end_along_its_axis_leaves_the_footing_where_it_is() {
    let (mut s, wall_id, footing) = under();
    // Its end pulled 1000 east: the wall is shorter, the footing as long as it was, and — the line and the
    // base being what they were — still under the wall and still riding it.
    s.execute("move", &json!({"ids": [wall_id], "by": [1000.0, 0.0], "end": "end"})).unwrap();
    assert_eq!((wall(&s, wall_id).start, wall(&s, wall_id).end), (P2(0.0, 0.0), P2(7000.0, 0.0)));
    assert_eq!((run(&s, footing).start, run(&s, footing).end), (P2(0.0, 0.0), P2(6000.0, 0.0)), "the footing keeps its length");
    assert!(crate::glue::wall_of(&s.model, footing) == Some(wall_id), "still glued: the same line, the same base");
    // And it still rides: the wall raised, the footing with it.
    s.execute("modify", &json!({"id": wall_id, "base_offset": 400.0})).unwrap();
    assert_eq!(top(&s, footing), 400.0);
    // Its end pulled across the line instead: the lines no longer bear on each other, and the next change of
    // base leaves the footing alone.
    s.execute("move", &json!({"ids": [wall_id], "by": [0.0, 300.0], "end": "start"})).unwrap();
    s.execute("modify", &json!({"id": wall_id, "base_offset": 900.0})).unwrap();
    assert_eq!(top(&s, footing), 400.0, "let go: its line left the footing's");
}

#[test]
fn a_footing_under_two_walls_on_one_line_rides_the_moved_segment_and_lets_the_rest_go() {
    // Two collinear wall segments over one footing (a wall drawn in two runs): the condition holds against each,
    // and a tie glues to the first. The collision the task spec 6.15 accepts as an edge: moving any of them
    // carries the footing — their ends joined, one wall drags the other, and the footing rides what is glued to
    // it — after which it lies on the moved line and no longer glues to the segment that stayed behind.
    let mut s = session(
        r#"wall {"points": [[0, 0], [3000, 0]]}
           wall {"points": [[3000, 0], [6000, 0]]}
           strip_footing {"points": [[0, 0], [6000, 0]]}"#,
    );
    let ids = walls(&s);
    let [first, second] = ids[..] else { panic!() };
    let footing = s.model.elements.iter().find(|(_, e)| e.category() == "stripfooting").map(|(id, _)| *id).unwrap();
    assert_eq!(crate::glue::wall_of(&s.model, footing), Some(first), "the tie takes the first of the two, both being as near the base");
    // The far segment moved: its end was on the first's end, the first is dragged, the footing rides along, and
    // the line it now lies on is the moved one.
    s.execute("move", &json!({"ids": [second], "by": [0.0, 1000.0]})).unwrap();
    assert_eq!(run(&s, footing).start, P2(0.0, 1000.0), "the footing rode the move");
    assert_eq!(crate::glue::wall_of(&s.model, footing), Some(second), "it now rides the segment whose line it lies on");
    // The rest holds no more: raising the segment left behind, whose line the footing crossed off, moves nothing,
    // while raising the one it rides takes the footing with it.
    s.execute("modify", &json!({"id": first, "base_offset": 500.0})).unwrap();
    assert_eq!(top(&s, footing), 0.0, "the wall it let go of moves nothing");
    s.execute("modify", &json!({"id": second, "base_offset": 700.0})).unwrap();
    assert_eq!(top(&s, footing), 700.0, "and the one it rides takes it up");
    assert_eq!(s.model.check(), Ok(()), "the model is sound through the ride");
}

/// The ids of the walls, in id order.
fn walls(s: &Session) -> Vec<u64> {
    let mut ids: Vec<u64> = s.model.elements.iter().filter(|(_, e)| e.category() == "wall").map(|(id, _)| *id).collect();
    ids.sort_unstable();
    ids
}

#[test]
fn a_wall_deleted_leaves_its_footing_where_it_is_and_the_model_sound() {
    let (mut s, wall_id, footing) = under();
    s.execute("delete", &json!({"ids": [wall_id]})).unwrap();
    assert!(s.model.run_of(footing).is_some(), "the footing is not deleted with its wall");
    assert!(s.model.wall(wall_id).is_none());
    assert_eq!(s.model.check(), Ok(()), "no reference dangled: none was kept");
}

#[test]
fn only_a_flat_strip_footing_glues() {
    // A sloped footing (an `end_offset`) has no flat top to meet a base: no glue, its heights left as typed.
    let mut s = session(
        r#"wall {"points": [[0, 0], [6000, 0]]}
           strip_footing {"points": [[0, 0], [6000, 0]], "end_offset": 200}"#,
    );
    let wall_id = s.model.elements.iter().find(|(_, e)| e.category() == "wall").map(|(id, _)| *id).unwrap();
    let footing = s.model.elements.iter().find(|(_, e)| e.category() == "stripfooting").map(|(id, _)| *id).unwrap();
    s.execute("modify", &json!({"id": wall_id, "base_offset": 1000.0})).unwrap();
    assert_eq!(run(&s, footing).end_offset, Some(200.0), "left as it was");
    assert_eq!(run(&s, footing).offset, 0.0);
    // A beam on the wall's line, its bottom on the wall's base, is no footing and doesn't glue.
    let mut s = session(
        r#"wall {"points": [[0, 0], [6000, 0]]}
           beam {"points": [[0, 0], [6000, 0]], "offset": -500, "justify": "bottom"}"#,
    );
    let wall_id = s.model.elements.iter().find(|(_, e)| e.category() == "wall").map(|(id, _)| *id).unwrap();
    let beam = s.model.elements.iter().find(|(_, e)| e.category() == "beam").map(|(id, _)| *id).unwrap();
    s.execute("modify", &json!({"id": wall_id, "base_offset": 900.0})).unwrap();
    assert_eq!(run(&s, beam).offset, -500.0, "a beam is never glued to a wall");
}
