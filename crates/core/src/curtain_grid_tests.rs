//! Corner mullions and a wall's own grid lines, through the commands every front end drives.

use serde_json::{Value, json};

use crate::curtain::{self, Address, Layout};
use crate::engine::Session;
use crate::geom::P2;
use crate::model::{Element, Wall};
use crate::walls::Axis;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn fails(s: &mut Session, id: &str, p: Value) -> String {
    let before = s.model.clone();
    let e = s.execute(id, &p).expect_err(&format!("{id} {p} should fail"));
    assert_eq!(*s.model, *before, "a refused {id} changes nothing");
    e
}

/// A type of 1000 mm columns and rows, with `extra` over it.
fn facade(s: &mut Session, extra: Value) {
    let mut curtain = json!({"columns": {"segments": [1000]}, "rows": {"segments": [1000]}});
    if let (Some(o), Some(e)) = (curtain.as_object_mut(), extra.as_object()) {
        o.extend(e.clone());
    }
    run(s, "type", json!({"category": "wall", "name": "CW", "curtain": curtain}));
}

fn wall_of(s: &Session, id: u64) -> &Wall {
    let Some(Element::Wall(w)) = s.model.elements.get(id) else { panic!("no wall {id}") };
    w
}

fn layout_of(s: &Session, id: u64) -> Layout {
    curtain::layout(&s.model, wall_of(s, id))
}

