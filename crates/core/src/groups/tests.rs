//! Groups driven through commands, as a front end drives them: every kind of element grouped, groups moved, copied, turned,
//! mirrored and placed on other levels, and groups broken on purpose (a member moved or deleted, a level moved under one, a
//! dimension to what isn't grouped), with what Revit offers to do about it.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom::P2;
use crate::groups::{CONFLICT, Kind, is_conflict};
use crate::model::{Element, Id};

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

fn err(s: &mut Session, c: &str, p: Value) -> String {
    s.execute(c, &p).err().unwrap_or_else(|| panic!("{c} {p} was accepted"))
}

fn id_of(v: &Value) -> Id {
    v["id"].as_u64().unwrap_or_else(|| panic!("no id in {v}"))
}

fn ids_of(v: &Value, key: &str) -> Vec<Id> {
    v[key].as_array().unwrap_or_else(|| panic!("no {key} in {v}")).iter().filter_map(Value::as_u64).collect()
}

fn wall(s: &Session, id: Id) -> crate::model::Wall {
    s.model.wall(id).cloned().unwrap_or_else(|| panic!("{id} is no wall"))
}

fn near(a: P2, b: P2) -> bool {
    a.dist(b) < 0.01
}

/// Three walls of a room's corner on Level 1, up to Level 2, a door in the first: what most tests group.
fn corner() -> (Session, Vec<Id>) {
    let mut s = Session::new();
    let w = run(&mut s, "wall", json!({"points": [[0, 3000], [0, 0], [4000, 0], [4000, 3000]], "top_level": "Level 2"}));
    let mut ids = ids_of(&w, "ids");
    let d = run(&mut s, "door", json!({"at": [2000, 0]}));
    ids.push(id_of(&d));
    (s, ids)
}

fn grouped() -> (Session, Id, Vec<Id>) {
    let (mut s, ids) = corner();
    let g = run(&mut s, "create_group", json!({"ids": ids}));
    (s, id_of(&g), ids)
}

fn instances(s: &Session) -> Vec<Id> {
    crate::groups::instances(&s.model)
}

#[test]
fn create_group_keeps_the_elements_where_they_are_as_the_members_of_a_model_group() {
    let (mut s, ids) = corner();
    let before: Vec<Element> = ids.iter().map(|id| s.model.elements.get(*id).cloned().unwrap_or_else(|| panic!("{id}"))).collect();
    let g = run(&mut s, "create_group", json!({"ids": ids, "name": "Corner"}));
    assert_eq!(g["kind"], "model");
    assert_eq!(g["name"], "Corner");
    let gid = id_of(&g);
    for (id, was) in ids.iter().zip(&before) {
        assert_eq!(s.model.elements.get(*id), Some(was), "member {id} stays as it was");
        assert_eq!(crate::groups::group_of(&s.model, *id), Some(gid));
    }
    assert_eq!(crate::groups::members(&s.model, gid), ids);
    // The type holds two storeys: its own level and the one above, its walls' tops.
    let t = &s.model.model_groups["Corner"];
    assert_eq!(t.members.len(), 4);
    assert_eq!(t.levels.len(), 2);
    assert_eq!(t.levels[1].above, 1);
    assert_eq!(t.levels[1].height, 3000.0);
    // Its origin is the middle of the walls.
    let Some(Element::ModelGroup(mg)) = s.model.elements.get(gid) else { panic!("no model group") };
    assert!(near(mg.at, P2(2000.0, 1500.0)), "{:?}", mg.at);
    // A click on a member picks the group first, Tab the member.
    let wall = ids[1];
    assert_eq!(crate::groups::group_picks(&s.model, &[wall], None), [gid, wall]);
    assert_eq!(crate::groups::group_picks(&s.model, &[wall], Some(s.model.guid(gid))), [wall]);
    assert_eq!(crate::groups::with_members(&s.model, &[gid]).len(), 5);
    // An element is in one group.
    let e = err(&mut s, "create_group", json!({"ids": [wall]}));
    assert!(e.contains("already in group"), "{e}");
    // And it all reads back from a file the same.
    let text = s.model.to_json().unwrap_or_default();
    let (back, left_out) = crate::model::Model::read(&text).unwrap_or_else(|e| panic!("{e}"));
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(back.check(), Ok(()));
    assert_eq!(crate::groups::members(&back, gid), ids);
}

#[test]
fn moving_turning_and_mirroring_a_group_takes_its_members_with_it() {
    let (mut s, gid, ids) = grouped();
    run(&mut s, "move", json!({"ids": [gid], "by": [1000, 500]}));
    assert!(near(wall(&s, ids[1]).start, P2(1000.0, 500.0)));
    assert!(near(wall(&s, ids[1]).end, P2(5000.0, 500.0)));
    // A quarter turn about the origin.
    run(&mut s, "rotate", json!({"ids": [gid], "about": [1000, 500], "angle": 90}));
    let w = wall(&s, ids[1]);
    assert!(near(w.start, P2(1000.0, 500.0)) && near(w.end, P2(1000.0, 4500.0)), "{:?} {:?}", w.start, w.end);
    // Mirrored across x = 0: the wall runs back the other way, the door on the other jamb.
    let Some(Element::Door(before)) = s.model.elements.get(ids[3]).cloned() else { panic!("no door") };
    run(&mut s, "mirror", json!({"ids": [gid], "axis": [[0, 0], [0, 1000]]}));
    let w = wall(&s, ids[1]);
    assert!(near(w.start, P2(-1000.0, 4500.0)) && near(w.end, P2(-1000.0, 500.0)), "{:?} {:?}", w.start, w.end);
    let Some(Element::Door(after)) = s.model.elements.get(ids[3]).cloned() else { panic!("no door") };
    assert_ne!(before.flip_hand, after.flip_hand);
    assert_eq!(crate::groups::conflicts(&s.model, gid), Vec::<String>::new());
    let Some(Element::ModelGroup(g)) = s.model.elements.get(gid) else { panic!() };
    assert!(g.mirrored);
    // Undo takes it all back, one step each.
    for _ in 0..3 {
        run(&mut s, "undo", json!({}));
    }
    assert!(near(wall(&s, ids[1]).start, P2(0.0, 0.0)));
}