/// Walls along `points`, of the facade type.
fn chain(s: &mut Session, points: Value) -> Vec<u64> {
    let made = run(s, "wall", json!({"points": points, "type": "CW", "height": 3000}));
    made["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect()
}

fn part(l: &Layout, at: Address) -> Option<&curtain::Part> {
    l.parts.iter().find(|p| p.address == at)
}

fn corner_of(l: &Layout, column: i64) -> Option<&curtain::Part> {
    part(l, Address::Corner { column, row: 0 })
}

/// The corners of a part, from above, on the plan.
fn ring(s: &Session, id: u64, at: Address) -> [P2; 4] {
    curtain::plan_ring(&s.model, wall_of(s, id), at).unwrap()
}

// ---------- corner mullions ----------

#[test]
fn a_project_starts_with_the_two_corner_mullions_of_their_own_family() {
    let s = Session::new();
    assert_eq!(s.model.type_names("cornermullion"), [curtain::SQUARE_CORNER, curtain::RECTANGULAR_CORNER]);
    assert_eq!(s.model.type_names("corner mullion"), [curtain::SQUARE_CORNER, curtain::RECTANGULAR_CORNER]);
    assert_eq!(s.model.default_type("cornermullion"), Some(curtain::RECTANGULAR_CORNER));
    let t = &s.model.types.corner_mullion;
    assert_eq!((t[curtain::RECTANGULAR_CORNER].width(), t[curtain::RECTANGULAR_CORNER].depth()), (50.0, 150.0));
    assert_eq!(t[curtain::RECTANGULAR_CORNER].rotation, 45.0);
    assert_eq!((t[curtain::SQUARE_CORNER].width(), t[curtain::SQUARE_CORNER].depth()), (150.0, 150.0));
    assert_eq!(t[curtain::SQUARE_CORNER].rotation, 0.0);
    assert!(s.model.types.mullion.values().all(|x| x.rotation == 0.0), "the plain family doesn't turn");
    let wall = s.model.types.wall[curtain::CURTAIN_WALL].curtain.as_ref().unwrap();
    assert_eq!(wall.corner.as_deref(), Some(curtain::RECTANGULAR_CORNER));
}

#[test]
fn corner_mullion_types_take_a_rotation_and_refuse_a_bad_one() {
    let mut s = Session::new();
    let out = run(
        &mut s,
        "type",
        json!({"category": "corner mullion", "name": "Skew", "width": 60, "depth": 200, "rotation": 30, "offset_x": 10, "offset_y": 20, "material": "Metal"}),
    );
    assert_eq!(out["type"]["rotation"], json!(30.0));
    // What isn't given stays.
    let out = run(&mut s, "type", json!({"category": "cornermullion", "name": "Skew", "rotation": 60}));
    assert_eq!((out["type"]["rotation"].clone(), out["type"]["offset_y"].clone()), (json!(60.0), json!(20.0)));
    let why = fails(&mut s, "type", json!({"category": "cornermullion", "name": "Skew", "rotation": 500}));
    assert!(why.contains("rotation"), "{why}");
    let why = fails(&mut s, "type", json!({"category": "mullion", "name": "Plain", "width": 50, "depth": 50, "rotation": 45}));
    assert!(why.contains("rotation"), "{why}");
}

#[test]
fn two_curtain_walls_meeting_share_one_corner_mullion_in_place_of_their_borders() {
    let mut s = Session::new();
    facade(&mut s, json!({"corner": curtain::RECTANGULAR_CORNER}));
    let ids = chain(&mut s, json!([[0, 0], [4000, 0], [4000, 4000]]));
    let (a, b) = (layout_of(&s, ids[0]), layout_of(&s, ids[1]));
    let post = corner_of(&a, 1).expect("the wall ending at the corner holds it");
    assert!(corner_of(&b, 0).is_none(), "one mullion, on one wall");
    assert_eq!(a.shapes[post.shape as usize].type_name, curtain::RECTANGULAR_CORNER);
    assert!((post.turn.abs() - 45f64.to_radians()).abs() < 1e-9, "turned 45° from the wall: {}", post.turn);
    assert_eq!(a.shapes[post.shape as usize].size[1], 3000.0, "as tall as the walls");
    // No border mullion at the corner on either wall; those at the far ends stay.
    let last = a.columns.last().unwrap().index + 1;
    assert!(part(&a, Address::Vertical { column: last, row: 0 }).is_none());
    assert!(part(&b, Address::Vertical { column: b.columns[0].index, row: 0 }).is_none());
    assert!(part(&a, Address::Vertical { column: a.columns[0].index, row: 0 }).is_some());
    let listed = run(&mut s, "curtain_parts", json!({"wall": ids[0]}));
    assert!(listed["parts"].as_array().unwrap().iter().any(|p| p["part"] == "corner"), "{listed}");
}

#[test]
fn the_glass_stops_square_against_the_corner_mullion_and_is_never_mitred() {
    let mut s = Session::new();
    facade(&mut s, json!({"corner": curtain::SQUARE_CORNER}));
    let ids = chain(&mut s, json!([[0, 0], [4000, 0], [4000, 4000]]));
    let (a, b) = (layout_of(&s, ids[0]), layout_of(&s, ids[1]));
    let post = ring(&s, ids[0], Address::Corner { column: 1, row: 0 });
    let (axis_a, plane_a) = (Axis::of(wall_of(&s, ids[0])), curtain::grid_plane(&s.model, wall_of(&s, ids[0])));
    let (axis_b, plane_b) = (Axis::of(wall_of(&s, ids[1])), curtain::grid_plane(&s.model, wall_of(&s, ids[1])));
    let reach_a = post.iter().map(|q| axis_a.project(*q).0).fold(f64::MAX, f64::min);
    let reach_b = post.iter().map(|q| axis_b.project(*q).0).fold(f64::MIN, f64::max);
    assert!(plane_a.is_finite() && plane_b.is_finite());
    for (l, stop_end, stop_start) in [(&a, Some(reach_a), None), (&b, None, Some(reach_b))] {
        let panels = l.parts.iter().filter(|p| matches!(p.address, Address::Cell { .. })).collect::<Vec<_>>();
        assert!(!panels.is_empty());
        for p in panels {
            let shape = &l.shapes[p.shape as usize];
            assert!(shape.face.is_none(), "glass is a box, never a mitre");
            assert_eq!(p.turn, 0.0, "and square to its wall");
            let (from, to) = l.along(p);
            if let Some(stop) = stop_end {
                assert!(to <= stop + 1e-6, "panel to {to} past the mullion at {stop}");
            }
            if let Some(stop) = stop_start {
                assert!(from >= stop - 1e-6, "panel from {from} past the mullion at {stop}");
            }
        }
    }
    // It closes up to it: the last panel ends at the mullion's face, not at a border's.
    let last = a.parts.iter().filter(|p| matches!(p.address, Address::Cell { .. })).map(|p| a.along(p).1).fold(f64::MIN, f64::max);
    assert!((last - reach_a).abs() < 1e-6, "{last} against {reach_a}");
}

#[test]
fn the_offsets_put_a_corner_mullion_where_the_mullions_centre_lines_meet_whichever_way_the_corner_turns() {
    for (turn, end) in [("left", [4000, 4000]), ("right", [4000, -4000])] {
        let mut s = Session::new();
        run(
            &mut s,
            "type",
            json!({"category": "cornermullion", "name": "Post", "width": 100, "depth": 100, "offset_x": 50, "offset_y": 50}),
        );
        facade(&mut s, json!({"corner": "Post"}));
        let ids = chain(&mut s, json!([[0, 0], [4000, 0], end]));
        let r = ring(&s, ids[0], Address::Corner { column: 1, row: 0 });
        let centre = (r[0] + r[2]) * 0.5;
        let (wa, wb) = (wall_of(&s, ids[0]), wall_of(&s, ids[1]));
        let on_a = Axis::of(wa).project(centre).1 - curtain::grid_plane(&s.model, wa);
        let on_b = Axis::of(wb).project(centre).1 - curtain::grid_plane(&s.model, wb);
        assert!((on_a - 50.0).abs() < 1e-6, "turning {turn}: {on_a} from the one wall's plane");
        assert!((on_b - 50.0).abs() < 1e-6, "turning {turn}: {on_b} from the other's");
    }
}

#[test]
fn one_corner_takes_another_type_or_none_and_none_gives_the_borders_back() {
    let mut s = Session::new();
    facade(&mut s, json!({"corner": curtain::RECTANGULAR_CORNER}));
    let ids = chain(&mut s, json!([[0, 0], [4000, 0], [4000, 4000]]));
    let at = json!({"wall": ids[0], "part": "corner", "column": 1, "row": 0});
    let with = |mut v: Value, key: &str, x: Value| {
        v[key] = x;
        v
    };
    run(&mut s, "curtain_part", with(at.clone(), "type", json!(curtain::SQUARE_CORNER)));
    let a = layout_of(&s, ids[0]);
    assert_eq!(a.shapes[corner_of(&a, 1).unwrap().shape as usize].type_name, curtain::SQUARE_CORNER);
    assert_eq!(corner_of(&a, 1).unwrap().turn, 0.0);
    let why = fails(&mut s, "curtain_part", with(at.clone(), "type", json!("50 x 150")));
    assert!(why.contains("corner mullion"), "{why}");

    run(&mut s, "curtain_part", with(at.clone(), "type", Value::Null));
    let (a, b) = (layout_of(&s, ids[0]), layout_of(&s, ids[1]));
    assert!(corner_of(&a, 1).is_none());
    assert!(part(&a, Address::Vertical { column: a.columns.last().unwrap().index + 1, row: 0 }).is_some(), "the border is back");
    assert!(part(&b, Address::Vertical { column: b.columns[0].index, row: 0 }).is_some(), "on both walls");

    run(&mut s, "curtain_part", with(at.clone(), "reset", json!(true)));
    assert!(corner_of(&layout_of(&s, ids[0]), 1).is_some(), "as the type says again");
}

#[test]
fn a_corner_mullion_is_only_where_two_curtain_walls_meet_at_an_angle() {
    // In a straight line.
    let mut s = Session::new();
    facade(&mut s, json!({"corner": curtain::RECTANGULAR_CORNER}));
    let ids = chain(&mut s, json!([[0, 0], [4000, 0], [8000, 0]]));
    assert!(corner_of(&layout_of(&s, ids[0]), 1).is_none(), "straight on");
    // A third wall at the node: a T of walls isn't a corner.
    let mut s = Session::new();
    facade(&mut s, json!({"corner": curtain::RECTANGULAR_CORNER}));
    let ids = chain(&mut s, json!([[0, 0], [4000, 0], [4000, 4000]]));
    run(&mut s, "wall", json!({"points": [[4000, 0], [8000, 0]], "height": 3000}));
    assert!(corner_of(&layout_of(&s, ids[0]), 1).is_none(), "three walls");
    // Against a plain wall.
    let mut s = Session::new();
    facade(&mut s, json!({"corner": curtain::RECTANGULAR_CORNER}));
    let ids = chain(&mut s, json!([[0, 0], [4000, 0]]));
    run(&mut s, "wall", json!({"points": [[4000, 0], [4000, 4000]], "height": 3000}));
    assert!(corner_of(&layout_of(&s, ids[0]), 1).is_none(), "a plain wall");
    // A type that names none.
    let mut s = Session::new();
    facade(&mut s, json!({"corner": null}));
    let ids = chain(&mut s, json!([[0, 0], [4000, 0], [4000, 4000]]));
    assert!(corner_of(&layout_of(&s, ids[0]), 1).is_none(), "no corner in the type");
}

#[test]
fn a_corner_mullion_comes_and_goes_with_the_other_wall() {
    let mut s = Session::new();
    facade(&mut s, json!({"corner": curtain::RECTANGULAR_CORNER}));
    let ids = chain(&mut s, json!([[0, 0], [4000, 0], [4000, 4000]]));
    assert!(corner_of(&layout_of(&s, ids[0]), 1).is_some());
    run(&mut s, "delete", json!({"ids": [ids[1]]}));
    assert!(corner_of(&layout_of(&s, ids[0]), 1).is_none());
    let a = layout_of(&s, ids[0]);
    assert!(part(&a, Address::Vertical { column: a.columns.last().unwrap().index + 1, row: 0 }).is_some(), "its border is back");
}

#[test]
fn renaming_a_corner_mullion_type_follows_in_the_pattern_and_the_edits_and_a_used_one_stays() {
    let mut s = Session::new();
    facade(&mut s, json!({"corner": curtain::RECTANGULAR_CORNER}));
    let ids = chain(&mut s, json!([[0, 0], [4000, 0], [4000, 4000]]));
    run(&mut s, "curtain_part", json!({"wall": ids[0], "part": "corner", "column": 1, "row": 0, "type": curtain::SQUARE_CORNER}));
    run(&mut s, "rename_type", json!({"category": "cornermullion", "type": curtain::RECTANGULAR_CORNER, "name": "Diagonal"}));
    run(&mut s, "rename_type", json!({"category": "cornermullion", "type": curtain::SQUARE_CORNER, "name": "Block"}));
    let t = &s.model.types.wall["CW"];
    assert_eq!(t.curtain.as_ref().unwrap().corner.as_deref(), Some("Diagonal"));
    let a = layout_of(&s, ids[0]);
    assert_eq!(a.shapes[corner_of(&a, 1).unwrap().shape as usize].type_name, "Block");
    let why = fails(&mut s, "delete_type", json!({"category": "cornermullion", "type": "Diagonal"}));
    assert!(why.contains("Diagonal") || why.contains("use"), "{why}");
}

#[test]
fn a_corner_mullion_is_drawn_turned_in_a_plan_in_3d_and_in_ifc() {
    let mut s = Session::new();
    facade(&mut s, json!({"corner": curtain::RECTANGULAR_CORNER}));
    let ids = chain(&mut s, json!([[0, 0], [4000, 0], [4000, 4000]]));
    let r = ring(&s, ids[0], Address::Corner { column: 1, row: 0 });
    let side = (r[1] - r[0]).unit();
    assert!((side.0.abs() - side.1.abs()).abs() < 1e-9, "its sides on the diagonal: {side:?}");

    let solids = crate::view3d::solids(&s.model);
    let corners: Vec<[f64; 3]> = solids.iter().flat_map(crate::view3d::Solid::corners).collect();
    let diagonal = corners.iter().any(|p| P2(p[0], p[1]).dist(r[0]) < 1e-6) && corners.iter().any(|p| P2(p[0], p[1]).dist(r[1]) < 1e-6);
    assert!(diagonal, "the scene holds its turned corners");

    let file = crate::ifc::export(&s.model);
    assert!(file.contains("50 x 150 45"), "an IfcMember of its type: {}", file.len());
    assert!(file.lines().any(|l| l.contains("IFCMEMBER(") && l.contains("50 x 150 45")));
}

#[test]
fn the_mullion_schedule_lists_a_corner_mullion() {
    let mut s = Session::new();
    facade(&mut s, json!({"corner": curtain::RECTANGULAR_CORNER}));
    let ids = chain(&mut s, json!([[0, 0], [4000, 0], [4000, 4000]]));
    let table = run(&mut s, "schedule", json!({"name": "Mullions", "category": "mullion"}));
    let text = table.to_string();
    assert!(text.contains(curtain::RECTANGULAR_CORNER), "{text}");
    assert!(ids.len() == 2);
}

// ---------- a wall's own grid lines ----------

fn lines(spans: &[curtain::Span]) -> Vec<f64> {
    spans.iter().skip(1).map(|s| s.from).collect()
}

fn one_wall(s: &mut Session, length: f64) -> u64 {
    let made = run(s, "wall", json!({"points": [[0, 0], [length, 0]], "type": "CW", "height": 3000}));
    made["ids"][0].as_u64().unwrap()
}

#[test]
fn a_grid_added_by_hand_splits_its_cell_and_takes_the_mullion_of_a1() {
    let mut s = Session::new();
    run(&mut s, "type", json!({"category": "mullion", "name": "Fat", "width": 80, "depth": 150}));
    // Two columns, the first line of them Fat.
    let vertical = json!([[{"type": "Fat"}, {"type": "50 x 150"}]]);
    facade(&mut s, json!({"columns": {"segments": [1000, 1000]}, "vertical": vertical}));
    let id = one_wall(&mut s, 4000.0);
    let out = run(&mut s, "curtain_grid", json!({"wall": id, "way": "vertical", "at": 1500}));
    assert_eq!(out["manual"], json!(true));
    assert!(out["id"].as_u64().is_some() && out["guid"].is_string(), "{out}");
    let l = layout_of(&s, id);
    assert_eq!(lines(&l.columns), [1000.0, 1500.0, 2000.0, 3000.0]);
    let manual = l.columns.iter().find(|c| c.from == 1500.0).unwrap();
    assert!(manual.manual);
    let m = part(&l, Address::Vertical { column: manual.index, row: l.rows[0].index }).unwrap();
    assert_eq!(l.shapes[m.shape as usize].type_name, "Fat", "A1's mullion");
    // Both halves hold a panel.
    let cells = l.parts.iter().filter(|p| matches!(p.address, Address::Cell { row, .. } if row == l.rows[0].index)).count();
    assert_eq!(cells, 5);
}

#[test]
fn a_horizontal_grid_takes_a1s_horizontal_mullion() {
    let mut s = Session::new();
    run(&mut s, "type", json!({"category": "mullion", "name": "Sill", "width": 90, "depth": 150}));
    facade(&mut s, json!({"horizontal": [[{"type": "Sill"}]]}));
    let id = one_wall(&mut s, 2000.0);
    run(&mut s, "curtain_grid", json!({"wall": id, "way": "horizontal", "at": 1500}));
    let l = layout_of(&s, id);
    assert_eq!(lines(&l.rows), [1000.0, 1500.0, 2000.0]);
    let row = l.rows.iter().find(|r| r.from == 1500.0).unwrap().index;
    let m = part(&l, Address::Horizontal { column: l.columns[0].index, row }).unwrap();
    assert_eq!(l.shapes[m.shape as usize].type_name, "Sill");
}

#[test]
fn when_the_pattern_changes_a_manual_grid_stays_and_all_the_others_move() {
    let mut s = Session::new();
    facade(&mut s, json!({}));
    let id = one_wall(&mut s, 4000.0);
    run(&mut s, "curtain_grid", json!({"wall": id, "way": "vertical", "at": 1500}));
    assert_eq!(lines(&layout_of(&s, id).columns), [1000.0, 1500.0, 2000.0, 3000.0]);
    run(
        &mut s,
        "type",
        json!({"category": "wall", "name": "CW", "curtain": {"columns": {"segments": [1300]}, "rows": {"segments": [1000]}}}),
    );
    assert_eq!(lines(&layout_of(&s, id).columns), [1300.0, 1500.0, 2600.0, 3900.0]);
}

#[test]
fn moving_a_pattern_grid_makes_it_manual_and_the_pattern_no_longer_moves_it() {
    let mut s = Session::new();
    facade(&mut s, json!({}));
    let id = one_wall(&mut s, 4000.0);
    let l = layout_of(&s, id);
    let line = l.columns[2].index;
    assert!(!l.columns[2].manual, "from the pattern, an auto grid");
    let listed = run(&mut s, "curtain_parts", json!({"wall": id}));
    let grid = listed["grids"].as_array().unwrap().iter().find(|g| g["line"] == line && g["way"] == "vertical").unwrap().clone();
    assert_eq!((grid["at"].clone(), grid["kind"].clone()), (json!(2000.0), json!("auto")));

    let out = run(&mut s, "curtain_grid", json!({"wall": id, "way": "vertical", "line": line, "at": 2300}));
    let moved = out["line"].as_i64().unwrap();
    let l = layout_of(&s, id);
    assert_eq!(lines(&l.columns), [1000.0, 2300.0, 3000.0]);
    assert!(l.columns.iter().find(|c| c.index == moved).unwrap().manual, "no longer auto");
    let listed = run(&mut s, "curtain_parts", json!({"wall": id}));
    let grid = listed["grids"].as_array().unwrap().iter().find(|g| g["line"] == moved).unwrap().clone();
    assert_eq!((grid["at"].clone(), grid["kind"].clone()), (json!(2300.0), json!("manual")));

    // A change of pattern moves the rest and leaves it; the line it came from stays away.
    run(
        &mut s,
        "type",
        json!({"category": "wall", "name": "CW", "curtain": {"columns": {"segments": [1200]}, "rows": {"segments": [1000]}}}),
    );
    assert_eq!(lines(&layout_of(&s, id).columns), [1200.0, 2300.0, 3600.0]);

    // Moved again it's the same grid.
    let again = run(&mut s, "curtain_grid", json!({"wall": id, "way": "vertical", "line": moved, "at": 2500}));
    assert_eq!((again["line"].as_i64(), again["id"].clone()), (Some(moved), out["id"].clone()));
    // Back to auto: the pattern's line returns.
    run(&mut s, "curtain_grid", json!({"wall": id, "way": "vertical", "line": moved, "reset": true}));
    assert_eq!(lines(&layout_of(&s, id).columns), [1200.0, 2400.0, 3600.0]);
    assert!(wall_of(&s, id).curtain.is_none(), "nothing is left of it");
}

#[test]
fn moving_a_grid_keeps_the_edits_and_the_walls_along_it() {
    let mut s = Session::new();
    run(&mut s, "type", json!({"category": "mullion", "name": "Fat", "width": 80, "depth": 150}));
    facade(&mut s, json!({}));
    let id = one_wall(&mut s, 4000.0);
    let l = layout_of(&s, id);
    let (line, row) = (l.columns[2].index, l.rows[0].index);
    run(&mut s, "curtain_part", json!({"wall": id, "part": "vertical", "column": line, "row": row, "type": "Fat"}));
    run(&mut s, "curtain_part", json!({"wall": id, "part": "panel", "column": line, "row": row, "type": "Spandrel"}));
    let plain = run(&mut s, "curtain_panel_wall", json!({"wall": id, "column": line, "row": l.rows[1].index}))["id"].as_u64().unwrap();
    let moved = run(&mut s, "curtain_grid", json!({"wall": id, "way": "vertical", "line": line, "at": 2300}))["line"].as_i64().unwrap();
    let l = layout_of(&s, id);
    let name = |at: Address| l.shapes[part(&l, at).unwrap().shape as usize].type_name.clone();
    assert_eq!(name(Address::Vertical { column: moved, row }), "Fat");
    assert_eq!(name(Address::Cell { column: moved, row }), "Spandrel");
    assert!(s.model.elements.get(plain).is_some(), "the wall in its cell stays");
    assert!(l.walls.iter().any(|h| h.column == moved));
}

#[test]
fn a_moved_grid_keeps_the_panels_either_side_of_it() {
    let mut s = Session::new();
    // Glass, then spandrel, over and over.
    let panels = json!([["Glazed", "Spandrel"]]);
    facade(&mut s, json!({"columns": {"segments": [1000, 1000]}, "panels": panels}));
    let id = one_wall(&mut s, 4000.0);
    let line = layout_of(&s, id).columns[1].index;
    let moved = run(&mut s, "curtain_grid", json!({"wall": id, "way": "vertical", "line": line, "at": 1300}))["line"].as_i64().unwrap();
    let l = layout_of(&s, id);
    let row = l.rows[0].index;
    let name = |column: i64| l.shapes[part(&l, Address::Cell { column, row }).unwrap().shape as usize].type_name.clone();
    assert_eq!(name(l.columns[0].index), "Glazed");
    assert_eq!(name(moved), "Spandrel", "the panel past it stays what it was");
}

#[test]
fn a_grid_is_refused_where_it_cant_be() {
    let mut s = Session::new();
    facade(&mut s, json!({}));
    let id = one_wall(&mut s, 4000.0);
    let line = layout_of(&s, id).columns[1].index;
    for (p, says) in [
        (json!({"wall": id, "way": "vertical", "at": -100}), "along"),
        (json!({"wall": id, "way": "vertical", "at": 4000}), "along"),
        (json!({"wall": id, "way": "vertical", "at": 1e300}), "number"),
        (json!({"wall": id, "way": "vertical", "at": 1010}), "close"),
        (json!({"wall": id, "way": "horizontal", "at": 3000}), "along"),
        (json!({"wall": id, "way": "diagonal", "at": 500}), "way"),
        (json!({"wall": id, "way": "vertical"}), "at"),
        (json!({"wall": id, "way": "vertical", "line": 77777, "at": 1500}), "line"),
        (json!({"wall": id, "way": "vertical", "line": line, "reset": true}), "pattern"),
    ] {
        let why = fails(&mut s, "curtain_grid", p.clone());
        assert!(why.contains(says), "{p}: {why}");
    }
    let plain = run(&mut s, "wall", json!({"points": [[0, 5000], [4000, 5000]], "height": 3000}))["ids"][0].as_u64().unwrap();
    fails(&mut s, "curtain_grid", json!({"wall": plain, "way": "vertical", "at": 500}));
}

#[test]
fn a_manual_grid_past_the_wall_sleeps_and_comes_back() {
    let mut s = Session::new();
    facade(&mut s, json!({}));
    let id = one_wall(&mut s, 4000.0);
    run(&mut s, "curtain_grid", json!({"wall": id, "way": "vertical", "at": 3500}));
    run(&mut s, "modify", json!({"id": id, "end": [3000, 0]}));
    assert_eq!(lines(&layout_of(&s, id).columns), [1000.0, 2000.0]);
    run(&mut s, "modify", json!({"id": id, "end": [4000, 0]}));
    assert_eq!(lines(&layout_of(&s, id).columns), [1000.0, 2000.0, 3000.0, 3500.0]);
}

#[test]
fn a_grid_has_an_id_and_a_guid_and_a_copy_gets_its_own() {
    let mut s = Session::new();
    facade(&mut s, json!({}));
    let id = one_wall(&mut s, 4000.0);
    let out = run(&mut s, "curtain_grid", json!({"wall": id, "way": "vertical", "at": 1500}));
    let made = run(&mut s, "copy", json!({"ids": [id], "by": [0, 5000]}));
    let copy = made["ids"][0].as_u64().unwrap();
    let grid = |w: &Wall| w.curtain.as_ref().unwrap().grids[0].clone();
    let (a, b) = (grid(wall_of(&s, id)), grid(wall_of(&s, copy)));
    assert_eq!(a.id, out["id"].as_u64().unwrap());
    assert_ne!(a.id, b.id);
    assert_ne!(a.guid, b.guid);
    assert_eq!(a.at, b.at);
    assert_eq!(lines(&layout_of(&s, copy).columns), [1000.0, 1500.0, 2000.0, 3000.0]);
}

#[test]
fn a_file_keeps_grids_and_corners_and_reads_them_back() {
    let mut s = Session::new();
    facade(&mut s, json!({"corner": curtain::RECTANGULAR_CORNER}));
    let ids = chain(&mut s, json!([[0, 0], [4000, 0], [4000, 4000]]));
    run(&mut s, "curtain_grid", json!({"wall": ids[0], "way": "vertical", "at": 1500}));
    run(&mut s, "curtain_part", json!({"wall": ids[0], "part": "corner", "column": 1, "row": 0, "type": curtain::SQUARE_CORNER}));
    let text = s.model.to_json().unwrap();
    let back = crate::model::Model::read(&text).unwrap().0;
    let w = back.wall(ids[0]).unwrap();
    let l = curtain::layout(&back, w);
    assert_eq!(lines(&l.columns), [1000.0, 1500.0, 2000.0, 3000.0]);
    assert_eq!(l.shapes[corner_of(&l, 1).unwrap().shape as usize].type_name, curtain::SQUARE_CORNER);
    assert_eq!(back.types.corner_mullion, s.model.types.corner_mullion);
}

#[test]
fn a_file_of_the_format_before_gets_the_corner_mullions() {
    let mut s = Session::new();
    facade(&mut s, json!({"corner": null}));
    let mut v: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
    v["format"] = json!(18);
    v["types"].as_object_mut().unwrap().remove("corner_mullion");
    let back = crate::model::Model::read(&v.to_string()).unwrap().0;
    assert_eq!(back.type_names("cornermullion"), [curtain::SQUARE_CORNER, curtain::RECTANGULAR_CORNER]);
    assert_eq!(back.types.wall["CW"].curtain.as_ref().unwrap().corner, None, "a corner it didn't have stays as it was");
}

#[test]
fn along_a_grid_gives_every_mullion_on_its_line() {
    let mut s = Session::new();
    facade(&mut s, json!({}));
    let id = one_wall(&mut s, 4000.0);
    let l = layout_of(&s, id);
    let (column, row) = (l.columns[2].index, l.rows[1].index);
    let along = l.along_grid(Address::Vertical { column, row });
    assert_eq!(along.len(), l.rows.len());
    assert!(along.iter().all(|a| matches!(a, Address::Vertical { column: c, .. } if *c == column)));
    let across = l.along_grid(Address::Horizontal { column, row });
    assert_eq!(across.len(), l.columns.len());
    assert!(across.iter().all(|a| matches!(a, Address::Horizontal { row: r, .. } if *r == row)));
}

#[test]
fn several_parts_take_a_type_in_one_step() {
    let mut s = Session::new();
    run(&mut s, "type", json!({"category": "mullion", "name": "Fat", "width": 80, "depth": 150}));
    facade(&mut s, json!({}));
    let id = one_wall(&mut s, 4000.0);
    let l = layout_of(&s, id);
    let column = l.columns[2].index;
    let parts: Vec<Value> = l.rows.iter().map(|r| json!({"part": "vertical", "column": column, "row": r.index})).collect();
    run(&mut s, "curtain_part", json!({"wall": id, "parts": parts, "type": "Fat"}));
    let l = layout_of(&s, id);
    for r in &l.rows {
        let p = part(&l, Address::Vertical { column, row: r.index }).unwrap();
        assert_eq!(l.shapes[p.shape as usize].type_name, "Fat");
    }
    run(&mut s, "undo", json!({}));
    let l = layout_of(&s, id);
    assert!(
        l.rows.iter().all(|r| l.shapes[part(&l, Address::Vertical { column, row: r.index }).unwrap().shape as usize].type_name != "Fat"),
        "one step"
    );
}

#[test]
fn what_is_at_a_point_lists_the_mullion_the_grid_and_the_cell() {
    let mut s = Session::new();
    facade(&mut s, json!({}));
    let id = one_wall(&mut s, 4000.0);
    let l = layout_of(&s, id);
    let list = l.at(2010.0, 500.0, 60.0);
    let column = l.columns[2].index;
    let row = l.rows[0].index;
    assert_eq!(
        list,
        [Address::Vertical { column, row }, Address::VerticalGrid { column, row: 0 }, Address::Cell { column, row }],
        "the mullion, its grid, then the panel"
    );
    // A wall's own edge is no grid to move.
    assert!(l.at(10.0, 500.0, 60.0).iter().all(|a| !matches!(a, Address::VerticalGrid { .. })));
    // Grids mark on a plan and on the face of the wall.
    assert!(curtain::plan_ring(&s.model, wall_of(&s, id), Address::VerticalGrid { column, row: 0 }).is_some());
    let face = curtain::face_ring(&s.model, wall_of(&s, id), Address::HorizontalGrid { column: 0, row: l.rows[1].index }).unwrap();
    assert!((face[1][0] - face[0][0]).abs() > 3000.0, "a horizontal grid runs the wall's length");
    assert_eq!(l.grid_at(Address::VerticalGrid { column, row: 0 }), Some(2000.0));
}

#[test]
fn hostile_grid_params_never_panic() {
    let mut s = Session::new();
    facade(&mut s, json!({}));
    let id = one_wall(&mut s, 4000.0);
    for p in [
        json!({"wall": id, "way": "vertical", "at": "x"}),
        json!({"wall": id, "way": 5, "at": 100}),
        json!({"wall": id, "way": "vertical", "line": i64::MAX, "at": 100}),
        json!({"wall": id, "way": "vertical", "line": i64::MIN, "reset": true}),
        json!({"wall": "nope", "way": "vertical", "at": 100}),
        json!({"wall": id, "way": "vertical", "at": f64::MAX}),
    ] {
        fails(&mut s, "curtain_grid", p);
    }
}

// ---------- corners at any angle ----------

/// Whether two convex rings on a plan overlap by more than `slack` mm, by their separating axes.
fn overlap(a: &[P2; 4], b: &[P2; 4], slack: f64) -> bool {
    for ring in [a, b] {
        for i in 0..4 {
            let n = (ring[(i + 1) % 4] - ring[i]).unit().perp();
            let span = |r: &[P2; 4]| r.iter().fold((f64::MAX, f64::MIN), |(lo, hi), q| (lo.min(q.dot(n)), hi.max(q.dot(n))));
            let ((a0, a1), (b0, b1)) = (span(a), span(b));
            if a1 <= b0 + slack || b1 <= a0 + slack {
                return false;
            }
        }
    }
    true
}

/// How far apart two convex rings are on a plan: between their corners and edges.
fn gap(a: &[P2; 4], b: &[P2; 4]) -> f64 {
    let to_edge = |q: P2, s: P2, e: P2| {
        let d = e - s;
        let t = ((q - s).dot(d) / d.dot(d)).clamp(0.0, 1.0);
        q.dist(s + d * t)
    };
    let mut best = f64::MAX;
    for (x, y) in [(a, b), (b, a)] {
        for q in x {
            for i in 0..4 {
                best = best.min(to_edge(*q, y[i], y[(i + 1) % 4]));
            }
        }
    }
    best
}

/// Two walls at `angle` degrees between them, turning left or right, of type `corner`.
fn cornered(corner: &str, angle: f64, turn: f64) -> (Session, Vec<u64>) {
    let mut s = Session::new();
    facade(&mut s, json!({"corner": corner}));
    let d = (180.0 - angle).to_radians() * turn;
    let far = [4000.0 + 4000.0 * d.cos(), 4000.0 * d.sin()];
    let ids = chain(&mut s, json!([[0, 0], [4000, 0], far]));
    (s, ids)
}

/// Each part of a wall on the first row: its ring on the plan, and whether it's a panel.
fn rings_of(s: &Session, id: u64, row_of: usize) -> Vec<([P2; 4], Address)> {
    let l = layout_of(s, id);
    let row = l.rows[row_of].index;
    l.parts
        .iter()
        .filter(|p| match p.address {
            Address::Cell { row: r, .. } | Address::Vertical { row: r, .. } | Address::Horizontal { row: r, .. } => r == row,
            _ => false,
        })
        .filter_map(|p| Some((curtain::plan_ring(&s.model, wall_of(s, id), p.address)?, p.address)))
        .collect()
}

#[test]
fn at_any_angle_the_two_walls_glass_and_mullions_never_overlap_and_the_glass_is_never_turned() {
    let mut wrong = Vec::new();
    for corner in [curtain::RECTANGULAR_CORNER, curtain::SQUARE_CORNER] {
        for turn in [1.0, -1.0] {
            for angle in [20.0, 30.0, 45.0, 60.0, 75.0, 90.0, 105.0, 120.0, 135.0, 150.0, 165.0] {
                let (s, ids) = cornered(corner, angle, turn);
                let a = layout_of(&s, ids[0]);
                assert!(corner_of(&a, 1).is_some(), "{corner} at {angle}°: a mullion stands there");
                let post = ring(&s, ids[0], Address::Corner { column: 1, row: 0 });
                let (ra, rb) = (rings_of(&s, ids[0], 0), rings_of(&s, ids[1], 0));
                for (r, at) in ra.iter().chain(&rb) {
                    // A horizontal mullion mitres with its neighbour through the corner; the rest stop at the post.
                    if !matches!(at, Address::Horizontal { .. }) && overlap(r, &post, 0.01) {
                        wrong.push(format!("{corner} {angle}° turn {turn}: {at:?} runs into the post"));
                    }
                }
                for (x, at) in &ra {
                    for (y, other) in &rb {
                        // A horizontal mullion lies under the glass above it, at another height: only like with like.
                        let bar = |a: &Address| matches!(a, Address::Horizontal { .. });
                        if bar(at) == bar(other) && overlap(x, y, 0.01) {
                            wrong.push(format!("{corner} {angle}° turn {turn}: {at:?} crosses {other:?}"));
                        }
                    }
                }
            }
        }
    }
    assert!(wrong.is_empty(), "{} wrong:\n{}", wrong.len(), wrong.join("\n"));
}

#[test]
fn from_a_right_angle_outward_the_glass_closes_up_to_the_corner_mullion() {
    let mut wrong = Vec::new();
    for corner in [curtain::RECTANGULAR_CORNER, curtain::SQUARE_CORNER] {
        for turn in [1.0, -1.0] {
            for angle in [90.0, 105.0, 120.0, 135.0, 150.0, 165.0] {
                let (s, ids) = cornered(corner, angle, turn);
                let post = ring(&s, ids[0], Address::Corner { column: 1, row: 0 });
                for id in &ids {
                    let nearest = rings_of(&s, *id, 0)
                        .iter()
                        .filter(|(_, at)| matches!(at, Address::Cell { .. }))
                        .map(|(r, _)| gap(r, &post))
                        .fold(f64::MAX, f64::min);
                    if nearest > 1.0 {
                        wrong.push(format!("{corner} {angle}° turn {turn} wall {id}: glass stops {nearest:.1} mm short"));
                    }
                }
            }
        }
    }
    assert!(wrong.is_empty(), "{} wrong:\n{}", wrong.len(), wrong.join("\n"));
}

// ---------- horizontal mullions mitred round a corner ----------

/// The horizontal mullions on the bottom border of each wall, as their rings on the plan.
fn bottom_bars(s: &Session, ids: &[u64]) -> ([P2; 4], [P2; 4], curtain::Shape, curtain::Shape) {
    let (a, b) = (layout_of(s, ids[0]), layout_of(s, ids[1]));
    let (ca, cb) = (a.columns.last().unwrap().index, b.columns[0].index);
    let (ra, rb) = (a.rows[0].index, b.rows[0].index);
    let (at_a, at_b) = (Address::Horizontal { column: ca, row: ra }, Address::Horizontal { column: cb, row: rb });
    let shape = |l: &Layout, at| l.shapes[part(l, at).unwrap().shape as usize].clone();
    (ring(s, ids[0], at_a), ring(s, ids[1], at_b), shape(&a, at_a), shape(&b, at_b))
}

#[test]
fn horizontal_mullions_meet_in_a_mitre_round_a_corner_at_any_usable_angle() {
    for corner in [curtain::RECTANGULAR_CORNER, curtain::SQUARE_CORNER] {
        for turn in [1.0, -1.0] {
            for angle in [45.0, 60.0, 90.0, 120.0, 150.0, 165.0] {
                let (s, ids) = cornered(corner, angle, turn);
                let (x, y, sx, sy) = bottom_bars(&s, &ids);
                assert!(sx.plan.is_some() && sy.plan.is_some(), "{corner} {angle}° turn {turn}: both cut");
                assert!(!overlap(&x, &y, 0.01), "{corner} {angle}° turn {turn}: they don't overlap");
                assert!(gap(&x, &y) < 0.01, "{corner} {angle}° turn {turn}: they touch, {} apart", gap(&x, &y));
                // Both end on one line: the two corners each cut has at the corner are the other's.
                let shared = x.iter().filter(|q| y.iter().any(|o| q.dist(*o) < 0.01)).count();
                assert_eq!(shared, 2, "{corner} {angle}° turn {turn}: one mitre line");
            }
        }
    }
}

#[test]
fn a_mitre_round_a_corner_needs_a_match_on_the_other_wall() {
    // Mullions of another size on the other wall: square.
    let mut s = Session::new();
    run(&mut s, "type", json!({"category": "mullion", "name": "Deep", "width": 50, "depth": 300}));
    facade(&mut s, json!({"corner": curtain::SQUARE_CORNER}));
    let mut other = json!({"category": "wall", "name": "Other", "curtain": {"columns": {"segments": [1000]}, "rows": {"segments": [1000]}, "mullion": "Deep"}});
    other["curtain"]["corner"] = json!(curtain::SQUARE_CORNER);
    run(&mut s, "type", other);
    let a = chain(&mut s, json!([[0, 0], [4000, 0]]))[0];
    let made = run(&mut s, "wall", json!({"points": [[4000, 0], [4000, 4000]], "type": "Other", "height": 3000}));
    let b = made["ids"][0].as_u64().unwrap();
    let (_, _, sx, sy) = bottom_bars(&s, &[a, b]);
    assert!(sx.plan.is_none() && sy.plan.is_none(), "unlike mullions stay square");

    // At another height on the other wall: square, for the one with no neighbour.
    let mut s = Session::new();
    facade(&mut s, json!({"corner": curtain::SQUARE_CORNER}));
    let other = json!({"category": "wall", "name": "Other", "curtain": {"columns": {"segments": [1000]}, "rows": {"segments": [700]}, "corner": curtain::SQUARE_CORNER}});
    run(&mut s, "type", other);
    let a = chain(&mut s, json!([[0, 0], [4000, 0]]))[0];
    let made = run(&mut s, "wall", json!({"points": [[4000, 0], [4000, 4000]], "type": "Other", "height": 3000}));
    let b = made["ids"][0].as_u64().unwrap();
    let la = layout_of(&s, a);
    let row = la.rows[1].index;
    let lone = part(&la, Address::Horizontal { column: la.columns.last().unwrap().index, row }).unwrap();
    assert!(la.shapes[lone.shape as usize].plan.is_none(), "1000 mm up, the other wall has none");
    let (_, _, sx, _) = bottom_bars(&s, &[a, b]);
    assert!(sx.plan.is_some(), "the bottom borders do match");

    // Too sharp a corner would throw a spike out: square.
    let (s, ids) = cornered(curtain::SQUARE_CORNER, 20.0, 1.0);
    let (_, _, sx, _) = bottom_bars(&s, &ids);
    assert!(sx.plan.is_none(), "20° stays square");
}

#[test]
fn a_mitred_horizontal_is_a_closed_solid_in_3d_and_ifc_and_its_length_is_scheduled() {
    let (mut s, ids) = cornered(curtain::SQUARE_CORNER, 120.0, -1.0);
    let (_, _, shape, _) = bottom_bars(&s, &ids);
    let tris = shape.tris();
    assert_eq!(tris.len(), 12, "a closed six-sided solid");
    // Every edge is walked once each way: closed and turned outward.
    let key = |p: [f64; 3]| p.map(|v| (v * 1000.0).round() as i64);
    let mut edges = std::collections::BTreeMap::new();
    for t in &tris {
        for i in 0..3 {
            *edges.entry((key(t[i]), key(t[(i + 1) % 3]))).or_insert(0) += 1;
        }
    }
    assert!(edges.iter().all(|((a, b), n)| *n == 1 && edges.get(&(*b, *a)) == Some(&1)), "{edges:?}");
    // Its volume comes out positive, so its faces face out.
    let volume: f64 = tris
        .iter()
        .map(|[a, b, c]| {
            (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0])) / 6.0
        })
        .sum();
    assert!(volume > 0.0, "{volume}");
    let file = crate::ifc::export(&s.model);
    assert!(file.contains("IFCTRIANGULATEDFACESET"), "it goes to IFC as a face set");
    assert!(!run(&mut s, "schedule", json!({"name": "M", "category": "mullion"})).to_string().is_empty());
}

// ---------- auto rotation ----------

/// How far a corner mullion's width runs from the first wall's heading, degrees: the size of the turn, either way.
fn turned_by(s: &Session, id: u64) -> f64 {
    let l = layout_of(s, id);
    corner_of(&l, 1).unwrap().turn.to_degrees().abs()
}

#[test]
fn auto_rotation_adds_what_the_corner_asks_to_the_rotation_given() {
    for (angle, adds) in [(120.0, 30.0), (90.0, 45.0), (60.0, 60.0), (150.0, 15.0)] {
        for turn in [1.0, -1.0] {
            let mut s = Session::new();
            run(
                &mut s,
                "type",
                json!({"category": "cornermullion", "name": "Auto", "width": 50, "depth": 150, "rotation": 0, "auto_rotation": true, "offset_x": 50, "offset_y": 50}),
            );
            let (mut s, ids) = {
                let d = (180.0_f64 - angle).to_radians() * turn;
                facade(&mut s, json!({"corner": "Auto"}));
                let ids = chain(&mut s, json!([[0, 0], [4000, 0], [4000.0 + 4000.0 * d.cos(), 4000.0 * d.sin()]]));
                (s, ids)
            };
            assert!((turned_by(&s, ids[0]) - adds).abs() < 1e-6, "{angle}° turn {turn}: {}", turned_by(&s, ids[0]));
            // On the diagonal, so either wall's the same.
            let r = ring(&s, ids[0], Address::Corner { column: 1, row: 0 });
            let (u, v) = (
                (wall_of(&s, ids[0]).end - wall_of(&s, ids[0]).start).unit(),
                (wall_of(&s, ids[1]).end - wall_of(&s, ids[1]).start).unit(),
            );
            let along = (r[1] - r[0]).unit().dot((u + v).unit()).abs();
            assert!((along - 1.0).abs() < 1e-9, "{angle}°: square to the corner's diagonal");
            // What's given is added to it.
            run(&mut s, "type", json!({"category": "cornermullion", "name": "Auto", "rotation": 10}));
            assert!((turned_by(&s, ids[0]) - (adds + 10.0)).abs() < 1e-6, "{angle}° turn {turn}: {}", turned_by(&s, ids[0]));
            // Turned off, only what's given.
            run(&mut s, "type", json!({"category": "cornermullion", "name": "Auto", "auto_rotation": false}));
            assert!((turned_by(&s, ids[0]) - 10.0).abs() < 1e-6);
        }
    }
}