#[test]
fn a_copy_of_a_group_is_a_group_of_new_members() {
    let (mut s, gid, ids) = grouped();
    let copied = run(&mut s, "copy", json!({"ids": [gid], "by": [10000, 0]}));
    let all = instances(&s);
    assert_eq!(all.len(), 2, "{copied}");
    let copy = *all.iter().find(|i| **i != gid).unwrap_or(&0);
    let members = crate::groups::members(&s.model, copy);
    assert_eq!(members.len(), 4);
    assert!(members.iter().all(|m| !ids.contains(m)), "new elements");
    let walls: Vec<_> = members.iter().filter_map(|m| s.model.wall(*m)).collect();
    assert!(walls.iter().any(|w| near(w.start, P2(10000.0, 0.0)) && near(w.end, P2(14000.0, 0.0))));
    // Its door hangs in its own wall, with a mark of its own.
    let door = members.iter().find_map(|m| match s.model.elements.get(*m) {
        Some(Element::Door(o)) => Some(o.clone()),
        _ => None,
    });
    let door = door.unwrap_or_else(|| panic!("no door"));
    assert!(members.iter().any(|m| s.model.guid(*m) == door.host));
    let Some(Element::Door(first)) = s.model.elements.get(ids[3]) else { panic!() };
    assert_ne!(door.mark, first.mark);
    // Turned and mirrored copies too.
    run(&mut s, "rotate", json!({"ids": [gid], "about": [0, 0], "angle": 30, "copy": true}));
    run(&mut s, "mirror", json!({"ids": [gid], "axis": [[-500, 0], [-500, 10]], "copy": true}));
    assert_eq!(instances(&s).len(), 4);
    for g in instances(&s) {
        assert_eq!(crate::groups::conflicts(&s.model, g), Vec::<String>::new(), "group {g}");
    }
    // Deleting a group deletes its members.
    let n = s.model.elements.len();
    run(&mut s, "delete", json!({"ids": [copy]}));
    assert_eq!(s.model.elements.len(), n - 5);
}

#[test]
fn a_member_moved_or_deleted_on_its_own_is_a_conflict_that_resolve_group_settles() {
    let (mut s, gid, ids) = grouped();
    run(&mut s, "copy", json!({"ids": [gid], "by": [10000, 0]}));
    let copy = instances(&s).into_iter().find(|g| *g != gid).unwrap_or(0);
    // Moving a member alone breaks its group: refused, saying what to do.
    let e = err(&mut s, "move", json!({"ids": [ids[0]], "by": [0, -500]}));
    assert!(is_conflict(&e), "{e}");
    assert!(e.contains(CONFLICT) && e.contains("resolve_group") && e.contains("changed"), "{e}");
    assert!(near(wall(&s, ids[0]).start, P2(0.0, 3000.0)), "nothing moved");
    // Ungroup: the change is kept, the group goes, the other group stays.
    let params = json!({"ids": [ids[0]], "by": [0, -500]});
    run(&mut s, "resolve_group", json!({"fix": "ungroup", "command": "move", "params": params}));
    assert!(s.model.elements.get(gid).is_none());
    assert!(s.model.elements.get(copy).is_some());
    assert!(near(wall(&s, ids[0]).start, P2(0.0, 2500.0)));
    assert_eq!(crate::groups::group_of(&s.model, ids[1]), None);
    // One undo puts the group back with the wall.
    run(&mut s, "undo", json!({}));
    assert!(s.model.elements.get(gid).is_some());
    assert!(near(wall(&s, ids[0]).start, P2(0.0, 3000.0)));
    // Create New Type: the changed group is a type of its own; the other keeps the old one.
    run(&mut s, "resolve_group", json!({"fix": "new_type", "command": "move", "params": params}));
    let Some(Element::ModelGroup(g)) = s.model.elements.get(gid) else { panic!("the group stays") };
    assert_ne!(g.type_name, "Model Group 1");
    let Some(Element::ModelGroup(other)) = s.model.elements.get(copy) else { panic!() };
    assert_eq!(other.type_name, "Model Group 1");
    assert_eq!(crate::groups::conflicts(&s.model, gid), Vec::<String>::new());
    // Deleting a member alone is one too.
    let e = err(&mut s, "delete", json!({"ids": [ids[2]]}));
    assert!(is_conflict(&e) && e.contains("deleted"), "{e}");
    // So is changing one's type or height.
    let e = err(&mut s, "modify", json!({"id": ids[1], "type": "Exterior - 300mm"}));
    assert!(is_conflict(&e), "{e}");
    // But a door's mark is each group's own.
    run(&mut s, "modify", json!({"id": ids[3], "mark": "D-7"}));
    // Bad fixes are refused.
    let e = err(&mut s, "resolve_group", json!({"fix": "zap", "command": "move", "params": params}));
    assert!(e.contains("ungroup or new_type"), "{e}");
    let e = err(&mut s, "resolve_group", json!({"fix": "ungroup", "command": "resolve_group", "params": {}}));
    assert!(e.contains("isn't run through it"), "{e}");
}