#[test]
fn auto_rotation_is_the_corner_familys_and_goes_to_a_file() {
    let mut s = Session::new();
    let why = fails(&mut s, "type", json!({"category": "mullion", "name": "Plain", "width": 50, "depth": 50, "auto_rotation": true}));
    assert!(why.contains("auto_rotation"), "{why}");
    let why = fails(&mut s, "type", json!({"category": "cornermullion", "name": "X", "width": 50, "depth": 50, "auto_rotation": "yes"}));
    assert!(why.contains("auto_rotation"), "{why}");
    run(&mut s, "type", json!({"category": "cornermullion", "name": "X", "width": 50, "depth": 50, "auto_rotation": true}));
    let back = crate::model::Model::read(&s.model.to_json().unwrap()).unwrap().0;
    assert!(back.types.corner_mullion["X"].auto_rotation);
}

/// A plan of the model at 1 px to the mm round the corner, and how to read a pixel of it at a model point: dark or not.
fn inked_on_plan(s: &Session) -> impl Fn(P2) -> bool {
    inked_round(s, P2(3800.0, 200.0))
}

/// Whether a point of the plan is inked, at a pixel a millimetre, 800 mm square round `centre`.
fn inked_round(s: &Session, centre: P2) -> impl Fn(P2) -> bool {
    let level = s.model.levels()[0].0;
    let view = crate::render::View { scale: 1.0, cx: centre.0, cy: centre.1, width: 800, height: 800 };
    let d = crate::render::draw(&s.model, level, view);
    let image = tiny_skia::Pixmap::decode_png(&crate::render::png(&d).unwrap()).unwrap();
    move |q: P2| {
        let (x, y) = view.px(q);
        let c = image.pixel(x.round() as u32, y.round() as u32).unwrap();
        c.red() < 128 && c.green() < 128 && c.blue() < 128
    }
}