#[test]
fn exclude_takes_a_member_out_of_one_group_and_restore_puts_it_back() {
    let (mut s, gid, ids) = grouped();
    run(&mut s, "copy", json!({"ids": [gid], "by": [10000, 0]}));
    // The second wall holds the door: excluded, the door goes with it.
    let r = run(&mut s, "exclude_member", json!({"ids": [ids[1]]}));
    let excluded = ids_of(&r, "excluded");
    assert!(excluded.contains(&ids[1]) && excluded.contains(&ids[3]), "{r}");
    assert!(s.model.elements.get(ids[1]).is_none());
    assert_eq!(crate::groups::members(&s.model, gid).len(), 2);
    for g in instances(&s) {
        assert_eq!(crate::groups::conflicts(&s.model, g), Vec::<String>::new());
    }
    // The other group keeps all four.
    let other = instances(&s).into_iter().find(|g| *g != gid).unwrap_or(0);
    assert_eq!(crate::groups::members(&s.model, other).len(), 4);
    // Moving the group with one excluded is fine.
    run(&mut s, "move", json!({"ids": [gid], "by": [0, 100]}));
    run(&mut s, "restore_excluded", json!({"id": gid}));
    let members = crate::groups::members(&s.model, gid);
    assert_eq!(members.len(), 4);
    assert!(members.iter().any(|m| s.model.wall(*m).is_some_and(|w| near(w.start, P2(0.0, 100.0)) && near(w.end, P2(4000.0, 100.0)))));
    // Not a member: refused.
    let wall = ids_of(&run(&mut s, "wall", json!({"points": [[0, -5000], [1000, -5000]]})), "ids")[0];
    let e = err(&mut s, "exclude_member", json!({"ids": [wall]}));
    assert!(e.contains("isn't in a group"), "{e}");
}

#[test]
fn ungroup_leaves_the_members_as_plain_elements() {
    let (mut s, gid, ids) = grouped();
    let r = run(&mut s, "ungroup", json!({"id": gid}));
    assert_eq!(ids_of(&r, "ids"), ids);
    assert!(s.model.elements.get(gid).is_none());
    run(&mut s, "move", json!({"ids": [ids[0]], "by": [0, 100]}));
    // The type stays, and places again.
    let r = run(&mut s, "model_group", json!({"type": "Model Group 1", "at": [20000, 0]}));
    assert_eq!(ids_of(&r, "members").len(), 4);
    let e = err(&mut s, "ungroup", json!({"id": ids[0]}));
    assert!(e.contains("isn't a group"), "{e}");
}

#[test]
fn edit_group_changes_the_type_and_every_group_of_it() {
    let (mut s, gid, ids) = grouped();
    run(&mut s, "copy", json!({"ids": [gid], "by": [10000, 0]}));
    let other = instances(&s).into_iter().find(|g| *g != gid).unwrap_or(0);
    run(&mut s, "edit_group", json!({"id": gid}));
    assert_eq!(s.editing_group(), Some(s.model.guid(gid)));
    // Inside, a member moves freely, and a new wall joins it.
    run(&mut s, "move", json!({"ids": [ids[0]], "by": [-500, 0]}));
    let new = ids_of(&run(&mut s, "wall", json!({"points": [[0, 3000], [4000, 3000]], "top_level": "Level 2"})), "ids")[0];
    // And one is taken out.
    run(&mut s, "remove_from_group", json!({"ids": [ids[2]]}));
    let r = run(&mut s, "finish_group", json!({}));
    assert_eq!(s.editing_group(), None);
    assert_eq!(ids_of(&r, "updated"), [other], "{r}");
    let members = crate::groups::members(&s.model, gid);
    assert!(members.contains(&new) && !members.contains(&ids[2]), "{members:?}");
    assert_eq!(crate::groups::group_of(&s.model, ids[2]), None, "the removed wall stays, plain");
    assert!(s.model.wall(ids[2]).is_some());
    // The other group has the moved wall, the new one and not the removed one.
    let theirs = crate::groups::members(&s.model, other);
    assert_eq!(theirs.len(), members.len());
    let walls: Vec<_> = theirs.iter().filter_map(|m| s.model.wall(*m).cloned()).collect();
    assert!(walls.iter().any(|w| near(w.start, P2(9500.0, 3000.0))), "{walls:?}");
    assert!(walls.iter().any(|w| near(w.start, P2(10000.0, 3000.0)) && near(w.end, P2(14000.0, 3000.0))));
    assert!(!walls.iter().any(|w| near(w.start, P2(14000.0, 0.0))));
    for g in instances(&s) {
        assert_eq!(crate::groups::conflicts(&s.model, g), Vec::<String>::new());
    }
    // Cancel puts back what an edit did.
    let before = s.model.clone();
    run(&mut s, "edit_group", json!({"id": gid}));
    run(&mut s, "move", json!({"ids": [ids[0]], "by": [-500, 0]}));
    run(&mut s, "delete", json!({"ids": [new]}));
    run(&mut s, "cancel_group", json!({}));
    assert_eq!(s.model, before);
    assert_eq!(s.editing_group(), None);
    // Nothing to finish or cancel now; adding needs an edit.
    assert!(err(&mut s, "finish_group", json!({})).contains("no group is in Edit Group"));
    assert!(err(&mut s, "add_to_group", json!({"ids": [ids[2]]})).contains("no group is in Edit Group"));
    let e = err(&mut s, "edit_group", json!({"id": ids[0]}));
    assert!(e.contains("isn't a group"), "{e}");
}