#[test]
fn a_plan_draws_a_mitred_horizontal_mullion_cut_not_square() {
    let mut s = Session::new();
    run(&mut s, "type", json!({"category": "cornermullion", "name": "Thin", "width": 10, "depth": 10, "offset_x": 50, "offset_y": 50}));
    facade(&mut s, json!({"corner": "Thin"}));
    let ids = chain(&mut s, json!([[0, 0], [4000, 0], [4000, 4000]]));
    let (x, _, shape, _) = bottom_bars(&s, &ids);
    assert!(shape.plan.is_some());
    // Along the bar's two long faces: one stops short of where the other reaches, at the mitre.
    let wall = wall_of(&s, ids[0]);
    let (axis, _) = (Axis::of(wall), ());
    let along = |q: P2| (q - wall.start).dot((wall.end - wall.start).unit());
    let across = |q: P2| (q - wall.start).dot((wall.end - wall.start).unit().perp());
    let mut near: Vec<P2> = x.to_vec();
    near.sort_by(|a, b| along(*b).total_cmp(&along(*a)));
    let (long, short) = (near[0], near[1]);
    assert!(along(long) - along(short) > 50.0, "the cut runs across the bar: {x:?}");
    let inked = inked_on_plan(&s);
    // The square end's bounding box would run along the short face on to the long one's reach: nothing is drawn there.
    let past = axis.at(along(short) + (along(long) - along(short)) / 2.0, across(short));
    assert!(!inked(past), "nothing past the mitre on its short face, at {past:?}");
    // And the cut itself is drawn, halfway across the bar on the mitre line.
    assert!(inked((long + short) * 0.5), "the mitre line is drawn");
}