#[test]
fn edit_group_adds_and_finishes_with_an_element_drawn_meanwhile_and_one_added() {
    let (mut s, gid, _) = grouped();
    let loose = ids_of(&run(&mut s, "wall", json!({"points": [[0, -2000], [4000, -2000]]})), "ids")[0];
    run(&mut s, "edit_group", json!({"id": gid}));
    run(&mut s, "add_to_group", json!({"ids": [loose]}));
    // A detail line can't join a model group.
    let line = id_of(&run(&mut s, "detail_line", json!({"points": [[0, 0], [100, 0]]})));
    let e = err(&mut s, "add_to_group", json!({"ids": [line]}));
    assert!(e.contains("can't join a model group"), "{e}");
    run(&mut s, "finish_group", json!({}));
    assert!(crate::groups::members(&s.model, gid).contains(&loose));
    assert_eq!(crate::groups::group_of(&s.model, line), None, "the line drawn meanwhile was no model element");
}

#[test]
fn a_group_reaching_a_level_above_follows_its_storeys_and_a_forgiving_one_stretches() {
    let mut s = Session::new();
    run(&mut s, "level", json!({"name": "Level 3", "elevation": 6000}));
    run(&mut s, "level", json!({"name": "Level 4", "elevation": 9500}));
    let w = ids_of(&run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]], "top_level": "Level 2"})), "ids");
    let g = id_of(&run(&mut s, "create_group", json!({"ids": w})));
    // Level 2 to 3 is a storey as tall as 1 to 2: placed there, its wall goes up to Level 3.
    let r = run(&mut s, "model_group", json!({"type": "Model Group 1", "at": [2000, 0], "level": "Level 2"}));
    let on2 = ids_of(&r, "members")[0];
    let level3 = s.model.levels().iter().find(|(_, l)| l.name == "Level 3").map(|(id, _)| s.model.guid(*id));
    assert_eq!(wall(&s, on2).top_level, level3);
    assert_eq!(s.model.base_z(&wall(&s, on2)), 3000.0);
    // Level 3 to 4 is taller: a group that isn't forgiving isn't placed there…
    let e = err(&mut s, "model_group", json!({"type": "Model Group 1", "at": [2000, 0], "level": "Level 3"}));
    assert!(e.contains("forgiving"), "{e}");
    // …and a forgiving one is, its wall reaching Level 4.
    let r = run(&mut s, "model_group", json!({"type": "Model Group 1", "at": [2000, 0], "level": "Level 3", "forgiving": true}));
    let on3 = ids_of(&r, "members")[0];
    assert_eq!(s.model.top_z(&wall(&s, on3)), 9500.0);
    // On every level at once: Paste Aligned to Selected Levels.
    let e = err(&mut s, "model_group", json!({"type": "Model Group 1", "at": [8000, 0], "levels": ["Level 1", "Level 2", "Level 3"]}));
    assert!(e.contains("forgiving"), "{e}");
    let r = run(
        &mut s,
        "model_group",
        json!({"type": "Model Group 1", "at": [8000, 0], "levels": ["Level 1", "Level 2", "Level 3"], "forgiving": true}),
    );
    assert_eq!(ids_of(&r, "ids").len(), 3);
    // Nothing above Level 4.
    let e = err(&mut s, "model_group", json!({"type": "Model Group 1", "at": [2000, 0], "level": "Level 4", "forgiving": true}));
    assert!(e.contains("reaches 1 level above"), "{e}");
    // Level 2 moved up: the group on Level 1 isn't forgiving, so its storey breaks…
    let level2 = s.model.levels().iter().find(|(_, l)| l.name == "Level 2").map(|(id, _)| *id).unwrap_or(0);
    let e = err(&mut s, "modify", json!({"id": level2, "elevation": 3500}));
    assert!(is_conflict(&e) && e.contains("3500 mm above"), "{e}");
    // …and made forgiving, it lets its wall stretch. (The groups on Level 2 stand on it, and move with it whole.)
    run(&mut s, "modify", json!({"id": g, "forgiving": true}));
    let first = instances(&s).into_iter().find(|i| matches!(s.model.elements.get(*i), Some(Element::ModelGroup(mg)) if !mg.forgiving));
    if let Some(i) = first {
        run(&mut s, "modify", json!({"id": i, "forgiving": true}));
    }
    run(&mut s, "modify", json!({"id": level2, "elevation": 3500}));
    assert_eq!(s.model.top_z(&wall(&s, w[0])), 3500.0);
    assert_eq!(s.model.base_z(&wall(&s, w[0])), 0.0, "its base stays");
}

#[test]
fn a_forgiving_group_still_refuses_a_level_its_member_stands_on_moving() {
    let mut s = Session::new();
    run(&mut s, "level", json!({"name": "Level 3", "elevation": 6000}));
    // A wall on Level 2 grouped with one on Level 1: the group stands on Level 1 and its second storey is a member's base.
    let a = ids_of(&run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]]})), "ids");
    let b = ids_of(&run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]], "level": "Level 2"})), "ids");
    let g = id_of(&run(&mut s, "create_group", json!({"ids": [a[0], b[0]]})));
    run(&mut s, "modify", json!({"id": g, "forgiving": true}));
    let level2 = s.model.levels().iter().find(|(_, l)| l.name == "Level 2").map(|(id, _)| *id).unwrap_or(0);
    let e = err(&mut s, "modify", json!({"id": level2, "elevation": 3500}));
    assert!(is_conflict(&e) && e.contains("a member stands on it"), "{e}");
    // Ungrouping settles it.
    run(&mut s, "resolve_group", json!({"fix": "ungroup", "command": "modify", "params": {"id": level2, "elevation": 3500}}));
    assert!(s.model.elements.get(g).is_none());
}

#[test]
fn a_selection_of_both_kinds_or_of_what_no_group_holds_is_refused() {
    let (mut s, ids) = corner();
    let line = id_of(&run(&mut s, "detail_line", json!({"points": [[0, -1000], [1000, -1000]]})));
    let e = err(&mut s, "create_group", json!({"ids": [ids[0], line]}));
    assert!(e.contains("one kind"), "{e}");
    let e = err(&mut s, "create_group", json!({"ids": [100000]}));
    assert!(e.contains("datum"), "{e}");
    // A door without its wall.
    let e = err(&mut s, "create_group", json!({"ids": [ids[3]]}));
    assert!(e.contains("isn't in the group"), "{e}");
    assert_eq!(crate::groups::kind_of_all(&s.model, &ids), Ok(Kind::Model));
    assert_eq!(crate::groups::kind_of_all(&s.model, &[line]), Ok(Kind::Detail));
    let e = err(&mut s, "create_group", json!({"ids": []}));
    assert!(e.contains("the elements to group"), "{e}");
    let e = err(&mut s, "create_group", json!({"ids": ids, "bogus": 1}));
    assert!(e.contains("isn't one of its parameters"), "{e}");
}

#[test]
fn a_detail_group_holds_lines_regions_notes_and_dimensions_of_what_it_holds() {
    let mut s = Session::new();
    let a = id_of(&run(&mut s, "detail_line", json!({"points": [[0, 0], [0, 1000]]})));
    let b = id_of(&run(&mut s, "detail_line", json!({"points": [[2000, 0], [2000, 1000]]})));
    let n = id_of(&run(&mut s, "text", json!({"text": "Ties", "at": [500, 1500]})));
    let r = id_of(&run(
        &mut s,
        "filled_region",
        json!({"sketch": [[[0, 0], [500, 0]], [[500, 0], [500, 500]], [[500, 500], [0, 500]], [[0, 500], [0, 0]]]}),
    ));
    let d = id_of(&run(&mut s, "dimension", json!({"refs": [{"id": a}, {"id": b}], "at": [1000, 2000]})));
    let w = ids_of(&run(&mut s, "wall", json!({"points": [[5000, 0], [5000, 3000]]})), "ids")[0];
    let to_wall = id_of(&run(&mut s, "dimension", json!({"refs": [{"id": b}, {"id": w}], "at": [3500, 2500]})));
    // A dimension to a wall isn't grouped with lines; nor one to a line left out.
    let e = err(&mut s, "create_group", json!({"ids": [b, to_wall]}));
    assert!(e.contains("dimension") && e.contains("measures"), "{e}");
    let e = err(&mut s, "create_group", json!({"ids": [a, d]}));
    assert!(e.contains("measures") && e.contains("line"), "{e}");
    let g = run(&mut s, "create_group", json!({"ids": [a, b, n, r, d], "name": "Ties"}));
    assert_eq!(g["kind"], "detail");
    let gid = id_of(&g);
    let _ = w;
    // Placed again, turned and mirrored, in the plan of Level 2: its dimension measures its own lines there.
    let p = run(&mut s, "detail_group", json!({"type": "Ties", "at": [0, 10000], "angle": 90, "mirrored": true, "view": "Level 2"}));
    let members = ids_of(&p, "members");
    assert_eq!(members.len(), 5);
    let dim = members.iter().find_map(|m| match s.model.elements.get(*m) {
        Some(Element::Dimension(x)) => Some(x.clone()),
        _ => None,
    });
    let dim = dim.unwrap_or_else(|| panic!("no dimension"));
    let lines: Vec<_> = members.iter().map(|m| s.model.guid(*m)).collect();
    assert!(dim.refs.iter().all(|r| lines.contains(&r.id)), "it measures its own copies");
    let level2 = s.model.levels().iter().find(|(_, l)| l.name == "Level 2").map(|(id, _)| s.model.guid(*id));
    assert_eq!(Some(dim.level), level2);
    for i in instances(&s) {
        assert_eq!(crate::groups::conflicts(&s.model, i), Vec::<String>::new());
    }
    // A member line moved alone is a conflict there too; the group moved whole isn't.
    let e = err(&mut s, "move", json!({"ids": [a], "by": [10, 0]}));
    assert!(is_conflict(&e), "{e}");
    run(&mut s, "move", json!({"ids": [gid], "by": [10, 0]}));
    run(&mut s, "copy", json!({"ids": [gid], "by": [0, -5000]}));
    // The types list both kinds apart.
    let t = run(&mut s, "group_type", json!({}));
    assert_eq!(t["groups"]["Ties"]["placed"], 3);
    assert_eq!(t["model_groups"].as_object().map(|o| o.len()), Some(0));
}