#[test]
fn a_corner_mullion_hides_the_mitre_that_runs_under_it() {
    for angle in [60.0, 90.0, 120.0] {
        for turn in [1.0, -1.0] {
            let (mut s, ids) = cornered(curtain::RECTANGULAR_CORNER, angle, turn);
            // At 1:5 the pens are thin enough to tell a line from the fill it would cross.
            run(&mut s, "scale", json!({"view": "Level 1", "scale": 5}));
            let post = ids
                .iter()
                .find_map(|&id| {
                    let l = layout_of(&s, id);
                    let at = l.parts.iter().find(|p| matches!(p.address, Address::Corner { .. }))?.address;
                    Some(ring(&s, id, at))
                })
                .unwrap();
            let middle = crate::geom::centroid(&post);
            // The post's inside, well clear of its own outline.
            let inner: Vec<P2> = post.iter().map(|q| middle + (*q - middle) * 0.8).collect();
            let (x, y, _, _) = bottom_bars(&s, &ids);
            let on_y = |q: &&P2| y.iter().any(|o| (**q - *o).length() < 0.01);
            let mitre: Vec<P2> = x.iter().filter(on_y).copied().collect();
            assert_eq!(mitre.len(), 2, "{angle}° turn {turn}: the bars share their cut: {x:?} {y:?}");
            let inked = inked_round(&s, middle);
            let under: Vec<P2> = (0..=100)
                .map(|k| mitre[0] + (mitre[1] - mitre[0]) * (f64::from(k) / 100.0))
                .filter(|q| crate::geom::contains(&inner, *q))
                .collect();
            assert!(!under.is_empty(), "{angle}° turn {turn}: the mitre runs under the post");
            assert!(under.iter().all(|q| !inked(*q)), "{angle}° turn {turn}: the mitre shows through the post");
        }
    }
}

// ---------- segments of a grid removed, panels merged ----------

/// A wall of 3 columns of 1000 by 3 rows of 1000.
fn three_by_three() -> (Session, u64) {
    let mut s = Session::new();
    facade(&mut s, json!({}));
    let id = chain(&mut s, json!([[0, 0], [3000, 0]]))[0];
    (s, id)
}

fn panels(s: &Session, id: u64) -> Vec<(Address, [f64; 3])> {
    let l = layout_of(s, id);
    l.parts.iter().filter(|p| matches!(p.address, Address::Cell { .. })).map(|p| (p.address, l.shapes[p.shape as usize].size)).collect()
}

fn v(column: i64, row: i64) -> Value {
    json!({"part": "vertical", "column": column, "row": row})
}

fn h(column: i64, row: i64) -> Value {
    json!({"part": "horizontal", "column": column, "row": row})
}

#[test]
fn removing_a_segment_of_a_grid_merges_the_panels_either_side_and_takes_its_mullion_away() {
    let (mut s, id) = three_by_three();
    assert_eq!(panels(&s, id).len(), 9);
    let before = panels(&s, id)[0].1;
    run(&mut s, "curtain_segment", json!({"wall": id, "parts": [v(1, 0)]}));
    let now = panels(&s, id);
    assert_eq!(now.len(), 8, "two became one");
    let merged = now.iter().find(|(a, _)| *a == Address::Cell { column: 0, row: 0 }).unwrap().1;
    assert!(merged[0] > before[0] + 900.0, "it spans both: {merged:?} from {before:?}");
    assert_eq!(merged[1], before[1], "and is as tall as they were");
    let l = layout_of(&s, id);
    assert!(!l.parts.iter().any(|p| p.address == Address::Vertical { column: 1, row: 0 }), "its mullion is gone");
    assert!(l.parts.iter().any(|p| p.address == Address::Vertical { column: 1, row: 1 }), "the rest of the line stands");
    // Along a row too.
    run(&mut s, "curtain_segment", json!({"wall": id, "parts": [h(2, 2)]}));
    let tall = panels(&s, id).iter().find(|(a, _)| *a == Address::Cell { column: 2, row: 1 }).unwrap().1;
    assert!(tall[1] > before[1] + 900.0);
    // Undo, and it's as it was.
    s.execute("undo", &json!({})).unwrap();
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(panels(&s, id).len(), 9);
    assert!(wall_of(&s, id).curtain.is_none());
}

#[test]
fn a_merge_stays_a_rectangle() {
    let (mut s, id) = three_by_three();
    run(&mut s, "curtain_segment", json!({"wall": id, "parts": [v(1, 0)]}));
    let why = fails(&mut s, "curtain_segment", json!({"wall": id, "parts": [h(0, 1)]}));
    assert!(why.contains("rectangle"), "{why}");
    assert_eq!(panels(&s, id).len(), 8, "a refusal changes nothing");
    // What closes it to a rectangle is fine, and what's left inside it goes too.
    run(&mut s, "curtain_segment", json!({"wall": id, "parts": [h(0, 1), h(1, 1)]}));
    assert_eq!(panels(&s, id).len(), 6, "four cells in one");
    assert!(!layout_of(&s, id).parts.iter().any(|p| p.address == Address::Vertical { column: 1, row: 1 }), "nothing runs through it");
}

#[test]
fn restoring_a_segment_splits_a_merged_panel_along_that_line() {
    let (mut s, id) = three_by_three();
    run(&mut s, "curtain_segment", json!({"wall": id, "parts": [v(1, 0), v(1, 1), h(0, 1), h(1, 1)]}));
    assert_eq!(panels(&s, id).len(), 6);
    run(&mut s, "curtain_segment", json!({"wall": id, "parts": [v(1, 0)], "restore": true}));
    assert_eq!(panels(&s, id).len(), 7, "two tall panels now, either side of the whole line");
    let l = layout_of(&s, id);
    for row in [0, 1] {
        assert!(l.parts.iter().any(|p| p.address == Address::Vertical { column: 1, row }), "row {row}");
    }
    let why = fails(&mut s, "curtain_segment", json!({"wall": id, "parts": [v(2, 0)], "restore": true}));
    assert!(why.contains("removed"), "{why}");
}

#[test]
fn only_a_segment_inside_the_wall_can_go() {
    let (mut s, id) = three_by_three();
    for bad in [v(0, 0), v(3, 0), h(0, 0), h(0, 3), v(1, 99), v(99, 0), json!({"part": "panel", "column": 0, "row": 0})] {
        assert!(s.execute("curtain_segment", &json!({"wall": id, "parts": [bad]})).is_err(), "{bad}");
    }
    assert_eq!(panels(&s, id).len(), 9);
}

#[test]
fn a_wall_in_a_cell_and_a_merge_keep_apart() {
    let (mut s, id) = three_by_three();
    run(&mut s, "curtain_panel_wall", json!({"wall": id, "column": 0, "row": 0}));
    let why = fails(&mut s, "curtain_segment", json!({"wall": id, "parts": [v(1, 0)]}));
    assert!(why.contains("wall"), "{why}");
    run(&mut s, "curtain_segment", json!({"wall": id, "parts": [v(2, 1)]}));
    for column in [1, 2] {
        let why = fails(&mut s, "curtain_panel_wall", json!({"wall": id, "column": column, "row": 1}));
        assert!(why.contains("merged"), "{why}");
    }
}

#[test]
fn a_merged_panel_takes_the_first_cell_s_type_and_is_one_part_under_the_cursor() {
    let (mut s, id) = three_by_three();
    let cell = |column, row| json!({"wall": id, "part": "panel", "column": column, "row": row, "type": "Spandrel"});
    run(&mut s, "curtain_part", cell(0, 0));
    run(&mut s, "curtain_segment", json!({"wall": id, "parts": [v(1, 0)]}));
    let l = layout_of(&s, id);
    let p = l.parts.iter().find(|p| p.address == Address::Cell { column: 0, row: 0 }).unwrap();
    assert_eq!(l.shapes[p.shape as usize].type_name, "Spandrel");
    // Over the second cell, or where the line was: the merged panel, and no line.
    let under = l.at(1500.0, 500.0, 10.0);
    assert!(under.contains(&Address::Cell { column: 0, row: 0 }), "{under:?}");
    assert!(!under.contains(&Address::Cell { column: 1, row: 0 }), "{under:?}");
    let on_line = l.at(1000.0, 500.0, 10.0);
    assert!(on_line.contains(&Address::Cell { column: 0, row: 0 }), "{on_line:?}");
    assert!(
        !on_line.iter().any(|a| matches!(a, Address::Vertical { column: 1, .. } | Address::VerticalGrid { column: 1, .. })),
        "{on_line:?}"
    );
    assert_eq!(l.extent(Address::Cell { column: 0, row: 0 }).map(|e| e[1] > 1900.0), Some(true));
}

#[test]
fn a_merge_follows_a_grid_moved_and_sleeps_when_its_cells_go() {
    let (mut s, id) = three_by_three();
    run(&mut s, "curtain_segment", json!({"wall": id, "parts": [v(1, 0)]}));
    // The line moved keeps what was taken off it.
    let line = run(&mut s, "curtain_grid", json!({"wall": id, "way": "vertical", "line": 1, "at": 1200}))["line"].as_i64().unwrap();
    assert_eq!(panels(&s, id).len(), 8);
    let l = layout_of(&s, id);
    assert!(!l.parts.iter().any(|p| p.address == Address::Vertical { column: line, row: 0 }));
    run(&mut s, "curtain_grid", json!({"wall": id, "way": "vertical", "line": line, "reset": true}));
    assert_eq!(panels(&s, id).len(), 8, "and back on its pattern's place");
    // A pattern with fewer columns: what it named is off the wall, and it wakes when they're back.
    let wider = json!({"columns": {"segments": [3000]}, "rows": {"segments": [1000]}});
    run(&mut s, "type", json!({"category": "wall", "name": "CW", "curtain": wider}));
    assert_eq!(panels(&s, id).len(), 3);
    run(
        &mut s,
        "type",
        json!({"category": "wall", "name": "CW", "curtain": {"columns": {"segments": [1000]}, "rows": {"segments": [1000]}}}),
    );
    assert_eq!(panels(&s, id).len(), 8);
}