#[test]
fn every_kind_of_model_element_groups_copies_turns_and_mirrors() {
    let mut s = Session::new();
    let mut made: Vec<(String, Vec<Id>)> = Vec::new();
    let mut add = |s: &mut Session, what: &str, c: &str, p: Value| {
        let r = run(s, c, p);
        let ids = if r.get("ids").is_some() { ids_of(&r, "ids") } else { vec![id_of(&r)] };
        made.push((what.to_string(), ids));
    };
    add(&mut s, "walls", "wall", json!({"points": [[0, 0], [6000, 0], [6000, 5000], [0, 5000]], "closed": true, "top_level": "Level 2"}));
    add(&mut s, "floor", "floor", json!({"boundary": [[0, 0], [6000, 0], [6000, 5000], [0, 5000]]}));
    add(&mut s, "ceiling", "ceiling", json!({"boundary": [[0, 0], [6000, 0], [6000, 5000], [0, 5000]]}));
    add(&mut s, "shaft", "shaft", json!({"boundary": [[500, 500], [1000, 500], [1000, 1000], [500, 1000]]}));
    add(&mut s, "column", "column", json!({"at": [3000, 2500]}));
    add(&mut s, "structural column", "structural_column", json!({"at": [2000, 2500]}));
    add(&mut s, "footing", "footing", json!({"at": [2000, 2500]}));
    add(&mut s, "beam", "beam", json!({"points": [[0, 2500], [6000, 2500]], "level": "Level 2"}));
    add(&mut s, "strip footing", "strip_footing", json!({"points": [[0, -2000], [6000, -2000]]}));
    add(&mut s, "pipe", "pipe", json!({"points": [[500, 4000], [5500, 4000]], "offset": 500}));
    add(&mut s, "duct", "duct", json!({"points": [[500, 3500], [5500, 3500]], "offset": 2500}));
    add(&mut s, "stair", "stair", json!({"start": [1500, 1000], "angle": 0}));
    add(&mut s, "room", "room", json!({"point": [4500, 4000]}));
    add(&mut s, "room separator", "room_separator", json!({"points": [[3000, 3000], [6000, 3000]]}));
    add(&mut s, "property line", "property_line", json!({"points": [[-1000, -3000], [7000, -3000], [7000, 6000], [-1000, 6000]]}));
    add(&mut s, "mass", "mass", json!({"boundary": [[10000, 0], [12000, 0], [12000, 2000], [10000, 2000]]}));
    add(&mut s, "roof", "roof", json!({"boundary": [[0, 0], [6000, 0], [6000, 5000], [0, 5000]], "level": "Level 2"}));
    let all: Vec<Id> = made.iter().flat_map(|(_, ids)| ids.clone()).collect();
    // The stair's railings, if it made any, go with it.
    let railings: Vec<Id> = s.model.elements.iter().filter(|(_, e)| matches!(e, Element::Railing(_))).map(|(id, _)| *id).collect();
    let mut ids = all.clone();
    ids.extend(railings);
    // Each kind on its own first, then all of them at once.
    for (what, ids) in &made {
        let g = s.execute("create_group", &json!({"ids": ids}));
        match g {
            Ok(g) => {
                let gid = id_of(&g);
                run(&mut s, "copy", json!({"ids": [gid], "by": [20000, 0]}));
                run(&mut s, "ungroup", json!({"id": gid}));
            }
            Err(e) => panic!("{what}: {e}"),
        }
        // The copy too, so all of them can be grouped together again.
        for i in instances(&s) {
            run(&mut s, "delete", json!({"ids": [i]}));
        }
    }
    let made = run(&mut s, "create_group", json!({"ids": ids}));
    let (g, name) = (id_of(&made), made["name"].as_str().unwrap_or_default().to_string());
    run(&mut s, "copy", json!({"ids": [g], "by": [30000, 0]}));
    run(&mut s, "rotate", json!({"ids": [g], "about": [0, 0], "angle": 45, "copy": true}));
    run(&mut s, "mirror", json!({"ids": [g], "axis": [[-2000, 0], [-2000, 10]], "copy": true}));
    run(&mut s, "model_group", json!({"type": name, "at": [3000, 30000], "angle": 120, "mirrored": true}));
    assert_eq!(instances(&s).len(), 5);
    for i in instances(&s) {
        assert_eq!(crate::groups::conflicts(&s.model, i), Vec::<String>::new(), "group {i}");
        let have: Vec<&str> =
            crate::groups::members(&s.model, i).iter().filter_map(|m| s.model.elements.get(*m)).map(Element::category).collect();
        assert_eq!(have.len(), ids.len(), "group {i}: {have:?} {:?}", s.model.elements.get(i));
    }
    assert_eq!(s.model.check(), Ok(()));
    // Through a file and back.
    let text = s.model.to_json().unwrap_or_default();
    let (back, left_out) = crate::model::Model::read(&text).unwrap_or_else(|e| panic!("{e}"));
    assert!(left_out.is_empty(), "{left_out:?}");
    for i in crate::groups::instances(&back) {
        assert_eq!(crate::groups::conflicts(&back, i), Vec::<String>::new(), "group {i} read back");
    }
}

#[test]
fn a_file_of_20_with_a_detail_group_gets_its_members() {
    let mut s = Session::new();
    let a = id_of(&run(&mut s, "detail_line", json!({"points": [[0, 0], [0, 1000]]})));
    let g = id_of(&run(&mut s, "create_group", json!({"ids": [a], "name": "Old"})));
    let mut v: Value = serde_json::from_str(&s.model.to_json().unwrap_or_default()).unwrap_or_default();
    // As a file of 20 held it: the group with no members, its type's member drawn in its place; the line not there.
    v["format"] = json!(20);
    let gs = g.to_string();
    let line = a.to_string();
    if let Some(es) = v["elements"].as_object_mut() {
        es.remove(&line);
        if let Some(o) = es.get_mut(&gs).and_then(Value::as_object_mut) {
            o.remove("members");
        }
    }
    let (m, left_out) = crate::model::Model::read(&v.to_string()).unwrap_or_else(|e| panic!("{e}"));
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(m.check(), Ok(()));
    let members = crate::groups::members(&m, g);
    assert_eq!(members.len(), 1);
    assert!(matches!(m.elements.get(members[0]), Some(Element::DetailLine(_))));
}

#[test]
fn group_types_rename_and_delete_by_kind() {
    let (mut s, gid, _) = grouped();
    run(&mut s, "group_type", json!({"name": "Model Group 1", "rename": "Corner"}));
    let Some(Element::ModelGroup(g)) = s.model.elements.get(gid) else { panic!() };
    assert_eq!(g.type_name, "Corner");
    let e = err(&mut s, "group_type", json!({"name": "Corner", "delete": true}));
    assert!(e.contains("delete or ungroup it first"), "{e}");
    run(&mut s, "ungroup", json!({"id": gid}));
    run(&mut s, "group_type", json!({"name": "Corner", "kind": "model", "delete": true}));
    assert!(s.model.model_groups.is_empty());
    let e = err(&mut s, "group_type", json!({"name": "Corner", "kind": "plasma"}));
    assert!(e.contains("model or detail"), "{e}");
}