#[test]
fn a_new_grid_through_a_merged_panel_divides_it_again() {
    let (mut s, id) = three_by_three();
    run(&mut s, "curtain_segment", json!({"wall": id, "parts": [v(1, 0)]}));
    let line = run(&mut s, "curtain_grid", json!({"wall": id, "way": "vertical", "at": 500}))["line"].as_i64().unwrap();
    let l = layout_of(&s, id);
    assert!(l.parts.iter().any(|p| p.address == Address::Vertical { column: line, row: 0 }), "the new grid has its whole line");
    run(&mut s, "curtain_grid", json!({"wall": id, "way": "vertical", "line": line, "reset": true}));
    assert_eq!(panels(&s, id).len(), 8, "off again, the merge is back");
    assert!(
        wall_of(&s, id).curtain.as_ref().unwrap().removed.iter().all(|a| !matches!(a, Address::Vertical { column, .. } if *column == line))
    );
}

#[test]
fn removed_segments_go_through_a_file_and_curtain_parts_lists_them() {
    let (mut s, id) = three_by_three();
    run(&mut s, "curtain_segment", json!({"wall": id, "parts": [v(1, 0)]}));
    let (back, left) = crate::model::Model::read(&s.model.to_json().unwrap()).unwrap();
    assert!(left.is_empty(), "{left:?}");
    let Some(Element::Wall(w)) = back.elements.get(id) else { panic!("no wall") };
    assert_eq!(curtain::layout(&back, w).parts.len(), layout_of(&s, id).parts.len());
    let listed = run(&mut s, "curtain_parts", json!({"wall": id}));
    assert_eq!(listed["removed"][0]["column"], 1, "{listed}");
    assert_eq!(listed["removed"].as_array().unwrap().len(), 1);
}

#[test]
fn the_nearest_segment_under_a_cursor_is_the_cells_nearest_inner_edge() {
    let (s, id) = three_by_three();
    let l = layout_of(&s, id);
    assert_eq!(l.segment_near(1100.0, 1500.0), Some(Address::Vertical { column: 1, row: 1 }));
    assert_eq!(l.segment_near(1500.0, 1950.0), Some(Address::Horizontal { column: 1, row: 2 }));
    // At the wall's edge, the nearest that isn't its border.
    assert_eq!(l.segment_near(20.0, 1500.0), Some(Address::Horizontal { column: 0, row: 1 }));
    assert_eq!(l.segment_near(-500.0, 1500.0), None);
    let [a, b] = curtain::segment_on_face(&s.model, wall_of(&s, id), Address::Vertical { column: 1, row: 1 }).unwrap();
    assert_eq!((a[0], a[2], b[0], b[2]), (1000.0, 1000.0, 1000.0, 2000.0));
}

#[test]
fn a_corner_mitre_goes_square_with_its_partner_gone() {
    // The other wall's bottom row's top line taken off at the corner: this wall's stops square there.
    let mut s = Session::new();
    run(&mut s, "type", json!({"category": "mullion", "name": "M", "width": 50, "depth": 150}));
    let pick = json!({"type": "M", "join": "mitre"});
    facade(&mut s, json!({"corner": null, "horizontal": [[pick]]}));
    let ids = chain(&mut s, json!([[0, 0], [3000, 0], [3000, 3000]]));
    let bar_at = |s: &Session, id: u64, column: i64| {
        let l = layout_of(s, id);
        let p = l.parts.iter().find(|p| p.address == Address::Horizontal { column, row: 1 }).unwrap();
        l.shapes[p.shape as usize].plan.is_some()
    };
    assert!(bar_at(&s, ids[0], 2) && bar_at(&s, ids[1], 0), "mitred to start with");
    run(&mut s, "curtain_segment", json!({"wall": ids[1], "parts": [h(0, 1)]}));
    assert!(!bar_at(&s, ids[0], 2), "with nothing to meet it stops square");
}

// ---------- a continuous mullion is drawn as one ----------

#[test]
fn a_mullion_running_through_its_crossings_is_drawn_as_one_piece_with_no_joints() {
    let (s, id) = three_by_three();
    let l = layout_of(&s, id);
    let is_on = |p: &curtain::Piece, kind: fn(Address) -> bool| kind(l.parts[p.first].address);
    let pieces = l.pieces();
    let vertical: Vec<&curtain::Piece> = pieces.iter().filter(|p| is_on(p, |a| matches!(a, Address::Vertical { .. }))).collect();
    // Four lines up the wall, each one piece the wall's height; there are three parts on each, to pick apart.
    assert_eq!(vertical.len(), 4, "{vertical:?}");
    assert!(vertical.iter().all(|p| (p.size[1] - 3000.0).abs() < 1.0), "{vertical:?}");
    assert_eq!(l.parts.iter().filter(|p| matches!(p.address, Address::Vertical { .. })).count(), 12);
    // Horizontal ones stop at each vertical, so they stay as they are: a joint between them is a real one.
    let horizontal = pieces.iter().filter(|p| is_on(p, |a| matches!(a, Address::Horizontal { .. }))).count();
    assert_eq!(horizontal, l.parts.iter().filter(|p| matches!(p.address, Address::Horizontal { .. })).count());
}

#[test]
fn a_part_of_another_type_or_a_removed_segment_breaks_the_piece() {
    let (mut s, id) = three_by_three();
    run(&mut s, "type", json!({"category": "mullion", "name": "Thick", "width": 80, "depth": 150}));
    run(&mut s, "curtain_part", json!({"wall": id, "part": "vertical", "column": 1, "row": 1, "type": "Thick"}));
    run(&mut s, "curtain_segment", json!({"wall": id, "parts": [v(2, 1)]}));
    let l = layout_of(&s, id);
    let on = |column: i64| {
        l.pieces().iter().filter(|p| matches!(l.parts[p.first].address, Address::Vertical { column: c, .. } if c == column)).count()
    };
    assert_eq!(on(1), 3, "another type in the middle");
    assert_eq!(on(2), 2, "a gap in the middle");
    assert_eq!(on(3), 1);
}

#[test]
fn a_section_shows_no_line_across_a_continuous_mullion_where_two_runs_meet() {
    let (mut s, id) = three_by_three();
    let view = run(&mut s, "section", json!({"start": [-500, -1500], "end": [3500, -1500], "name": "S"}))["id"].as_u64().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.png");
    run(&mut s, "render", json!({"view": view, "path": path, "width": 1600, "height": 1600}));
    let image = tiny_skia::Pixmap::load_png(&path).unwrap();
    let dark = |x: u32, y: u32| {
        let c = image.pixel(x, y).unwrap();
        c.red() < 128 && c.green() < 128 && c.blue() < 128
    };
    // Up the middle of the mullion on line 1: find it as the two long lines close together, left of the wall's middle.
    let long = |x: u32| (0..1600).filter(|y| dark(x, *y)).count() > 500;
    let lines: Vec<u32> = (0..1600).filter(|x| long(*x)).collect();
    let (a, b) =
        lines.windows(2).filter(|w| w[1] > w[0] + 2 && w[1] - w[0] < 30).map(|w| (w[0], w[1])).nth(1).expect("a mullion's two faces");
    let middle = (a + b) / 2;
    let crossed = (0..1600).filter(|y| dark(middle, *y) && !dark(middle, *y - 1)).count();
    assert!(crossed <= 2, "up a mullion only its two ends cross it, not {crossed} lines (id {id})");
}

#[test]
fn a_grid_line_picked_shows_only_the_stretches_still_on_it() {
    let (mut s, id) = three_by_three();
    let grid = Address::VerticalGrid { column: 1, row: 0 };
    assert_eq!(curtain::face_rings(&s.model, wall_of(&s, id), grid).len(), 1, "the whole line, in one");
    run(&mut s, "curtain_segment", json!({"wall": id, "parts": [v(1, 1)]}));
    let rings = curtain::face_rings(&s.model, wall_of(&s, id), grid);
    assert_eq!(rings.len(), 2, "either side of the merged panel");
    assert_eq!((rings[0][0][2], rings[0][2][2]), (0.0, 1000.0));
    assert_eq!((rings[1][0][2], rings[1][2][2]), (2000.0, 3000.0));
}