#[test]
fn hostile_group_params_are_refused_and_change_nothing() {
    let (mut s, gid, ids) = grouped();
    let before = s.model.clone();
    let tries = [
        ("create_group", json!({"ids": "x"})),
        ("create_group", json!({"ids": ids, "origin": [1e300, 0]})),
        ("create_group", json!({"ids": ids, "level": "nowhere"})),
        ("model_group", json!({"type": "nope", "at": [0, 0]})),
        ("model_group", json!({"type": "Model Group 1", "at": [0, 0], "levels": []})),
        ("model_group", json!({"type": "Model Group 1", "at": [0, 0], "levels": 5})),
        ("model_group", json!({"type": "Model Group 1", "at": [0, 0], "angle": 1e300})),
        ("model_group", json!({"type": "Model Group 1"})),
        ("detail_group", json!({"type": "Model Group 1", "at": [0, 0]})),
        ("ungroup", json!({"id": u64::MAX})),
        ("exclude_member", json!({"ids": [gid]})),
        ("restore_excluded", json!({"id": ids[0]})),
        ("edit_group", json!({"id": "x"})),
        ("remove_from_group", json!({"ids": [ids[0]]})),
        ("cancel_group", json!({})),
        ("resolve_group", json!({"fix": "ungroup"})),
        ("resolve_group", json!({"fix": "ungroup", "command": "nope", "params": {}})),
        ("modify", json!({"id": gid, "level": "nowhere"})),
        ("modify", json!({"id": gid, "type": "nope"})),
        ("modify", json!({"id": gid, "members": []})),
        ("modify", json!({"id": gid, "levels": [100000, 100000, 100000]})),
    ];
    for (c, p) in tries {
        assert!(s.execute(c, &p).is_err(), "{c} {p} was accepted");
        assert_eq!(s.model, before, "{c} {p} changed the model");
    }
}

/// A model group in the sweep's base (walls and a door well away from the rest), and payloads for every group command: as
/// meant, and with what is wrong.
pub(crate) fn sweep_payloads(base: &mut Session) -> Vec<(&'static str, Value)> {
    let w = ids_of(
        &run(base, "wall", json!({"points": [[50000, 0], [54000, 0], [54000, 3000], [58000, 3000]], "top_level": "Level 2"})),
        "ids",
    );
    let d = id_of(&run(base, "door", json!({"at": [52000, 0]})));
    let g = run(base, "create_group", json!({"ids": [w[0], w[1], d], "name": "Sweep"}));
    let gid = id_of(&g);
    vec![
        ("model_group", json!({"type": "Sweep", "at": [60000, 0], "levels": ["Level 1", "Level 2"], "forgiving": true})),
        ("model_group", json!({"type": "Sweep", "at": [60000, 0], "angle": 45, "mirrored": true})),
        ("model_group", json!({"type": "Sweep", "at": [1e300, 0], "levels": [null, 5, "x"], "forgiving": "yes"})),
        ("model_group", json!({"type": "Sweep", "at": [0, 0], "levels": vec![100000; 500]})),
        ("create_group", json!({"ids": [w[1], d]})),
        ("create_group", json!({"ids": [d]})),
        ("create_group", json!({"ids": [gid]})),
        ("exclude_member", json!({"ids": [w[0]]})),
        ("exclude_member", json!({"ids": [gid, d]})),
        ("restore_excluded", json!({"id": gid})),
        ("restore_excluded", json!({"ids": [w[0], u64::MAX]})),
        ("edit_group", json!({"id": gid})),
        ("edit_group", json!({"id": w[0]})),
        ("add_to_group", json!({"ids": [w[2]]})),
        ("remove_from_group", json!({"ids": [d]})),
        ("finish_group", json!({})),
        ("finish_group", json!({"x": 1})),
        ("cancel_group", json!({})),
        ("resolve_group", json!({"fix": "ungroup", "command": "move", "params": {"ids": [w[0]], "by": [0, 100]}})),
        ("resolve_group", json!({"fix": "new_type", "command": "delete", "params": {"ids": [d]}})),
        ("resolve_group", json!({"fix": "new_type", "command": "resolve_group", "params": {"fix": "ungroup"}})),
        ("resolve_group", json!({"fix": 5, "command": [], "params": "x"})),
        ("ungroup", json!({"ids": [gid, w[0]]})),
        ("group_type", json!({"name": "Sweep", "kind": "model", "rename": "Swept"})),
        ("group_type", json!({"name": "Sweep", "kind": "model", "delete": true})),
        ("modify", json!({"id": gid, "forgiving": true, "level": "Level 2"})),
        ("modify", json!({"id": gid, "type": "Nope", "members": [], "levels": 5})),
        ("move", json!({"ids": [gid], "by": [1e300, 0]})),
        ("rotate", json!({"ids": [gid], "about": [0, 0], "angle": 33, "copy": true})),
        ("mirror", json!({"ids": [gid], "axis": [[0, 0], [0, 1]]})),
        ("delete", json!({"ids": [gid]})),
        ("move", json!({"ids": [w[0]], "by": [0, 100]})),
    ]
}

#[test]
fn a_wall_outside_moved_into_a_grouped_wall_it_joins_is_a_conflict() {
    let (mut s, gid, ids) = grouped();
    // A wall of its own, its end on the group's corner: moving it drags the grouped wall's end with it.
    let loose = ids_of(&run(&mut s, "wall", json!({"points": [[4000, 0], [7000, 0]], "top_level": "Level 2"})), "ids")[0];
    let before = s.model.clone();
    match s.execute("move", &json!({"ids": [loose], "by": [0, 1000], "end": "start"})) {
        Err(e) => assert!(is_conflict(&e) || e.contains("move"), "{e}"),
        // Where the join doesn't pull the grouped wall, nothing in the group changed.
        Ok(_) => assert_eq!(crate::groups::conflicts(&s.model, gid), Vec::<String>::new()),
    }
    let _ = (before, ids);
}

#[test]
fn edit_group_on_a_turned_and_a_mirrored_group_changes_the_others_as_their_type_says() {
    let (mut s, gid, ids) = grouped();
    // The original stays; one copy turned a quarter, one mirrored.
    // (Far enough off that no copy's wall meets another's.)
    run(&mut s, "rotate", json!({"ids": [gid], "about": [-10000, 0], "angle": 90, "copy": true}));
    run(&mut s, "mirror", json!({"ids": [gid], "axis": [[-30000, 0], [-30000, 10]], "copy": true}));
    let all = instances(&s);
    let turned = all[1];
    let turned_walls = crate::groups::members(&s.model, turned);
    // Edit the turned one: its first wall moves out along its own x (the project's -y after the turn).
    run(&mut s, "edit_group", json!({"id": turned}));
    run(&mut s, "move", json!({"ids": [turned_walls[0]], "by": [0, -700]}));
    run(&mut s, "finish_group", json!({}));
    for g in instances(&s) {
        assert_eq!(crate::groups::conflicts(&s.model, g), Vec::<String>::new(), "group {g}");
    }
    // In the type's frame, that is the wall moved 700 toward -x: the original's first wall now starts at x = -700.
    let w = wall(&s, ids[0]);
    assert!(near(w.start, P2(-700.0, 3000.0)) && near(w.end, P2(-700.0, 0.0)), "{:?} {:?}", w.start, w.end);
}

#[test]
fn a_pinned_group_stays_and_a_member_copied_alone_is_no_group_s() {
    let (mut s, gid, ids) = grouped();
    run(&mut s, "pin", json!({"ids": [gid]}));
    let e = err(&mut s, "move", json!({"ids": [gid], "by": [100, 0]}));
    assert!(e.contains("pinned"), "{e}");
    run(&mut s, "unpin", json!({"ids": [gid]}));
    let copied = run(&mut s, "copy", json!({"ids": [ids[1]], "by": [0, -3000]}));
    let copy = ids_of(&copied, "ids").into_iter().find(|i| s.model.wall(*i).is_some()).unwrap_or(0);
    assert_eq!(crate::groups::group_of(&s.model, copy), None);
    assert_eq!(crate::groups::conflicts(&s.model, gid), Vec::<String>::new());
}

#[test]
fn a_member_deleted_in_edit_group_leaves_the_type_and_an_excluded_one_stays_excluded() {
    let (mut s, gid, ids) = grouped();
    run(&mut s, "copy", json!({"ids": [gid], "by": [10000, 0]}));
    let other = instances(&s).into_iter().find(|g| *g != gid).unwrap_or(0);
    // The other has its first wall excluded.
    let theirs = crate::groups::members(&s.model, other);
    run(&mut s, "exclude_member", json!({"ids": [theirs[0]]}));
    // In the first, the last wall is deleted while it is edited.
    run(&mut s, "edit_group", json!({"id": gid}));
    run(&mut s, "delete", json!({"ids": [ids[2]]}));
    run(&mut s, "finish_group", json!({}));
    assert_eq!(s.model.model_groups["Model Group 1"].members.len(), 3, "the deleted wall left the type");
    let theirs = crate::groups::members(&s.model, other);
    assert_eq!(theirs.len(), 2, "the other lost that wall, and keeps its first excluded: {theirs:?}");
    let Some(Element::ModelGroup(o)) = s.model.elements.get(other) else { panic!() };
    assert_eq!(o.members.iter().filter(|x| x.excluded).count(), 1);
    run(&mut s, "restore_excluded", json!({"id": other}));
    assert_eq!(crate::groups::members(&s.model, other).len(), 3);
    for g in instances(&s) {
        assert_eq!(crate::groups::conflicts(&s.model, g), Vec::<String>::new());
    }
    // Undo and redo step through it all.
    for _ in 0..3 {
        run(&mut s, "undo", json!({}));
    }
    for _ in 0..3 {
        run(&mut s, "redo", json!({}));
    }
    assert_eq!(s.model.check(), Ok(()));
}

#[test]
fn a_room_with_its_own_floor_groups_and_copies_with_it() {
    let mut s = Session::new();
    let w = ids_of(&run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0], [4000, 3000], [0, 3000]], "closed": true})), "ids");
    let room = id_of(&run(&mut s, "room", json!({"point": [2000, 1500]})));
    let mut ids = w.clone();
    ids.push(room);
    // Whatever the room has of its own (a floor, a ceiling) goes with it.
    if let Some(Element::Room(r)) = s.model.elements.get(room) {
        for f in [r.floor, r.ceiling] {
            if let Some(g) = f.connected() {
                ids.extend(s.model.elements.id(g));
            }
        }
    }
    let g = id_of(&run(&mut s, "create_group", json!({"ids": ids})));
    run(&mut s, "copy", json!({"ids": [g], "by": [10000, 0]}));
    let rooms: Vec<String> = s
        .model
        .elements
        .values()
        .filter_map(|e| match e {
            Element::Room(r) => Some(r.number.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(rooms.len(), 2);
    assert_ne!(rooms[0], rooms[1], "each room its own number");
    for i in instances(&s) {
        assert_eq!(crate::groups::conflicts(&s.model, i), Vec::<String>::new());
    }
}
