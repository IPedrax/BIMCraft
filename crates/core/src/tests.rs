//! The engine driven the way every front end drives it: commands with JSON params, and checked
//! through commands too.

use std::sync::Arc;

use serde_json::{Value, json};

use crate::engine::{Session, commands};
use crate::geom::P2;
use crate::model::{Facing, Model};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// The id of the element a reference in an element's JSON (its GUID) points at.
fn id_of(s: &Session, v: &Value) -> Option<u64> {
    serde_json::from_value::<crate::model::Guid>(v.clone()).ok().and_then(|g| s.model.elements.id(g))
}

fn count(s: &mut Session, category: &str) -> u64 {
    run(s, "elements", json!({"category": category}))["count"].as_u64().unwrap()
}

/// An 8 x 6 m box of walls on Level 1; the ids of its walls, bottom one first.
fn house(s: &mut Session) -> Vec<u64> {
    let r = run(s, "wall", json!({"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true}));
    r["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect()
}

#[test]
fn a_closed_wall_chain_makes_one_wall_per_side_up_to_the_next_level() {
    let mut s = Session::new();
    assert_eq!(house(&mut s).len(), 4);
    let w = run(&mut s, "elements", json!({"category": "walls"}));
    let first = &w["elements"][0];
    assert_eq!(first["length"], 8000.0);
    assert_eq!(first["height"], 3000.0);
    assert_eq!(id_of(&s, &first["top_level"]), Some(100_001), "Level 2");
}

#[test]
fn doors_and_windows_find_the_wall_under_a_point() {
    let mut s = Session::new();
    let ids = house(&mut s);
    let d = run(&mut s, "door", json!({"at": [2000, 30]}));
    assert_eq!((id_of(&s, &d["host"]), d["offset"].as_f64(), d["mark"].as_str()), (Some(ids[0]), Some(2000.0), Some("1")));
    let w = run(&mut s, "WN", json!({"at": [8000, 3000]}));
    assert_eq!(id_of(&s, &w["host"]), Some(ids[1]));
    let e = s.execute("door", &json!({"at": [4000, 3000]})).unwrap_err();
    assert!(e.contains("no wall"), "{e}");
}

#[test]
fn a_door_or_window_is_sketched_as_the_command_would_make_it() {
    let mut s = Session::new();
    house(&mut s);
    let view = crate::render::View { scale: 0.1, cx: 4000.0, cy: 3000.0, width: 1000, height: 800 };
    for (is_door, at) in [(true, [2000, 30]), (false, [5000, 0])] {
        let p = json!({"at": at, "level": 100000});
        let sketched = crate::opening_of(&s.model, &p, is_door).unwrap();
        let d = crate::render::sketch(&s.model, view, 999, &sketched);
        assert!(d.owners.iter().all(|o| *o == Some(999)));
        assert!(d.prims.iter().any(|p| matches!(p, crate::render::Prim::Fill { .. })), "the gap it leaves in the wall");
        assert!(d.prims.len() >= 4, "the gap, its jambs and its swing or glass: {}", d.prims.len());
        let made = run(&mut s, if is_door { "door" } else { "window" }, p);
        assert_eq!(s.model.elements[&made["id"].as_u64().unwrap()], sketched, "the same element the click makes");
    }
    assert!(crate::opening_of(&s.model, &json!({"at": [4000, 3000], "level": 100000}), true).unwrap_err().contains("no wall"));
}

#[test]
fn a_sketch_draws_its_pens_as_wide_as_the_plan_it_is_in_at_any_zoom() {
    let mut s = Session::new();
    house(&mut s);
    let sketched = crate::opening_of(&s.model, &json!({"at": [2000, 30], "level": 100000}), true).unwrap();
    let widest = |zoom: f64| {
        let view = crate::render::View { scale: zoom, cx: 4000.0, cy: 3000.0, width: 1000, height: 800 };
        let d = crate::render::sketch(&s.model, view, 999, &sketched);
        d.prims.iter().filter_map(|p| if let crate::render::Prim::Line { width, .. } = p { Some(*width) } else { None }).fold(0.0, f32::max)
    };
    // Zoomed in twice as far, the paper the plan stands for is twice the pixels, and so is every pen.
    let (near, far) = (widest(0.2), widest(0.1));
    assert!(far > 0.0 && (near - 2.0 * far).abs() < 1e-4, "{near} at 0.2 px to the mm, {far} at 0.1");
}

#[test]
fn a_point_knows_how_far_it_stands_from_the_nearest_wall_centre_line_or_grid_each_way() {
    let mut s = Session::new();
    // Walls 100002 (south), 100003 (east), 100004 (north) and 100005 (west).
    house(&mut s);
    let reach =
        |s: &Session, at: P2| s.model.clearances(100000, at).map(|c| c.map(|c| (c.distance, s.model.elements.id(c.to.id).unwrap())));
    // East, north, west, south.
    assert_eq!(
        reach(&s, P2(3000.0, 2000.0)),
        [Some((5000.0, 100003)), Some((4000.0, 100004)), Some((3000.0, 100005)), Some((2000.0, 100002))]
    );
    // A grid nearer than the wall takes its place; one that doesn't reach the point's row is passed over.
    let g = run(&mut s, "grid", json!({"start": [1000, -500], "end": [1000, 6500]}))["id"].as_u64().unwrap();
    run(&mut s, "grid", json!({"start": [2000, 3000], "end": [2000, 4000]}));
    assert_eq!(reach(&s, P2(3000.0, 2000.0))[2], Some((2000.0, g)));
    assert_eq!(reach(&s, P2(3000.0, 2000.0))[0], Some((5000.0, 100003)));
    // Standing on a line, it is that line's nearest one on neither side: the next one is.
    assert_eq!(reach(&s, P2(1000.0, 2000.0))[2], Some((1000.0, 100005)));
    // Outside the box only the way back in finds a wall, and the lines that end short of it find none.
    assert_eq!(reach(&s, P2(9000.0, 3000.0)), [None, None, Some((1000.0, 100003)), None]);
    // Which way each looks, and that a clearance is the distance along it to its line.
    let c = s.model.clearances(100000, P2(3000.0, 2000.0));
    assert_eq!(
        [c[0], c[1], c[2], c[3]].map(|c| c.map(|c| c.toward)),
        [Some(P2(1.0, 0.0)), Some(P2(0.0, 1.0)), Some(P2(-1.0, 0.0)), Some(P2(0.0, -1.0))]
    );
}

#[test]
fn a_door_stands_between_the_faces_of_the_walls_that_meet_its_wall() {
    use crate::model::Element;
    let mut s = Session::new();
    house(&mut s);
    let span = |s: &mut Session, at: [i32; 2]| {
        let id = run(s, "door", json!({"at": at}))["id"].as_u64().unwrap();
        let Some(Element::Door(o)) = s.model.elements.get(id) else { panic!("no door {id}") };
        let span = s.model.opening_span(o);
        run(s, "delete", json!({"ids": [id]}));
        span
    };
    // Between the inner faces of the west and east walls, 200 thick on the corners of its location line.
    assert_eq!(span(&mut s, [2000, 0]), Some((100.0, 7900.0)));
    // A wall crossing the room stops it at its nearer face; one whose line only would cross, short of the wall, doesn't.
    run(&mut s, "wall", json!({"points": [[5000, 0], [5000, 6000]], "type": "Interior - 100mm"}));
    run(&mut s, "wall", json!({"points": [[3000, 3000], [3000, 6000]], "type": "Interior - 100mm"}));
    assert_eq!(span(&mut s, [2000, 0]), Some((100.0, 4950.0)));
    assert_eq!(span(&mut s, [6500, 0]), Some((5050.0, 7900.0)));
    // A wall meeting it from outside, up to its face, counts too.
    run(&mut s, "wall", json!({"points": [[1000, -2000], [1000, -100]], "type": "Interior - 100mm"}));
    assert_eq!(span(&mut s, [2000, 0]), Some((1050.0, 4950.0)));
}

#[test]
fn a_door_near_the_middle_between_the_faces_finds_the_middle() {
    use crate::model::Element;
    let mut s = Session::new();
    house(&mut s);
    run(&mut s, "wall", json!({"points": [[5000, 0], [5000, 6000]], "type": "Interior - 100mm"}));
    let id = run(&mut s, "door", json!({"at": [2560, 0]}))["id"].as_u64().unwrap();
    let Some(Element::Door(o)) = s.model.elements.get(id) else { panic!("no door {id}") };
    // Halfway between 100 and 4950.
    assert_eq!(s.model.centred(o, 50.0), Some(2525.0));
    assert_eq!(s.model.centred(o, 30.0), None, "35 away is further than 30");
}

#[test]
fn a_views_scale_and_detail_level_are_its_own_set_by_commands_and_kept_in_the_file() {
    let mut s = Session::new();
    house(&mut s);
    // The section is 7, looking north from the south.
    run(&mut s, "section", json!({"start": [-1000, 3000], "end": [9000, 3000]}));
    let g = |s: &Session, id| crate::graphics::graphics_of(&s.model, id).cloned().unwrap();
    // Asking changes nothing, and is no undo step: a view is at 1 : 100 until one is set, and draws as fine.
    let steps = s.history().len();
    let r = run(&mut s, "scale", json!({"view": "Level 1"}));
    assert_eq!((id_of(&s, &r["view"]), &r["scale"]), (Some(100000), &json!(100.0)));
    assert_eq!(run(&mut s, "detail", json!({"view": 100000}))["detail"], "fine");
    assert_eq!(s.history().len(), steps);
    // Set, on a plan and on a section, each one undo step; what the view hides stays hidden.
    run(&mut s, "graphics", json!({"view": 100000, "hide": ["door"]}));
    assert_eq!(run(&mut s, "scale", json!({"view": "Level 1", "scale": 50}))["scale"], 50.0);
    assert_eq!(run(&mut s, "detail", json!({"view": 100000, "detail": "coarse"}))["detail"], "coarse");
    run(&mut s, "scale", json!({"view": 100006, "scale": 20.5}));
    run(&mut s, "detail", json!({"view": "Section 1", "detail": "Medium"}));
    assert_eq!(
        (g(&s, 100000).scale, g(&s, 100000).detail, g(&s, 100000).hides("door")),
        (Some(50.0), crate::graphics::Detail::Coarse, true)
    );
    assert_eq!((g(&s, 100006).scale, g(&s, 100006).detail), (Some(20.5), crate::graphics::Detail::Medium));
    assert_eq!(&s.history()[steps + 1..], ["View Scale", "Detail Level", "View Scale", "Detail Level"]);
    // A view with the scale it already has changes nothing.
    run(&mut s, "scale", json!({"view": 100000, "scale": 50}));
    assert_eq!(s.history().len(), steps + 5);
    // Saved and opened again, the same; a view always has a scale, so null is refused.
    let project = temp("view-settings.bimcraft");
    run(&mut s, "save", json!({"path": project}));
    let saved = s.model.clone();
    run(&mut s, "open", json!({"path": project}));
    assert_eq!(*s.model, *saved);
    let _ = std::fs::remove_file(project);
    assert!(s.execute("scale", &json!({"view": 100000, "scale": null})).unwrap_err().contains("always has a scale"));
    assert_eq!(g(&s, 100000).scale, Some(50.0));
    // What isn't a scale or a level of detail, or a view that has one, is refused and changes nothing.
    let was = s.model.clone();
    for (cmd, p) in [
        ("scale", json!({"view": 100000, "scale": 0})),
        ("scale", json!({"view": 100000, "scale": -100})),
        ("scale", json!({"view": 100000, "scale": 1e9})),
        ("scale", json!({"view": 100000, "scale": "1:100"})),
        ("scale", json!({"view": 100002, "scale": 100})),
        ("detail", json!({"view": 100000, "detail": "ultra"})),
        ("detail", json!({"view": 100000, "detail": 3})),
        ("detail", json!({"view": 100002, "detail": "fine"})),
    ] {
        let e = s.execute(cmd, &p).unwrap_err();
        assert!(e.contains(cmd), "{cmd} {p}: {e}");
    }
    assert!(s.execute("detail", &json!({"view": 100000, "detail": "ultra"})).unwrap_err().contains("coarse, medium or fine"));
    assert!(Arc::ptr_eq(&s.model, &was));
    // A project that holds a scale it can't (a hand-edited file) is refused whole.
    let mut m = (*s.model).clone();
    if let Some(crate::model::Element::Level(l)) = m.elements.get_mut(100000) {
        l.graphics.scale = Some(0.0);
    }
    assert!(m.check().unwrap_err().contains("scale"));
}

#[test]
fn a_view_in_feet_and_inches_is_at_an_eighth_of_an_inch_to_the_foot_until_given_a_scale() {
    let mut s = Session::new();
    house(&mut s);
    run(&mut s, "units", json!({"length": "ft-in"}));
    assert_eq!(run(&mut s, "scale", json!({"view": 100000}))["scale"], 96.0);
    run(&mut s, "sheet", json!({}));
    let r = run(&mut s, "place", json!({"sheet": "A101", "view": 100000}));
    assert!(matches!(&s.model.elements[&id_of(&s, &r["id"]).unwrap()], crate::model::Element::Viewport(v) if v.scale == 96.0));
    // Set, it is the view's own, whatever the units.
    run(&mut s, "scale", json!({"view": 100000, "scale": 48}));
    run(&mut s, "units", json!({"length": "mm"}));
    assert_eq!(run(&mut s, "scale", json!({"view": 100000}))["scale"], 48.0);
}

#[test]
fn a_view_placed_on_a_sheet_takes_the_scale_it_has_unless_it_is_told_another() {
    let mut s = Session::new();
    house(&mut s);
    run(&mut s, "section", json!({"start": [-1000, 3000], "end": [9000, 3000]}));
    run(&mut s, "scale", json!({"view": 100000, "scale": 50}));
    run(&mut s, "sheet", json!({}));
    // The plan's viewport takes its 1 : 50, the section's, with none, the usual 1 : 100.
    let vp = |s: &mut Session, view: u64, scale: Option<u64>| {
        let mut p = json!({"sheet": "A101", "view": view});
        if let Some(n) = scale {
            p["scale"] = json!(n);
        }
        let id = run(s, "place", p)["id"].as_u64().unwrap();
        run(s, "elements", json!({"category": "viewport"}))["elements"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["id"].as_u64() == Some(id))
            .map(|e| e["scale"].clone())
            .unwrap()
    };
    assert_eq!(vp(&mut s, 100_000, None), json!(50.0));
    assert_eq!(vp(&mut s, 100_006, None), json!(100.0));
    let other = run(&mut s, "section", json!({"start": [-1000, 2000], "end": [9000, 2000]}))["id"].as_u64().unwrap();
    run(&mut s, "scale", json!({"view": other, "scale": 20}));
    assert_eq!(vp(&mut s, other, Some(200)), json!(200.0), "told another, it is that");
}

#[test]
fn the_3d_view_has_a_visual_style_that_a_command_sets_and_the_file_keeps() {
    let mut s = Session::new();
    house(&mut s);
    // Asking makes no 3D view, and says it is shaded.
    assert_eq!(run(&mut s, "visual_style", json!({}))["style"], "shaded");
    assert!(s.model.default_3d().is_none());
    let steps = s.history().len();
    // Setting one makes the default view, with its style in its graphics, as one undo step.
    assert_eq!(run(&mut s, "visual_style", json!({"style": "wireframe"}))["style"], "wireframe");
    let view = s.model.default_3d().unwrap();
    let style = |s: &Session| crate::graphics::graphics_of(&s.model, view).unwrap().style;
    assert_eq!(style(&s), crate::graphics::VisualStyle::Wireframe);
    assert_eq!(s.history().len(), steps + 1);
    assert_eq!(s.history().last(), Some(&"Visual Style"));
    // Any case, spaces for underscores; the view by name or id; the one it has changes nothing.
    assert_eq!(run(&mut s, "visual_style", json!({"style": "Hidden Line", "view": "{3D}"}))["style"], "hidden_line");
    assert_eq!(run(&mut s, "visual_style", json!({"style": "HIDDENLINE", "view": view}))["style"], "hidden_line");
    assert_eq!(s.history().len(), steps + 2);
    run(&mut s, "graphics", json!({"view": "{3D}", "hide": ["door"]}));
    assert!(crate::graphics::graphics_of(&s.model, view).unwrap().hides("door"), "what it hides is kept apart from its style");
    // Undo takes it back, redo returns it; saved and opened again, the same.
    run(&mut s, "undo", json!({}));
    run(&mut s, "undo", json!({}));
    assert_eq!(style(&s), crate::graphics::VisualStyle::Wireframe);
    run(&mut s, "redo", json!({}));
    run(&mut s, "redo", json!({}));
    assert_eq!(style(&s), crate::graphics::VisualStyle::HiddenLine);
    let project = temp("visual-style.bimcraft");
    run(&mut s, "save", json!({"path": project}));
    let saved = s.model.clone();
    run(&mut s, "open", json!({"path": project}));
    assert_eq!(*s.model, *saved);
    let _ = std::fs::remove_file(project);
    // Not a style, and not a 3D view: refused, and nothing changes.
    let was = s.model.clone();
    for p in [
        json!({"style": "x-ray"}),
        json!({"style": 7}),
        json!({"style": "shaded", "view": 100000}),
        json!({"style": "shaded", "view": 100002}),
    ] {
        let e = s.execute("visual_style", &p).unwrap_err();
        assert!(e.contains("visual_style"), "{p}: {e}");
    }
    assert!(s.execute("visual_style", &json!({"style": "x-ray"})).unwrap_err().contains("shaded, hidden_line or wireframe"));
    assert!(Arc::ptr_eq(&s.model, &was));
}

#[test]
fn a_failed_command_changes_nothing() {
    let mut s = Session::new();
    let ids = house(&mut s);
    let before = s.model.clone();
    let e = s.execute("door", &json!({"wall": ids[0], "offset": 100})).unwrap_err();
    assert!(e.contains("fit"), "{e}");
    assert_eq!(*s.model, *before);
    // The first wall of this chain is fine, the second one has no length.
    assert!(s.execute("wall", &json!({"points": [[0, 0], [1000, 0], [1000, 0]]})).is_err());
    assert_eq!(*s.model, *before);
}

#[test]
fn undo_and_redo_step_through_commands() {
    let mut s = Session::new();
    house(&mut s);
    run(&mut s, "door", json!({"at": [2000, 0]}));
    assert_eq!(run(&mut s, "undo", json!({}))["undone"], "Door");
    assert_eq!((count(&mut s, "door"), count(&mut s, "wall")), (0, 4));
    assert_eq!(run(&mut s, "redo", json!({}))["redone"], "Door");
    assert_eq!(count(&mut s, "door"), 1);
    assert!(s.execute("redo", &json!({})).is_err());
}

#[test]
fn rooms_need_walls_around_them_and_follow_the_walls() {
    let mut s = Session::new();
    house(&mut s);
    let e = s.execute("room", &json!({"point": [9000, 1000]})).unwrap_err();
    assert!(e.contains("enclosed"), "{e}");
    let r = run(&mut s, "RM", json!({"point": [4000, 3000], "name": "Living"}));
    assert_eq!((r["number"].as_str(), r["area_m2"].as_f64()), (Some("1"), Some(48.0)));
    // A partition splits the box; the room keeps the part around its point.
    run(&mut s, "wall", json!({"points": [[5000, 0], [5000, 6000]], "type": "Interior - 100mm"}));
    let rooms = run(&mut s, "elements", json!({"category": "room"}));
    assert_eq!(rooms["elements"][0]["area_m2"], 30.0);
}

#[test]
fn floors_default_to_the_outline_of_the_walls() {
    let mut s = Session::new();
    house(&mut s);
    assert_eq!(run(&mut s, "floor", json!({}))["area_m2"], 48.0);
    let e = s.execute("floor", &json!({"level": "Level 2"})).unwrap_err();
    assert!(e.contains("boundary"), "{e}");
}

#[test]
fn modify_changes_fields_and_refuses_typos() {
    let mut s = Session::new();
    let ids = house(&mut s);
    run(&mut s, "modify", json!({"id": ids[0], "type": "Exterior - 300mm"}));
    let w = run(&mut s, "elements", json!({"category": "wall"}));
    assert_eq!(w["elements"][0]["thickness"], 300.0);
    let e = s.execute("modify", &json!({"id": ids[0], "hieght": 2000})).unwrap_err();
    assert!(e.contains("hieght"), "{e}");
    let e = s.execute("modify", &json!({"id": ids[0], "category": "floor"})).unwrap_err();
    assert!(e.contains("category"), "{e}");
    run(&mut s, "modify", json!({"id": ids[0], "top_level": null, "height": 2500, "level": "Level 1"}));
    let w = run(&mut s, "elements", json!({"category": "wall"}));
    assert_eq!(w["elements"][0]["height"], 2500.0);
}

/// A wall's ends, as `elements` shows them.
fn ends(s: &mut Session, id: u64) -> (Value, Value) {
    let w = run(s, "elements", json!({"category": "wall"}));
    let w = w["elements"].as_array().unwrap().iter().find(|e| e["id"] == id).cloned().unwrap();
    (w["start"].clone(), w["end"].clone())
}

fn element(s: &mut Session, id: u64) -> Value {
    let all = run(s, "elements", json!({}));
    all["elements"].as_array().unwrap().iter().find(|e| e["id"] == id).cloned().unwrap()
}

#[test]
fn moving_a_wall_drags_the_walls_joined_to_it_and_the_rooms_follow() {
    let mut s = Session::new();
    let ids = house(&mut s);
    let part = run(&mut s, "wall", json!({"points": [[0, 3000], [8000, 3000]], "type": "Interior - 100mm"}))["ids"][0].as_u64().unwrap();
    let room = run(&mut s, "room", json!({"point": [4000, 1500]}))["id"].as_u64().unwrap();
    let door = run(&mut s, "door", json!({"at": [2000, 0]}))["id"].as_u64().unwrap();
    let window = run(&mut s, "window", json!({"at": [8000, 1500]}))["id"].as_u64().unwrap();
    assert_eq!(element(&mut s, room)["area_m2"], 24.0);
    run(&mut s, "MV", json!({"ids": [ids[1]], "by": [1000, 0]}));
    assert_eq!(ends(&mut s, ids[1]), (json!([9000.0, 0.0]), json!([9000.0, 6000.0])));
    assert_eq!(ends(&mut s, ids[0]).1, json!([9000.0, 0.0]), "the corners followed");
    assert_eq!(ends(&mut s, ids[2]).0, json!([9000.0, 6000.0]));
    assert_eq!(ends(&mut s, part).1, json!([9000.0, 3000.0]), "the partition, joined mid-wall, stretched");
    assert_eq!(element(&mut s, room)["area_m2"], 27.0);
    assert_eq!(element(&mut s, door)["at"], json!([2000.0, 0.0]), "a door in a stretched wall stays put");
    assert_eq!(element(&mut s, window)["at"], json!([9000.0, 1500.0]), "a window in the moved wall goes with it");
    // A wall on another level, right above, isn't joined to these.
    let up = run(&mut s, "wall", json!({"points": [[9000, 6000], [9000, 9000]], "level": "Level 2", "height": 3000}))["ids"][0]
        .as_u64()
        .unwrap();
    run(&mut s, "move", json!({"ids": [ids[1]], "by": [-1000, 0]}));
    assert_eq!(ends(&mut s, up).0, json!([9000.0, 6000.0]));
}

#[test]
fn moving_one_end_of_a_wall_stretches_it_and_the_walls_joined_there() {
    let mut s = Session::new();
    let ids = house(&mut s);
    let door = run(&mut s, "door", json!({"at": [6000, 0]}))["id"].as_u64().unwrap();
    // The bottom wall's start, at the corner with the left wall, pulled 1000 west: 9 m long now.
    run(&mut s, "move", json!({"ids": [ids[0]], "by": [-1000, 0], "end": "start"}));
    assert_eq!(ends(&mut s, ids[0]), (json!([-1000.0, 0.0]), json!([8000.0, 0.0])));
    assert_eq!(ends(&mut s, ids[3]).1, json!([-1000.0, 0.0]), "the left wall's end followed the corner");
    assert_eq!(element(&mut s, door)["at"], json!([6000.0, 0.0]), "its door stayed put");
    let e = s.execute("move", &json!({"ids": [ids[0], ids[1]], "by": [10, 0], "end": "end"})).unwrap_err();
    assert!(e.contains("one wall"), "{e}");
}

#[test]
fn doors_move_along_their_walls_and_copies_bring_their_doors() {
    let mut s = Session::new();
    let ids = house(&mut s);
    let door = run(&mut s, "door", json!({"at": [2000, 0]}))["id"].as_u64().unwrap();
    run(&mut s, "move", json!({"ids": [door], "by": [1500, 700]}));
    assert_eq!(element(&mut s, door)["at"], json!([3500.0, 0.0]), "only the part along the wall counts");
    let made = run(&mut s, "CO", json!({"ids": [ids[0]], "by": [0, -2000]}))["ids"].clone();
    let (wall, copy) = (made[0].as_u64().unwrap(), made[1].as_u64().unwrap());
    assert_eq!(ends(&mut s, wall), (json!([0.0, -2000.0]), json!([8000.0, -2000.0])));
    let copy = element(&mut s, copy);
    assert_eq!((id_of(&s, &copy["host"]), copy["mark"].as_str(), copy["at"].clone()), (Some(wall), Some("2"), json!([3500.0, -2000.0])));
    let room = run(&mut s, "room", json!({"point": [4000, 3000], "name": "Hall"}))["id"].as_u64().unwrap();
    let made = run(&mut s, "copy", json!({"ids": [room], "by": [100, 100]}))["ids"].clone();
    let twin = element(&mut s, made[0].as_u64().unwrap());
    assert_eq!((twin["name"].as_str(), twin["number"].as_str()), (Some("Hall"), Some("2")));
}

fn near(a: &Value, b: [f64; 2]) -> bool {
    (a[0].as_f64().unwrap() - b[0]).abs() < 0.5 && (a[1].as_f64().unwrap() - b[1]).abs() < 0.5
}

#[test]
fn rotating_turns_elements_about_a_point_and_joined_walls_stay_joined() {
    let mut s = Session::new();
    let ids = house(&mut s);
    let part = run(&mut s, "wall", json!({"points": [[4000, 0], [4000, 6000]], "type": "Interior - 100mm"}))["ids"][0].as_u64().unwrap();
    // The partition turned 10° about its middle: its ends slide back onto the walls it met.
    run(&mut s, "RO", json!({"ids": [part], "about": [4000, 3000], "angle": 10}));
    let (a, b) = ends(&mut s, part);
    assert!(near(&a, [4529.0, 0.0]) && near(&b, [3471.0, 6000.0]), "{a} {b}");
    // The whole box a quarter turn: whole numbers stay whole, and so do the joins.
    run(&mut s, "rotate", json!({"ids": [ids[0], ids[1], ids[2], ids[3]], "about": [4000, 3000], "angle": 90}));
    assert_eq!(ends(&mut s, ids[0]), (json!([7000.0, -1000.0]), json!([7000.0, 7000.0])));
    let door = run(&mut s, "door", json!({"at": [7000, 3000]}))["id"].as_u64().unwrap();
    assert!(s.execute("rotate", &json!({"ids": [door], "about": [0, 0], "angle": 5})).unwrap_err().contains("with its wall"));
}

#[test]
fn mirror_copies_with_doors_swinging_the_mirrored_way() {
    let mut s = Session::new();
    let ids = house(&mut s);
    let door = run(&mut s, "door", json!({"at": [2000, 0]}))["id"].as_u64().unwrap();
    let made = run(&mut s, "MM", json!({"ids": [ids[0]], "axis": [[10000, -5000], [10000, 5000]], "copy": true}))["ids"].clone();
    let (wall, twin) = (made[0].as_u64().unwrap(), made[1].as_u64().unwrap());
    // A mirror image runs back the other way, so its exterior side mirrors the original's.
    assert_eq!(ends(&mut s, wall), (json!([12000.0, 0.0]), json!([20000.0, 0.0])));
    let (d, t) = (element(&mut s, door), element(&mut s, twin));
    assert_eq!((id_of(&s, &t["host"]), t["at"].clone()), (Some(wall), json!([18000.0, 0.0])));
    // Hung on the mirrored jamb, swinging to the same side: a mirror image.
    assert_eq!((&t["flip_facing"], &d["flip_hand"], &t["flip_hand"]), (&d["flip_facing"], &json!(false), &json!(true)));
    let grid = run(&mut s, "grid", json!({"start": [0, -1000], "end": [0, 7000]}))["id"].as_u64().unwrap();
    run(&mut s, "mirror", json!({"ids": [grid], "axis": [[4000, 0], [4000, 1000]]}));
    assert_eq!(element(&mut s, grid)["start"], json!([8000.0, -1000.0]), "without copy, the grid itself goes");
    assert!(s.execute("mirror", &json!({"ids": [grid], "axis": [[1, 1], [1, 1]]})).unwrap_err().contains("axis"));
}

#[test]
fn trim_to_corner_extends_or_trims_two_walls_to_meet() {
    let mut s = Session::new();
    let a = run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]]}))["ids"][0].as_u64().unwrap();
    let b = run(&mut s, "wall", json!({"points": [[5000, 1000], [5000, 6000]]}))["ids"][0].as_u64().unwrap();
    run(&mut s, "TR", json!({"ids": [a, b]}));
    assert_eq!((ends(&mut s, a).1, ends(&mut s, b).0), (json!([5000.0, 0.0]), json!([5000.0, 0.0])));
    // Crossing walls: `keep` says which part of each stays.
    let c = run(&mut s, "wall", json!({"points": [[-2000, 3000], [3000, 3000]]}))["ids"][0].as_u64().unwrap();
    let d = run(&mut s, "wall", json!({"points": [[1000, 1500], [1000, 5000]]}))["ids"][0].as_u64().unwrap();
    run(&mut s, "corner", json!({"ids": [c, d], "keep": [[-1000, 3000], [1000, 4500]]}));
    assert_eq!(ends(&mut s, c), (json!([-2000.0, 3000.0]), json!([1000.0, 3000.0])));
    assert_eq!(ends(&mut s, d), (json!([1000.0, 3000.0]), json!([1000.0, 5000.0])));
    let e = run(&mut s, "wall", json!({"points": [[0, 8000], [4000, 8000]]}))["ids"][0].as_u64().unwrap();
    assert!(s.execute("corner", &json!({"ids": [a, e]})).unwrap_err().contains("parallel"));
}

#[test]
fn split_cuts_a_wall_in_two_and_its_doors_go_with_their_part() {
    let mut s = Session::new();
    let ids = house(&mut s);
    let room = run(&mut s, "room", json!({"point": [4000, 3000]}))["id"].as_u64().unwrap();
    let door = run(&mut s, "door", json!({"at": [2000, 0]}))["id"].as_u64().unwrap();
    let window = run(&mut s, "window", json!({"at": [6000, 0]}))["id"].as_u64().unwrap();
    let pieces = run(&mut s, "SL", json!({"id": ids[0], "at": [4000, 20]}))["ids"].clone();
    let new = pieces[1].as_u64().unwrap();
    assert_eq!(ends(&mut s, ids[0]), (json!([0.0, 0.0]), json!([4000.0, 0.0])));
    assert_eq!(ends(&mut s, new), (json!([4000.0, 0.0]), json!([8000.0, 0.0])));
    assert_eq!(
        (id_of(&s, &crate::element_json(&s.model, door)["host"]), id_of(&s, &crate::element_json(&s.model, window)["host"])),
        (Some(ids[0]), Some(new))
    );
    assert_eq!(element(&mut s, window)["at"], json!([6000.0, 0.0]));
    assert_eq!(element(&mut s, room)["area_m2"], 48.0);
    assert!(crate::walls::cap(&s.model, ids[0], true).iter().all(|p| p.0 == 4000.0), "pieces in line meet square, not overlapping");
    assert!(s.execute("split", &json!({"id": ids[0], "at": [2000, 0]})).unwrap_err().contains("in the way"));
    assert!(s.execute("split", &json!({"id": ids[0], "at": [2000, 3000]})).unwrap_err().contains("isn't on"));
}

#[test]
fn modify_edits_many_elements_in_one_undo_step() {
    let mut s = Session::new();
    let ids = house(&mut s);
    let r = run(&mut s, "modify", json!({"ids": [ids[0], ids[2]], "type": "Exterior - 300mm"}));
    assert_eq!(r["modified"], json!([ids[0], ids[2]]));
    let thick = |s: &mut Session| [ids[0], ids[1], ids[2]].map(|id| element(s, id)["thickness"].as_f64().unwrap());
    assert_eq!(thick(&mut s), [300.0, 200.0, 300.0]);
    assert_eq!(s.history().last(), Some(&"Modify"));
    run(&mut s, "undo", json!({}));
    assert_eq!(thick(&mut s), [200.0, 200.0, 200.0]);
}

#[test]
fn match_properties_gives_the_walls_one_wall_type_and_leaves_other_categories_alone() {
    let mut s = Session::new();
    let ids = house(&mut s);
    let door = run(&mut s, "door", json!({"at": [2000, 30]}))["id"].as_u64().unwrap();
    run(&mut s, "modify", json!({"id": ids[0], "type": "Exterior - 300mm"}));
    let thick = |s: &mut Session| [ids[0], ids[1], ids[2]].map(|id| element(s, id)["thickness"].as_f64().unwrap());
    assert_eq!(thick(&mut s), [300.0, 200.0, 200.0]);
    // MA is its alias; the door is a different category, so it's skipped and named.
    let r = run(&mut s, "MA", json!({"from": ids[0], "ids": [ids[1], ids[2], door, ids[0]]}));
    assert_eq!((&r["modified"], &r["skipped"]), (&json!([ids[1], ids[2]]), &json!([door])));
    assert_eq!(thick(&mut s), [300.0, 300.0, 300.0]);
    assert_eq!(s.history().last(), Some(&"Match Properties"));
    run(&mut s, "undo", json!({}));
    assert_eq!(thick(&mut s), [300.0, 200.0, 200.0], "one undo step");
    // Nothing to match: the model stays as it was.
    let before = s.model.clone();
    for p in
        [json!({"ids": [ids[1]]}), json!({"from": 9_999_999, "ids": [ids[1]]}), json!({"from": ids[0]}), json!({"from": ids[0], "ids": []})]
    {
        assert!(s.execute("match_properties", &p).is_err(), "{p}");
    }
    assert_eq!(*s.model, *before);
}

#[test]
fn move_and_copy_refuse_levels_and_what_breaks_the_model() {
    let mut s = Session::new();
    let ids = house(&mut s);
    assert!(s.execute("move", &json!({"ids": [100000], "by": [10, 0]})).unwrap_err().contains("elevation"));
    assert!(s.execute("copy", &json!({"ids": [100000], "by": [10, 0]})).unwrap_err().contains("level"));
    let door = run(&mut s, "door", json!({"at": [2000, 0]}))["id"].as_u64().unwrap();
    assert!(s.execute("copy", &json!({"ids": [door], "by": [300, 0]})).unwrap_err().contains("overlap"));
    assert!(s.execute("move", &json!({"ids": [ids[0]], "by": [1e300, 0]})).is_err());
    assert!(s.execute("move", &json!({"ids": [ids[0]]})).unwrap_err().contains("by"));
}

#[test]
fn deleting_a_wall_takes_its_openings_and_used_levels_stay() {
    let mut s = Session::new();
    let ids = house(&mut s);
    run(&mut s, "door", json!({"at": [2000, 0]}));
    let d = run(&mut s, "DE", json!({"ids": [ids[0]]}));
    assert_eq!(d["deleted"].as_array().map(Vec::len), Some(2));
    assert_eq!(count(&mut s, "door"), 0);
    let e = s.execute("delete", &json!({"ids": [100000]})).unwrap_err();
    assert!(e.contains("level") && e.contains("doesn't exist"), "Level 1 still has walls: {e}");
}

#[test]
fn types_are_created_and_changed_everywhere_they_are_used() {
    let mut s = Session::new();
    let ids = house(&mut s);
    run(&mut s, "type", json!({"category": "wall", "name": "Brick - 250mm", "thickness": 250}));
    run(&mut s, "modify", json!({"id": ids[0], "type": "Brick - 250mm"}));
    run(&mut s, "type", json!({"category": "wall", "name": "Brick - 250mm", "thickness": 280}));
    let w = run(&mut s, "elements", json!({"category": "wall"}));
    assert_eq!(w["elements"][0]["thickness"], 280.0);
    let e = s.execute("type", &json!({"category": "door", "name": "Wide"})).unwrap_err();
    assert!(e.contains("width"), "a new type needs its sizes: {e}");
}

#[test]
fn levels_and_grids_name_themselves() {
    let mut s = Session::new();
    assert_eq!(run(&mut s, "level", json!({"elevation": 6000}))["name"], "Level 3");
    assert_eq!(run(&mut s, "grid", json!({"start": [0, -1000], "end": [0, 7000]}))["name"], "1");
    assert_eq!(run(&mut s, "GR", json!({"start": [8000, -1000], "end": [8000, 7000]}))["name"], "2");
    assert_eq!(run(&mut s, "grid", json!({"start": [-1000, 0], "end": [9000, 0], "name": "A"}))["name"], "A");
    assert_eq!(run(&mut s, "grid", json!({"start": [-1000, 6000], "end": [9000, 6000]}))["name"], "B");
}

#[test]
fn inspect_summarizes_levels_counts_and_warnings() {
    let mut s = Session::new();
    house(&mut s);
    run(&mut s, "room", json!({"point": [4000, 3000], "name": "Living"}));
    run(&mut s, "delete", json!({"ids": [100002]}));
    let i = run(&mut s, "inspect", json!({}));
    assert_eq!(i["counts"]["wall"], 3);
    assert_eq!(i["levels"][0]["name"], "Level 1");
    assert!(i["warnings"][0].as_str().unwrap().contains("not enclosed"), "{i}");
    assert_eq!(i["history"], json!(["Wall", "Room", "Delete"]));
}

#[test]
fn scripts_run_line_by_line_and_name_the_line_that_fails() {
    let mut s = Session::new();
    let out =
        s.script("# a box\nwall {\"points\": [[0,0],[4000,0],[4000,3000],[0,3000]], \"closed\": true}\n\nroom {\"point\": [2000,1500]}\n");
    assert_eq!(out.map(|v| v.len()), Ok(2));
    let e = s.script("level {\"elevation\": 9000}\nnope\n").unwrap_err();
    assert!(e.contains("line 2"), "{e}");
    assert_eq!(count(&mut s, "level"), 3, "the line before the failure stays done");
}

#[test]
fn an_atomic_script_is_one_undo_step_and_leaves_nothing_done_when_a_line_fails() {
    let mut s = Session::new();
    run(&mut s, "level", json!({"elevation": 3000}));
    let (history, levels) = (s.history(), count(&mut s, "level"));
    let e = s.script_atomic("level {\"elevation\": 6000}\nphase {\"name\": \"Later\"}\nnope\n").unwrap_err();
    assert!(e.contains("line 3"), "{e}");
    assert_eq!(count(&mut s, "level"), levels, "the lines before the failure are undone too");
    assert_eq!(s.history(), history, "and leave no undo step");
    // One that goes through is one step: undo takes all of it back, redo puts it all again.
    let out = s.script_atomic("# two levels\nlevel {\"elevation\": 6000}\nlevel {\"elevation\": 9000}\n").unwrap();
    assert_eq!(out.len(), 2);
    assert_eq!(count(&mut s, "level"), levels + 2);
    assert_eq!(s.history().len(), history.len() + 1);
    assert_eq!(s.history().last(), Some(&"Script"));
    run(&mut s, "undo", json!({}));
    assert_eq!(count(&mut s, "level"), levels);
    run(&mut s, "redo", json!({}));
    assert_eq!(count(&mut s, "level"), levels + 2);
    // Nothing changed is no step at all.
    let steps = s.history().len();
    s.script_atomic("inspect\n").unwrap();
    assert_eq!(s.history().len(), steps);
}

#[test]
fn a_failed_transaction_puts_back_the_history_the_phase_and_the_path_too() {
    let mut s = Session::new();
    run(&mut s, "level", json!({"elevation": 3000}));
    run(&mut s, "undo", json!({}));
    let (model, history) = (Arc::clone(&s.model), s.history());
    s.path = Some("kept.bimcraft".into());
    let e = s
        .transaction("Grouped", |s| {
            // Undo inside reaches no further back than the transaction, and a new project inside is taken back with the rest.
            assert!(s.execute("undo", &json!({})).is_err());
            s.execute("redo", &json!({}))?;
            s.execute("phase", &json!({"now": "Existing"}))?;
            s.execute("new", &json!({}))?;
            s.execute("level", &json!({"elevation": f64::NAN}))
        })
        .unwrap_err();
    assert!(!e.is_empty());
    assert!(Arc::ptr_eq(&s.model, &model), "the very model it started from");
    assert_eq!(s.history(), history);
    assert_eq!(s.phase, None);
    assert_eq!(s.path.as_deref(), Some(std::path::Path::new("kept.bimcraft")));
    // The redo that was there before is still there.
    run(&mut s, "redo", json!({}));
    assert_eq!(count(&mut s, "level"), 3);
    // One that went through a new project leaves the old project's history behind, as `new` does.
    s.script_atomic("new\nlevel {\"elevation\": 3000}\n").unwrap();
    assert_eq!(s.history(), vec!["Level"]);
    assert_eq!(count(&mut s, "level"), 3);
}

#[test]
fn dimensions_measure_between_parallel_walls_and_grids_and_follow_them() {
    let mut s = Session::new();
    let ids = house(&mut s);
    let (right, left) = (ids[1], ids[3]);
    let grid = run(&mut s, "grid", json!({"start": [3000, -2000], "end": [3000, 8000]}))["id"].as_u64().unwrap();
    // Centre line to grid to centre line: a chain of two, west to east whatever order the refs come in.
    let r = run(&mut s, "DI", json!({"refs": [{"id": right, "face": "centre"}, {"id": grid}, {"id": left}], "at": [0, -1000]}));
    let dim = r["id"].as_u64().unwrap();
    assert_eq!(r["values"], json!([3000.0, 5000.0]));
    // Faces, by points on them. The left wall runs from [0, 6000] down to [0, 0]: its left face is inside.
    let inside = run(&mut s, "dimension", json!({"refs": [[100, 3000], [7900, 3000]], "at": [0, 3000]}));
    assert_eq!(inside["values"], json!([7800.0]));
    let refs = element(&mut s, inside["id"].as_u64().unwrap())["refs"].clone();
    let (left_g, right_g) = (s.model.guid(left), s.model.guid(right));
    assert_eq!(refs, json!([{"id": left_g, "face": "left"}, {"id": right_g, "face": "left"}]));
    // The right wall moves: what measures to it follows.
    run(&mut s, "move", json!({"ids": [right], "by": [1000, 0]}));
    assert_eq!(element(&mut s, dim)["values"], json!([3000.0, 6000.0]));
    // References stay parallel: a turn that breaks that is refused, and so is a dimension across.
    let e = s.execute("rotate", &json!({"ids": [right], "about": [9000, 3000], "angle": 10})).unwrap_err();
    assert!(e.contains("parallel"), "{e}");
    let e = s.execute("dimension", &json!({"refs": [{"id": left}, {"id": ids[0]}], "at": [0, 0]})).unwrap_err();
    assert!(e.contains("parallel"), "{e}");
    let e = s.execute("dimension", &json!({"refs": [{"id": grid, "face": "left"}, {"id": left}], "at": [0, 0]})).unwrap_err();
    assert!(e.contains("face"), "{e}");
    let e = s.execute("dimension", &json!({"refs": [[3000, 3000], [5000, 3000]], "at": [0, 0]})).unwrap_err();
    assert!(e.contains("no wall"), "{e}");
    // Deleting what it measures: the grid drops out of the chain; with one reference left, it goes.
    run(&mut s, "delete", json!({"ids": [grid]}));
    assert_eq!(element(&mut s, dim)["values"], json!([9000.0]));
    let d = run(&mut s, "delete", json!({"ids": [right]}));
    assert!(d["deleted"].as_array().unwrap().contains(&json!(dim)), "{d}");
    assert_eq!(count(&mut s, "dimension"), 0);
}

#[test]
fn mirrored_walls_keep_their_dimensions_on_the_same_faces() {
    let mut s = Session::new();
    let ids = house(&mut s);
    // The bottom wall's outer face (its right, going east) to the top wall's centre line.
    let dim = run(&mut s, "DI", json!({"refs": [[4000, -100], [4000, 6000]], "at": [-1000, 0]}))["id"].as_u64().unwrap();
    assert_eq!(element(&mut s, dim)["values"], json!([6100.0]));
    // The whole box mirrored across x = 10000: each wall runs back the other way, so the bottom one
    // still runs east, its outer face on its right.
    run(&mut s, "mirror", json!({"ids": [ids[0], ids[1], ids[2], ids[3], dim], "axis": [[10000, 0], [10000, 1000]]}));
    assert_eq!(element(&mut s, dim)["values"], json!([6100.0]));
    assert_eq!(element(&mut s, dim)["refs"][0], json!({"id": s.model.guid(ids[0]), "face": "right"}));
}

#[test]
fn tags_name_doors_and_windows_and_go_where_they_go() {
    let mut s = Session::new();
    let ids = house(&mut s);
    let door = run(&mut s, "door", json!({"at": [2000, 0]}))["id"].as_u64().unwrap();
    run(&mut s, "window", json!({"at": [8000, 3000], "mark": ""}));
    // Beside the wall, on the side the door doesn't swing to: outside, below the bottom wall.
    let t = run(&mut s, "TG", json!({"ids": [door]}))["ids"][0].as_u64().unwrap();
    assert_eq!((element(&mut s, t)["text"].clone(), element(&mut s, t)["at"].clone()), (json!("1"), json!([2000.0, -500.0])));
    // Without ids, the doors and windows not tagged yet: Tag All. A window with no mark shows `?`.
    let all = run(&mut s, "tag", json!({}))["ids"].clone();
    assert_eq!(all.as_array().map(Vec::len), Some(1));
    assert_eq!(element(&mut s, all[0].as_u64().unwrap())["text"], "?");
    assert!(s.execute("tag", &json!({})).unwrap_err().contains("nothing to tag"));
    // What isn't in a plan to point at is no host: a grid has no place for a tag to name.
    let grid = run(&mut s, "grid", json!({"start": [0, -2000], "end": [0, 8000]}))["id"].as_u64().unwrap();
    let e = s.execute("tag", &json!({"ids": [grid]})).unwrap_err();
    assert!(e.contains("grid") && e.contains("wall"), "{e}");
    // A tag goes with its door's wall, and on its own wherever it's moved.
    run(&mut s, "move", json!({"ids": [ids[0]], "by": [0, -1000]}));
    assert_eq!(element(&mut s, t)["at"], json!([2000.0, -1500.0]));
    run(&mut s, "move", json!({"ids": [t], "by": [300, 0]}));
    assert_eq!(element(&mut s, t)["at"], json!([2300.0, -1500.0]));
    // Copied with the wall, the tag names the door's copy.
    let made = run(&mut s, "copy", json!({"ids": [ids[0], t], "by": [0, -3000]}))["ids"].clone();
    let twin = element(&mut s, made.as_array().unwrap().last().unwrap().as_u64().unwrap());
    assert_eq!((twin["category"].as_str(), twin["text"].as_str(), twin["at"].clone()), (Some("tag"), Some("2"), json!([2300.0, -4500.0])));
    // Deleting a door takes its tags.
    run(&mut s, "delete", json!({"ids": [door]}));
    assert_eq!(count(&mut s, "tag"), 2);
}

#[test]
fn a_tag_names_any_element_by_a_field_of_it() {
    let mut s = Session::new();
    let ids = house(&mut s);
    // By default a wall's tag names its type, a little way from the middle of its centre line.
    let t = run(&mut s, "tag", json!({"ids": [ids[0]]}))["ids"][0].as_u64().unwrap();
    let kind = element(&mut s, ids[0])["type"].as_str().unwrap().to_string();
    let tag = element(&mut s, t);
    assert_eq!((tag["text"].clone(), tag["at"].clone(), tag["label"].clone()), (json!(kind), json!([4400.0, 400.0]), Value::Null));
    // Any field its category's schedule has, as the schedule shows it: a length in the project's units.
    let h = run(&mut s, "tag", json!({"ids": [ids[0]], "label": "Height"}))["ids"][0].as_u64().unwrap();
    let height = s.model.units.length(3000.0);
    assert_eq!((element(&mut s, h)["text"].clone(), element(&mut s, h)["label"].clone()), (json!(height), json!("Height")));
    // A field its category hasn't is refused, naming the ones it has.
    let e = s.execute("tag", &json!({"ids": [ids[0]], "label": "Colour"})).unwrap_err();
    assert!(e.contains("Colour") && e.contains("Height") && e.contains("Type"), "{e}");
    // A floor's is where its outline is centred; a room's, its name; a level or a grid isn't a host.
    let floor = run(&mut s, "floor", json!({}))["id"].as_u64().unwrap();
    let room = run(&mut s, "room", json!({"point": [4000, 3000], "name": "Living"}))["id"].as_u64().unwrap();
    let both = run(&mut s, "tag", json!({"ids": [floor, room]}))["ids"].clone();
    let [on_floor, on_room] = [0, 1].map(|i| element(&mut s, both[i].as_u64().unwrap()));
    assert_eq!((on_floor["at"].clone(), on_room["text"].clone()), (json!([4400.0, 3400.0]), json!("Living")));
    let level = s.model.levels()[0].0;
    assert!(s.execute("tag", &json!({"ids": [level]})).unwrap_err().contains("level"));
    // Tag All takes a category: its elements on the level not tagged yet (the first wall has two tags already).
    let rest = run(&mut s, "tag", json!({"category": "wall"}))["ids"].clone();
    assert_eq!(rest.as_array().map(Vec::len), Some(3));
    assert!(s.execute("tag", &json!({"category": "wall"})).unwrap_err().contains("nothing to tag"));
    // A tag goes with its host, and a label is its own to change.
    run(&mut s, "move", json!({"ids": [ids[0]], "by": [0, -1000]}));
    assert_eq!(element(&mut s, t)["at"], json!([4400.0, -600.0]));
    run(&mut s, "modify", json!({"id": h, "label": "Thickness"}));
    assert_eq!(element(&mut s, h)["text"], json!(s.model.units.length(200.0)));
    assert!(s.execute("modify", &json!({"id": h, "label": "Colour"})).is_err());
    // Deleting the host takes its tags.
    let before = count(&mut s, "tag");
    run(&mut s, "delete", json!({"ids": [ids[0]]}));
    assert_eq!(count(&mut s, "tag"), before - 2);
}

#[test]
fn measure_reads_the_length_parts_and_angle_between_two_points_and_changes_nothing() {
    let mut s = Session::new();
    house(&mut s);
    let before = run(&mut s, "inspect", json!({}))["history"].clone();
    let r = run(&mut s, "measure", json!({"from": [0, 0], "to": [3000, 4000]}));
    assert_eq!((&r["length"], &r["dx"], &r["dy"], &r["angle"]), (&json!(5000.0), &json!(3000.0), &json!(4000.0), &json!(53.1301)));
    // Any way round: 180° west, 270° south; a point on itself is nothing to measure but still reads.
    assert_eq!(run(&mut s, "measure", json!({"from": [0, 0], "to": [-1000, 0]}))["angle"], json!(180.0));
    assert_eq!(run(&mut s, "measure", json!({"from": [500, 500], "to": [500, -500]}))["angle"], json!(270.0));
    assert_eq!(run(&mut s, "measure", json!({"from": [7, 7], "to": [7, 7]}))["length"], json!(0.0));
    // Its ME shortcut runs it, and it's no undo step.
    assert_eq!(bimcraft_core_find("ME"), Some("measure"));
    assert_eq!(run(&mut s, "inspect", json!({}))["history"], before);
    for (bad, says) in [
        (json!({"from": [0, 0]}), "to"),
        (json!({"to": [0, 0]}), "from"),
        (json!({"from": [1e300, 0], "to": [0, 0]}), "from"),
        (json!({"from": "x", "to": [0, 0]}), "from"),
    ] {
        let e = s.execute("measure", &bad).unwrap_err();
        assert!(e.contains(says), "{bad}: {e}");
    }
}

fn bimcraft_core_find(alias: &str) -> Option<&'static str> {
    crate::engine::find(alias).map(|c| c.id)
}

#[test]
fn project_parameters_give_the_elements_of_their_categories_values_of_their_own() {
    let mut s = Session::new();
    let ids = house(&mut s);
    run(&mut s, "parameter", json!({"name": "Fire Rating", "categories": ["walls", "doors"]}));
    run(&mut s, "parameter", json!({"name": "Cost", "kind": "number", "categories": ["wall"]}));
    run(&mut s, "modify", json!({"ids": ids, "params": {"Fire Rating": "60 min", "Cost": 120}}));
    let w = crate::element_json(&s.model, ids[0]);
    assert_eq!((&w["params"]["Fire Rating"], &w["params"]["Cost"]), (&json!("60 min"), &json!(120)));
    // A value is what its kind holds, and only its categories' elements have one.
    let e = s.execute("modify", &json!({"id": ids[0], "params": {"Cost": "cheap"}})).unwrap_err();
    assert!(e.contains("Cost") && e.contains("number"), "{e}");
    let room = run(&mut s, "room", json!({"point": [4000, 3000]}))["id"].as_u64().unwrap();
    let e = s.execute("modify", &json!({"id": room, "params": {"Cost": 5}})).unwrap_err();
    assert!(e.contains("Cost") && e.contains("room"), "{e}");
    // A copy takes its values along; a null takes one away.
    let copied = run(&mut s, "copy", json!({"ids": [ids[0]], "by": [0, -3000]}))["ids"][0].as_u64().unwrap();
    assert_eq!(crate::element_json(&s.model, copied)["params"]["Cost"], json!(120));
    run(&mut s, "modify", json!({"id": copied, "params": {"Cost": null}}));
    assert_eq!(crate::element_json(&s.model, copied)["params"].get("Cost"), None);
    // Renamed, the values follow; off a category, its elements' values go; deleted, all of them.
    run(&mut s, "parameter", json!({"name": "Cost", "rename": "Price"}));
    assert_eq!(crate::element_json(&s.model, ids[0])["params"]["Price"], json!(120));
    run(&mut s, "parameter", json!({"name": "Fire Rating", "categories": ["door"]}));
    assert_eq!(crate::element_json(&s.model, ids[0])["params"].get("Fire Rating"), None);
    run(&mut s, "parameter", json!({"name": "Price", "delete": true}));
    assert_eq!(crate::element_json(&s.model, ids[0])["params"], Value::Null);
    let names: Vec<String> = s.model.parameters.keys().cloned().collect();
    assert_eq!(names, ["Fire Rating"]);
    for (bad, says) in [
        (json!({"name": "X", "categories": ["clouds"]}), "clouds"),
        (json!({"name": "X", "kind": "colour", "categories": ["wall"]}), "colour"),
        (json!({"name": "X", "categories": []}), "categor"),
        (json!({"name": "1st", "categories": ["wall"]}), "name"),
        (json!({"name": "Width", "categories": ["component"]}), "family"),
    ] {
        let e = s.execute("parameter", &bad).unwrap_err();
        assert!(e.contains(says), "{bad}: {e}");
    }
}

/// Two rooms, Kitchen (18 m²) and Living (30 m²), and three doors: D2 and two numbered ones.
fn flat(s: &mut Session) {
    s.script(
        r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
           wall {"points": [[5000,0],[5000,6000]], "type": "Interior - 100mm"}
           room {"point": [6500, 3000], "name": "Kitchen"}
           room {"point": [2000, 3000], "name": "Living", "number": "10"}
           door {"at": [6000, 0], "mark": "D2"}
           door {"at": [2000, 0]}
           door {"at": [5000, 3000], "type": "Single 800 x 2100"}"#,
    )
    .unwrap();
}

#[test]
fn schedules_show_the_fields_chosen_of_the_elements_their_filters_keep_sorted_grouped_and_totalled() {
    let mut s = Session::new();
    flat(&mut s);
    let fields = json!(["Name", {"name": "Area", "total": true}, "Number"]);
    let r = run(
        &mut s,
        "schedule",
        json!({"name": "Rooms by Area", "category": "rooms", "fields": fields, "sort": [{"field": "Area", "descending": true}]}),
    );
    assert_eq!(r["columns"], json!(["Name", "Area (m²)", "Number"]));
    assert_eq!(r["rows"], json!([["Living", 30.0, "10"], ["Kitchen", 18.0, "1"]]));
    assert_eq!(r["totals"], json!([null, 48.0, null]));
    assert!(r["available"].as_array().unwrap().contains(&json!("Perimeter")), "{}", r["available"]);
    // Filters keep what they all hold for; text in any case.
    let r = run(&mut s, "schedule", json!({"name": "Rooms by Area", "filters": [{"field": "Name", "op": "begins-with", "value": "kit"}]}));
    assert_eq!((r["rows"].clone(), r["totals"].clone()), (json!([["Kitchen", 18.0, "1"]]), json!([null, 18.0, null])));
    let r = run(&mut s, "schedule", json!({"name": "Rooms by Area", "filters": [{"field": "Area", "op": "greater", "value": 20}]}));
    assert_eq!(r["rows"], json!([["Living", 30.0, "10"]]));
    // It's the project's view now: by its name, and as it was left. A name every project's
    // schedule of another category has is that one's.
    assert_eq!(count(&mut s, "schedule"), 1);
    let id = run(&mut s, "schedule", json!({"name": "Rooms by Area"}))["id"].as_u64().unwrap();
    let e = s.execute("modify", &json!({"id": id, "name": "Window Schedule"})).unwrap_err();
    assert!(e.contains("Window Schedule") && e.contains("window"), "{e}");
    assert_eq!(run(&mut s, "schedule", json!({"name": "Rooms by Area"}))["rows"], json!([["Living", 30.0, "10"]]));
    // A schedule every project has becomes the project's once changed: grouped by type here, its
    // doors counted, a mark that differs left empty.
    let r = run(
        &mut s,
        "schedule",
        json!({"name": "Door Schedule", "fields": ["Type", "Width", {"name": "Count", "total": true}, "Mark"], "sort": ["Type"], "grouped": true}),
    );
    assert_eq!(r["rows"], json!([["Single 800 x 2100", 800.0, 1.0, "2"], ["Single 900 x 2100", 900.0, 2.0, null]]));
    assert_eq!(r["totals"], json!([null, null, 3.0, null]));
    // Each row says which elements it lists: a group, all of them.
    let doors: Vec<u64> = run(&mut s, "elements", json!({"category": "door"}))["elements"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d["type"] == "Single 900 x 2100")
        .map(|d| d["id"].as_u64().unwrap())
        .collect();
    assert_eq!(r["elements"][1], json!(doors));
    assert_eq!(r["elements"].as_array().map(Vec::len), Some(2));
    assert_eq!(count(&mut s, "schedule"), 2);
    assert_eq!(run(&mut s, "schedule", json!({"category": "doors"}))["rows"].as_array().map(Vec::len), Some(2));
    // Back to its category's fields, every element its row, sorted by its first column.
    let r = run(&mut s, "schedule", json!({"name": "Door Schedule", "fields": [], "sort": [], "grouped": false}));
    assert_eq!(r["columns"], json!(["Mark", "Type", "Width", "Height", "Level", "Wall"]));
    let marks: Vec<&str> = r["rows"].as_array().unwrap().iter().filter_map(|row| row[0].as_str()).collect();
    assert_eq!(marks, ["1", "2", "D2"]);
    // Every category the model holds has a schedule; the room schedule's perimeter is a length.
    assert_eq!(run(&mut s, "schedule", json!({"category": "rooms"}))["rows"][0][4], json!(18000.0));
    run(&mut s, "stair", json!({"start": [1000, 1000], "angle": 90}));
    let r = run(&mut s, "schedule", json!({"category": "stairs"}));
    assert_eq!(r["columns"], json!(["Id", "Level", "Width", "Height", "Risers", "Riser Height", "Tread"]));
    assert_eq!(r["rows"].as_array().map(Vec::len), Some(1));
    // Undo takes a change back.
    run(&mut s, "undo", json!({}));
    run(&mut s, "undo", json!({}));
    assert_eq!(run(&mut s, "schedule", json!({"name": "Door Schedule"}))["columns"], json!(["Type", "Width", "Count", "Mark"]));
}

#[test]
fn calculated_fields_work_out_formulas_over_the_rows_other_fields() {
    let mut s = Session::new();
    flat(&mut s);
    let fields = json!([
        "Name",
        "Area",
        {"name": "Cost", "formula": "Area * 120", "total": true},
        {"name": "Half Perimeter", "formula": "Perimeter / 2", "kind": "length"},
        {"name": "Large", "formula": "Area > 20", "kind": "yesno"},
        {"name": "Cost With Tax", "formula": "Cost * 1.1", "heading": "Cost + 10%"},
    ]);
    let r = run(&mut s, "schedule", json!({"name": "Costs", "category": "rooms", "fields": fields}));
    assert_eq!(r["columns"], json!(["Name", "Area (m²)", "Cost", "Half Perimeter", "Large", "Cost + 10%"]));
    assert_eq!(r["rows"], json!([["Kitchen", 18.0, 2160.0, 9000.0, false, 2376.0], ["Living", 30.0, 3600.0, 11000.0, true, 3960.0]]));
    assert_eq!(r["totals"], json!([null, null, 5760.0, null, null, null]));
    // Filtered and sorted by like any other.
    let r = run(&mut s, "schedule", json!({"name": "Costs", "filters": [{"field": "Large", "op": "equals", "value": true}]}));
    assert_eq!(r["rows"].as_array().map(Vec::len), Some(1));
    // A field with no value leaves a formula reading it with none.
    run(&mut s, "parameter", json!({"name": "Rate", "kind": "number", "categories": ["room"]}));
    let living = run(&mut s, "elements", json!({"category": "rooms"}))["elements"][1]["id"].as_u64().unwrap();
    run(&mut s, "modify", json!({"id": living, "params": {"Rate": 100}}));
    let r =
        run(&mut s, "schedule", json!({"name": "Costs", "filters": [], "fields": ["Name", {"name": "Cost", "formula": "Area * Rate"}]}));
    assert_eq!(r["rows"], json!([["Kitchen", null], ["Living", 3000.0]]));
    for (bad, says) in [
        (json!([{"name": "X", "formula": "Nope * 2"}]), "Nope"),
        (json!([{"name": "A", "formula": "B + 1"}, {"name": "B", "formula": "A * 2"}]), "comes back to itself"),
        (json!([{"name": "X", "formula": "Name * 2"}]), "text"),
        (json!([{"name": "Area", "formula": "1"}]), "call it something else"),
        (json!([{"name": "X", "formula": "(1 + Area"}]), "isn't closed"),
        (json!([{"name": "X", "formula": "sqrt(Area, 2)"}]), "sqrt"),
        (json!([{"name": "X", "formula": "Area", "kind": "text"}]), "number"),
        (json!(["Colour"]), "Colour"),
        (json!(["Name", "Name"]), "twice"),
        (json!([{"name": "Name", "total": true}]), "total"),
        (json!([{"name": "Area", "kind": "length"}]), "formula"),
    ] {
        let e = s.execute("schedule", &json!({"name": "Costs", "fields": bad})).unwrap_err();
        assert!(e.contains(says), "{bad}: {e}");
    }
    for (p, says) in [
        (json!({"filters": [{"field": "Area", "op": "contains", "value": "1"}]}), "text"),
        (json!({"filters": [{"field": "Area", "op": "greater", "value": "big"}]}), "area"),
        (json!({"filters": [{"field": "Area", "op": "near", "value": 1}]}), "near"),
        (json!({"filters": [{"field": "Colour", "op": "has-value"}]}), "Colour"),
        (json!({"sort": ["Colour"]}), "Colour"),
        (json!({"grouped": "yes"}), "grouped"),
        (json!({"category": "doors"}), "rooms"),
    ] {
        let mut p = p;
        p["name"] = json!("Costs");
        let e = s.execute("schedule", &p).unwrap_err();
        assert!(e.contains(says), "{p}: {e}");
    }
    assert!(s.execute("schedule", &json!({"name": "Nowhere"})).unwrap_err().contains("category"));
    assert!(s.execute("schedule", &json!({"name": "S", "category": "clouds"})).unwrap_err().contains("clouds"));
}

#[test]
fn schedules_follow_the_project_parameters_they_show() {
    let mut s = Session::new();
    flat(&mut s);
    run(&mut s, "parameter", json!({"name": "Fire Rating", "categories": ["door"]}));
    let d2 = run(&mut s, "elements", json!({"category": "doors"}))["elements"][0]["id"].as_u64().unwrap();
    run(&mut s, "modify", json!({"id": d2, "params": {"Fire Rating": "30 min"}}));
    let filter = json!([{"field": "Fire Rating", "op": "has-value"}]);
    let r = run(
        &mut s,
        "schedule",
        json!({"name": "Fire Doors", "category": "doors", "fields": ["Mark", "Fire Rating"], "filters": filter, "sort": ["Fire Rating"]}),
    );
    assert_eq!(r["rows"], json!([["D2", "30 min"]]));
    // Renamed, the schedule shows it by its new name; deleted, it goes, and its filter with it.
    run(&mut s, "parameter", json!({"name": "Fire Rating", "rename": "Fire Resistance"}));
    let r = run(&mut s, "schedule", json!({"name": "Fire Doors"}));
    assert_eq!((r["columns"].clone(), r["rows"].clone()), (json!(["Mark", "Fire Resistance"]), json!([["D2", "30 min"]])));
    run(&mut s, "parameter", json!({"name": "Fire Resistance", "categories": ["window"]}));
    let r = run(&mut s, "schedule", json!({"name": "Fire Doors"}));
    assert_eq!((r["columns"].clone(), r["rows"].as_array().map(Vec::len)), (json!(["Mark"]), Some(3)));
    // A formula reading one stops it changing under it.
    run(&mut s, "parameter", json!({"name": "Rate", "kind": "number", "categories": ["door"]}));
    run(&mut s, "schedule", json!({"name": "Fire Doors", "fields": ["Mark", {"name": "Cost", "formula": "Rate * 2"}]}));
    let e = s.execute("parameter", &json!({"name": "Rate", "delete": true})).unwrap_err();
    assert!(e.contains("Rate") && e.contains("Fire Doors"), "{e}");
    // A parameter can't take a name its categories' own fields have.
    let e = s.execute("parameter", &json!({"name": "Width", "categories": ["door"]})).unwrap_err();
    assert!(e.contains("Width") && e.contains("own"), "{e}");
}

#[test]
fn component_schedules_show_their_families_parameters() {
    let mut s = Session::new();
    flat(&mut s);
    for (family, x) in [("Chair", 1000), ("Chair", 2000), ("Table", 3000)] {
        run(&mut s, "component", json!({"family": family, "at": [x, 2000]}));
    }
    let r = run(
        &mut s,
        "schedule",
        json!({"name": "Furniture", "category": "components", "fields": ["Family", "Width", {"name": "Count", "total": true}], "sort": ["Family"], "grouped": true}),
    );
    let rows = r["rows"].as_array().unwrap();
    assert_eq!(
        rows.iter().map(|row| (row[0].clone(), row[2].clone())).collect::<Vec<_>>(),
        [(json!("Chair"), json!(2.0)), (json!("Table"), json!(1.0))]
    );
    assert!(rows.iter().all(|row| row[1].as_f64().is_some_and(|w| w > 0.0)), "{rows:?}");
    assert_eq!(r["totals"][2], json!(3.0));
}

#[test]
fn a_project_holds_up_to_its_most_parameters() {
    let mut s = Session::new();
    for i in 0..crate::model::MAX_PROJECT_PARAMETERS {
        run(&mut s, "parameter", json!({"name": format!("P{i}"), "categories": ["wall"]}));
    }
    let e = s.execute("parameter", &json!({"name": "One More", "categories": ["wall"]})).unwrap_err();
    assert!(e.contains(&crate::model::MAX_PROJECT_PARAMETERS.to_string()), "{e}");
}

#[test]
fn a_schedule_is_checked_before_its_table_is_made_or_written() {
    let mut s = Session::new();
    flat(&mut s);
    // One no command makes its own: what's wrong with it is said all the same.
    let circle = json!(["Id", {"name": "A", "formula": "B + 1"}, {"name": "B", "formula": "A + 1"}]);
    let e = s.execute("schedule", &json!({"category": "stairs", "fields": circle})).unwrap_err();
    assert!(e.contains("comes back to itself"), "{e}");
    let many: Vec<Value> = (0..1000).map(|_| json!("Id")).collect();
    let e = s.execute("schedule", &json!({"category": "walls", "fields": many})).unwrap_err();
    assert!(e.contains("64"), "{e}");
    // A file is written only for a schedule the model keeps.
    for (i, bad) in [json!({"name": "X", "category": "rooms", "fields": ["Nope"]}), json!({"name": "Level 1", "category": "rooms"})]
        .into_iter()
        .enumerate()
    {
        let path = temp(&format!("refused-{i}.csv"));
        let mut p = bad;
        p["path"] = json!(path);
        assert!(s.execute("schedule", &p).is_err(), "{p}");
        assert!(!path.exists(), "{p} wrote {}", path.display());
    }
    // Whatever a filter compares, its value is a small one.
    let long = "x".repeat(crate::model::MAX_TEXT + 1);
    let e =
        s.execute("schedule", &json!({"name": "Y", "category": "rooms", "filters": [{"field": "Name", "op": "has-value", "value": long}]}));
    assert!(e.is_err(), "{e:?}");
}

#[test]
fn a_schedule_every_project_has_is_one_whatever_language_or_case_names_it() {
    let mut s = Session::new();
    flat(&mut s);
    s.script(
        r#"language {"code": "pt"}
           sheet {}
           place {"sheet": "A101", "view": "Room Schedule"}
           language {"code": "en"}"#,
    )
    .unwrap();
    // Named in Portuguese, it's still the Room Schedule: placed already, it isn't made again.
    assert!(s.execute("place", &json!({"sheet": "A101", "view": "Room Schedule"})).is_err());
    assert_eq!(count(&mut s, "schedule"), 1);
    let id = run(&mut s, "schedule", json!({"category": "rooms"}))["id"].as_u64();
    assert!(id.is_some());
    // In any case.
    run(&mut s, "modify", json!({"id": id, "name": "room schedule"}));
    assert_eq!(run(&mut s, "schedule", json!({"category": "rooms"}))["id"].as_u64(), id);
    // And no other schedule takes its name in another language.
    let other = run(&mut s, "schedule", json!({"name": "Rooms 2", "category": "rooms"}))["id"].as_u64();
    let e = s.execute("modify", &json!({"id": other, "name": "Tabla de habitaciones"})).unwrap_err();
    assert!(e.contains("Tabla de habitaciones") && e.contains("already"), "{e}");
}

#[test]
fn a_chain_of_calculated_fields_is_capped_whichever_order_they_come_in() {
    let mut s = Session::new();
    flat(&mut s);
    // B01 reads B02 … B10 reads A01 … A10 reads Id: 20 formulas in a row.
    let chain = |first: &str, second: &str| -> Vec<Value> {
        let mut fields = vec![json!("Id")];
        for (group, last) in [
            (first, if first == "A" { "Id".to_string() } else { "A01".into() }),
            (second, if second == "A" { "Id".to_string() } else { "A01".into() }),
        ] {
            for i in 1..=10 {
                let next = if i == 10 { last.clone() } else { format!("{group}{:02}", i + 1) };
                fields.push(json!({"name": format!("{group}{i:02}"), "formula": format!("{next} + 1")}));
            }
        }
        fields
    };
    for (first, second) in [("A", "B"), ("B", "A")] {
        let e = s.execute("schedule", &json!({"name": "Chain", "category": "walls", "fields": chain(first, second)})).unwrap_err();
        assert!(e.contains("in a row"), "{first} first: {e}");
    }
}

#[test]
fn a_components_familys_values_are_its_familys_not_the_projects() {
    let mut s = Session::new();
    flat(&mut s);
    run(
        &mut s,
        "family",
        json!({"name": "Crate", "parameters": [{"name": "Cost", "kind": "number", "instance": true, "value": 5}], "types": {"Small": {}},
               "forms": [{"profile": [[0, 0], [500, 0], [500, 500], [0, 500]], "to": 500}]}),
    );
    run(&mut s, "component", json!({"family": "Crate", "at": [1000, 1000], "params": {"Cost": 7}}));
    // A project parameter of the same name for doors: the crate's value is its family's.
    run(&mut s, "parameter", json!({"name": "Cost", "kind": "number", "categories": ["door"]}));
    let door = run(&mut s, "elements", json!({"category": "doors"}))["elements"][0]["id"].as_u64().unwrap();
    run(&mut s, "modify", json!({"id": door, "params": {"Cost": 100}}));
    let crates = json!({"name": "Crates", "category": "components", "fields": ["Family", "Cost"], "filters": [{"field": "Cost", "op": "has-value"}], "sort": ["Cost"]});
    assert_eq!(run(&mut s, "schedule", crates)["rows"], json!([["Crate", 7.0]]));
    // Off doors, the doors' values go; the crates' schedule keeps its family's Cost.
    run(&mut s, "parameter", json!({"name": "Cost", "categories": ["wall"]}));
    let r = run(&mut s, "schedule", json!({"name": "Crates"}));
    assert_eq!((r["columns"].clone(), r["rows"].clone()), (json!(["Family", "Cost"]), json!([["Crate", 7.0]])));
    match s.model.elements.values().find_map(|e| if let crate::model::Element::Schedule(sc) = e { Some(sc.clone()) } else { None }) {
        Some(sc) => assert_eq!((sc.filters.len(), sc.sort.len()), (1, 1)),
        None => panic!("no schedule"),
    }
    // IFC carries the door's project value, not the crate's family one.
    run(&mut s, "parameter", json!({"name": "Cost", "categories": ["door"]}));
    run(&mut s, "modify", json!({"id": door, "params": {"Cost": 100}}));
    let file = crate::ifc::export(&s.model);
    assert_eq!(file.matches("IFCPROPERTYSINGLEVALUE('Cost'").count(), 1, "the door's alone");
}

#[test]
fn an_unset_yes_or_no_is_no() {
    let mut s = Session::new();
    flat(&mut s);
    run(&mut s, "parameter", json!({"name": "Accessible", "kind": "yesno", "categories": ["room"]}));
    let kitchen = run(&mut s, "elements", json!({"category": "rooms"}))["elements"][0]["id"].as_u64().unwrap();
    run(&mut s, "modify", json!({"id": kitchen, "params": {"Accessible": true}}));
    let r = run(&mut s, "schedule", json!({"name": "Access", "category": "rooms", "fields": ["Name", "Accessible"]}));
    assert_eq!(r["rows"], json!([["Kitchen", true], ["Living", false]]));
    let r = run(&mut s, "schedule", json!({"name": "Access", "filters": [{"field": "Accessible", "op": "equals", "value": false}]}));
    assert_eq!(r["rows"], json!([["Living", false]]));
}

/// A desk family: a top `Height` up, as wide and deep as its type says.
fn desk(s: &mut Session) {
    run(
        s,
        "family",
        json!({
            "name": "Work Desk",
            "parameters": [
                {"name": "Width", "value": 1200},
                {"name": "Depth", "value": 600},
                {"name": "Height", "instance": true, "value": 740},
                {"name": "Top", "formula": "Height - 30"}
            ],
            "types": {"1200 x 600": {}, "1600 x 800": {"Width": 1600, "Depth": 800}},
            "forms": [{"profile": [[0, 0], ["Width", 0], ["Width", "Depth"], [0, "Depth"]], "from": "Top", "to": "Height", "material": "Timber"}]
        }),
    );
}

#[test]
fn components_place_a_familys_type_sized_by_its_parameters() {
    let mut s = Session::new();
    desk(&mut s);
    let id = run(&mut s, "component", json!({"family": "Work Desk", "type": "1600 x 800", "at": [1000, 2000], "angle": 90}))["id"]
        .as_u64()
        .unwrap();
    let e = crate::element_json(&s.model, id);
    assert_eq!(
        (&e["family"], &e["type"], &e["values"]["Width"], &e["values"]["Top"]),
        (&json!("Work Desk"), &json!("1600 x 800"), &json!(1600.0), &json!(710.0))
    );
    let piece = |s: &Session| {
        let Some(crate::model::Element::Component(c)) = s.model.elements.get(id) else { panic!("no component") };
        s.model.component_pieces(c).into_iter().next().unwrap()
    };
    // Turned a quarter: its x runs north.
    let p = piece(&s);
    assert_eq!((p.bottom, p.top), (710.0, 740.0));
    let corners: Vec<P2> = p.outline.iter().map(|q| P2((q.0 * 1e6).round() / 1e6 + 0.0, (q.1 * 1e6).round() / 1e6 + 0.0)).collect();
    assert_eq!(corners, [P2(1000.0, 2000.0), P2(1000.0, 3600.0), P2(200.0, 3600.0), P2(200.0, 2000.0)]);
    // An instance parameter is the component's own; a type parameter its type's.
    run(&mut s, "modify", json!({"id": id, "params": {"Height": 720}, "offset": 100}));
    assert_eq!((piece(&s).bottom, piece(&s).top), (790.0, 820.0));
    let e = s.execute("modify", &json!({"id": id, "params": {"Width": 5}})).unwrap_err();
    assert!(e.contains("Width") && e.contains("type parameter"), "{e}");
    let e = s.execute("modify", &json!({"id": id, "params": {"Height": "tall"}})).unwrap_err();
    assert!(e.contains("Height") && e.contains("number"), "{e}");
    let e = s.execute("family", &json!({"name": "Work Desk", "types": {"1600 x 800": {"Width": 0}}})).unwrap_err();
    assert!(e.contains("no area"), "{e}");
    // A family's types change every component of them; renamed, they follow; used, it stays.
    run(&mut s, "family", json!({"name": "Work Desk", "types": {"1600 x 800": {"Width": 1700}}}));
    assert_eq!(piece(&s).outline[1].1.round(), 3700.0);
    assert_eq!(s.model.families["Work Desk"].types["1600 x 800"]["Depth"], json!(800), "a type's values merge");
    run(&mut s, "family", json!({"name": "Work Desk", "rename": "Writing desk"}));
    assert_eq!(crate::element_json(&s.model, id)["family"], json!("Writing desk"));
    let e = s.execute("family", &json!({"name": "Writing desk", "delete": true})).unwrap_err();
    assert!(e.contains(&id.to_string()), "{e}");
    let e = s.execute("component", &json!({"family": "Work Desk", "at": [0, 0]})).unwrap_err();
    assert!(e.contains("Writing desk"), "it lists the families there are: {e}");
    // Placed without a type: the family's first.
    let other = run(&mut s, "component", json!({"family": "Writing desk", "at": [0, 0]}))["id"].as_u64().unwrap();
    assert_eq!(crate::element_json(&s.model, other)["type"], json!("1200 x 600"));
    run(&mut s, "delete", json!({"ids": [id, other]}));
    run(&mut s, "family", json!({"name": "Writing desk", "delete": true}));
    assert!(!s.model.families.contains_key("Writing desk"));
}

#[test]
fn a_family_loads_from_its_file() {
    let mut s = Session::new();
    desk(&mut s);
    let saved = run(&mut s, "inspect", json!({}));
    assert_eq!(saved["families"]["Work Desk"]["types"], json!(["1200 x 600", "1600 x 800"]));
    // A family as `family` returns it, with its name, saved to a file and loaded by another project.
    let mut f = run(&mut s, "family", json!({"name": "Work Desk"}))["family"].clone();
    f["name"] = json!("Work Desk");
    let path = std::env::temp_dir().join(format!("bimcraft-desk-{}.json", std::process::id()));
    std::fs::write(&path, f.to_string()).unwrap();
    let mut other = Session::new();
    assert_eq!(run(&mut other, "family", json!({"path": path}))["name"], json!("Work Desk"));
    assert_eq!(other.model.families["Work Desk"], s.model.families["Work Desk"]);
    // Under another name.
    run(&mut other, "family", json!({"path": path, "name": "Writing Table"}));
    assert!(other.model.families.contains_key("Writing Table"));
    std::fs::write(&path, "[1, 2]").unwrap();
    assert!(other.execute("family", &json!({"path": path})).unwrap_err().contains("isn't an object"));
    let _ = std::fs::remove_file(&path);
}

/// A table: a top 750 mm up over two legs, one square at a corner, one round.
fn table(s: &mut Session) {
    run(
        s,
        "family",
        json!({"name": "Table", "types": {"A": {}}, "forms": [
            {"profile": [[0, 0], [1000, 0], [1000, 600], [0, 600]], "from": 720, "to": 750, "material": "Timber"},
            {"profile": [[0, 0], [50, 0], [50, 50], [0, 50]], "to": 720},
            {"circle": {"centre": [960, 560], "radius": 25}, "to": 720}
        ]}),
    );
}

#[test]
fn components_show_in_plan_from_above_in_3d_and_beyond_a_section() {
    let mut s = Session::new();
    table(&mut s);
    let id = run(&mut s, "component", json!({"family": "Table", "at": [1000, 1000]}))["id"].as_u64().unwrap();
    let view = crate::render::View { scale: 0.5, cx: 1500.0, cy: 1300.0, width: 1000, height: 800 };
    let lines = |s: &Session| -> Vec<P2> {
        let d = crate::render::draw(&s.model, 100000, view);
        d.prims
            .iter()
            .zip(&d.owners)
            .filter_map(|(p, o)| match p {
                crate::render::Prim::Line { pts, .. } if *o == Some(id) => Some(pts.iter().map(|q| view.mm(*q)).collect::<Vec<_>>()),
                _ => None,
            })
            .flatten()
            .collect()
    };
    // From above: the top's edges; the legs under it, hidden.
    let on_top = |p: &P2| {
        (((p.0 - 1000.0).abs() < 3.0 || (p.0 - 2000.0).abs() < 3.0) && (997.0..=1603.0).contains(&p.1))
            || (((p.1 - 1000.0).abs() < 3.0 || (p.1 - 1600.0).abs() < 3.0) && (997.0..=2003.0).contains(&p.0))
    };
    let seen = lines(&s);
    assert!(!seen.is_empty() && seen.iter().all(on_top), "{seen:?}");
    let d = crate::render::draw(&s.model, 100000, view);
    assert_eq!(d.pick(view.px(P2(1500.0, 1000.0)), 3.0), Some(id), "picked by its edge");
    // Over the cut plane, a plan doesn't show it.
    run(&mut s, "modify", json!({"id": id, "offset": 1500}));
    assert!(lines(&s).is_empty());
    run(&mut s, "modify", json!({"id": id, "offset": 0}));
    // In 3D, each form a solid, the top in its material's colour.
    let solids = crate::view3d::shaded(&s.model);
    let colours: Vec<[u8; 3]> = solids.iter().filter(|x| x.owner() == id).map(|x| x.color()).map(|c| [c.0, c.1, c.2]).collect();
    assert_eq!(colours.len(), 3);
    assert!(colours.contains(&s.model.materials["Timber"].color), "{colours:?}");
    // A section across it, looking north: it shows beyond the line, and none of it is cut.
    let sec = run(&mut s, "section", json!({"start": [0, 1300], "end": [3000, 1300]}))["id"].as_u64().unwrap();
    let sview = crate::render::section_fit(&s.model, sec, 1000, 800);
    let d = crate::render::section(&s.model, sec, sview);
    assert!(d.prims.iter().zip(&d.owners).all(|(p, o)| *o != Some(id) || !matches!(p, crate::render::Prim::Fill { .. })), "cut");
    let (cam, clip) = crate::render::section_camera(&s.model, sec, sview).unwrap();
    let image = crate::view3d::Scene::new(&s.model).cut(&cam, &clip, &[]);
    let p = sview.px(P2(1500.0, 735.0));
    assert_eq!(image.pick(f64::from(p.0), f64::from(p.1)), Some(id), "the top's middle, beyond the line");
}

#[test]
fn a_new_project_comes_with_a_library_of_families() {
    use crate::language::Language;
    for language in Language::ALL {
        let m = Model::new_in("x", language);
        assert_eq!(m.check(), Ok(()), "{language:?}");
        let names: Vec<&str> = m.families.keys().map(String::as_str).collect();
        assert_eq!(
            names.len(),
            10 + 6 + crate::fixtures::library(language).len(),
            "{names:?}: furniture, fittings, a door's and a window's of three kinds each, and the tabs' fixtures"
        );
        // Every type of every family places and builds: a component's as one, a door's or a window's in a wall's opening.
        let mut s = Session::new();
        s.reset(m.clone(), None);
        run(&mut s, "wall", json!({"points": [[-30000, 5000], [30000, 5000]]}));
        let mut along = -27500.0;
        for (family, f) in &m.families {
            for t in f.types.keys().filter(|_| f.category.hosted()) {
                let kind = if f.category == crate::family::Category::Door { "door" } else { "window" };
                along += 2500.0;
                let id = run(&mut s, kind, json!({"at": [along, 5000], "family": family, "type": t}))["id"].as_u64().unwrap();
                let (pieces, strokes) = s.model.opening_local(&s.model.elements[&id]).unwrap_or_else(|| panic!("{family}: {t}"));
                assert!(!pieces.is_empty() && (f.category != crate::family::Category::Door || !strokes.is_empty()), "{family}: {t}");
            }
            for t in f.types.keys().filter(|_| !f.category.hosted()) {
                let id = run(&mut s, "component", json!({"family": family, "type": t, "at": [0, 0]}))["id"].as_u64().unwrap();
                let Some(crate::model::Element::Component(c)) = s.model.elements.get(id) else { panic!("no component") };
                assert!(!s.model.component_pieces(c).is_empty(), "{family}: {t}");
            }
        }
    }
    let pt = Model::new_in("x", Language::Portuguese);
    assert!(pt.families.contains_key("Cama") && pt.families["Cama"].parameters.iter().any(|p| p.name == "Largura"));
    // A table's legs and a chair's legs and back stand within its top or its seat.
    let m = Model::new("x");
    for family in ["Table", "Chair"] {
        let f = &m.families[family];
        for t in f.types.keys() {
            let pieces = f.pieces(&f.values(t, &Default::default()).unwrap()).unwrap();
            let (lo, hi) = pieces[0].outline.iter().fold((P2(f64::MAX, f64::MAX), P2(f64::MIN, f64::MIN)), |(lo, hi), q| {
                (P2(lo.0.min(q.0), lo.1.min(q.1)), P2(hi.0.max(q.0), hi.1.max(q.1)))
            });
            for q in pieces.iter().flat_map(|p| &p.outline) {
                assert!(q.0 >= lo.0 - 1e-6 && q.0 <= hi.0 + 1e-6 && q.1 >= lo.1 - 1e-6 && q.1 <= hi.1 + 1e-6, "{family} {t}: {q:?}");
            }
        }
    }
    // A double bed has two pillows; a single bed, one.
    let mut s = Session::new();
    let bed = |s: &mut Session, t: &str| {
        let id = run(s, "component", json!({"family": "Bed", "type": t, "at": [0, 0]}))["id"].as_u64().unwrap();
        let Some(crate::model::Element::Component(c)) = s.model.elements.get(id) else { panic!("no component") };
        s.model.component_pieces(c).len()
    };
    assert_eq!(bed(&mut s, "1600 x 2000") - bed(&mut s, "900 x 2000"), 1);
}

#[test]
fn a_family_file_brings_only_a_family_and_nulls_take_values_away() {
    let mut s = Session::new();
    // A file that says to delete or rename: only the family it holds comes in.
    let path = std::env::temp_dir().join(format!("bimcraft-chair-{}.json", std::process::id()));
    std::fs::write(&path, json!({"name": "Chair", "delete": true, "rename": "Gone", "types": {"Tall": {"Height": 1100}}}).to_string())
        .unwrap();
    run(&mut s, "family", json!({"path": path}));
    let _ = std::fs::remove_file(&path);
    assert!(s.model.families.contains_key("Chair") && !s.model.families.contains_key("Gone"));
    assert_eq!(s.model.families["Chair"].types["Tall"]["Height"], json!(1100));
    // A null in a new type, in a component's own values, or in `modify`: no value, the family's.
    run(&mut s, "family", json!({"name": "Chair", "types": {"Low": {"Height": null, "Width": 500}}}));
    assert_eq!(s.model.families["Chair"].types["Low"], std::collections::BTreeMap::from([("Width".to_string(), json!(500))]));
    run(
        &mut s,
        "family",
        json!({"name": "Stool", "parameters": [{"name": "Seat", "instance": true, "value": 450}], "types": {"A": {}}, "forms": [{"profile": [[0, 0], [400, 0], [400, 400], [0, 400]], "to": "Seat"}]}),
    );
    let id = run(&mut s, "component", json!({"family": "Stool", "at": [0, 0], "params": {"Seat": null}}))["id"].as_u64().unwrap();
    run(&mut s, "modify", json!({"id": id, "params": {"Seat": null}}));
    assert_eq!(crate::element_json(&s.model, id)["values"]["Seat"], json!(450.0));
}

#[test]
fn an_undo_step_shares_a_familys_triangles() {
    let mut s = Session::new();
    let tri = json!([[0, 0, 0], [100, 0, 0], [0, 100, 0]]);
    run(&mut s, "family", json!({"name": "Mesh", "types": {"A": {}}, "forms": [{"mesh": [tri, tri]}]}));
    let copy = (*s.model).clone();
    assert!(std::sync::Arc::ptr_eq(&copy.families["Mesh"].forms[0].mesh, &s.model.families["Mesh"].forms[0].mesh));
}

#[test]
fn a_mirrored_component_is_its_familys_mirror_image() {
    let mut s = Session::new();
    // An L: what's mirrored shows.
    run(
        &mut s,
        "family",
        json!({"name": "L", "types": {"A": {}}, "forms": [{"profile": [[0, 0], [1000, 0], [1000, 300], [300, 300], [300, 1000], [0, 1000]], "to": 500}]}),
    );
    let id = run(&mut s, "component", json!({"family": "L", "at": [2000, 0]}))["id"].as_u64().unwrap();
    let outline = |s: &Session, id: u64| {
        let Some(crate::model::Element::Component(c)) = s.model.elements.get(id) else { panic!("no component") };
        let mut pts: Vec<(i64, i64)> =
            s.model.component_pieces(c)[0].outline.iter().map(|q| (q.0.round() as i64, q.1.round() as i64)).collect();
        pts.sort();
        pts
    };
    let before = outline(&s, id);
    run(&mut s, "mirror", json!({"ids": [id], "axis": [[0, 0], [0, 1000]]}));
    let mirrored: Vec<(i64, i64)> = {
        let mut m: Vec<(i64, i64)> = before.iter().map(|(x, y)| (-x, *y)).collect();
        m.sort();
        m
    };
    assert_eq!(outline(&s, id), mirrored);
    assert_eq!(crate::element_json(&s.model, id)["mirrored"], json!(true));
    // Turned and moved, it keeps its shape.
    run(&mut s, "rotate", json!({"ids": [id], "about": [0, 0], "angle": 180}));
    run(&mut s, "move", json!({"ids": [id], "by": [100, 0]}));
    let turned: Vec<(i64, i64)> = {
        let mut m: Vec<(i64, i64)> = mirrored.iter().map(|(x, y)| (-x + 100, -y)).collect();
        m.sort();
        m
    };
    assert_eq!(outline(&s, id), turned);
    // Mirrored or not, a chair's triangles face out, and hold as much.
    let volume = |s: &Session, id: u64| -> f64 {
        let Some(crate::model::Element::Component(c)) = s.model.elements.get(id) else { panic!("no component") };
        s.model
            .component_local_pieces(c)
            .iter()
            .flat_map(|p| &p.tris)
            .map(|[a, b, c]| crate::view3d::dot(*a, crate::view3d::cross(*b, *c)) / 6.0)
            .sum()
    };
    let chair = run(&mut s, "component", json!({"family": "Chair", "at": [0, 0]}))["id"].as_u64().unwrap();
    let mirrored = run(&mut s, "component", json!({"family": "Chair", "at": [0, 0], "mirrored": true}))["id"].as_u64().unwrap();
    assert!(
        volume(&s, chair) > 0.0 && (volume(&s, chair) - volume(&s, mirrored)).abs() < 1e-3,
        "{} {}",
        volume(&s, chair),
        volume(&s, mirrored)
    );
}

#[test]
fn hostile_params_never_panic_or_break_the_model() {
    let hostile = [
        json!(null),
        json!([]),
        json!("x"),
        json!({"points": "x"}),
        json!({"points": [[0, 0]]}),
        json!({"points": [[0, 0, 0], [1000, 0, 0]], "diameter": 12, "count": 1000, "step": [1e9, 1e9, 1e9]}),
        json!({"points": [[0, 0, 0], [1000, 0, 0]], "diameter": f64::MAX, "count": 1e300}),
        json!({"points": [[0, 0, 0], [1000, 0, 0]], "diameter": 12, "host": 100000, "level": [], "grade": 5}),
        json!({"host": 100002, "cover": 25, "bottom": {"count": 1e300, "diameter": 1e300}, "stirrup": {"diameter": 5, "spacing": 1e-300}}),
        json!({"host": 100002, "cover": [], "bars": {"diameter": 16, "x": 1e300}, "mesh": {"x": {"diameter": 8, "spacing": -1}}, "anchor": {"below": 1e300}}),
        json!({"host": 100002, "cover": {"top": 1e300, "bottom": -1, "sides": "x"}, "bottom": {"x": {"diameter": 8, "spacing": 5e-324}}, "stirrup": 5}),
        json!({"host": 100002, "cover": 25, "stirrup": {"diameter": 5, "spacing": 150, "ends": {"length": 1e300, "spacing": 1}, "hook": 1e300}, "skin": {"diameter": 8, "spacing": 1e-9}}),
        json!({"points": [[0, 0], [1e308, 1e308]]}),
        json!({"points": [[0, 0], [-1e300, 0]], "closed": true}),
        json!({"at": [1e300, -1e300]}),
        json!({"wall": -1, "offset": 1e300}),
        json!({"wall": 100002, "offset": -5e8, "sill": 1e9}),
        json!({"id": u64::MAX}),
        json!({"ids": [99999, 100000, 100001, 100002, 100002]}),
        json!({"point": [f64::MAX, 0]}),
        json!({"elevation": "high"}),
        json!({"elevation": 1e300}),
        json!({"category": "wall", "name": "", "thickness": -5}),
        json!({"category": "window", "name": "W", "width": 1e12, "height": 1}),
        json!({"boundary": [[0, 0], [1, 0], [0, 0]]}),
        json!({"start": [0, 0], "end": [0, 0]}),
        json!({"level": "nowhere"}),
        json!({"path": 5}),
        json!({"path": "/nonexistent/scan.las", "units": "furlong", "spacing": -1}),
        json!({"cloud": "nope", "origin": "file", "at": [1e300, 0]}),
        json!({"path": "/nonexistent/plan.dxf", "units": "furlong", "scale": -1, "position": 3}),
        json!({"cad": "nope", "layers": [1, null], "colors": {}, "view": "nowhere", "link": "yes"}),
        json!({"id": 100002, "hide": "x", "show": [[]], "reach": -1}),
        json!({"id": 100002, "level": [], "start": "x"}),
        json!({"id": 100002, "top_level": 100002}),
        json!({"limit": -1, "offset": 1e30}),
        json!({"ids": [100002, 100003], "by": [9.99e8, 0]}),
        json!({"ids": [100002, 100003, 100004, 100005], "by": [f64::MAX, 1]}),
        json!({"ids": [100003], "by": "x"}),
        json!({"ids": [100002, 100003], "about": [0, 0], "angle": 1e300}),
        json!({"ids": [100002, 100003], "about": [0, 0], "angle": 45, "copy": true}),
        json!({"ids": [100002, 100003], "axis": [[0, 0], [0, 0]]}),
        json!({"ids": [100002, 100004], "axis": [[0, 0], [1, 1e300]], "copy": true}),
        json!({"ids": [100002, 100004], "keep": "x"}),
        json!({"ids": [100002, 100003], "keep": [[0, 0]]}),
        json!({"id": 100002, "at": [1e300, 0]}),
        json!({"id": 100002, "at": [0, 0]}),
        json!({"from": 100002, "ids": [100003, 100004, 100002]}),
        json!({"from": u64::MAX, "ids": [100002]}),
        json!({"refs": "x", "at": [0, 0]}),
        json!({"refs": [[0, 0], [8000, 0]], "at": [1e300, 0]}),
        json!({"refs": [{"id": 100002}, {"id": 100002}], "at": [0, 0]}),
        json!({"refs": [{"id": 100002}, {"id": 100003}], "at": [0, 0]}),
        json!({"refs": [{"id": 100002, "face": "up"}, {"id": 100004}], "at": [0, 0]}),
        json!({"refs": [{"id": u64::MAX}, [0, 6000]], "at": [0, 0]}),
        json!({"ids": [100002], "offset": [1e300, 0]}),
        json!({"ids": [100002, 100003], "leader": 5}),
        json!({"view": "{3D}", "azimuth": 1e300}),
        json!({"view": "{3D}", "elevation": "x", "width": 1e9}),
        json!({"facing": "up"}),
        json!({"at": [1e300, 0]}),
        json!({"start": [0, 0], "end": [1000, 0], "depth": -5}),
        json!({"start": [0, 0], "end": [1000, 0], "name": ""}),
        json!({"sheet": 100002, "view": 100000}),
        json!({"sheet": "A101", "view": "nowhere", "scale": 0}),
        json!({"size": "A9", "number": ""}),
        json!({"view": 100000, "scale": 1e300, "at": [1e300, 0]}),
        json!({"at": [0, 0], "height": -5, "angle": 1e300}),
        json!({"start": [0, 0], "end": [0, 0], "width": 0, "tread": 1e300}),
        json!({"slope": 89.99, "overhang": -1e9}),
        json!({"slope": 1e300, "boundary": [[0, 0], [1, 0], [0, 0]]}),
        json!({"id": 100000, "view_range": {"cut": 1e300}}),
        json!({"id": 100000, "view_range": {"cut": -5, "bottom": 10, "top": "x"}}),
        json!({"id": 100000, "view_range": [1, 2]}),
        json!({"name": "T", "from": 1, "scale": 1e300, "detail": 5, "visual_style": []}),
        json!({"name": "T", "controls": ["scale", "scale", "style"], "scale": null, "detail": "coarse"}),
        json!({"view": 100000, "scale": [], "detail": {}, "style": 7}),
        json!({"id": 100000, "crop": [[5, 5], [0, 0]]}),
        json!({"view": 100000, "region": [[0, 0], [0, 0]]}),
        json!({"view": 100000, "region": "x"}),
        json!({"view": 100000, "region": [[-1e300, 0], [1e300, 1]]}),
        json!({"view": "{3D}", "region": null}),
        json!({"text": "", "at": [0, 0]}),
        json!({"text": "x", "at": [0, 0], "size": 1e300, "angle": f64::MAX, "leader": [1e300, 0]}),
        json!({"text": "x", "at": [0, 0], "view": 100002}),
        json!({"at": [4000, 3000], "view": 100002, "host": u64::MAX}),
        json!({"kind": "angular", "refs": [{"id": 100002}, {"id": 100004}], "at": [0, 0]}),
        json!({"kind": "angular", "refs": [{"id": 100002}, {"id": 100003}, {"id": 100005}], "at": [0, 0]}),
        json!({"kind": "radial", "refs": [{"id": 100002}], "at": [0, 0]}),
        json!({"kind": 7, "refs": [{"id": 100002}, {"id": 100003, "face": "end"}], "at": [0, 0]}),
        json!({"kind": "linear", "orient": "up", "refs": [{"id": 100002, "point": "start"}, {"id": 100003, "point": "end"}], "at": [0, 0]}),
        json!({"kind": "linear", "refs": [{"id": 100002, "point": "middle"}, {"endpoint": [1e300, 0]}], "at": [f64::MAX, 0]}),
        json!({"refs": [{"endpoint": "x"}, {"endpoint": [0, 0], "id": 1}], "at": [0, 0], "type": "", "prefix": 5}),
        json!({"kind": "diameter", "refs": [[0, 0]], "at": [0, 0], "above": "a\nb", "equality": "x"}),
        json!({"kind": "arc_length", "refs": [{"id": 100002, "face": "start"}], "at": [1e300, 1e300]}),
        json!({"name": "D", "tick": "star", "tick_size": 1e300, "text_size": -1, "decimals": 300, "units": "furlong"}),
        json!({"name": "D", "like": "nope", "witness": 5, "line_pen": 99, "color": "x", "font": "", "string": []}),
        json!({"name": "Diagonal - 2.5mm Arial", "rename": "Arrow - 2.5mm Arial", "delete": "x", "unknown": 1}),
        json!({"name": "Arrow - 2.5mm Arial", "delete": true}),
        json!({"id": 100007, "segment": 1e300, "prefix": "x", "below": 5}),
        json!({"id": 100007, "segment": -1, "replace": "a\nb", "nope": 1}),
        json!({"id": 100007, "equal": "x", "locked": [1e300, -1, "a"]}),
        json!({"id": 100007, "equal": true, "locked": [0, 0, 1]}),
        json!({"sheet": "A101", "view": "Room Schedule", "scale": -1}),
        json!({"sheet": "A101", "view": "{3D}", "azimuth": 1e300, "elevation": 90}),
        json!({"id": 100000, "name": "{3D}"}),
        json!({"length": 5}),
        json!({"length": "parsecs"}),
        json!({"length": "ft-in", "width": "10'"}),
        json!({"code": 5}),
        json!({"code": "klingon"}),
        json!({"language": "xx", "name": ""}),
        json!({"points": [[0, 0], [1000, 0]], "arc": 359.9999}),
        json!({"points": [[0, 0], [1000, 0]], "arc": -1e300, "location": 5}),
        json!({"points": [[0, 0], [1000, 0]], "attach_top": 100002, "attach_base": u64::MAX}),
        json!({"category": "wall", "name": "L", "layers": [{"thickness": 1e300}, {"thickness": 0, "function": "membrane"}]}),
        json!({"category": "wall", "name": "L", "layers": "x"}),
        json!({"category": "wall", "name": "Generic - 200mm", "layers": [{"thickness": 5, "material": "x", "extra": 1}]}),
        json!({"name": "M", "color": "#zzzzzz", "cut": 5, "surface": "brick", "rename": "", "delete": "yes"}),
        json!({"name": "Brick", "rename": "Brick", "color": [1, 2]}),
        json!({"name": "Glass", "transparency": 101}),
        json!({"name": "Glass", "transparency": -5}),
        json!({"name": "Glass", "transparency": 1e300}),
        json!({"name": "Glass", "transparency": "x"}),
        json!({"name": "Glass", "transparency": 50}),
        json!({"category": "floor", "name": "F", "layers": [{"thickness": 1e300, "material": "Concrete"}]}),
        json!({"category": "roof", "name": "R", "layers": [{"thickness": 0, "function": "membrane"}]}),
        json!({"category": "ceiling", "name": "C", "layers": "x", "grid": -5}),
        json!({"ids": [100002, 100003], "arc": 1e300}),
        json!({"id": 100002, "location": "exterior-core", "arc": 179.99}),
        json!({"id": 100002, "attach_top": 100002}),
        json!({"id": 100002, "arc": 90, "location": "interior-finish", "start": [0, 0]}),
        json!({"boundary": [[0, 0], [4000, 0], [4000, 4000]], "slope": 89, "arrow": [[0, 0], [0, 0]]}),
        json!({"boundary": [[0, 0], [4000, 0], [4000, 4000]], "slope": 5, "arrow": "x", "offset": 1e300}),
        json!({"point": [4000, 3000], "offset": 1e300, "slope": -1e300}),
        json!({"point": [4000, 3000], "top_level": u64::MAX, "height": -1, "base_offset": f64::MAX}),
        json!({"boundary": [[1000, 1000], [2000, 1000], [2000, 2000]], "level": 100001, "top_level": 100000}),
        json!({"view": 100000, "ceiling": "yes"}),
        json!({"view": "Level 1", "ceiling": true, "width": 1e9}),
        json!({"category": "ceiling", "name": "T", "thickness": 20, "grid": 1e-9}),
        json!({"category": "ceiling", "name": "T", "thickness": 1e300, "grid": -600}),
        json!({"id": 100002, "slope": 5, "arrow": [[0, 0], [1, 1]]}),
        json!({"ids": [100002, 100003], "offset": 1e300, "grid": 600}),
        json!({"start": [0, 0], "landings": [{"at": 1e300, "turn": 90}]}),
        json!({"start": [0, 0], "landings": [{"at": 8, "turn": "left"}], "slope": 1e-300}),
        json!({"start": [0, 0], "landings": "x", "height": 1e9, "slope": 50}),
        json!({"start": [0, 0], "height": 600, "landings": [{"at": 300, "turn": 180}, {"at": 300, "turn": -180}]}),
        json!({"host": 100002, "side": "up"}),
        json!({"host": u64::MAX}),
        json!({"path": [[0, 0], [1e9, 1e9]], "base_offset": 1e300}),
        json!({"path": [[0, 0], [0, 0], [0, 0]]}),
        json!({"category": "railing", "name": "R", "height": 1e300, "spacing": 1e-9}),
        json!({"id": 100002, "landings": [{"at": 2, "turn": 90}]}),
        json!({"gables": [usize::MAX], "slope": 45, "boundary": [[0, 0], [4000, 0], [0, 4000]]}),
        json!({"extrusion": {"line": [[0, 0], [1e300, 0]], "profile": [[0, 0], [1, 1e300]], "to": 1e300}}),
        json!({"extrusion": {"line": [[0, 0], [0, 0]], "profile": "x"}}),
        json!({"extrusion": 5, "join": 100002, "openings": [[[0, 0]]]}),
        json!({"id": 100002, "join": 100002, "offset": 1e300}),
        json!({"host": 100002, "at": [4000, 3000], "width": -1, "height": 1e300, "slope": 89.9}),
        json!({"host": u64::MAX, "at": [1e300, 0], "window": "nowhere"}),
        json!({"id": 100002, "openings": [[[0, 0], [1e9, 0], [0, 1e9]]], "gables": [0, 0, 1]}),
        json!({"courtyards": [[[0, 0], [1e9, 0], [0, 1e9]]], "gables": [7]}),
        json!({"id": 100002, "courtyards": [[[1000, 1000], [2000, 1000], [2000, 2000]], [[1000, 1000], [2000, 1000], [2000, 2000]]]}),
        json!({"courtyards": [[[1, 1]]], "slope": 1e-300}),
        json!({"courtyards": "x", "extrusion": {"line": [[0, 0], [1000, 0]], "profile": [[0, 0], [1000, 500]], "to": 1000}}),
        json!({"point": [4000, 3000], "openings": [[[0, 0], [1e9, 0], [0, 1e9]], [[1, 1]]]}),
        json!({"family": "nowhere", "at": [0, 0]}),
        json!({"family": 5, "type": [], "at": [1e300, 0], "angle": f64::MAX, "offset": -1e300, "params": "x"}),
        json!({"name": "F", "types": {"A": {}}, "forms": [{"profile": [[0, 0], [1e300, 0], [0, 1]], "to": 1}]}),
        json!({"name": "F", "types": {"A": {}}, "parameters": [{"name": "W", "formula": "W + 1"}], "forms": [{"circle": {"centre": [0, 0], "radius": "W"}, "to": 1}]}),
        json!({"name": "F", "types": {"A": {"Nope": 1}}, "forms": [{"profile": "x"}]}),
        json!({"name": "F", "types": {"A": {}}, "forms": [{"mesh": [[[0, 0, 0], [1e300, 0, 0], [0, 1, 0]]]}]}),
        json!({"name": "F", "types": {"A": {}}, "forms": [{"plane": "xz", "profile": [[1, 0], [2, 0], [2, 1]], "axis": [[0, 0], [1e300, 1e300]], "to": 1e300}]}),
        json!({"name": "F", "types": {"A": {}}, "forms": [{"profile": [[1, 0], [2, 0], [2, 1]], "axis": [[0, 0], [0, 0]], "to": 360}]}),
        json!({"name": "F", "types": {"A": {}}, "forms": [{"profile": [[1, 0], [2, 0], [2, 1]], "axis": "x", "to": 360}]}),
        json!({"name": "F", "types": {"A": {}}, "forms": [{"profile": [[1, 0], [2, 0], [2, 1]], "axis": [[0, 0], [0, 1]], "to": f64::MAX, "void": "x"}]}),
        json!({"name": "F", "types": {"A": {}}, "forms": [{"profile": [[0, 0], [1e9, 0], [1e9, 1e9]], "to": 1e9}, {"circle": {"centre": [0, 0], "radius": 1e9}, "from": -1e9, "to": 1e9, "void": true}]}),
        json!({"name": "F", "types": {"A": {}}, "forms": [{"circle": {"centre": [0, 0], "radius": 5}, "to": 1}, {"plane": "yz", "circle": {"centre": [0, 0], "radius": 1e-9}, "to": 1, "void": true}]}),
        json!({"name": "F", "types": {}, "parameters": [{"name": " ", "kind": "material", "value": 5}]}),
        json!({"name": "", "delete": true, "rename": ""}),
        json!({"path": "/nonexistent/family.json"}),
        json!({"name": "P", "kind": "length", "categories": ["wall", "wall", "clouds"], "rename": "Q"}),
        json!({"name": "P", "kind": "yesno", "categories": ["door", "component"], "rename": "1 bad"}),
        json!({"name": "Count", "categories": ["room"]}),
        json!({"ids": [100002, 100003], "params": {"X": 1e300, "Y": [1, 2]}}),
        json!({"name": "Room Schedule", "fields": [{"name": "X", "formula": "X + 1"}, {"name": "Y", "formula": "if(1, 2)"}], "sort": "x"}),
        json!({"name": "S", "category": "walls", "fields": ["Length", {"name": "Big", "formula": "((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((Length"}], "grouped": true}),
        json!({"name": "S", "category": "stairs", "filters": [{"field": "Risers", "op": "greater", "value": 1e300}], "sort": [{"field": "Risers", "descending": "yes"}]}),
        json!({"category": "doors", "fields": [{"name": "Count", "total": true, "heading": 5}, {"name": "Z", "formula": "1/0", "total": true}]}),
        json!({"name": "Door Schedule", "category": "rooms", "path": "/nonexistent/x.csv"}),
        json!({"name": "W", "category": "walls", "fields": [{"name": "A", "formula": "B"}, {"name": "B", "formula": "C"}, {"name": "C", "formula": "A"}]}),
        json!({"from": [1e300, -1e300], "to": [f64::MAX, 0]}),
        json!({"from": [0, 0], "to": "x"}),
        json!({"view": "{3D}", "hide": ["walls", 5]}),
        json!({"view": 100000, "hide": 5, "show": [null]}),
        json!({"view": 100000, "hide": ["walls"], "hide_elements": [3, u64::MAX], "show_elements": "x"}),
        json!({"view": 100000, "categories": 5}),
        json!({"view": 100000, "categories": {"walls": 5, "doors": {"cut": {"weight": 1e300, "dash": 5}}}}),
        json!({"view": 100000, "categories": {"walls": {"cut": {"color": "red"}, "projection": {"fill": [1, 2]}, "transparency": 300}}}),
        json!({"view": 100000, "categories": {"walls": {"cut": {"weight": -1}, "halftone": "yes"}}}),
        json!({"view": 100000, "elements": {"100002": 5, "-1": {}, "x": {"halftone": true}, "99999": {"halftone": true}}}),
        json!({"view": 100000, "elements": {"100002": {"cut": {"weight": f64::MAX}}}}),
        json!({"view": 100000, "filters": 5}),
        json!({"view": 100000, "filters": [{}, {"filter": 5}, {"filter": "x", "cut": []}]}),
        json!({"view": 100000, "filters": [{"filter": "x"}, {"filter": "x"}], "template": ["x"]}),
        json!({"view": 100000, "template": 5, "reset": "yes"}),
        json!({"view": "{3D}", "template": "nowhere", "reset": true}),
        json!({"name": "F", "categories": "walls"}),
        json!({"name": "F", "categories": [5, null, "walls"], "rules": "x"}),
        json!({"name": "F", "categories": ["walls"], "rules": [{"field": "Type"}, {"op": "equals"}, 5]}),
        json!({"name": "F", "categories": ["walls", "doors", "rooms"], "rules": [{"field": "Length", "op": "greater", "value": 1e300}]}),
        json!({"name": "F", "categories": ["levels"], "rules": [{"field": "Name", "op": "contains", "value": "x"}], "delete": "yes"}),
        json!({"name": "T", "controls": [1], "view_range": {"cut": 1e300}}),
        json!({"name": "T", "controls": ["range"], "view_range": [1, 2], "from": [1]}),
        json!({"name": "T", "from": u64::MAX, "controls": "all"}),
        json!({"name": "T", "from": "Level 1", "view_range": {"cut": -5, "top": "x"}}),
        json!({"name": "T", "from": 100000, "hide": ["walls"], "hide_elements": [100002], "template": "T"}),
        json!({"name": "T", "controls": ["range", "range"], "view_range": {"depth": 5, "bottom": 1}}),
        json!({"name": 5}),
        json!({"name": "P", "before": 5, "after": []}),
        json!({"name": "P", "before": "Existing", "after": "Existing"}),
        json!({"name": "P", "rename": 5, "delete": "yes", "into": 7}),
        json!({"name": "Existing", "delete": true, "into": "New Construction"}),
        json!({"name": "New Construction", "delete": true, "into": "Existing", "now": "New Construction"}),
        json!({"name": "Existing", "rename": "New Construction"}),
        json!({"now": 5}),
        json!({"now": "nowhere"}),
        json!({"now": "A: b", "overrides": {"existing": {"hidden": true}, "future": 5}}),
        json!({"overrides": 5}),
        json!({"overrides": {"demolished": {"transparency": 300, "cut": {"weight": 1e300}}}}),
        json!({"overrides": {"new": 5, "temporary": null}}),
        json!({"name": "F", "new": "sometimes"}),
        json!({"name": "Show All", "delete": true}),
        json!({"name": "F", "demolished": 5, "existing": null, "temporary": [1]}),
        json!({"set": "S", "options": "x"}),
        json!({"set": "S", "options": [1, null]}),
        json!({"set": "S", "options": ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q"]}),
        json!({"set": "S", "options": ["a"], "primary": 5}),
        json!({"set": "S", "options": ["a", "a"]}),
        json!({"set": ":", "options": ["x"]}),
        json!({"set": "S", "accept": true, "delete": true}),
        json!({"set": "S", "options": ["a"], "rename": "T", "option": "zzz"}),
        json!({"set": "S", "options": ["a"], "now": "S: a"}),
        json!({"set": "S", "delete": true, "now": 5}),
        json!({"view": 100000, "phase": 5, "phase_filter": ["x"], "options": 5}),
        json!({"view": 100000, "options": {"S": 5, "T": null}}),
        json!({"view": 100000, "phase": "nowhere", "phase_filter": "nowhere"}),
        json!({"view": 100000, "options": {"nowhere": "x"}}),
        json!({"view": 100000, "options": {"nowhere": "x"}}),
        json!({"view": 100000, "scale": "x"}),
        json!({"view": 100000, "scale": -1}),
        json!({"view": 100000, "detail": 5}),
        json!({"view": 100000, "detail": "ultra"}),
        json!({"view": "{3D}", "style": 7}),
        json!({"view": "{3D}", "style": "wireframe"}),
        json!({"id": 100002, "params": {"Phase Created": 5, "Phase Demolished": [], "Design Option": {}}}),
        json!({"id": 100002, "params": {"Phase Created": "New Construction", "Phase Demolished": "Existing"}}),
        // The masses the sweep starts with are 7 and, lofted, 8.
        json!({"boundary": [[0, 0], [1e300, 0], [0, 1e300]], "height": 1e300}),
        json!({"boundary": [[0, 0], [1, 0], [0, 1]], "top_level": 100098, "name": 5}),
        json!({"mass": u64::MAX}),
        json!({"mass": -1}),
        json!({"mass": 100002}),
        json!({"mass": 100006, "faces": [99, -1, "x", 1e300]}),
        json!({"mass": 100006, "faces": "top"}),
        json!({"mass": 100006, "faces": [0, 0, 1], "location": 5}),
        json!({"mass": 100006, "type": 5, "slope": 1e300, "overhang": -1e9}),
        json!({"mass": 100006, "type": "Nothing"}),
        json!({"mass": 100006, "prune": 5, "type": 5}),
        json!({"prune": "yes"}),
        json!({"prune": true}),
        json!({"boundary": [[0, 0], [1, 0], [0, 1]], "top": [[0, 0], [1, 0]], "height": 5}),
        json!({"boundary": [[0, 0], [1000, 0], [0, 1000]], "top": [[0, 0], [1e300, 0], [0, 1]], "height": 5, "lean": [1, 1]}),
        json!({"boundary": [[0, 0], [1000, 0], [0, 1000]], "top": [[5, 5], [5, 5], [5, 5]], "height": 5}),
        json!({"boundary": [[0, 0], [1000, 0], [0, 1000]], "lean": [1e300, f64::MAX], "height": 5}),
        json!({"boundary": [[0, 0], [1000, 0], [0, 1000]], "lean": "x", "height": 5}),
        json!({"boundary": [[0, 0], [1000, 0], [0, 1000]], "lean": [3000, -2000], "top_level": 100001}),
        json!({"id": 100006, "top": [[0, 0], [1, 0], [0, 1]]}),
        json!({"id": 100006, "top": "x"}),
        json!({"id": 100006, "top": [[0, 0], [1e300, 0], [0, 1e300], [1, 1]]}),
        json!({"id": 100006, "top": [[10000, 6000], [0, 6000], [0, 0], [10000, 0]]}),
        json!({"id": 100006, "top": [[0, 0], [10000, 0], [10000, 6000], [0, 6000]], "height": 1e-9}),
        json!({"mass": 100007, "faces": [0, 1, 2, 3], "type": "Generic - 200mm"}),
        json!({"mass": 100007, "prune": true}),
        json!({"mass": 100007}),
        json!({"id": 100007, "top": null}),
        json!({"id": 100007, "boundary": [[0, 0], [1, 0], [0, 1]]}),
        json!({"id": 100007, "top": [[0, 0], [1, 0], [0, 1]], "boundary": [[0, 0], [1, 0], [0, 1]]}),
        json!({"id": 100007, "height": 1e-9, "top_level": 100000}),
        json!({"mass": 100006, "ids": [100000]}),
        json!({"ids": [u64::MAX, -1]}),
        json!({"mass": "x"}),
        json!({"mass": 100006}),
        json!({"view": 100000, "show": "x"}),
        json!({"view": "{3D}", "show": 5}),
        json!({"view": 1e300, "show": true}),
        json!({"id": 100002, "params": {"Mass Floor": {"mass": 100006, "level": 100000}, "Mass Face": 5}}),
        json!({"id": 100002, "params": {"Mass Face": {"mass": 100006, "face": "top"}}}),
        json!({"id": 100006, "height": 1e300, "top_level": 100000}),
        json!({"id": 100006, "boundary": [[0, 0], [1e300, 1e300], [0, 1]]}),
        json!({"id": 100006, "level": 1e300, "base_offset": f64::MAX}),
        json!({"id": 100006, "params": {"Phase Created": "Existing"}}),
    ];
    let mut base = Session::new();
    house(&mut base);
    let _ = base.execute("mass", &json!({"boundary": block(), "height": 9000}));
    // Another, lofted, which is 8.
    let _ = base.execute("mass", &json!({"boundary": block(), "height": 9000, "lean": [1500, 500]}));
    // Detailing, one of each (detail lines, regions, a group, a repeating detail, a sketch floor, Linework), and the payloads
    // that name them: some for every command, some for the one they are for.
    let (detailing, targeted) = crate::detail_tests::sweep_payloads(&mut base);
    // A section and an elevation of it, after those, so that no id the payloads name moves: their views are drawn too.
    let _ = base.execute("section", &json!({"start": [-1000, 3000], "end": [9000, 3000]}));
    let _ = base.execute("elevation", &json!({"facing": "south"}));
    let views = |m: &Model| {
        m.elements.values().filter(|e| matches!(e, crate::model::Element::Section(_) | crate::model::Element::Elevation(_))).count()
    };
    assert_eq!(views(&base.model), 2, "the sweep's base has a section and an elevation to draw");
    let hostile: Vec<Value> = hostile.into_iter().chain(detailing.into_iter().map(|(_, p)| p)).collect();
    // Each command against each value, on a copy of the same session; the commands shared out between the cores.
    let cmds: Vec<_> = commands().iter().filter(|c| !matches!(c.id, "open" | "save")).collect();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|scope| {
        for share in 0..threads {
            let (cmds, hostile, base) = (&cmds, &hostile, &base);
            scope.spawn(move || {
                for cmd in cmds.iter().skip(share).step_by(threads) {
                    for p in hostile {
                        // A picture as small as it comes, unless the value is about its size: drawing 1600 x 1000 px each time
                        // took most of the sweep.
                        let small = match p {
                            Value::Object(o) if cmd.id == "render" && !o.contains_key("width") && !o.contains_key("height") => {
                                let mut o = o.clone();
                                o.extend([("width".into(), json!(16)), ("height".into(), json!(16))]);
                                Some(Value::Object(o))
                            }
                            _ => None,
                        };
                        let p = small.as_ref().unwrap_or(p);
                        let mut s = base.clone();
                        let made = s.execute(cmd.id, p).is_ok();
                        assert_eq!(s.model.check(), Ok(()), "{} {p} left a broken model", cmd.id);
                        // What a command made is drawn too (a revision cloud round a circle 1,000 km across once listed
                        // 16 GB of scallops and aborted the plan, and nothing here drew it): the plans, sections and
                        // elevations, as small as they come; a failed command changed nothing.
                        if made && cmd.id != "render" {
                            draw_plans(&s.model);
                        }
                    }
                }
            });
        }
    });
    for (cmd, p) in &targeted {
        let mut s = base.clone();
        let made = s.execute(cmd, p).is_ok();
        assert_eq!(s.model.check(), Ok(()), "{cmd} {p} left a broken model");
        if made {
            draw_plans(&s.model);
        }
    }
}

/// Draws every level's plan and ceiling plan, and every section and elevation, at the smallest size, which builds all that
/// they hold.
fn draw_plans(m: &Model) {
    for (level, _) in m.levels() {
        for d in [crate::render::plan(m, level, 16, 16), crate::render::ceiling_plan(m, level, 16, 16)] {
            let _ = crate::render::png(&d);
        }
    }
    // (Not their pictures: the rasterizer is the plans' too, and a section's drawing costs a third as much without it.)
    for (id, _) in m.elements.iter().filter(|(_, e)| matches!(e, crate::model::Element::Section(_) | crate::model::Element::Elevation(_))) {
        let _ = crate::render::section(m, *id, crate::render::section_fit(m, *id, 16, 16));
    }
}

fn temp(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("bimcraft-test-{}-{name}", std::process::id()))
}

#[test]
fn plans_have_a_view_range_that_modify_merges_into() {
    let mut s = Session::new();
    house(&mut s);
    // Revit's: cut 1200 mm up, the top at the level above, the bottom and the view depth at the level.
    assert_eq!(element(&mut s, 100_000)["view_range"], json!({"top": null, "cut": 1200.0, "bottom": 0.0, "depth": 0.0}));
    run(&mut s, "modify", json!({"id": 100000, "view_range": {"cut": 1500, "top": 2800}}));
    run(&mut s, "modify", json!({"id": 100000, "view_range": {"depth": -500}}));
    assert_eq!(element(&mut s, 100_000)["view_range"], json!({"top": 2800.0, "cut": 1500.0, "bottom": 0.0, "depth": -500.0}));
    // Back to the level above, field by field.
    run(&mut s, "modify", json!({"id": 100000, "view_range": {"top": null}}));
    assert_eq!(element(&mut s, 100_000)["view_range"]["top"], Value::Null);
    // Top ≥ cut plane ≥ bottom ≥ view depth.
    let e = s.execute("modify", &json!({"id": 100000, "view_range": {"bottom": 2000}})).unwrap_err();
    assert!(e.contains("view range"), "{e}");
    // Rooms keep to their walls whatever a plan cuts: raised over the walls, the cut plane leaves the room as it was.
    let room = run(&mut s, "room", json!({"point": [4000, 3000]}))["id"].as_u64().unwrap();
    run(&mut s, "modify", json!({"id": 100000, "view_range": {"cut": 3500, "top": 4000}}));
    assert_eq!(element(&mut s, room)["area_m2"], 48.0);
}

#[test]
fn crop_regions_frame_plans_and_sections() {
    let mut s = Session::new();
    house(&mut s);
    run(&mut s, "sheet", json!({}));
    let vp = run(&mut s, "place", json!({"sheet": "A101", "view": "Level 1"}))["id"].as_u64().unwrap();
    let whole = element(&mut s, vp)["size"][0].as_f64().unwrap();
    // A plan cropped to its west half takes half the paper.
    run(&mut s, "crop", json!({"view": "Level 1", "region": [[-500, -500], [4000, 6500]]}));
    assert_eq!(element(&mut s, 100_000)["crop"], json!([[-500.0, -500.0], [4000.0, 6500.0]]));
    let half = element(&mut s, vp)["size"][0].as_f64().unwrap();
    assert!(half < whole - 30.0, "{half} {whole}");
    // Null uncrops; no region crops to all it shows.
    run(&mut s, "crop", json!({"view": "Level 1", "region": null}));
    assert_eq!(element(&mut s, 100_000)["crop"], Value::Null);
    run(&mut s, "crop", json!({"view": 100000}));
    let c = element(&mut s, 100_000)["crop"].clone();
    assert!(c[0][0].as_f64().unwrap() <= -100.0 && c[1][1].as_f64().unwrap() >= 6100.0, "{c}");
    // A section's crop region is its line's ends, along it, and the heights it shows; what's noted
    // in it stays over the same spot.
    let sec = run(&mut s, "section", json!({"start": [-1000, 3000], "end": [9000, 3000]}))["id"].as_u64().unwrap();
    let note = run(&mut s, "text", json!({"text": "Insulated", "at": [3000, 1000], "view": sec}))["id"].as_u64().unwrap();
    run(&mut s, "crop", json!({"view": "Section 1", "region": [[500, -200], [6000, 2500]]}));
    let e = element(&mut s, sec);
    assert_eq!(
        (e["start"].clone(), e["end"].clone(), e["crop"].clone()),
        (json!([-500.0, 3000.0]), json!([5000.0, 3000.0]), json!([-200.0, 2500.0]))
    );
    assert_eq!(element(&mut s, note)["at"], json!([2500.0, 1000.0]));
    // Corners the wrong way round, and views that don't crop, are refused.
    assert!(s.execute("crop", &json!({"view": 100000, "region": [[4000, 0], [0, 4000]]})).unwrap_err().contains("south-west"));
    assert!(s.execute("crop", &json!({"view": "A101"})).unwrap_err().contains("A101"));
}

#[test]
fn text_notes_go_in_plans_sections_and_sheets() {
    let mut s = Session::new();
    house(&mut s);
    // In the lowest level's plan by default, 2.5 mm capitals on paper.
    let n = run(&mut s, "TX", json!({"text": "Tiled floor\nfalls to the drain", "at": [1000, 5000]}))["id"].as_u64().unwrap();
    let e = element(&mut s, n);
    assert_eq!((id_of(&s, &e["view"]), e["size"].as_f64(), id_of(&s, &e["level"])), (Some(100000), Some(2.5), Some(100000)));
    // In a section along it and up, with a leader; on a sheet, in paper mm.
    let sec = run(&mut s, "section", json!({"start": [-1000, 3000], "end": [9000, 3000]}))["id"].as_u64().unwrap();
    run(&mut s, "sheet", json!({}));
    let cut = run(&mut s, "text", json!({"text": "Insulated", "at": [500, 2500], "view": "Section 1", "leader": [1000, 1500]}));
    assert_eq!(element(&mut s, cut["id"].as_u64().unwrap())["leader"], json!([1000.0, 1500.0]));
    run(&mut s, "text", json!({"text": "General notes", "at": [30, 280], "view": "A101", "type": "5mm"}));
    for (p, what) in [
        (json!({"text": " ", "at": [0, 0]}), "text"),
        (json!({"text": "x", "at": [0, 0], "view": "Nowhere"}), "Nowhere"),
        (json!({"text": "x", "at": [0, 0], "size": 0}), "size"),
        (json!({"text": "x"}), "at"),
    ] {
        let e = s.execute("text", &p).unwrap_err();
        assert!(e.contains(what), "{p}: {e}");
    }
    // A note moves like anything else, and goes with its view.
    run(&mut s, "move", json!({"ids": [n], "by": [500, 0]}));
    assert_eq!(element(&mut s, n)["at"], json!([1500.0, 5000.0]));
    run(&mut s, "delete", json!({"ids": [sec]}));
    assert_eq!(count(&mut s, "text"), 2);
}

#[test]
fn dimensions_measure_to_wall_ends_and_openings() {
    let mut s = Session::new();
    let ids = house(&mut s);
    let door = run(&mut s, "door", json!({"at": [2000, 0]}))["id"].as_u64().unwrap();
    // From the bottom wall's west end (its outline reaches the corner, 100 mm past the centre line)
    // to the door's centre and its jamb toward the wall's end.
    let r =
        run(&mut s, "DI", json!({"refs": [{"id": ids[0], "face": "start"}, {"id": door}, {"id": door, "face": "end"}], "at": [0, -1000]}));
    assert_eq!(r["values"], json!([2100.0, 450.0]));
    let dim = r["id"].as_u64().unwrap();
    // By points: the door's other jamb, then the bottom wall's east end.
    let r = run(&mut s, "dimension", json!({"refs": [[1550, 50], [8100, 50]], "at": [0, -1500]}));
    assert_eq!(r["values"], json!([6550.0]));
    // The door slides along: what measures to it follows.
    run(&mut s, "move", json!({"ids": [door], "by": [500, 0]}));
    assert_eq!(element(&mut s, dim)["values"], json!([2600.0, 450.0]));
    let e = s.execute("dimension", &json!({"refs": [{"id": door, "face": "left"}, {"id": ids[1]}], "at": [0, 0]})).unwrap_err();
    assert!(e.contains("jambs"), "{e}");
}

#[test]
fn angular_and_radial_dimensions_measure_angles_and_radii() {
    let mut s = Session::new();
    let ids = house(&mut s);
    let slant = run(&mut s, "wall", json!({"points": [[0, 0], [3000, 3000]], "type": "Interior - 100mm"}))["ids"][0].as_u64().unwrap();
    // Between the bottom wall and the slanted one, the angle on `at`'s side.
    let r = run(&mut s, "dimension", json!({"kind": "angular", "refs": [{"id": ids[0]}, {"id": slant}], "at": [2000, 800]}));
    assert_eq!(r["values"], json!([45.0]));
    let r = run(&mut s, "dimension", json!({"kind": "angular", "refs": [[4000, 0], [1500, 1500]], "at": [-2000, 800]}));
    assert_eq!(r["values"], json!([135.0]));
    let e = s.execute("dimension", &json!({"kind": "angular", "refs": [{"id": ids[0]}, {"id": ids[2]}], "at": [0, 0]})).unwrap_err();
    assert!(e.contains("parallel"), "{e}");
    let e = s.execute("dimension", &json!({"kind": "spiral", "refs": [{"id": ids[0]}], "at": [0, 0]})).unwrap_err();
    assert!(e.contains("radial"), "{e}");
    // A round column's radius, by id or by a point on its edge; walls have none.
    let col = run(&mut s, "column", json!({"at": [5000, 3000], "type": "Round 300"}))["id"].as_u64().unwrap();
    let r = run(&mut s, "dimension", json!({"kind": "radial", "refs": [{"id": col}], "at": [5600, 3600]}));
    assert_eq!(r["values"], json!([150.0]));
    let radius = r["id"].as_u64().unwrap();
    let r = run(&mut s, "dimension", json!({"kind": "radial", "refs": [[5150, 3000]], "at": [5600, 2400]}));
    assert_eq!(r["values"], json!([150.0]));
    let e = s.execute("dimension", &json!({"kind": "radial", "refs": [{"id": ids[0]}], "at": [0, 0]})).unwrap_err();
    assert!(e.contains("round"), "{e}");
    // It follows its column's type.
    run(&mut s, "type", json!({"category": "column", "name": "Round 300", "width": 500}));
    assert_eq!(element(&mut s, radius)["values"], json!([250.0]));
}

#[test]
fn spot_elevations_read_the_height_of_what_they_stand_on() {
    let mut s = Session::new();
    house(&mut s);
    let floor = run(&mut s, "floor", json!({}))["id"].as_u64().unwrap();
    run(&mut s, "roof", json!({}));
    // In Level 1's plan, the top of the highest thing standing on the level there: the floor.
    let spot = run(&mut s, "EL", json!({"at": [4000, 3000]}));
    assert_eq!((spot["elevation"].as_f64(), id_of(&s, &spot["host"])), (Some(0.0), Some(floor)));
    // In Level 2's, the roof's top 100 mm short of its ridge: 3000 mm in from the walls' faces at 30°, and its thickness upright.
    let roof = run(&mut s, "spot", json!({"at": [4000, 2900], "view": "Level 2"}));
    let (tan, cos) = (30f64.to_radians().tan(), 30f64.to_radians().cos());
    assert!((roof["elevation"].as_f64().unwrap() - (3000.0 + 3000.0 * tan + 300.0 / cos)).abs() < 0.01, "{roof}");
    // In a section, the height of its point.
    run(&mut s, "section", json!({"start": [-1000, 3000], "end": [9000, 3000]}));
    assert_eq!(run(&mut s, "spot", json!({"at": [1000, 2700], "view": "Section 1"}))["elevation"], 2700.0);
    assert!(s.execute("spot", &json!({"at": [20000, 0]})).unwrap_err().contains("nothing"));
    // It follows the model: the level goes up, the floor's top with it; the floor goes, and its spot.
    run(&mut s, "modify", json!({"id": 100000, "elevation": 100}));
    assert_eq!(element(&mut s, spot["id"].as_u64().unwrap())["elevation"], 100.0);
    run(&mut s, "delete", json!({"ids": [floor]}));
    assert_eq!(count(&mut s, "spot"), 2);
}

#[test]
fn schedules_and_the_3d_view_go_on_sheets() {
    let mut s = Session::new();
    house(&mut s);
    run(&mut s, "sheet", json!({}));
    // A schedule by its name: its view comes to be as it's placed.
    let vp = run(&mut s, "place", json!({"sheet": "A101", "view": "Door Schedule"}))["id"].as_u64().unwrap();
    let all = run(&mut s, "elements", json!({"category": "schedule"}));
    assert_eq!((all["count"].as_u64(), all["elements"][0]["of"].as_str()), (Some(1), Some("door")));
    assert_eq!(element(&mut s, vp)["title"], "Door Schedule");
    // The 3D view, from where it's asked to look.
    let v3 = run(&mut s, "place", json!({"sheet": "A101", "view": "{3D}", "azimuth": 30, "elevation": 20}))["id"].as_u64().unwrap();
    let view = id_of(&s, &crate::element_json(&s.model, v3)["view"]).unwrap();
    assert_eq!((element(&mut s, view)["azimuth"].clone(), element(&mut s, view)["elevation"].clone()), (json!(30.0), json!(20.0)));
    // On one sheet each, as every view; a schedule that isn't one is refused.
    run(&mut s, "sheet", json!({}));
    assert!(s.execute("place", &json!({"sheet": "A102", "view": "{3D}"})).unwrap_err().contains("A101"));
    assert!(s.execute("place", &json!({"sheet": "A102", "view": "Furniture Schedule"})).unwrap_err().contains("Furniture"));
    // Printed with the rest; saved and opened again, the same.
    let pdf = temp("schedules.pdf");
    run(&mut s, "save", json!({"path": pdf}));
    assert!(std::fs::read(&pdf).unwrap().starts_with(b"%PDF-"));
    let project = temp("schedules.bimcraft");
    run(&mut s, "save", json!({"path": project}));
    let saved = s.model.clone();
    run(&mut s, "open", json!({"path": project}));
    assert_eq!(*s.model, *saved);
    let _ = (std::fs::remove_file(pdf), std::fs::remove_file(project));
}

#[test]
fn base64_matches_the_standard_vectors() {
    use crate::commands::base64;
    assert_eq!([base64(b""), base64(b"M"), base64(b"Ma"), base64(b"Man")], ["", "TQ==", "TWE=", "TWFu"]);
}

#[test]
fn render_returns_a_png_or_writes_svg_and_png_files() {
    let mut s = Session::new();
    house(&mut s);
    let r = run(&mut s, "render", json!({"width": 320, "height": 200}));
    assert!(r["pngBase64"].as_str().unwrap().starts_with("iVBORw0KGgo"), "PNG magic");
    assert_eq!((r["width"].as_u64(), r["view"].as_str()), (Some(320), Some("Level 1")));
    let svg = temp("plan.svg");
    run(&mut s, "render", json!({"view": "Level 1", "path": svg}));
    assert!(std::fs::read_to_string(&svg).unwrap().starts_with("<svg"));
    let png = temp("plan.png");
    run(&mut s, "save", json!({"path": png}));
    assert!(std::fs::read(&png).unwrap().starts_with(b"\x89PNG"));
    assert_eq!(s.path, None, "exporting an image doesn't make it the project's file");
    let _ = (std::fs::remove_file(svg), std::fs::remove_file(png));
}

#[test]
fn render_draws_the_3d_view_too() {
    let mut s = Session::new();
    house(&mut s);
    let r = run(&mut s, "render", json!({"view": "{3D}", "width": 320, "height": 200, "azimuth": 30, "elevation": 45}));
    assert!(r["pngBase64"].as_str().unwrap().starts_with("iVBORw0KGgo"), "PNG magic");
    assert_eq!((r["width"].as_u64(), r["view"].as_str()), (Some(320), Some("{3D}")));
    let e = s.execute("render", &json!({"view": "{3D}", "path": temp("view.svg")})).unwrap_err();
    assert!(e.contains(".png"), "{e}");
}

#[test]
fn sections_and_elevations_are_views_along_a_line() {
    let mut s = Session::new();
    house(&mut s);
    // A section drawn west to east looks north: to the left of its line.
    let section = run(&mut s, "section", json!({"start": [-1000, 3000], "end": [9000, 3000]}))["id"].as_u64().unwrap();
    let e = element(&mut s, section);
    assert_eq!((e["name"].as_str(), e["looking"].clone(), e["length"].clone()), (Some("Section 1"), json!([0.0, 1.0]), json!(10000.0)));
    // The four building elevations, each outside the walls on its side, looking at them.
    let ids = run(&mut s, "elevation", json!({}))["ids"].clone();
    let ids: Vec<u64> = ids.as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    let names: Vec<Value> = ids.iter().map(|id| element(&mut s, *id)["name"].clone()).collect();
    assert_eq!(names, ["North", "South", "East", "West"]);
    let north = element(&mut s, ids[0]);
    assert_eq!(north["looking"], json!([0.0, -1.0]), "the North elevation shows the north side, looking south");
    assert!(north["start"][1].as_f64().unwrap() > 6100.0, "{north}");
    assert!(s.execute("elevation", &json!({})).unwrap_err().contains("already"));
    // One placed south of the house looks north at it.
    let south = run(&mut s, "elevation", json!({"at": [4000, -5000]}))["id"].as_u64().unwrap();
    let e = element(&mut s, south);
    assert_eq!((e["name"].as_str(), e["looking"].clone()), (Some("South 2"), json!([0.0, 1.0])));
    // A view's name is its own: no other view's, nor a level's.
    for name in ["North", "Level 1"] {
        let e = s.execute("section", &json!({"start": [0, 0], "end": [1000, 0], "name": name})).unwrap_err();
        assert!(e.contains("named"), "{e}");
    }
    // Mirrored, a section looks the mirrored way.
    run(&mut s, "mirror", json!({"ids": [section], "axis": [[0, 0], [1000, 0]]}));
    assert_eq!(element(&mut s, section)["looking"], json!([0.0, -1.0]));
    // Each renders as a PNG; only plans make SVGs.
    let png = run(&mut s, "render", json!({"view": "Section 1", "width": 400, "height": 300}));
    assert!(png["pngBase64"].as_str().unwrap().starts_with("iVBORw0KGgo"), "PNG magic");
    let e = s.execute("render", &json!({"view": "North", "path": temp("north.svg")})).unwrap_err();
    assert!(e.contains(".png"), "{e}");
}

#[test]
fn sheets_hold_views_at_a_scale_and_print_to_pdf() {
    let mut s = Session::new();
    house(&mut s);
    let section = run(&mut s, "section", json!({"start": [-1000, 3000], "end": [9000, 3000]}))["id"].as_u64().unwrap();
    let sheet = run(&mut s, "sheet", json!({}))["id"].as_u64().unwrap();
    let e = element(&mut s, sheet);
    assert_eq!((e["number"].as_str(), e["name"].as_str(), e["paper"].clone()), (Some("A101"), Some("Unnamed"), json!([420.0, 297.0])));
    // Level 1's plan at 1 : 100, by the level's name: its 10 m extent is 100 mm wide, with room around it.
    let vp = run(&mut s, "place", json!({"sheet": "A101", "view": "Level 1"}))["id"].as_u64().unwrap();
    let v = element(&mut s, vp);
    assert_eq!((id_of(&s, &v["view"]), v["scale"].as_f64(), v["title"].as_str()), (Some(100000), Some(100.0), Some("Level 1")));
    let w = v["size"][0].as_f64().unwrap();
    assert!((100.0..140.0).contains(&w), "{w}");
    // A view goes on one sheet only.
    let e = s.execute("place", &json!({"sheet": "A101", "view": 100000})).unwrap_err();
    assert!(e.contains("A101"), "{e}");
    // The section on a second sheet, at 1 : 50.
    let two = run(&mut s, "sheet", json!({"name": "Sections", "size": "A2"}))["id"].as_u64().unwrap();
    assert_eq!(element(&mut s, two)["number"], "A102");
    run(&mut s, "place", json!({"sheet": two, "view": section, "scale": 50}));
    assert!(s.execute("sheet", &json!({"size": "B5"})).unwrap_err().contains("A4"));
    // A sheet renders as a PNG, or a PDF page; saving to .pdf prints every sheet, a page each.
    assert!(run(&mut s, "render", json!({"view": "A101"}))["pngBase64"].as_str().unwrap().starts_with("iVBORw0KGgo"));
    let pdf = temp("set.pdf");
    run(&mut s, "save", json!({"path": pdf}));
    let bytes = std::fs::read(&pdf).unwrap();
    assert!(bytes.starts_with(b"%PDF-"));
    assert_eq!(bytes.windows(6).filter(|w| *w == b"/Page ").count(), 2, "a page a sheet");
    // Deleting a view takes it off its sheet; deleting a sheet takes its views off.
    run(&mut s, "delete", json!({"ids": [section]}));
    assert_eq!(count(&mut s, "viewport"), 1);
    run(&mut s, "delete", json!({"ids": [sheet]}));
    assert_eq!(count(&mut s, "viewport"), 0);
    let _ = std::fs::remove_file(pdf);
}

#[test]
fn columns_stand_from_their_level_to_the_next_and_turn_with_rotate() {
    let mut s = Session::new();
    house(&mut s);
    let c = run(&mut s, "CL", json!({"at": [4000, 3000]}))["id"].as_u64().unwrap();
    let e = element(&mut s, c);
    assert_eq!((e["type"].as_str(), e["height"].as_f64(), e["width"].as_f64()), (Some("Rectangular 400 x 400"), Some(3000.0), Some(400.0)));
    let round = run(&mut s, "column", json!({"at": [2000, 3000], "type": "Round 300", "height": 2500}))["id"].as_u64().unwrap();
    assert_eq!(element(&mut s, round)["height"], 2500.0);
    run(&mut s, "rotate", json!({"ids": [c], "about": [4000, 3000], "angle": 45}));
    assert!((element(&mut s, c)["angle"].as_f64().unwrap() - 45.0).abs() < 1e-6);
    run(&mut s, "type", json!({"category": "column", "name": "Steel 200 x 300", "width": 200, "depth": 300}));
    run(&mut s, "modify", json!({"id": c, "type": "Steel 200 x 300"}));
    assert_eq!(element(&mut s, c)["depth"], 300.0);
    assert!(s.execute("column", &json!({"at": [0, 0], "type": "Marble"})).unwrap_err().contains("Marble"));
}

#[test]
fn stairs_climb_in_risers_no_higher_than_their_riser() {
    let mut s = Session::new();
    house(&mut s);
    // From [1000, 1000] toward north: 3 m in risers of at most 180 mm is 17 of 176.47 mm, 16 treads of 280 mm.
    let st = run(&mut s, "stair", json!({"start": [1000, 1000], "end": [1000, 2000]}))["id"].as_u64().unwrap();
    let e = element(&mut s, st);
    assert_eq!((e["risers"].as_u64(), e["length"].as_f64(), e["angle"].as_f64()), (Some(17), Some(4480.0), Some(90.0)));
    assert!((e["riser_height"].as_f64().unwrap() - 176.47).abs() < 0.01, "{e}");
    assert!(near(&e["end"], [1000.0, 5480.0]), "{e}");
    let e = s.execute("stair", &json!({"start": [0, 0], "end": [1000, 0], "riser": 1})).unwrap_err();
    assert!(e.contains("risers"), "{e}");
    // Moved and turned like anything else.
    run(&mut s, "move", json!({"ids": [st], "by": [500, 0]}));
    assert_eq!(element(&mut s, st)["start"], json!([1500.0, 1000.0]));
}

#[test]
fn roofs_cover_the_walls_below_flat_or_pitched() {
    let mut s = Session::new();
    house(&mut s);
    run(&mut s, "wall", json!({"points": [[4000, 0], [4000, 6000]]}));
    // Over the walls of Level 1, on Level 2: hipped at 30°, 500 mm past the walls' faces; its four
    // corners, from the south-west, and none where the partition meets the outer walls.
    let roof = run(&mut s, "roof", json!({}))["id"].as_u64().unwrap();
    let e = element(&mut s, roof);
    assert_eq!((id_of(&s, &e["level"]), e["slope"].as_f64(), e["overhang"].as_f64()), (Some(100001), Some(30.0), Some(500.0)));
    assert!(near(&e["boundary"][0], [-600.0, -600.0]) && e["boundary"].as_array().map(Vec::len) == Some(4), "{e}");
    // The ridge: 3100 mm in from the faces at 30°, above Level 2.
    assert!((e["ridge"].as_f64().unwrap() - (3000.0 + 3100.0 * 30f64.to_radians().tan())).abs() < 0.01, "{e}");
    let flat = run(&mut s, "roof", json!({"slope": 0, "boundary": [[0, 0], [4000, 0], [2000, 3000]]}))["id"].as_u64().unwrap();
    assert_eq!(element(&mut s, flat)["ridge"], 3000.0);
    // Over a triangle, pitched: three slopes.
    run(&mut s, "roof", json!({"boundary": [[0, 0], [4000, 0], [2000, 3000]]}));
    // In IFC: a column, a stair and the roofs.
    run(&mut s, "column", json!({"at": [4000, 3000]}));
    run(&mut s, "stair", json!({"start": [1000, 1000], "end": [1000, 2000]}));
    let path = temp("roofed.ifc");
    run(&mut s, "save", json!({"path": path}));
    let ifc = std::fs::read_to_string(&path).unwrap();
    assert!(ifc.contains("IFCCOLUMN(") && ifc.contains("IFCSTAIR(") && ifc.matches("IFCROOF(").count() == 3, "{ifc}");
    let _ = std::fs::remove_file(path);
}

#[test]
fn schedules_come_back_as_rows_or_csv_files() {
    let mut s = Session::new();
    house(&mut s);
    run(&mut s, "room", json!({"point": [4000, 3000], "name": "Living"}));
    let t = run(&mut s, "schedule", json!({"category": "rooms"}));
    assert_eq!(t["rows"][0][1], "Living");
    let csv = temp("rooms.csv");
    run(&mut s, "schedule", json!({"category": "rooms", "path": csv}));
    assert!(std::fs::read_to_string(&csv).unwrap().starts_with("Number,Name,Level"));
    let _ = std::fs::remove_file(csv);
}

#[test]
fn projects_save_and_open_unchanged() {
    let mut s = Session::new();
    house(&mut s);
    run(&mut s, "door", json!({"at": [2000, 0], "flip_hand": true}));
    let path = temp("house.bimcraft");
    run(&mut s, "save", json!({"path": path}));
    let saved = s.model.clone();
    let mut t = Session::new();
    run(&mut t, "open", json!({"path": path}));
    assert_eq!(*t.model, *saved);
    assert_eq!(t.path.as_deref(), Some(path.as_path()));
    std::fs::write(&path, "{\"name\": 3}").unwrap();
    assert!(t.execute("open", &json!({"path": path})).unwrap_err().contains("not a BIMCraft project"));
    assert_eq!(*t.model, *saved, "a failed open keeps the open project");
    let _ = std::fs::remove_file(path);
}

#[test]
fn the_sample_house_builds_with_four_rooms() {
    let mut s = Session::new();
    s.script(crate::sample::SAMPLE).unwrap();
    let i = run(&mut s, "inspect", json!({}));
    assert_eq!((i["counts"]["wall"].as_u64(), i["counts"]["door"].as_u64(), i["counts"]["room"].as_u64()), (Some(7), Some(4), Some(4)));
    assert_eq!(i["warnings"], json!([]));
    assert_eq!((i["counts"]["dimension"].as_u64(), i["counts"]["tag"].as_u64()), (Some(3), Some(9)), "every door and window tagged");
    assert_eq!((i["counts"]["elevation"].as_u64(), i["counts"]["section"].as_u64()), (Some(4), Some(1)));
    assert_eq!((i["counts"]["sheet"].as_u64(), i["counts"]["viewport"].as_u64()), (Some(3), Some(6)));
    assert_eq!((i["counts"]["text"].as_u64(), i["counts"]["spot"].as_u64(), i["counts"]["schedule"].as_u64()), (Some(1), Some(1), Some(1)));
    assert_eq!(i["counts"]["ceiling"].as_u64(), Some(4), "a ceiling in every room");
    let rooms = run(&mut s, "schedule", json!({"category": "rooms"}));
    let areas: Vec<f64> = rooms["rows"].as_array().unwrap().iter().map(|r| r[3].as_f64().unwrap()).collect();
    assert_eq!(areas, [24.0, 18.0, 14.0, 14.0]);
}

#[test]
fn saving_to_ifc_exports_without_changing_the_project_file() {
    let mut s = Session::new();
    house(&mut s);
    let path = temp("house.ifc");
    assert_eq!(run(&mut s, "save", json!({"path": path}))["format"], "IFC4");
    assert!(std::fs::read_to_string(&path).unwrap().contains("FILE_SCHEMA(('IFC4'));"));
    assert_eq!(s.path, None);
    let _ = std::fs::remove_file(path);
}

/// A value with its numbers to the millimetre, so two models that hold the same building print
/// the same.
fn mm(v: &Value) -> String {
    match v {
        Value::Number(x) => format!("{}", x.as_f64().unwrap_or(f64::NAN).round() + 0.0),
        Value::Array(a) => format!("[{}]", a.iter().map(mm).collect::<Vec<_>>().join(",")),
        other => other.to_string(),
    }
}

/// What an element is and where: its level by name, a door's wall by what that wall is.
fn describe(m: &Model, id: u64) -> String {
    let e = crate::element_json(m, id);
    let keys: &[&str] = match e["category"].as_str() {
        Some("wall") => &["start", "end", "arc", "thickness", "base_offset", "height", "level", "top_level", "attach_top"],
        Some("door" | "window") => &["host", "offset", "width", "height", "sill"],
        Some("floor") => &["level", "area_m2", "slope", "top", "openings"],
        Some("ceiling") => &["level", "offset", "thickness", "area_m2", "openings"],
        Some("shaft") => &["level", "base_offset", "top_level", "height", "boundary"],
        Some("mass") => {
            &["name", "level", "base_offset", "top_level", "height", "boundary", "top", "area_m2", "volume_m3", "gross_floor_area_m2"]
        }
        Some("room") => &["level", "name", "number", "area_m2"],
        Some("column") => &["level", "at", "width", "depth", "round", "height", "base_offset"],
        Some("stair") => &["level", "start", "angle", "width", "risers", "riser_height", "length", "landings"],
        Some("ramp") => &["level", "start", "angle", "width", "slope", "height", "length", "landings"],
        Some("railing") => &["level", "host", "side", "path", "height"],
        Some("roof") => &["level", "slope", "ridge", "area_m2", "outline", "gables", "openings", "courtyards"],
        Some("grid") => &["name", "start", "end"],
        _ => &[],
    };
    let text = |k: &str, v: &Value| match k {
        "level" | "top_level" => guid(v).and_then(|l| m.level(l)).map_or_else(|| "-".to_string(), |l| l.name.clone()),
        "host" => guid(v).and_then(|h| m.elements.id(h)).map_or_else(|| "-".to_string(), |h| format!("({})", describe(m, h))),
        "landings" => {
            v.as_array().into_iter().flatten().map(|l| format!("{}@{}", mm(&l["turn"]), mm(&l["at"]))).collect::<Vec<_>>().join(" ")
        }
        "boundary" | "outline" => {
            let mut pts: Vec<String> = v.as_array().into_iter().flatten().map(mm).collect();
            pts.sort();
            pts.join(" ")
        }
        "openings" | "courtyards" => {
            let sorted = |h: &Value| {
                let mut pts: Vec<String> = h.as_array().into_iter().flatten().map(mm).collect();
                pts.sort();
                pts.join(" ")
            };
            let mut holes: Vec<String> = v.as_array().into_iter().flatten().map(sorted).collect();
            holes.sort();
            holes.join(" | ")
        }
        // A roof's gables, by where they are: the middle of each edge left upright, the courtyards'
        // edges numbered on after the outline's.
        "gables" => {
            let rings = std::iter::once(&e["boundary"]).chain(e["courtyards"].as_array().into_iter().flatten());
            let edges: Vec<(&Value, &Value)> = rings
                .flat_map(|r| {
                    let ring: Vec<&Value> = r.as_array().into_iter().flatten().collect();
                    (0..ring.len()).map(move |i| (ring[i], ring[(i + 1) % ring.len()]))
                })
                .collect();
            let mid = |i: usize| -> Option<String> {
                let (a, b) = edges.get(i)?;
                Some(mm(&json!([(a[0].as_f64()? + b[0].as_f64()?) / 2.0, (a[1].as_f64()? + b[1].as_f64()?) / 2.0])))
            };
            let mut mids: Vec<String> = v.as_array().into_iter().flatten().filter_map(|i| mid(i.as_u64()? as usize)).collect();
            mids.sort();
            mids.join(" ")
        }
        "area_m2" | "riser_height" | "name" | "number" | "round" | "gable" | "side" => v.to_string(),
        "slope" => format!("{:.4}", v.as_f64().unwrap_or(f64::NAN)),
        // Attached to what: a roof or a floor, whatever its id.
        "attach_top" => guid(v).and_then(|g| m.elements.get(g)).map_or("-", |e| e.category()).to_string(),
        _ => mm(v),
    };
    // A straight wall has no arc to tell; an attached one's top follows the roof, whatever its top
    // constraint (which IFC doesn't carry).
    let untold = |k: &str| {
        (matches!(k, "arc" | "attach_top" | "slope" | "host" | "side" | "landings" | "gables" | "openings" | "courtyards")
            && e[k].is_null())
            || (k == "top_level" && !e["attach_top"].is_null())
    };
    keys.iter().filter(|k| !untold(k)).map(|k| format!("{k}={}", text(k, &e[*k]))).collect::<Vec<_>>().join(" ")
}

/// Every element of a category, described, in order.
/// The GUID a reference in an element's JSON holds.
fn guid(v: &Value) -> Option<crate::model::Guid> {
    serde_json::from_value(v.clone()).ok()
}

fn building(m: &Model, category: &str) -> Vec<String> {
    let mut all: Vec<String> = m.elements.iter().filter(|(_, e)| e.category() == category).map(|(id, _)| describe(m, *id)).collect();
    all.sort();
    all
}

#[test]
fn ifc_files_open_as_the_building_they_describe() {
    // The sample, with a column and a stair, out to IFC and back in.
    let mut s = Session::new();
    s.script(crate::sample::SAMPLE).unwrap();
    run(&mut s, "column", json!({"at": [3000, 5500], "type": "Round 300"}));
    run(&mut s, "column", json!({"at": [8000, 5000], "angle": 30}));
    run(&mut s, "stair", json!({"start": [1000, 2000], "end": [2000, 2000], "width": 900}));
    run(&mut s, "wall", json!({"points": [[14000, 0], [18000, 0]], "arc": 90, "type": "Exterior - 300mm"}));
    // A ramp rising north beside the house with a hole in it, a loft over the living room open over
    // the stair, and a shaft up through the living room's ceiling and the loft.
    let hole = json!([[[11500, 1500], [12500, 1500], [12500, 2500], [11500, 2500]]]);
    run(
        &mut s,
        "floor",
        json!({"boundary": [[11000, 0], [13000, 0], [13000, 4000], [11000, 4000]], "slope": 4, "arrow": [[12000, 0], [12000, 4000]], "openings": hole}),
    );
    let well = json!([[[1000, 1500], [3500, 1500], [3500, 2500], [1000, 2500]]]);
    run(&mut s, "floor", json!({"level": "Level 2", "boundary": [[0, 0], [6000, 0], [6000, 4000], [0, 4000]], "openings": well}));
    run(&mut s, "shaft", json!({"boundary": [[4000, 1000], [5000, 1000], [5000, 2000], [4000, 2000]]}));
    // A ceiling round a hole, away from the house.
    let hole = json!([[[81000, 1000], [82500, 1000], [82500, 2000], [81000, 2000]]]);
    run(&mut s, "ceiling", json!({"boundary": [[80000, 0], [84000, 0], [84000, 4000], [80000, 4000]], "openings": hole}));
    // A stair that turns back at a landing, with its railings; a ramp turning right at one; a railing along a path.
    let u =
        run(&mut s, "stair", json!({"start": [20000, 0], "angle": 90, "width": 1100, "landings": [{"at": 9, "turn": 180}]}))["id"].clone();
    run(&mut s, "railing", json!({"host": u}));
    let ramp =
        run(&mut s, "ramp", json!({"start": [11000, -4000], "angle": 0, "height": 450, "landings": [{"at": 300, "turn": -90}]}))["id"]
            .clone();
    run(&mut s, "railing", json!({"host": ramp, "side": "left"}));
    run(&mut s, "railing", json!({"path": [[0, -500], [3000, -500]]}));
    // A roof over an L, one wing's end a gable, a skylight through it; an extrusion roof, its
    // ridge off its middle.
    let l = json!([[30000, 0], [42000, 0], [42000, 6000], [36000, 6000], [36000, 12000], [30000, 12000]]);
    let skylight = json!([[[33000, 1000], [35000, 1000], [35000, 2000], [33000, 2000]]]);
    run(&mut s, "roof", json!({"level": "Level 2", "boundary": l, "slope": 25, "gables": [1], "openings": skylight}));
    let profile = json!({"line": [[50000, 0], [58000, 0]], "profile": [[0, 0], [3000, 2000], [8000, 0]], "to": 6000});
    run(&mut s, "roof", json!({"level": "Level 2", "extrusion": profile}));
    // A roof round a courtyard, the courtyard's east side a gable.
    let square = json!([[60000, 0], [76000, 0], [76000, 16000], [60000, 16000]]);
    let court = json!([[[66000, 6000], [70000, 6000], [70000, 10000], [66000, 10000]]]);
    run(&mut s, "roof", json!({"level": "Level 2", "boundary": square, "courtyards": court, "gables": [5]}));
    // Masses: one 500 mm over Level 1 up to Level 2, holding a parameter, and one on Level 2 with no top level.
    run(&mut s, "parameter", json!({"name": "Use", "kind": "text", "categories": ["mass"]}));
    let tower = run(
        &mut s,
        "mass",
        json!({"boundary": [[80000, 0], [90000, 0], [90000, 6000], [80000, 6000]], "name": "Tower", "base_offset": 500}),
    );
    run(&mut s, "modify", json!({"id": tower["id"], "params": {"Use": "Housing"}}));
    run(
        &mut s,
        "mass",
        json!({"level": "Level 2", "boundary": [[80000, 10000], [86000, 10000], [86000, 14000]], "height": 4200, "name": "Wing"}),
    );
    // A lofted one, 10 x 6 m on Level 1 narrowing to 6 x 2 m 5 m up, and one leaning, both with their corners pairing up as they were made.
    let taper = json!([[102000, 2000], [108000, 2000], [108000, 4000], [102000, 4000]]);
    run(
        &mut s,
        "mass",
        json!({"boundary": [[100000, 0], [110000, 0], [110000, 6000], [100000, 6000]], "name": "Taper", "height": 5000, "top": taper}),
    );
    run(
        &mut s,
        "mass",
        json!({"boundary": [[120000, 0], [124000, 0], [124000, 4000]], "name": "Lean", "height": 3500, "lean": [1500, -500]}),
    );
    let path = temp("round-trip.ifc");
    run(&mut s, "save", json!({"path": path}));
    let mut t = Session::new();
    let r = run(&mut t, "open", json!({"path": path}));
    let _ = std::fs::remove_file(&path);
    assert_eq!((r["format"].as_str(), r["skipped"].clone()), (Some("IFC4"), json!([])), "{r}");
    assert_eq!(t.path, None, "an import is a new project: saving asks where");
    let levels = |m: &Model| m.levels().into_iter().map(|(_, l)| (l.name.clone(), l.elevation)).collect::<Vec<_>>();
    assert_eq!(levels(&t.model), levels(&s.model));
    for category in ["wall", "door", "window", "floor", "room", "column", "stair", "roof", "ceiling", "shaft", "ramp", "railing", "mass"] {
        let (before, after) = (building(&s.model, category), building(&t.model, category));
        assert!(!before.is_empty(), "{category}");
        assert_eq!(after, before, "{category}");
    }
    // A mass keeps its parameter's value too.
    let uses: Vec<Value> = t
        .model
        .elements
        .values()
        .filter_map(|e| if let crate::model::Element::Mass(ms) = e { ms.params.get("Use").cloned() } else { None })
        .collect();
    assert_eq!(uses, [json!("Housing")]);
    // It saves as a project like any other.
    let project = temp("imported.bimcraft");
    run(&mut t, "save", json!({"path": project}));
    let _ = std::fs::remove_file(project);
}

#[test]
fn a_lofted_mass_goes_to_ifc_as_a_face_set_and_comes_back_only_where_its_corners_pair_up() {
    let mut s = Session::new();
    let (id, _) = frustum(&mut s);
    run(&mut s, "mass", json!({"boundary": block(), "height": 3000, "name": "Plain"}));
    let file = crate::ifc::export(&s.model);
    // One closed face set and one property set for the lofted mass; the plain one is swept up as ever.
    assert_eq!((file.matches("IFCTRIANGULATEDFACESET").count(), file.matches("'BIMCraft_Mass'").count()), (1, 1));
    assert_eq!(file.matches("IFCEXTRUDEDAREASOLID").count(), 1);
    // Read back, the loft is as it was: the same corners, twin for twin, and the same volume.
    let back = crate::import::ifc(file.as_bytes(), "t").unwrap();
    assert!(back.skipped.is_empty(), "{:?}", back.skipped);
    let mut t = Session::new();
    t.reset(back.model, None);
    let lofted = t
        .model
        .elements
        .values()
        .find_map(|e| if let crate::model::Element::Mass(ms) = e { ms.lofted().then(|| ms.clone()) } else { None });
    let Some(ms) = lofted else { panic!("no lofted mass came back") };
    let Some(crate::model::Element::Mass(was)) = s.model.elements.get(id) else { panic!("the frustum is a mass") };
    assert_eq!((ms.boundary.clone(), ms.top.clone()), (was.boundary.clone(), was.top.clone()));
    let (bottom, top) = t.model.span(ms.level, ms.base_offset, ms.top_level, ms.height);
    assert!((t.model.mass_volume(&ms, bottom, top) - 200e9).abs() < 1e3);
    // Without its property set, or with one that no longer fits the body, the corners can't be paired and it is left out, listed.
    for broken in [file.replace("'BIMCraft_Mass'", "'Other'"), file.replace("[2000.0,2000.0]", "[2100.0,2000.0]")] {
        assert_ne!(broken, file);
        let back = crate::import::ifc(broken.as_bytes(), "t").unwrap();
        let masses: Vec<_> = back.model.elements.values().filter(|e| e.category() == "mass").collect();
        assert_eq!(masses.len(), 1, "only the plain one: {:?}", back.skipped);
        assert!(back.skipped.iter().any(|why| why.contains("BIMCraft_Mass")), "{:?}", back.skipped);
    }
}

/// A small IFC2x3 file in metres, written the way other BIM tools write them: a storey 3 m up; a
/// wall drawn on its outer face, rotated to run north, with an axis; a partition with no axis
/// stopping at the wall's face; a window in an opening cut sideways through the wall; an L-shaped
/// slab; a room; two grids; and a desk.
const OFFICE: &str = r"ISO-10303-21;
HEADER;
FILE_DESCRIPTION(('ViewDefinition [CoordinationView_V2.0]'),'2;1');
FILE_NAME('office.ifc','2026-10-15T10:00:00',(''),(''),'hand','hand','');
FILE_SCHEMA(('IFC2X3'));
ENDSEC;
DATA;
/* metres */
#1=IFCSIUNIT(*,.LENGTHUNIT.,$,.METRE.);
#2=IFCUNITASSIGNMENT((#1));
#3=IFCCARTESIANPOINT((0.,0.,0.));
#4=IFCAXIS2PLACEMENT3D(#3,$,$);
#5=IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-05,#4,$);
#6=IFCPROJECT('0000000000000000000001',$,'Small \X2\00D6\X0\ffice',$,$,$,$,(#5),#2);
#10=IFCLOCALPLACEMENT($,#4);
#11=IFCSITE('0000000000000000000002',$,'Site',$,$,#10,$,$,.ELEMENT.,$,$,$,$,$);
#12=IFCLOCALPLACEMENT(#10,#4);
#13=IFCBUILDING('0000000000000000000003',$,'Building',$,$,#12,$,$,.ELEMENT.,$,$,$);
#20=IFCLOCALPLACEMENT(#12,#4);
#21=IFCBUILDINGSTOREY('0000000000000000000004',$,'Ground',$,$,#20,$,$,.ELEMENT.,0.);
#22=IFCCARTESIANPOINT((0.,0.,3.));
#23=IFCAXIS2PLACEMENT3D(#22,$,$);
#24=IFCLOCALPLACEMENT(#12,#23);
#25=IFCBUILDINGSTOREY('0000000000000000000005',$,'First',$,$,#24,$,$,.ELEMENT.,3.);
#30=IFCCARTESIANPOINT((1.,2.,0.));
#31=IFCDIRECTION((0.,0.,1.));
#32=IFCDIRECTION((0.,1.,0.));
#33=IFCAXIS2PLACEMENT3D(#30,#31,#32);
#34=IFCLOCALPLACEMENT(#24,#33);
#35=IFCCARTESIANPOINT((0.,0.));
#36=IFCCARTESIANPOINT((5.,0.));
#37=IFCPOLYLINE((#35,#36));
#38=IFCSHAPEREPRESENTATION(#5,'Axis','Curve2D',(#37));
#39=IFCCARTESIANPOINT((0.,0.2));
#40=IFCCARTESIANPOINT((5.,0.2));
#41=IFCPOLYLINE((#35,#36,#40,#39,#35));
#42=IFCARBITRARYCLOSEDPROFILEDEF(.AREA.,$,#41);
#43=IFCEXTRUDEDAREASOLID(#42,#4,#31,2.7);
#44=IFCSHAPEREPRESENTATION(#5,'Body','SweptSolid',(#43));
#45=IFCPRODUCTDEFINITIONSHAPE($,$,(#38,#44));
#46=IFCWALLSTANDARDCASE('0000000000000000000006',$,'Basic Wall:Exterior:101',$,'Basic Wall:Exterior',#34,#45,'101');
#50=IFCCARTESIANPOINT((5.,4.,0.));
#51=IFCDIRECTION((-1.,0.,0.));
#52=IFCAXIS2PLACEMENT3D(#50,#31,#51);
#53=IFCLOCALPLACEMENT(#24,#52);
#54=IFCCARTESIANPOINT((2.,0.));
#55=IFCAXIS2PLACEMENT2D(#54,$);
#56=IFCRECTANGLEPROFILEDEF(.AREA.,$,#55,4.,0.1);
#57=IFCEXTRUDEDAREASOLID(#56,#4,#31,2.7);
#58=IFCSHAPEREPRESENTATION(#5,'Body','SweptSolid',(#57));
#59=IFCPRODUCTDEFINITIONSHAPE($,$,(#58));
#60=IFCWALL('0000000000000000000007',$,'Partition',$,$,#53,#59,$);
#70=IFCCARTESIANPOINT((3.,0.3,0.9));
#71=IFCDIRECTION((0.,-1.,0.));
#72=IFCDIRECTION((1.,0.,0.));
#73=IFCAXIS2PLACEMENT3D(#70,#71,#72);
#74=IFCLOCALPLACEMENT(#34,#4);
#75=IFCCARTESIANPOINT((0.5,0.6));
#76=IFCAXIS2PLACEMENT2D(#75,$);
#77=IFCRECTANGLEPROFILEDEF(.AREA.,$,#76,1.,1.2);
#78=IFCEXTRUDEDAREASOLID(#77,#73,#31,0.4);
#79=IFCSHAPEREPRESENTATION(#5,'Body','SweptSolid',(#78));
#80=IFCPRODUCTDEFINITIONSHAPE($,$,(#79));
#81=IFCOPENINGELEMENT('0000000000000000000008',$,'Opening',$,$,#74,#80,$);
#82=IFCRELVOIDSELEMENT('0000000000000000000009',$,$,$,#46,#81);
#83=IFCLOCALPLACEMENT(#74,#4);
#84=IFCWINDOW('000000000000000000000A',$,'Fixed:1000 x 1200:201',$,'Fixed:1000 x 1200',#83,$,'201',1.2,1.);
#85=IFCRELFILLSELEMENT('000000000000000000000B',$,$,$,#81,#84);
#90=IFCCARTESIANPOINT((1.,2.));
#91=IFCCARTESIANPOINT((5.,2.));
#92=IFCCARTESIANPOINT((5.,3.9));
#93=IFCCARTESIANPOINT((1.,3.9));
#94=IFCPOLYLINE((#90,#91,#92,#93,#90));
#95=IFCARBITRARYCLOSEDPROFILEDEF(.AREA.,$,#94);
#96=IFCEXTRUDEDAREASOLID(#95,#4,#31,2.5);
#97=IFCSHAPEREPRESENTATION(#5,'Body','SweptSolid',(#96));
#98=IFCPRODUCTDEFINITIONSHAPE($,$,(#97));
#99=IFCLOCALPLACEMENT(#24,#4);
#100=IFCSPACE('000000000000000000000C',$,'101',$,$,#99,#98,'Office',.ELEMENT.,.INTERNAL.,$);
#101=IFCRELAGGREGATES('000000000000000000000D',$,$,$,#25,(#100));
#110=IFCCARTESIANPOINT((0.,0.,-0.25));
#111=IFCAXIS2PLACEMENT3D(#110,$,$);
#112=IFCCARTESIANPOINT((0.8,1.9));
#113=IFCCARTESIANPOINT((5.2,1.9));
#114=IFCCARTESIANPOINT((5.2,4.1));
#115=IFCCARTESIANPOINT((2.,4.1));
#116=IFCCARTESIANPOINT((2.,7.1));
#117=IFCCARTESIANPOINT((0.8,7.1));
#118=IFCPOLYLINE((#112,#113,#114,#115,#116,#117,#112));
#119=IFCARBITRARYCLOSEDPROFILEDEF(.AREA.,$,#118);
#120=IFCEXTRUDEDAREASOLID(#119,#111,#31,0.25);
#121=IFCSHAPEREPRESENTATION(#5,'Body','SweptSolid',(#120));
#122=IFCPRODUCTDEFINITIONSHAPE($,$,(#121));
#123=IFCLOCALPLACEMENT(#24,#4);
#124=IFCSLAB('000000000000000000000E',$,'Floor:Generic 250mm',$,'Floor:Generic 250mm',#123,#122,$,.FLOOR.);
#130=IFCCARTESIANPOINT((0.,-1.));
#131=IFCCARTESIANPOINT((0.,8.));
#132=IFCPOLYLINE((#130,#131));
#133=IFCGRIDAXIS('1',#132,.T.);
#134=IFCCARTESIANPOINT((-1.,0.));
#135=IFCCARTESIANPOINT((6.,0.));
#136=IFCPOLYLINE((#134,#135));
#137=IFCGRIDAXIS('A',#136,.T.);
#138=IFCLOCALPLACEMENT(#12,#4);
#139=IFCGRID('000000000000000000000F',$,'Grid',$,$,#138,$,(#133),(#137),$);
#140=IFCFURNISHINGELEMENT('000000000000000000000G',$,'Desk',$,$,#99,$,$);
#150=IFCRELCONTAINEDINSPATIALSTRUCTURE('000000000000000000000H',$,$,$,(#46,#60,#84,#124,#140),#25);
#151=IFCRELCONTAINEDINSPATIALSTRUCTURE('000000000000000000000I',$,$,$,(#139),#21);
#152=IFCRELAGGREGATES('000000000000000000000J',$,$,$,#13,(#21,#25));
ENDSEC;
END-ISO-10303-21;
";

#[test]
fn ifc_files_from_other_tools_come_in_measured_from_their_geometry() {
    let path = temp("office.ifc");
    std::fs::write(&path, OFFICE).unwrap();
    let mut s = Session::new();
    let r = run(&mut s, "open", json!({"path": path}));
    let _ = std::fs::remove_file(&path);
    assert_eq!((r["format"].as_str(), r["name"].as_str()), (Some("IFC2X3"), Some("Small Öffice")), "{r}");
    let m = &s.model;
    assert_eq!(
        m.levels().into_iter().map(|(_, l)| (l.name.clone(), l.elevation)).collect::<Vec<_>>(),
        [("Ground".into(), 0.0), ("First".into(), 3000.0)]
    );
    // In mm; the wall's centre line 100 mm in from the face it was drawn on; the partition carried on
    // from that face to the wall's centre line, so the two meet.
    assert_eq!(
        building(m, "wall"),
        [
            "start=[5000,4000] end=[900,4000] thickness=100 base_offset=0 height=2700 level=First top_level=-",
            "start=[900,2000] end=[900,7000] thickness=200 base_offset=0 height=2700 level=First top_level=-",
        ]
    );
    assert_eq!(
        building(m, "window"),
        [
            "host=(start=[900,2000] end=[900,7000] thickness=200 base_offset=0 height=2700 level=First top_level=-) offset=3500 width=1000 height=1200 sill=900"
        ]
    );
    assert_eq!(building(m, "floor"), ["level=First area_m2=13.28 top=[3000,3000]"]);
    assert_eq!(building(m, "grid"), ["name=\"1\" start=[0,-1000] end=[0,8000]", "name=\"A\" start=[-1000,0] end=[6000,0]"]);
    let room = m.elements.values().find_map(|e| match e {
        crate::model::Element::Room(r) => Some(r.clone()),
        _ => None,
    });
    let room = room.unwrap();
    assert_eq!((room.name.as_str(), room.number.as_str(), m.level(room.level).map(|l| l.name.as_str())), ("Office", "101", Some("First")));
    assert!((1000.0..5000.0).contains(&room.point.0) && (2000.0..3900.0).contains(&room.point.1), "{:?}", room.point);
    // A tag that is only the element's id, as Revit writes it, is no mark: the window is numbered.
    let marks: Vec<&str> =
        m.elements.values().filter_map(|e| if let crate::model::Element::Window(o) = e { Some(o.mark.as_str()) } else { None }).collect();
    assert_eq!(marks, ["1"]);
    // What BIMCraft has no element for is left out, and said so.
    let skipped = r["skipped"].to_string();
    assert!(skipped.contains("IFCFURNISHINGELEMENT"), "{skipped}");
}

#[test]
fn marks_number_on_in_the_series_they_are_in() {
    use crate::commands::next_number;
    let next = |taken: &[&str]| next_number(taken.iter().copied());
    assert_eq!(next(&[]), "1");
    assert_eq!(next(&["1", "2", "7"]), "8");
    // A prefix two marks share is a series; padding is kept.
    assert_eq!(next(&["P1", "P2", "P5"]), "P6");
    assert_eq!(next(&["J-01", "J-02", "J-09"]), "J-10");
    // A lone prefixed mark among numbered ones, or fewer than them, is an exception.
    assert_eq!(next(&["D2"]), "1");
    assert_eq!(next(&["D2", "1"]), "2");
    assert_eq!(next(&["P1", "P2", "1", "2", "3"]), "4");
    // The bigger series wins; never a mark already taken; text with no number is no series.
    assert_eq!(next(&["P1", "P2", "P3", "J1", "J2"]), "P4");
    assert_eq!(next(&["P01", "P02", "P3"]), "P04");
    assert_eq!(next(&["Entry", "Back"]), "1");
}

#[test]
fn a_tag_of_a_long_run_of_zeros_starts_no_series_and_pads_no_further_than_a_name_goes() {
    use crate::commands::next_number;
    use crate::model::MAX_TEXT;
    // A hostile file's tags, `P`, zeros and a number, longer than any name: ignored, not padded to.
    let hostile = [format!("P{}1", "0".repeat(MAX_TEXT)), format!("P{}2", "0".repeat(MAX_TEXT))];
    assert_eq!(next_number(hostile.iter().map(String::as_str)), "1");
    // One that fits is a series as ever, and its next is no longer than it.
    let fits = [format!("P{}1", "0".repeat(MAX_TEXT - 3)), format!("P{}2", "0".repeat(MAX_TEXT - 3))];
    assert_eq!(next_number(fits.iter().map(String::as_str)), format!("P{}3", "0".repeat(MAX_TEXT - 3)));
}

#[test]
fn doors_and_windows_keep_their_marks_through_ifc_and_new_ones_follow_the_series() {
    let mut s = Session::new();
    house(&mut s);
    s.script(
        r#"door {"at": [1000, 0], "mark": "P1"}
           door {"at": [3000, 0], "mark": "P2"}
           window {"at": [8000, 2000], "mark": "J1"}
           window {"at": [8000, 4000], "mark": "J2"}"#,
    )
    .unwrap();
    // The next door and window carry on their series, and so does a copy.
    assert_eq!(run(&mut s, "door", json!({"at": [5000, 0]}))["mark"], "P3");
    let window = run(&mut s, "window", json!({"at": [4000, 6000]}));
    assert_eq!(window["mark"], "J3");
    let copy = run(&mut s, "copy", json!({"ids": [window["id"]], "by": [-2000, 0]}))["ids"][0].as_u64().unwrap();
    assert_eq!(element(&mut s, copy)["mark"], "J4");
    // Out to IFC and back: each keeps its tag; one with no tag is numbered on after the rest.
    let marks = |m: &Model| {
        let mut all: Vec<String> = m
            .elements
            .values()
            .filter_map(|e| match e {
                crate::model::Element::Door(o) | crate::model::Element::Window(o) => Some(o.mark.clone()),
                _ => None,
            })
            .collect();
        all.sort();
        all
    };
    let file = crate::ifc::export(&s.model);
    let back = crate::import::ifc(file.as_bytes(), "t").unwrap();
    assert_eq!(marks(&back.model), marks(&s.model));
    let untagged = file.replacen(",'P2',", ",$,", 1);
    assert_ne!(untagged, file);
    let back = crate::import::ifc(untagged.as_bytes(), "t").unwrap();
    assert_eq!(marks(&back.model), ["J1", "J2", "J3", "J4", "P1", "P3", "P4"]);
}

#[test]
fn broken_or_hostile_ifc_files_fail_cleanly() {
    let mut s = Session::new();
    s.script(crate::sample::SAMPLE).unwrap();
    let before = s.model.clone();
    let path = temp("broken.ifc");
    let cases = [
        "this is not STEP at all",
        "ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('AUTOMOTIVE_DESIGN'));\nENDSEC;\nDATA;\nENDSEC;\nEND-ISO-10303-21;\n",
        "ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('IFC4'));\nENDSEC;\nDATA;\n#1=IFCWALL('x',$,(((((;\n",
        "ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('IFC4'));\nENDSEC;\nDATA;\n#1=IFCPROJECT('x',$,'unterminated);\nENDSEC;\n",
    ];
    for text in cases {
        std::fs::write(&path, text).unwrap();
        let e = s.execute("open", &json!({"path": path})).unwrap_err();
        assert!(!e.is_empty());
        assert_eq!(*s.model, *before, "a failed import keeps the open project: {text}");
    }
    // Nested a thousand deep, and placements that point at each other: an error or a model, never a hang or a crash.
    let deep = format!(
        "ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('IFC4'));\nENDSEC;\nDATA;\n#1=IFCX({}1{});\nENDSEC;\n",
        "(".repeat(1000),
        ")".repeat(1000)
    );
    let cycle = OFFICE.replace("#24=IFCLOCALPLACEMENT(#12,#23);", "#24=IFCLOCALPLACEMENT(#34,#23);");
    for text in [deep, cycle] {
        std::fs::write(&path, &text).unwrap();
        let _ = s.execute("open", &json!({"path": path}));
    }
    let _ = std::fs::remove_file(path);
}

#[test]
fn project_units_change_what_is_shown_not_the_model() {
    let mut s = Session::new();
    let ids = house(&mut s);
    // Millimetres by default; feet and inches on request, an undo step like any change.
    assert_eq!(run(&mut s, "inspect", json!({}))["project_units"], "mm");
    assert_eq!(run(&mut s, "UN", json!({"length": "ft-in"}))["length"], "ft-in");
    assert_eq!(run(&mut s, "inspect", json!({}))["project_units"], "ft-in");
    // The model, and every command, keep millimetres.
    assert_eq!(element(&mut s, ids[0])["length"], 8000.0);
    assert!(s.execute("units", &json!({"length": "furlongs"})).unwrap_err().contains("ft-in"));
    run(&mut s, "undo", json!({}));
    assert_eq!(run(&mut s, "inspect", json!({}))["project_units"], "mm");
    // Saved with the project.
    run(&mut s, "units", json!({"length": "m"}));
    let path = temp("units.bimcraft");
    run(&mut s, "save", json!({"path": path}));
    let mut t = Session::new();
    run(&mut t, "open", json!({"path": path}));
    assert_eq!(run(&mut t, "inspect", json!({}))["project_units"], "m");
    let _ = std::fs::remove_file(path);
}

#[test]
fn a_project_in_portuguese_names_what_it_makes_in_portuguese() {
    let mut s = Session::new();
    run(&mut s, "new", json!({"name": "Casa", "language": "pt"}));
    let i = run(&mut s, "inspect", json!({}));
    assert_eq!((i["levels"][0]["name"].clone(), i["language"].clone()), (json!("Nível 1"), json!("pt")));
    house(&mut s);
    assert_eq!(run(&mut s, "section", json!({"start": [-1000, 3000], "end": [9000, 3000]}))["name"], "Corte 1");
    let ids = run(&mut s, "elevation", json!({}))["ids"].clone();
    let names: Vec<Value> = ids.as_array().unwrap().iter().map(|id| element(&mut s, id.as_u64().unwrap())["name"].clone()).collect();
    assert_eq!(names, ["Norte", "Sul", "Leste", "Oeste"]);
    let room = run(&mut s, "room", json!({"point": [4000, 3000]}))["id"].as_u64().unwrap();
    assert_eq!(element(&mut s, room)["name"], "Ambiente");
    let sheet = run(&mut s, "sheet", json!({}))["id"].as_u64().unwrap();
    assert_eq!(element(&mut s, sheet)["name"], "Sem nome");
    // A schedule placed by its name in either language is named in the project's.
    let vp = run(&mut s, "place", json!({"sheet": "A101", "view": "Room Schedule"}))["id"].as_u64().unwrap();
    assert_eq!(element(&mut s, vp)["title"], "Tabela de ambientes");
    run(&mut s, "sheet", json!({}));
    assert!(s.execute("place", &json!({"sheet": "A102", "view": "Tabela de ambientes"})).unwrap_err().contains("A101"));
    // What's named stays named; what comes next follows the language.
    run(&mut s, "language", json!({"code": "en"}));
    assert_eq!(run(&mut s, "level", json!({"elevation": 6000}))["name"], "Level 3");
    assert_eq!(element(&mut s, room)["name"], "Ambiente");
    assert!(s.execute("language", &json!({"code": "klingon"})).unwrap_err().contains("pt"));
}

#[test]
fn a_project_in_spanish_names_what_it_makes_in_spanish() {
    let mut s = Session::new();
    run(&mut s, "new", json!({"language": "es"}));
    let i = run(&mut s, "inspect", json!({}));
    assert_eq!([&i["name"], &i["levels"][1]["name"], &i["language"]], [&json!("Proyecto"), &json!("Nivel 2"), &json!("es")]);
    house(&mut s);
    assert_eq!(run(&mut s, "section", json!({"start": [-1000, 3000], "end": [9000, 3000]}))["name"], "Sección 1");
    let ids = run(&mut s, "elevation", json!({}))["ids"].clone();
    let names: Vec<Value> = ids.as_array().unwrap().iter().map(|id| element(&mut s, id.as_u64().unwrap())["name"].clone()).collect();
    assert_eq!(names, ["Norte", "Sur", "Este", "Oeste"]);
    let room = run(&mut s, "room", json!({"point": [4000, 3000]}))["id"].as_u64().unwrap();
    assert_eq!(element(&mut s, room)["name"], "Habitación");
    run(&mut s, "sheet", json!({}));
    let vp = run(&mut s, "place", json!({"sheet": "A101", "view": "tabla de habitaciones"}))["id"].as_u64().unwrap();
    assert_eq!(element(&mut s, vp)["title"], "Tabla de habitaciones");
    // Sides are named in any language there is.
    assert_eq!(
        [Facing::parse("Sur"), Facing::parse("leste"), Facing::parse("West")],
        [Some(Facing::South), Some(Facing::East), Some(Facing::West)]
    );
}

#[test]
fn a_project_in_swedish_names_what_it_makes_in_swedish() {
    let mut s = Session::new();
    run(&mut s, "new", json!({"language": "sv"}));
    let i = run(&mut s, "inspect", json!({}));
    assert_eq!([&i["name"], &i["levels"][1]["name"], &i["language"]], [&json!("Projekt"), &json!("Nivå 2"), &json!("sv")]);
    house(&mut s);
    assert_eq!(run(&mut s, "section", json!({"start": [-1000, 3000], "end": [9000, 3000]}))["name"], "Sektion 1");
    let ids = run(&mut s, "elevation", json!({}))["ids"].clone();
    let names: Vec<Value> = ids.as_array().unwrap().iter().map(|id| element(&mut s, id.as_u64().unwrap())["name"].clone()).collect();
    assert_eq!(names, ["Norr", "Söder", "Öster", "Väster"]);
    let room = run(&mut s, "room", json!({"point": [4000, 3000]}))["id"].as_u64().unwrap();
    assert_eq!(element(&mut s, room)["name"], "Rum");
    run(&mut s, "sheet", json!({}));
    let vp = run(&mut s, "place", json!({"sheet": "A101", "view": "rumsförteckning"}))["id"].as_u64().unwrap();
    assert_eq!(element(&mut s, vp)["title"], "Rumsförteckning");
}

/// A cavity wall type, outside in: brick, insulation, block and plaster, 300 mm in all; its
/// insulation a material of its own.
fn cavity(s: &mut Session) {
    run(s, "material", json!({"name": "Mineral wool", "color": "#e6d68c", "cut": "insulation"}));
    let layers = json!([
        {"function": "finish", "material": "Brick", "thickness": 100},
        {"function": "thermal", "material": "Mineral wool", "thickness": 50},
        {"function": "structure", "material": "Concrete block", "thickness": 140},
        {"function": "finish", "material": "Plaster", "thickness": 10}
    ]);
    run(s, "type", json!({"category": "wall", "name": "Cavity 300", "layers": layers}));
}

#[test]
fn compound_wall_types_are_as_thick_as_their_layers() {
    let mut s = Session::new();
    cavity(&mut s);
    let t = run(&mut s, "inspect", json!({}))["types"]["wall"]["Cavity 300"].clone();
    assert_eq!(t["thickness"], 300.0);
    // Each layer with its id and GUID, as an element has them.
    let mut first = t["layers"][0].clone();
    let (id, guid) = (first["id"].as_u64(), first["guid"].as_str().map(String::from));
    assert!(id.is_some_and(|id| id >= 100_000) && guid.is_some_and(|g| crate::model::Guid::parse(&g).is_some()), "{first}");
    first.as_object_mut().unwrap().retain(|k, _| k != "id" && k != "guid");
    assert_eq!(first, json!({"function": "finish", "material": "Brick", "thickness": 100.0}));
    // A new thickness goes to its structure.
    let t = run(&mut s, "type", json!({"category": "wall", "name": "Cavity 300", "thickness": 320}))["type"].clone();
    assert_eq!((&t["thickness"], &t["layers"][2]["thickness"]), (&json!(320.0), &json!(160.0)));
    for (bad, says) in [
        (json!({"category": "wall", "name": "Cavity 300", "thickness": 100}), "structure"),
        (json!({"category": "wall", "name": "Thin", "layers": [{"thickness": -5}]}), "thickness"),
        (json!({"category": "wall", "name": "Odd", "layers": [{"function": "rubber", "thickness": 5}]}), "rubber"),
        (json!({"category": "wall", "name": "None", "layers": []}), "thickness"),
        (json!({"category": "wall", "name": "Both", "thickness": 200, "layers": [{"thickness": 100}]}), "200"),
    ] {
        let e = s.execute("type", &bad).unwrap_err();
        assert!(e.contains(says), "{bad}: {e}");
    }
}

#[test]
fn walls_stand_on_their_location_line_with_their_exterior_to_the_left() {
    use crate::model::{Face, Reference};
    let mut s = Session::new();
    cavity(&mut s);
    // Drawn west to east, its exterior side is north, to its left: walls drawn clockwise face out, as in Revit.
    let w = run(&mut s, "wall", json!({"points": [[0, 0], [5000, 0]], "type": "Cavity 300", "location": "exterior-finish"}))["ids"][0]
        .as_u64()
        .unwrap();
    let y = |s: &Session, face: Face| s.model.reference_line(&Reference::line(s.model.guid(w), face)).unwrap().0.1;
    assert_eq!([y(&s, Face::Left), y(&s, Face::Centre), y(&s, Face::Right)], [0.0, -150.0, -300.0]);
    // Put on its core's interior face, it stays where it is.
    run(&mut s, "modify", json!({"id": w, "location": "interior-core"}));
    let e = element(&mut s, w);
    assert_eq!((&e["start"], &e["end"]), (&json!([0.0, -290.0]), &json!([5000.0, -290.0])));
    assert_eq!([y(&s, Face::Left), y(&s, Face::Right)], [0.0, -300.0]);
    // Flipped, its exterior goes to the other side of its line; a door in it stays where it is.
    let door = run(&mut s, "door", json!({"wall": w, "offset": 1000}))["id"].as_u64().unwrap();
    let at = element(&mut s, door)["at"].clone();
    run(&mut s, "flip", json!({"ids": [w]}));
    let e = element(&mut s, w);
    assert_eq!((&e["start"], &e["end"]), (&json!([5000.0, -290.0]), &json!([0.0, -290.0])));
    assert_eq!([y(&s, Face::Left), y(&s, Face::Right)], [-580.0, -280.0]);
    let d = element(&mut s, door);
    assert_eq!((&d["at"], &d["flip_facing"], &d["flip_hand"]), (&at, &json!(true), &json!(true)));
    assert!(s.execute("wall", &json!({"points": [[0, 0], [1, 1000]], "location": "middle"})).unwrap_err().contains("middle"));
}

#[test]
fn curved_walls_bound_rooms_and_carry_openings_along_their_arc() {
    let mut s = Session::new();
    // A room closed on the east by a half circle 3 m across: turning left from south to north, it bulges east.
    run(&mut s, "wall", json!({"points": [[4000, 3000], [0, 3000], [0, 0], [4000, 0]]}));
    let arc = run(&mut s, "wall", json!({"points": [[4000, 0], [4000, 3000]], "arc": 180}))["ids"][0].as_u64().unwrap();
    let e = element(&mut s, arc);
    assert_eq!((&e["arc"], &e["radius"]), (&json!(180.0), &json!(1500.0)));
    assert!((e["length"].as_f64().unwrap() - 1500.0 * std::f64::consts::PI).abs() < 0.01, "{e}");
    let room = run(&mut s, "room", json!({"point": [4500, 1500]}));
    assert_eq!(room["area_m2"], 15.53, "12 m² and half a circle");
    // A window at the crown: halfway along the arc.
    let win = run(&mut s, "window", json!({"at": [5500, 1500]}));
    assert_eq!((id_of(&s, &win["host"]), win["offset"].as_f64().map(f64::round)), (Some(arc), Some(2356.0)));
    let at = element(&mut s, win["id"].as_u64().unwrap())["at"].clone();
    assert!((at[0].as_f64().unwrap() - 5500.0).abs() < 1e-6 && (at[1].as_f64().unwrap() - 1500.0).abs() < 1e-6, "{at}");
    for bad in [json!(360), json!(1e300), json!("x")] {
        assert!(s.execute("wall", &json!({"points": [[0, 0], [1000, 0]], "arc": bad})).is_err(), "{bad}");
    }
}

#[test]
fn curved_walls_split_mirror_and_flip_as_arcs() {
    let mut s = Session::new();
    // Turning left from west to east, it bulges south: its crown at [2000, -2000].
    let w = run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]], "arc": 180}))["ids"][0].as_u64().unwrap();
    let ids = run(&mut s, "split", json!({"id": w, "at": [2000, -2000]}))["ids"].clone();
    let new = ids[1].as_u64().unwrap();
    let (a, b) = (element(&mut s, w), element(&mut s, new));
    assert_eq!((&a["arc"], &b["arc"], &b["end"]), (&json!(90.0), &json!(90.0), &json!([4000.0, 0.0])));
    assert_eq!(a["end"], json!([2000.0, -2000.0]));
    // Mirrored across y = 1000, they bulge north; running back the other way, they still turn left.
    run(&mut s, "mirror", json!({"ids": [w, new], "axis": [[0, 1000], [1000, 1000]]}));
    let a = element(&mut s, w);
    assert_eq!((&a["start"], &a["end"], &a["arc"]), (&json!([2000.0, 4000.0]), &json!([0.0, 2000.0]), &json!(90.0)));
    // Flipped, it runs back the way it came, turning right.
    run(&mut s, "flip", json!({"ids": [new]}));
    let b = element(&mut s, new);
    assert_eq!((&b["start"], &b["end"], &b["arc"]), (&json!([2000.0, 4000.0]), &json!([4000.0, 2000.0]), &json!(-90.0)));
    // Trim/Extend to Corner works on straight walls.
    let line = run(&mut s, "wall", json!({"points": [[0, 6000], [1000, 6000]]}))["ids"][0].as_u64().unwrap();
    assert!(s.execute("corner", &json!({"ids": [w, line]})).unwrap_err().contains("straight"));
}

#[test]
fn walls_attached_to_a_roof_follow_its_underside() {
    let mut s = Session::new();
    let ids = house(&mut s);
    // Gabled over the 8 x 6 m box: its slopes face south and north, the ridge runs east and west.
    let roof = run(&mut s, "roof", json!({"gable": true}))["id"].as_u64().unwrap();
    // A window above the walls' tops doesn't fit the east wall, until it rises into the gable.
    assert!(s.execute("window", &json!({"at": [8000, 3000], "sill": 2000})).unwrap_err().contains("fit"));
    run(&mut s, "modify", json!({"ids": ids, "attach_top": roof}));
    let tan = 30f64.to_radians().tan();
    let top = |s: &Session, id: u64, p: P2| crate::view3d::tops_at(&s.model, p)[&id];
    assert!((top(&s, ids[1], P2(8000.0, 3000.0)) - (3000.0 + 3100.0 * tan)).abs() < 1.0, "the east wall reaches the ridge");
    assert!((top(&s, ids[0], P2(4000.0, 0.0)) - (3000.0 + 100.0 * tan)).abs() < 1.0, "the south wall keeps under its slope");
    run(&mut s, "window", json!({"at": [8000, 3000], "sill": 2000}));
    assert!((element(&mut s, ids[1])["height"].as_f64().unwrap() - (3000.0 + 3100.0 * tan)).abs() < 1.0);
    // Detached, the wall wouldn't hold the window.
    assert!(s.execute("modify", &json!({"id": ids[1], "attach_top": null})).unwrap_err().contains("fit"));
    // A wall attached to a floor stops under it; its base attached to one stands on it.
    let floor =
        run(&mut s, "floor", json!({"level": 100001, "boundary": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]]}))["id"].as_u64().unwrap();
    let part = run(&mut s, "wall", json!({"points": [[4000, 0], [4000, 6000]], "base_offset": 500, "attach_top": floor}))["ids"][0]
        .as_u64()
        .unwrap();
    assert!((top(&s, part, P2(4000.0, 3000.0)) - 2800.0).abs() < 1e-6, "under the 200 mm slab");
    let low = run(&mut s, "floor", json!({}))["id"].as_u64().unwrap();
    assert_eq!(element(&mut s, part)["height"], 2300.0);
    run(&mut s, "modify", json!({"id": part, "attach_base": low}));
    assert_eq!(element(&mut s, part)["height"], 2800.0);
    // A roof or floor going takes no wall with it: they come loose.
    run(&mut s, "delete", json!({"ids": [low]}));
    assert_eq!(element(&mut s, part).get("attach_base"), None);
    assert!(s.execute("modify", &json!({"id": part, "attach_top": ids[0]})).unwrap_err().contains("roof or a floor"));
}

#[test]
fn sloped_floors_rise_along_their_slope_arrow() {
    let mut s = Session::new();
    house(&mut s);
    // A ramp along the south wall, 6 m long, rising 5° east from 100 mm up at its arrow's tail.
    let ramp = run(
        &mut s,
        "floor",
        json!({"boundary": [[1000, 500], [7000, 500], [7000, 2000], [1000, 2000]], "offset": 100, "slope": 5, "arrow": [[1000, 1250], [7000, 1250]]}),
    )["id"]
        .as_u64()
        .unwrap();
    let (tan, cos) = (5f64.to_radians().tan(), 5f64.to_radians().cos());
    let top = |s: &Session, x: f64| crate::view3d::tops_at(&s.model, P2(x, 1000.0))[&ramp];
    assert!((top(&s, 1500.0) - (100.0 + 500.0 * tan)).abs() < 1e-6);
    assert!((top(&s, 4000.0) - (100.0 + 3000.0 * tan)).abs() < 1e-6);
    let e = element(&mut s, ramp);
    assert_eq!((e["slope"].as_f64(), e["offset"].as_f64(), e["top"].clone()), (Some(5.0), Some(100.0), json!([100.0, 624.93])));
    // 200 mm thick across its slope: a wall under it, attached, stops 200 / cos 5° below its top.
    let under = run(&mut s, "wall", json!({"points": [[4000, 500], [4000, 2000]], "type": "Interior - 100mm", "attach_top": ramp}))["ids"]
        [0]
    .as_u64()
    .unwrap();
    let tops = crate::view3d::tops_at(&s.model, P2(4000.0, 1000.0));
    assert!((tops[&under] - (100.0 + 3000.0 * tan - 200.0 / cos)).abs() < 1e-6, "{tops:?}");
    // A slope needs its arrow, and no more than 75°.
    assert!(s.execute("modify", &json!({"id": ramp, "arrow": null})).unwrap_err().contains("arrow"));
    assert!(s.execute("modify", &json!({"id": ramp, "slope": 80})).unwrap_err().contains("75°"));
    // Mirrored, it rises the other way.
    run(&mut s, "mirror", json!({"ids": [ramp], "axis": [[4000, 0], [4000, 1000]]}));
    assert!((top(&s, 6500.0) - (100.0 + 500.0 * tan)).abs() < 1e-6 && (top(&s, 1500.0) - (100.0 + 5500.0 * tan)).abs() < 1e-6);
    // Level, at its offset: (the wall under it, attached, needs it higher than its 200 mm).
    assert!(s.execute("modify", &json!({"id": ramp, "slope": 0})).unwrap_err().contains("above its base"));
    run(&mut s, "modify", json!({"id": ramp, "slope": 0, "offset": 500}));
    assert_eq!((top(&s, 1500.0), top(&s, 6500.0)), (500.0, 500.0));
}

#[test]
fn ceilings_fill_rooms_out_to_the_faces_of_their_walls() {
    let mut s = Session::new();
    house(&mut s);
    run(&mut s, "wall", json!({"points": [[5000, 0], [5000, 6000]], "type": "Interior - 100mm"}));
    // The room around the point, out to its walls' faces: 4850 x 5800 mm; 2600 mm up, 20 mm thick.
    let c = run(&mut s, "ceiling", json!({"point": [2000, 3000]}));
    assert_eq!(c["area_m2"], 28.13);
    let id = c["id"].as_u64().unwrap();
    let e = element(&mut s, id);
    assert_eq!((id_of(&s, &e["level"]), e["offset"].as_f64(), e["thickness"].as_f64()), (Some(100000), Some(2600.0), Some(20.0)));
    assert_eq!(crate::view3d::tops_at(&s.model, P2(2000.0, 3000.0))[&id], 2620.0);
    // Sketched, lower, in tiles; where no walls close around a point, it needs a boundary.
    let tiles = run(
        &mut s,
        "ceiling",
        json!({"boundary": [[5050, 100], [7900, 100], [7900, 5900], [5050, 5900]], "offset": 2400, "type": "Tile 600 x 600"}),
    );
    assert_eq!(element(&mut s, tiles["id"].as_u64().unwrap())["grid"], 600.0);
    assert!(s.execute("ceiling", &json!({"point": [9000, 3000]})).unwrap_err().contains("boundary"));
    // A spot elevation in the plan reads the floor under a ceiling, not the ceiling.
    run(&mut s, "floor", json!({}));
    let spot = run(&mut s, "spot", json!({"at": [2000, 3000]}))["id"].as_u64().unwrap();
    assert_eq!(element(&mut s, spot)["elevation"], 0.0);
    // The level's reflected ceiling plan.
    let rcp = run(&mut s, "render", json!({"view": "Level 1", "ceiling": true}));
    assert_eq!((rcp["kind"].as_str(), rcp["view"].as_str()), (Some("ceiling plan"), Some("Level 1")));
    let t = run(&mut s, "type", json!({"category": "ceiling", "name": "Tile 1200", "thickness": 15, "grid": 1200}));
    assert_eq!(t["type"], json!({"thickness": 15.0, "grid": 1200.0}));
    assert!(s.execute("type", &json!({"category": "ceiling", "name": "Fine", "thickness": 15, "grid": 10})).unwrap_err().contains("grid"));
}

#[test]
fn shafts_cut_the_floors_ceilings_and_roofs_they_pass_through() {
    let mut s = Session::new();
    house(&mut s);
    let ground = run(&mut s, "floor", json!({}))["id"].as_u64().unwrap();
    let upper =
        run(&mut s, "floor", json!({"level": 100001, "boundary": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]]}))["id"].as_u64().unwrap();
    let ceiling = run(&mut s, "ceiling", json!({"point": [4000, 3000]}))["id"].as_u64().unwrap();
    let roof = run(&mut s, "roof", json!({}))["id"].as_u64().unwrap();
    // A 1 x 2 m shaft from Level 1 up to the next: through the ceiling and the upper floor, not the
    // ground floor under it, nor the roof high over it.
    let shaft = run(&mut s, "shaft", json!({"boundary": [[3000, 2000], [4000, 2000], [4000, 4000], [3000, 4000]]}))["id"].as_u64().unwrap();
    let tops = |s: &Session, x: f64| crate::view3d::tops_at(&s.model, P2(x, 3000.0));
    let inside = tops(&s, 3500.0);
    assert!(inside.contains_key(&ground) && inside.contains_key(&roof), "{inside:?}");
    assert!(!inside.contains_key(&upper) && !inside.contains_key(&ceiling), "{inside:?}");
    assert!(tops(&s, 5000.0).contains_key(&upper), "beside it, the floor is whole");
    assert_eq!(element(&mut s, upper)["area_m2"], 46.0);
    let e = element(&mut s, shaft);
    assert_eq!((id_of(&s, &e["level"]), id_of(&s, &e["top_level"]), e["height"].as_f64()), (Some(100000), Some(100001), Some(3000.0)));
    // Up through the roof, it cuts that too.
    run(&mut s, "modify", json!({"id": shaft, "top_level": null, "height": 8000}));
    assert!(!tops(&s, 3500.0).contains_key(&roof));
    assert!(s.execute("modify", &json!({"id": shaft, "height": -5})).unwrap_err().contains("height"));
    // In IFC, an opening voids each element it cuts.
    let path = temp("shaft.ifc");
    run(&mut s, "save", json!({"path": path}));
    let ifc = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(path);
    assert_eq!((ifc.matches("IFCOPENINGELEMENT(").count(), ifc.matches("IFCRELVOIDSELEMENT(").count()), (3, 3));
    assert!(ifc.contains(".CEILING.)"), "the ceiling, as a covering");
}

#[test]
fn stairs_turn_at_landings_into_several_runs() {
    let mut s = Session::new();
    house(&mut s);
    // 17 risers of 176.47 mm up to Level 2: 8 north, a landing a metre square, turning right, 9 east.
    let st =
        run(&mut s, "stair", json!({"start": [1000, 1000], "angle": 90, "landings": [{"at": 8, "turn": -90}]}))["id"].as_u64().unwrap();
    let e = element(&mut s, st);
    assert_eq!(e["risers"], 17);
    assert_eq!(e["runs"], json!([[[1000.0, 1000.0], [1000.0, 2960.0]], [[1500.0, 3460.0], [3740.0, 3460.0]]]), "{e}");
    assert!(near(&e["end"], [3740.0, 3460.0]), "{e}");
    let each = 3000.0 / 17.0;
    let tops = |s: &Session, x: f64, y: f64| crate::view3d::tops_at(&s.model, P2(x, y)).get(&st).copied();
    assert!((tops(&s, 1000.0, 3460.0).unwrap() - 8.0 * each).abs() < 1e-6, "the landing, 8 risers up");
    assert!((tops(&s, 3600.0, 3460.0).unwrap() - 16.0 * each).abs() < 1e-6, "the last tread, a riser under Level 2");
    assert_eq!(tops(&s, 2500.0, 2000.0), None, "nothing between the runs");
    // A landing goes between two runs of two risers or more, and turns square or back.
    for (landings, why) in [
        (json!([{"at": 1, "turn": 0}]), "risers"),
        (json!([{"at": 16, "turn": 0}]), "risers"),
        (json!([{"at": 8, "turn": 45}]), "turn"),
        (json!([{"at": 9, "turn": 0}, {"at": 8, "turn": 0}]), "risers"),
        (json!([{"at": 8.5, "turn": 0}]), "whole"),
    ] {
        let e = s.execute("modify", &json!({"id": st, "landings": landings})).unwrap_err();
        assert!(e.contains(why), "{landings}: {e}");
    }
    // Back beside itself, to its left: a U, the landing two runs wide.
    run(&mut s, "modify", json!({"id": st, "landings": [{"at": 8, "turn": 180}]}));
    assert_eq!(element(&mut s, st)["runs"][1], json!([[0.0, 2960.0], [0.0, 720.0]]));
    assert!(tops(&s, 0.0, 3460.0).is_some() && tops(&s, 1000.0, 3460.0).is_some(), "one landing across both runs");
    // Mirrored, it turns the other way.
    run(&mut s, "mirror", json!({"ids": [st], "axis": [[4000, 0], [4000, 1000]]}));
    let e = element(&mut s, st);
    assert_eq!((e["landings"][0]["turn"].as_f64(), e["runs"][1][0].clone()), (Some(-180.0), json!([8000.0, 2960.0])));
}

#[test]
fn ramps_climb_at_their_slope_with_landings_between_runs() {
    let mut s = Session::new();
    house(&mut s);
    // 600 mm up at 1 in 12 (8.33%): 7.2 m of slope, in two runs of 300 mm, back beside each other.
    let r = run(
        &mut s,
        "ramp",
        json!({"start": [-4000, -6000], "angle": 0, "height": 600, "slope": 100.0 / 12.0, "landings": [{"at": 300, "turn": 180}]}),
    )["id"]
        .as_u64()
        .unwrap();
    let e = element(&mut s, r);
    assert_eq!((e["width"].as_f64(), e["length"].as_f64()), (Some(1200.0), Some(7200.0)), "{e}");
    assert_eq!(e["runs"], json!([[[-4000.0, -6000.0], [-400.0, -6000.0]], [[-400.0, -4800.0], [-4000.0, -4800.0]]]), "{e}");
    let top = |s: &Session, x: f64, y: f64| crate::view3d::tops_at(&s.model, P2(x, y)).get(&r).copied();
    assert!((top(&s, -2200.0, -6000.0).unwrap() - 150.0).abs() < 1e-6, "halfway up its first run");
    assert!((top(&s, 200.0, -5400.0).unwrap() - 300.0).abs() < 1e-6, "the landing");
    assert!((top(&s, -2200.0, -4800.0).unwrap() - 450.0).abs() < 1e-6, "halfway up its second run");
    // A slope from 1% to 50%; landings within its rise, going up.
    assert!(s.execute("modify", &json!({"id": r, "slope": 60})).unwrap_err().contains("slope"));
    assert!(s.execute("modify", &json!({"id": r, "landings": [{"at": 600, "turn": 0}]})).unwrap_err().contains("landing"));
    assert!(s.execute("ramp", &json!({"start": [0, 0], "slope": 0})).unwrap_err().contains("slope"));
}

#[test]
fn railings_run_along_stairs_and_ramps_or_along_a_path() {
    let mut s = Session::new();
    house(&mut s);
    let st = run(&mut s, "stair", json!({"start": [1000, 1000], "end": [1000, 2000]}))["id"].as_u64().unwrap();
    // Both sides of the stair, 50 mm in from its edges: going north, its left is west.
    let ids: Vec<u64> =
        run(&mut s, "railing", json!({"host": st}))["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    assert_eq!(ids.len(), 2);
    let left = element(&mut s, ids[0]);
    assert_eq!((left["side"].as_str(), left["height"].as_f64()), (Some("left"), Some(1000.0)));
    assert_eq!(left["path"], json!([[550.0, 1000.0], [550.0, 5480.0]]), "{left}");
    // Its rail rises with the stair, a metre over its walk line.
    let rail = |s: &Session, id: u64, p: P2| crate::view3d::tops_at(&s.model, p).get(&id).copied();
    let at = |y: f64| 3000.0 * (y - 1000.0) / 4480.0 + 1000.0;
    assert!((rail(&s, ids[0], P2(550.0, 3000.0)).unwrap() - at(3000.0)).abs() < 1.0, "{:?}", rail(&s, ids[0], P2(550.0, 3000.0)));
    // It goes where its stair goes, and with it.
    run(&mut s, "move", json!({"ids": [st], "by": [500, 0]}));
    assert_eq!(element(&mut s, ids[1])["path"][0], json!([1950.0, 1000.0]));
    assert!(s.execute("move", &json!({"ids": [ids[0]], "by": [100, 0]})).unwrap_err().contains("stair"));
    let copies = run(&mut s, "copy", json!({"ids": [st], "by": [3000, 0]}))["ids"].clone();
    assert_eq!(copies.as_array().map(Vec::len), Some(3), "the stair and its railings: {copies}");
    run(&mut s, "delete", json!({"ids": [st]}));
    assert_eq!(count(&mut s, "railing"), 2, "only the copy's");
    // Along a path on a level, its rail a metre over it; its type sets how high and its balusters.
    let edge =
        run(&mut s, "railing", json!({"path": [[0, 7000], [4000, 7000], [4000, 9000]], "level": 100001}))["ids"][0].as_u64().unwrap();
    assert_eq!(rail(&s, edge, P2(2000.0, 7000.0)), Some(4000.0));
    run(&mut s, "type", json!({"category": "railing", "name": "Glass - 1100mm", "height": 1100, "spacing": 0}));
    run(&mut s, "modify", json!({"id": edge, "type": "Glass - 1100mm"}));
    assert_eq!(rail(&s, edge, P2(2000.0, 7000.0)), Some(4100.0));
    // A ramp made with its railings, in one step.
    let before = count(&mut s, "railing");
    let r = run(&mut s, "ramp", json!({"start": [-5000, -5000], "height": 300, "railings": true}));
    assert_eq!((r["railings"].as_array().map(Vec::len), count(&mut s, "railing")), (Some(2), before + 2));
    run(&mut s, "undo", json!({}));
    assert_eq!((count(&mut s, "railing"), count(&mut s, "ramp")), (before, 0));
    assert!(s.execute("railing", &json!({"path": [[0, 0]]})).unwrap_err().contains("path"));
    assert!(s.execute("railing", &json!({"host": 100002})).unwrap_err().contains("stair or a ramp"));
    assert!(
        s.execute("type", &json!({"category": "railing", "name": "Dense", "height": 1000, "spacing": 5})).unwrap_err().contains("spacing")
    );
}

#[test]
fn hip_roofs_cover_any_footprint_with_valleys_where_wings_meet() {
    let mut s = Session::new();
    // An L of two 6 m wings, roofed on Level 2: 500 mm past the walls' faces, at 30°.
    run(&mut s, "wall", json!({"points": [[0, 0], [12000, 0], [12000, 6000], [6000, 6000], [6000, 12000], [0, 12000]], "closed": true}));
    let roof = run(&mut s, "roof", json!({}))["id"].as_u64().unwrap();
    let (tan, rise) = (30f64.to_radians().tan(), 300.0 / 30f64.to_radians().cos());
    let eave = 3000.0 - 500.0 * tan;
    let top = |s: &Session, x: f64, y: f64| crate::view3d::tops_at(&s.model, P2(x, y)).get(&roof).copied();
    let ridge = eave + 3600.0 * tan + rise;
    // Both wings' ridges, and over the corner where they meet.
    for (x, y) in [(9000.0, 3000.0), (3000.0, 9000.0), (3000.0, 3000.0)] {
        assert!((top(&s, x, y).unwrap() - ridge).abs() < 1e-6, "({x}, {y}): {:?}", top(&s, x, y));
    }
    // Inside the corner, a valley: each slope rises from its own eaves.
    assert!((top(&s, 9000.0, 5000.0).unwrap() - (eave + 1600.0 * tan + rise)).abs() < 1e-6);
    assert!((top(&s, 4800.0, 4800.0).unwrap() - (eave + 1800.0 * tan + rise)).abs() < 1e-6, "on the valley");
    // Its wings' ends left upright: gables, the ridge running out to them.
    run(&mut s, "modify", json!({"id": roof, "gables": [1, 4]}));
    assert!((top(&s, 12000.0, 3000.0).unwrap() - ridge).abs() < 1e-6);
    assert!(s.execute("modify", &json!({"id": roof, "gable": true})).unwrap_err().contains("rectangle"));
    assert!(s.execute("modify", &json!({"id": roof, "gables": [6]})).unwrap_err().contains("edge"));
    // A triangle pitches too: three slopes meeting over its middle.
    let tri = run(&mut s, "roof", json!({"boundary": [[20000, 0], [26000, 0], [23000, 5196.152]]}))["id"].as_u64().unwrap();
    let e = element(&mut s, tri);
    assert!((e["ridge"].as_f64().unwrap() - (3000.0 + 6000.0 / (2.0 * 3f64.sqrt()) * tan)).abs() < 0.01, "{e}");
}

#[test]
fn extrusion_roofs_sweep_their_profile_across() {
    let mut s = Session::new();
    let ids = house(&mut s);
    // Over the 8 x 6 m box on Level 2: its underside's profile along the south side, 500 mm past each
    // end, up 2 m to a ridge 3 m along, swept north over the walls.
    let profile = json!({"line": [[0, 0], [8000, 0]], "profile": [[-500, 0], [3000, 2000], [8500, 0]], "from": -600, "to": 6600});
    let roof = run(&mut s, "roof", json!({"extrusion": profile}))["id"].as_u64().unwrap();
    let e = element(&mut s, roof);
    assert_eq!((e["ridge"].as_f64(), e["area_m2"].as_f64()), (Some(5000.0), Some(64.8)), "{e}");
    // Halfway up its west slope: 1 m up, 300 mm thick across the slope.
    let top = crate::view3d::tops_at(&s.model, P2(1250.0, 3000.0))[&roof];
    assert!((top - (4000.0 + 300.0 * 3500f64.hypot(2000.0) / 3500.0)).abs() < 1e-6, "{top}");
    // Walls attach to it: the south wall's top follows its east slope.
    run(&mut s, "modify", json!({"ids": ids, "attach_top": roof}));
    let wall = crate::view3d::tops_at(&s.model, P2(4000.0, 0.0))[&ids[0]];
    assert!((wall - (3000.0 + 2000.0 * 4500.0 / 5500.0)).abs() < 1e-6, "{wall}");
    // A profile runs along its line one way, and it's swept across some depth.
    let e = s.execute("modify", &json!({"id": roof, "extrusion": {"profile": [[0, 0], [0, 1000]]}})).unwrap_err();
    assert!(e.contains("profile"), "{e}");
    assert!(s.execute("modify", &json!({"id": roof, "extrusion": {"to": -600}})).unwrap_err().contains("across"));
}

#[test]
fn dormers_stand_out_of_a_roof_and_open_it_under_them() {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [12000, 0], [12000, 8000], [0, 8000]], "closed": true}));
    let host = run(&mut s, "roof", json!({}))["id"].as_u64().unwrap();
    // On the south slope, its front 1.6 m up it from the eaves: walls 800 mm up from the roof, a
    // window in its front, a gable roof of its own at 30°.
    let d = run(&mut s, "dormer", json!({"host": host, "at": [6000, 1000], "width": 1600, "height": 800, "window": "Fixed 600 x 900"}));
    let roof = d["roof"].as_u64().unwrap();
    assert_eq!(d["walls"].as_array().map(Vec::len), Some(3), "{d}");
    let (tan, rise) = (30f64.to_radians().tan(), 300.0 / 30f64.to_radians().cos());
    let front = 3000.0 + 1100.0 * tan + rise;
    let ridge = front + 800.0 + 800.0 * tan;
    let top = |s: &Session, id: u64, x: f64, y: f64| crate::view3d::tops_at(&s.model, P2(x, y)).get(&id).copied();
    assert!((top(&s, roof, 6000.0, 1300.0).unwrap() - (ridge + rise)).abs() < 1e-6, "{:?}", top(&s, roof, 6000.0, 1300.0));
    assert_eq!(top(&s, host, 6000.0, 1300.0), None, "the roof is open under it");
    assert!(top(&s, host, 2000.0, 1300.0).is_some(), "and whole beside it");
    // Its roof ends where it runs into the roof it stands on.
    assert_eq!(top(&s, roof, 6000.0, 3900.0), None);
    assert!(top(&s, host, 6000.0, 3900.0).is_some());
    // Its front, a gable wall up to its ridge, with the window in it.
    let window = run(&mut s, "elements", json!({"category": "window"}));
    assert_eq!((window["count"].as_u64(), id_of(&s, &window["elements"][0]["host"])), (Some(1), d["walls"][0].as_u64()));
    assert!((top(&s, d["walls"][0].as_u64().unwrap(), 6000.0, 1000.0).unwrap() - ridge).abs() < 1.0);
    // The roof it stands on gone, its roof stays, ending nowhere now, and its walls stand on their own.
    let mut t = Session::new();
    t.model = s.model.clone();
    run(&mut t, "delete", json!({"ids": [host]}));
    assert_eq!(element(&mut t, roof)["join"], Value::Null);
    assert_eq!(element(&mut t, d["walls"][1].as_u64().unwrap())["attach_base"], Value::Null);
    // A level roof has no slope to stand a dormer on.
    let flat = run(&mut s, "roof", json!({"slope": 0, "boundary": [[20000, 0], [24000, 0], [24000, 4000], [20000, 4000]]}))["id"].clone();
    assert!(s.execute("dormer", &json!({"host": flat, "at": [22000, 2000]})).unwrap_err().contains("slope"));
}

#[test]
fn walls_under_a_trimmed_roof_stop_at_its_underside() {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [12000, 0], [12000, 8000], [0, 8000]], "closed": true, "type": "Exterior - 300mm"}));
    let host = run(&mut s, "roof", json!({"slope": 45}))["id"].as_u64().unwrap();
    // A dormer's walls under its roof, trimmed where it runs into the host's.
    let d = run(&mut s, "dormer", json!({"host": host, "at": [9000, 800], "width": 1800, "height": 1000}));
    let roof = d["roof"].as_u64().unwrap();
    // No part of them stands over its underside.
    for w in d["walls"].as_array().unwrap().iter().filter_map(Value::as_u64) {
        for p in crate::walls::prisms(&s.model, w, false) {
            for q in &p.ring {
                if let Some(under) = crate::walls::underside(&s.model, roof, *q) {
                    assert!(p.top.at(*q) <= under + 1e-6, "wall {w} at {q:?}: {} over {under}", p.top.at(*q));
                }
            }
        }
    }
}

#[test]
fn roofs_slope_round_a_courtyard_up_to_a_ring_of_ridges() {
    let mut s = Session::new();
    // A 16 m square round a 4 m courtyard: four 6 m wings, roofed on Level 2 at 30°.
    let outline = json!([[0, 0], [16000, 0], [16000, 16000], [0, 16000]]);
    let court = json!([[6000, 6000], [10000, 6000], [10000, 10000], [6000, 10000]]);
    let roof = run(&mut s, "roof", json!({"boundary": outline, "courtyards": [court]}))["id"].as_u64().unwrap();
    let (tan, rise) = (30f64.to_radians().tan(), 300.0 / 30f64.to_radians().cos());
    let top = |s: &Session, x: f64, y: f64| crate::view3d::tops_at(&s.model, P2(x, y)).get(&roof).copied();
    // Each wing's ridge 3 m in from both its eaves: a ring of ridges round the courtyard, open over it.
    let ridge = 3000.0 + 3000.0 * tan;
    for (x, y) in [(8000.0, 3000.0), (13000.0, 8000.0), (8000.0, 13000.0), (3000.0, 8000.0), (3000.0, 3000.0), (13000.0, 13000.0)] {
        assert!((top(&s, x, y).unwrap() - (ridge + rise)).abs() < 1e-6, "({x}, {y}): {:?}", top(&s, x, y));
    }
    assert_eq!(top(&s, 8000.0, 8000.0), None, "the courtyard is open");
    // Its slopes rise from the courtyard's eaves as from the outline's.
    for y in [5000.0, 1000.0] {
        assert!((top(&s, 8000.0, y).unwrap() - (3000.0 + 1000.0 * tan + rise)).abs() < 1e-6, "{y}");
    }
    let e = element(&mut s, roof);
    assert_eq!((e["ridge"].as_f64(), e["area_m2"].as_f64()), (Some(4732.05), Some(240.0)), "{e}");
    // In plan, its eaves round the courtyard; from Level 1, they're dashed overhead.
    let d = crate::render::plan(&s.model, 100001, 800, 600);
    assert_eq!(d.pick(d.view.px(P2(8000.0, 6000.0)), 2.0), Some(roof));
    let below = crate::render::plan(&s.model, 100000, 800, 600);
    assert_eq!(below.pick(below.view.px(P2(6000.0, 8000.0)), 2.0), Some(roof), "the courtyard's edge, overhead");
    // A section across it cuts both wings at their ridges, and nothing over the courtyard.
    let sec = run(&mut s, "section", json!({"start": [8000, -2000], "end": [8000, 18000]}))["id"].as_u64().unwrap();
    let view = crate::render::section_fit(&s.model, sec, 800, 600);
    let d = crate::render::section(&s.model, sec, view);
    let cut = |y: f64, z: f64| d.pick(view.px(P2(y + 2000.0, z)), 2.0);
    assert_eq!((cut(3000.0, ridge + 100.0), cut(13000.0, ridge + 100.0), cut(8000.0, 4000.0)), (Some(roof), Some(roof), None));
    // A courtyard's edges number on after the outline's, and stand upright from the desktop too:
    // its south side (4) a gable, the south wing's slope runs up to it, 6 m in.
    assert_eq!(s.model.roof_edge(2, P2(8000.0, 6010.0), 50.0), Some((roof, 4)));
    run(&mut s, "modify", json!({"id": roof, "gables": [4]}));
    assert!((top(&s, 8000.0, 5900.0).unwrap() - (3000.0 + 5900.0 * tan + rise)).abs() < 1e-6, "{:?}", top(&s, 8000.0, 5900.0));
    assert!(s.execute("modify", &json!({"id": roof, "gables": [8]})).unwrap_err().contains("edge 8"));
    // A courtyard lies inside the outline, apart from the others; an extrusion roof has none.
    for (bad, why) in [
        (json!([[[20000, 0], [22000, 0], [22000, 2000]]]), "inside"),
        (json!([[[-1000, 6000], [2000, 6000], [2000, 8000], [-1000, 8000]]]), "inside"),
        (json!([court, [[7000, 7000], [12000, 7000], [12000, 12000]]]), "overlap"),
    ] {
        let e = s.execute("modify", &json!({"id": roof, "courtyards": bad})).unwrap_err();
        assert!(e.contains(why), "{e}");
    }
    let profile = json!({"line": [[0, 0], [16000, 0]], "profile": [[0, 0], [8000, 3000], [16000, 0]], "to": 16000});
    let e = s.execute("roof", &json!({"extrusion": profile, "courtyards": [court]})).unwrap_err();
    assert!(e.contains("footprint"), "{e}");
    // Flat, it's a hole through it.
    run(&mut s, "modify", json!({"id": roof, "slope": 0, "gables": []}));
    assert_eq!((top(&s, 8000.0, 8000.0), element(&mut s, roof)["area_m2"].as_f64()), (None, Some(240.0)));
    assert!(top(&s, 8000.0, 5000.0).is_some());
}

#[test]
fn roofs_over_the_walls_leave_a_loop_of_walls_standing_free_open_as_a_courtyard() {
    let mut s = Session::new();
    // A 16 m square house round a 4 m courtyard, its walls 200 mm thick: no wall joins the loop round
    // the courtyard to the outer walls.
    run(&mut s, "wall", json!({"points": [[0, 0], [16000, 0], [16000, 16000], [0, 16000]], "closed": true}));
    run(&mut s, "wall", json!({"points": [[6000, 6000], [10000, 6000], [10000, 10000], [6000, 10000]], "closed": true}));
    let roof = run(&mut s, "roof", json!({}))["id"].as_u64().unwrap();
    let e = element(&mut s, roof);
    // 500 mm past the walls' faces: out round the house, and in round the courtyard, clockwise from
    // its south-west corner.
    assert_eq!(e["boundary"][0], json!([-600.0, -600.0]));
    assert_eq!(e["courtyards"], json!([[[6600.0, 6600.0], [6600.0, 9400.0], [9400.0, 9400.0], [9400.0, 6600.0]]]));
    // Each wing 7.2 m from eaves to eaves, its ridge 3.6 m in from both, where the slopes have risen
    // from 500 mm below the base: 3000 + 3100 tan 30°. Its area: 17.2² less 2.8².
    assert_eq!((e["ridge"].as_f64(), e["area_m2"].as_f64()), (Some(4789.79), Some(288.0)), "{e}");
    let top = |s: &Session, id: u64, x: f64, y: f64| crate::view3d::tops_at(&s.model, P2(x, y)).get(&id).copied();
    assert_eq!(top(&s, roof, 8000.0, 8000.0), None, "the courtyard is open");
    // Courtyards given are the roof's own; one too small to stay open past the overhang is roofed over.
    let given = run(&mut s, "roof", json!({"courtyards": []}))["id"].as_u64().unwrap();
    assert!(top(&s, given, 8000.0, 8000.0).is_some());
    let wide = run(&mut s, "roof", json!({"overhang": 2100}))["id"].as_u64().unwrap();
    assert_eq!((element(&mut s, wide)["courtyards"].clone(), top(&s, wide, 8000.0, 8000.0).is_some()), (Value::Null, true));
    // A loop with a room in it is indoors, and so is one joined to the outer walls: rooms, roofed over.
    let room = run(&mut s, "room", json!({"point": [8000, 8000], "name": "Hall"}))["id"].as_u64().unwrap();
    let hall = run(&mut s, "roof", json!({}))["id"].as_u64().unwrap();
    assert_eq!((element(&mut s, hall)["courtyards"].clone(), top(&s, hall, 8000.0, 8000.0).is_some()), (Value::Null, true));
    run(&mut s, "delete", json!({"ids": [room]}));
    run(&mut s, "wall", json!({"points": [[8000, 0], [8000, 6000]]}));
    let joined = run(&mut s, "roof", json!({}))["id"].as_u64().unwrap();
    assert_eq!((element(&mut s, joined)["courtyards"].clone(), top(&s, joined, 8000.0, 8000.0).is_some()), (Value::Null, true));
}

#[test]
fn floors_over_the_walls_leave_a_loop_of_walls_standing_free_open_as_a_courtyard() {
    let mut s = Session::new();
    // A two-storey 16 m square house round a 4 m courtyard: on each level, no wall joins the loop
    // round the courtyard to the outer walls.
    for level in ["Level 1", "Level 2"] {
        run(&mut s, "wall", json!({"level": level, "points": [[0, 0], [16000, 0], [16000, 16000], [0, 16000]], "closed": true}));
        run(
            &mut s,
            "wall",
            json!({"level": level, "points": [[6000, 6000], [10000, 6000], [10000, 10000], [6000, 10000]], "closed": true}),
        );
    }
    // Level 2's floor runs along the walls' centre lines, round the loop too, clockwise from its
    // south-west corner: 16² less 4².
    let made = run(&mut s, "floor", json!({"level": "Level 2"}));
    let floor = made["id"].as_u64().unwrap();
    let e = element(&mut s, floor);
    let court = json!([[[6000.0, 6000.0], [6000.0, 10000.0], [10000.0, 10000.0], [10000.0, 6000.0]]]);
    assert_eq!((made["area_m2"].as_f64(), e["area_m2"].as_f64(), e["openings"].clone()), (Some(240.0), Some(240.0), court), "{e}");
    // Open over the courtyard in 3D, and in a section across it: cut in both wings, not between.
    let top = |s: &Session, id: u64, x: f64, y: f64| crate::view3d::tops_at(&s.model, P2(x, y)).get(&id).copied();
    assert_eq!((top(&s, floor, 8000.0, 8000.0), top(&s, floor, 8000.0, 3000.0)), (None, Some(3000.0)));
    let sec = run(&mut s, "section", json!({"start": [8000, -2000], "end": [8000, 18000]}))["id"].as_u64().unwrap();
    let view = crate::render::section_fit(&s.model, sec, 800, 600);
    let d = crate::render::section(&s.model, sec, view);
    let cut = |y: f64| d.pick(view.px(P2(y + 2000.0, 2900.0)), 1.0);
    assert_eq!((cut(3000.0), cut(13000.0), cut(8000.0)), (Some(floor), Some(floor), None));
    // In plan, the hole's edge, under the courtyard's walls.
    let plan = crate::render::plan(&s.model, 100001, 800, 600);
    assert!(plan.pick_all(plan.view.px(P2(8000.0, 6000.0)), 2.0).contains(&floor), "the courtyard's edge");
    // Openings given are the floor's own.
    let given = run(&mut s, "floor", json!({"level": "Level 2", "openings": []}))["id"].as_u64().unwrap();
    assert_eq!(top(&s, given, 8000.0, 8000.0), Some(3000.0));
    // A loop with a room in it is indoors, and so is one joined to the outer walls: rooms, floored over.
    let room = run(&mut s, "room", json!({"level": "Level 2", "point": [8000, 8000], "name": "Hall"}))["id"].as_u64().unwrap();
    let hall = run(&mut s, "floor", json!({"level": "Level 2"}));
    assert_eq!((element(&mut s, hall["id"].as_u64().unwrap())["openings"].clone(), hall["area_m2"].as_f64()), (Value::Null, Some(256.0)));
    run(&mut s, "delete", json!({"ids": [room]}));
    run(&mut s, "wall", json!({"level": "Level 2", "points": [[8000, 0], [8000, 6000]]}));
    let joined = run(&mut s, "floor", json!({"level": "Level 2"}));
    assert_eq!(
        (element(&mut s, joined["id"].as_u64().unwrap())["openings"].clone(), joined["area_m2"].as_f64()),
        (Value::Null, Some(256.0))
    );
}

#[test]
fn ceilings_in_a_room_round_a_loop_of_walls_stop_at_its_faces() {
    let mut s = Session::new();
    // A 16 m square house round a 4 m courtyard, a loop of walls standing free inside: the room
    // round it is a ring.
    run(&mut s, "wall", json!({"points": [[0, 0], [16000, 0], [16000, 16000], [0, 16000]], "closed": true}));
    run(&mut s, "wall", json!({"points": [[6000, 6000], [10000, 6000], [10000, 10000], [6000, 10000]], "closed": true}));
    // Out to the walls' faces, round the loop's too, clockwise from its south-west corner: 15.8² less 4.2².
    let made = run(&mut s, "ceiling", json!({"point": [3000, 3000], "type": "Tile 600 x 600"}));
    let id = made["id"].as_u64().unwrap();
    let e = element(&mut s, id);
    let island = json!([[[5900.0, 5900.0], [5900.0, 10100.0], [10100.0, 10100.0], [10100.0, 5900.0]]]);
    assert_eq!((made["area_m2"].as_f64(), e["area_m2"].as_f64(), e["openings"].clone()), (Some(232.0), Some(232.0), island), "{e}");
    // Open over the courtyard in 3D, and in a section across it: cut in both wings, not between.
    let top = |s: &Session, x: f64, y: f64| crate::view3d::tops_at(&s.model, P2(x, y)).get(&id).copied();
    assert_eq!((top(&s, 8000.0, 8000.0), top(&s, 8000.0, 3000.0)), (None, Some(2620.0)));
    let sec = run(&mut s, "section", json!({"start": [8000, -2000], "end": [8000, 18000]}))["id"].as_u64().unwrap();
    let view = crate::render::section_fit(&s.model, sec, 800, 600);
    let d = crate::render::section(&s.model, sec, view);
    let cut = |y: f64| d.pick(view.px(P2(y + 2000.0, 2610.0)), 1.0);
    assert_eq!((cut(3000.0), cut(13000.0), cut(8000.0)), (Some(id), Some(id), None));
    // Its reflected ceiling plan draws the hole's edge (between two tile lines), and its tiles stop
    // there: a tile line runs through the middle of its box, across the south wing, not the courtyard.
    let rcp = crate::render::ceiling_plan(&s.model, 100000, 800, 600);
    let at = |x: f64, y: f64| rcp.pick_all(rcp.view.px(P2(x, y)), 2.0).contains(&id);
    assert_eq!((at(8300.0, 5900.0), at(8000.0, 3000.0), at(8000.0, 8000.0)), (true, true, false), "the hole's edge, a tile, the courtyard");
    // Moved, its openings go with it.
    run(&mut s, "move", json!({"ids": [id], "by": [0, 1000]}));
    assert_eq!(element(&mut s, id)["openings"][0][0], json!([5900.0, 6900.0]));
    // Sketched, its openings are the ones given, each inside it.
    let square = json!([[20000, 0], [24800, 0], [24800, 4800], [20000, 4800]]);
    let plain = run(&mut s, "ceiling", json!({"boundary": square}))["id"].as_u64().unwrap();
    assert_eq!(element(&mut s, plain)["openings"], Value::Null);
    run(&mut s, "delete", json!({"ids": [plain]}));
    let out = json!([[[25000, 1000], [26000, 1000], [26000, 2000], [25000, 2000]]]);
    assert!(s.execute("ceiling", &json!({"boundary": square, "openings": out})).unwrap_err().contains("isn't inside its outline"));
    // Tiled from its middle, (22400, 2400), round a hole whose edges are on its grid: the tile lines
    // along them run on past it, where the hole cuts the ceiling into pieces.
    let hole = json!([[[21800, 1800], [23000, 1800], [23000, 3000], [21800, 3000]]]);
    let tiled = run(&mut s, "ceiling", json!({"boundary": square, "openings": hole, "type": "Tile 600 x 600"}));
    assert_eq!(tiled["area_m2"], 21.6);
    let (rcp, tiled) = (crate::render::ceiling_plan(&s.model, 100000, 800, 600), tiled["id"].as_u64().unwrap());
    let at = |x: f64, y: f64| rcp.pick_all(rcp.view.px(P2(x, y)), 2.0).contains(&tiled);
    assert_eq!((at(24000.0, 1800.0), at(23000.0, 4000.0), at(22400.0, 2400.0)), (true, true, false), "past the hole, and in it");
}

#[test]
fn layers_pick_their_materials_from_the_projects_library() {
    let mut s = Session::new();
    // A new project's library: what its default types are made of, and more.
    let lib = run(&mut s, "inspect", json!({}))["materials"].clone();
    for name in ["Brick", "Insulation", "Concrete block", "Plaster", "Concrete", "Screed", "Timber", "Ceramic tile", "Roof tiles"] {
        assert!(lib[name]["color"].is_array(), "{name}: {lib}");
    }
    assert_eq!((&lib["Brick"]["cut"], &lib["Insulation"]["cut"]), (&json!("brick"), &json!("insulation")));
    // A material: its shading colour, the pattern its cut shows and the one its surfaces show.
    let oak = run(&mut s, "material", json!({"name": "Oak", "color": "#a0703c", "cut": "diagonal"}));
    assert_eq!(oak["material"], json!({"color": [160, 112, 60], "cut": "diagonal", "surface": "none"}));
    let oak = run(&mut s, "material", json!({"name": "Oak", "surface": "horizontal", "color": [150, 100, 50]}));
    assert_eq!(oak["material"], json!({"color": [150, 100, 50], "cut": "diagonal", "surface": "horizontal"}));
    // A material's transparency, percent: kept, cleared, and refused past 100.
    let glass = run(&mut s, "material", json!({"name": "Oak", "transparency": 40}));
    assert_eq!(glass["material"]["transparency"], 40);
    assert_eq!(s.model.materials["Oak"].transparency, 40);
    let glass = run(&mut s, "material", json!({"name": "Oak", "transparency": 0}));
    assert!(glass["material"].get("transparency").is_none(), "{glass}");
    for bad in [json!(101), json!(-1), json!(2.5), json!("x"), json!(1e300)] {
        let e = s.execute("material", &json!({"name": "Oak", "transparency": bad})).unwrap_err();
        assert!(e.contains("transparency"), "{bad}: {e}");
    }
    assert_eq!(s.model.materials["Oak"].transparency, 0);
    assert!(s.model.materials["Glass"].transparency > 0, "the starter glass is see-through");
    // Layers name one of its materials, or none.
    let floor = |m: &str| json!({"category": "floor", "name": "Oak floor", "layers": [{"function": "finish", "material": m, "thickness": 20}, {"thickness": 180}]});
    let e = s.execute("type", &floor("Oke")).unwrap_err();
    assert!(e.contains("Oke") && e.contains("material"), "{e}");
    run(&mut s, "type", floor("Oak"));
    // Renamed, its layers follow it; while a layer uses it, it can't go.
    run(&mut s, "material", json!({"name": "Oak", "rename": "European oak"}));
    let t = run(&mut s, "inspect", json!({}))["types"]["floor"]["Oak floor"].clone();
    assert_eq!(t["layers"][0]["material"], "European oak");
    let e = s.execute("material", &json!({"name": "European oak", "delete": true})).unwrap_err();
    assert!(e.contains("Oak floor"), "{e}");
    run(&mut s, "material", json!({"name": "Gypsum board", "delete": true}));
    assert_eq!(run(&mut s, "inspect", json!({}))["materials"]["Gypsum board"], Value::Null);
    // The families use timber: it can't go, and renamed, they follow it.
    let e = s.execute("material", &json!({"name": "Timber", "delete": true})).unwrap_err();
    assert!(e.contains("family"), "{e}");
    run(&mut s, "material", json!({"name": "Timber", "rename": "Pine"}));
    let table = &s.model.families["Table"];
    assert_eq!(table.parameters.iter().find(|p| p.name == "Material").map(|p| p.value.clone()), Some(json!("Pine")));
    for (bad, says) in [
        (json!({"name": "X", "color": "#12"}), "color"),
        (json!({"name": "X", "color": [300, 0, 0]}), "color"),
        (json!({"name": "X", "cut": "polka"}), "polka"),
        (json!({"name": "Brick", "rename": "Plaster"}), "Plaster"),
        (json!({"name": "Nothing", "delete": true}), "Nothing"),
        (json!({"name": ""}), "name"),
    ] {
        let e = s.execute("material", &bad).unwrap_err();
        assert!(e.contains(says), "{bad}: {e}");
    }
}

#[test]
fn floors_roofs_and_ceilings_are_built_of_layers_from_the_top_down() {
    let mut s = Session::new();
    let ids = house(&mut s);
    let tiled = json!([
        {"function": "finish", "material": "Ceramic tile", "thickness": 10},
        {"function": "substrate", "material": "Screed", "thickness": 40},
        {"function": "structure", "material": "Concrete", "thickness": 200}
    ]);
    let t = run(&mut s, "type", json!({"category": "floor", "name": "Tiled 250", "layers": tiled}))["type"].clone();
    assert_eq!(t["thickness"], 250.0);
    // A new thickness goes to its structure, as a wall type's does.
    let t = run(&mut s, "type", json!({"category": "floor", "name": "Tiled 250", "thickness": 300}))["type"].clone();
    assert_eq!(t["layers"][2]["thickness"], 250.0);
    let roof = json!([
        {"function": "finish", "material": "Roof tiles", "thickness": 50},
        {"function": "membrane", "material": "Membrane", "thickness": 0},
        {"function": "thermal", "material": "Insulation", "thickness": 150},
        {"function": "structure", "material": "Timber", "thickness": 100}
    ]);
    run(&mut s, "type", json!({"category": "roof", "name": "Tiled roof", "layers": roof}));
    let board = json!([{"function": "substrate", "material": "Timber", "thickness": 25}, {"function": "finish", "material": "Gypsum board", "thickness": 13}]);
    let t = run(&mut s, "type", json!({"category": "ceiling", "name": "Board", "layers": board, "grid": 600}))["type"].clone();
    assert_eq!((&t["thickness"], &t["grid"]), (&json!(38.0), &json!(600.0)));
    let e = s.execute("type", &json!({"category": "roof", "name": "Odd", "thickness": 10, "layers": [{"thickness": 50}]})).unwrap_err();
    assert!(e.contains("50"), "{e}");
    // In 3D each layer is a slab of its own, in its material's colour, the first on top.
    let boundary = json!([[0, 0], [8000, 0], [8000, 6000], [0, 6000]]);
    let floor = run(&mut s, "floor", json!({"level": "Level 2", "boundary": boundary, "type": "Tiled 250"}))["id"].as_u64().unwrap();
    let r = run(&mut s, "roof", json!({"type": "Tiled roof"}))["id"].as_u64().unwrap();
    run(&mut s, "modify", json!({"ids": ids, "attach_top": r}));
    let ceiling = run(&mut s, "ceiling", json!({"point": [4000, 3000], "type": "Board"}))["id"].as_u64().unwrap();
    let solids = crate::view3d::shaded(&s.model);
    let of = |id: u64| {
        solids.iter().filter(|x| x.owner() == id).map(|x| (x.heights(), x.color())).map(|(h, c)| (h, [c.0, c.1, c.2])).collect::<Vec<_>>()
    };
    let colour = |name: &str| s.model.materials[name].color;
    let floor_layers = of(floor);
    assert_eq!(floor_layers.len(), 3, "{floor_layers:?}");
    for (((lo, hi), c), (want, name)) in
        floor_layers.iter().zip([((2990.0, 3000.0), "Ceramic tile"), ((2950.0, 2990.0), "Screed"), ((2700.0, 2950.0), "Concrete")])
    {
        assert!((lo - want.0).abs() < 1e-6 && (hi - want.1).abs() < 1e-6, "{name}: {lo} {hi}");
        assert_eq!(*c, colour(name), "{name}");
    }
    // A roof's layers across its slope, the membrane none; a ceiling's, its underside 2600 mm up.
    assert_eq!(of(r).iter().map(|(_, c)| *c).collect::<std::collections::BTreeSet<_>>().len(), 3);
    let c = of(ceiling);
    assert_eq!(c.len(), 2);
    assert!((c[1].0.0 - 2600.0).abs() < 1e-6 && (c[0].0.1 - 2638.0).abs() < 1e-6, "{c:?}");
    // A section through the floor: a thin line between each two of its layers.
    run(&mut s, "section", json!({"start": [4000, -2000], "end": [4000, 8000]}));
    let id = s.model.elements.iter().find(|(_, e)| matches!(e, crate::model::Element::Section(_))).map(|(id, _)| *id).unwrap();
    let view = crate::render::section_fit(&s.model, id, 2000, 1500);
    let d = crate::render::section(&s.model, id, view);
    let thin = |z: f64| {
        let y = view.px(P2(0.0, z)).1;
        d.prims.iter().zip(&d.owners).any(|(p, o)| {
            *o == Some(floor)
                && matches!(p, crate::render::Prim::Line { pts, width, .. }
                    if *width < d.width(crate::pen::Pen(7)) && pts.len() == 2 && pts.iter().all(|q| (q.1 - y).abs() < 0.5) && (pts[1].0 - pts[0].0).abs() > 50.0)
        })
    };
    assert!(thin(2990.0) && thin(2950.0), "between its layers");
    assert!(!thin(2970.0), "not across a layer");
}

#[test]
fn cut_patterns_show_where_a_layer_is_wide_enough_to_hold_them() {
    let mut s = Session::new();
    cavity(&mut s);
    let w = run(&mut s, "wall", json!({"points": [[0, 0], [6000, 0]], "type": "Cavity 300"}))["ids"][0].as_u64().unwrap();
    // Close in, 2 px a millimetre: its brick (100 mm, to its left, north) holds a brick pattern's lines.
    let lines = |scale: f64| {
        let view = crate::render::View { scale, cx: 3000.0, cy: 0.0, width: 1200, height: 800 };
        let d = crate::render::draw(&s.model, 100000, view);
        let (north, south) = (view.px(P2(3000.0, 150.0)).1, view.px(P2(3000.0, 50.0)).1);
        d.prims
            .iter()
            .zip(&d.owners)
            .filter(|(p, o)| {
                **o == Some(w)
                    && matches!(p, crate::render::Prim::Line { pts, width, .. }
                        if *width < d.width(crate::pen::Pen(4)) && pts.iter().all(|q| q.1 >= north - 0.5 && q.1 <= south + 0.5))
            })
            .count()
    };
    assert!(lines(2.0) > 20, "close in: {}", lines(2.0));
    // Far out, too thin for them: the layer is filled as before.
    assert_eq!(lines(0.02), 0);
}

#[test]
fn a_section_crossing_a_curved_wall_twice_shows_its_layers_at_both_cuts() {
    let mut s = Session::new();
    let w =
        run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]], "arc": -170, "type": "Exterior - 300mm"}))["ids"][0].as_u64().unwrap();
    run(&mut s, "section", json!({"start": [-500, 1000], "end": [4500, 1000]}));
    let id = s.model.elements.iter().find(|(_, e)| matches!(e, crate::model::Element::Section(_))).map(|(id, _)| *id).unwrap();
    let view = crate::render::section_fit(&s.model, id, 2000, 1500);
    let d = crate::render::section(&s.model, id, view);
    // The section runs west to east: the wall's west cut lies before its middle, its east cut past it.
    let middle = view.px(P2(2500.0, 0.0)).0;
    let hatched = |west: bool| {
        d.prims.iter().zip(&d.owners).any(|(p, o)| {
            *o == Some(w)
                && matches!(p, crate::render::Prim::Line { pts, width, .. } if *width < d.width(crate::pen::Pen(4)) && pts.iter().all(|q| (q.0 < middle) == west))
        })
    };
    assert!(hatched(true) && hatched(false), "west {}, east {}", hatched(true), hatched(false));
}

#[test]
fn surface_patterns_keep_their_size_in_the_building() {
    let mut s = Session::new();
    let tiled = json!([{"function": "finish", "material": "Ceramic tile", "thickness": 10}, {"function": "structure", "material": "Concrete", "thickness": 200}]);
    run(&mut s, "type", json!({"category": "floor", "name": "Tiled", "layers": tiled}));
    let boundary = json!([[0, 0], [6000, 0], [6000, 4000], [0, 4000]]);
    let f = run(&mut s, "floor", json!({"boundary": boundary, "type": "Tiled"}))["id"].as_u64().unwrap();
    // Where the tiles' joints running east cross the plan, px down it.
    let rows = |scale: f64| {
        let view = crate::render::View { scale, cx: 3000.0, cy: 2000.0, width: 1200, height: 800 };
        let d = crate::render::draw(&s.model, 100000, view);
        let mut ys: Vec<f32> = d
            .prims
            .iter()
            .zip(&d.owners)
            .filter_map(|(p, o)| match p {
                crate::render::Prim::Line { pts, width, .. }
                    if *o == Some(f)
                        && *width < d.width(crate::pen::Pen(4))
                        && pts.len() == 2
                        && (pts[0].1 - pts[1].1).abs() < 1e-6
                        && (pts[1].0 - pts[0].0).abs() > 1.0 =>
                {
                    Some(pts[0].1)
                }
                _ => None,
            })
            .collect();
        ys.sort_by(f32::total_cmp);
        ys
    };
    // A tile is 200 mm whatever the zoom: 40 px at 0.2 px a millimetre, 80 px at 0.4.
    for scale in [0.2, 0.4] {
        let ys = rows(scale);
        assert!(ys.len() > 3 && ys.windows(2).all(|w| (f64::from(w[1] - w[0]) - 200.0 * scale).abs() < 0.01), "{scale}: {ys:?}");
    }
    // Far out, too fine to read: left out.
    assert!(rows(0.02).is_empty());
}

#[test]
fn materials_and_layers_go_to_ifc_and_come_back() {
    let mut s = Session::new();
    house(&mut s);
    run(&mut s, "material", json!({"name": "Oak", "color": "#a0703c", "cut": "diagonal"}));
    let layers = json!([{"function": "finish", "material": "Oak", "thickness": 20}, {"function": "structure", "material": "Concrete", "thickness": 180}]);
    run(&mut s, "type", json!({"category": "floor", "name": "Oak 200", "layers": layers}));
    run(&mut s, "floor", json!({"type": "Oak 200"}));
    let path = temp("materials.ifc");
    run(&mut s, "save", json!({"path": path}));
    let ifc = std::fs::read_to_string(&path).unwrap();
    assert!(ifc.contains("IFCMATERIAL('Oak'") && ifc.contains("IFCMATERIALLAYERSET(") && ifc.contains("'Oak 200'"), "{ifc}");
    // Its colour, for viewers to shade it.
    assert!(ifc.contains("IFCCOLOURRGB($,0.627") && ifc.contains("IFCMATERIALDEFINITIONREPRESENTATION("), "{ifc}");
    let mut t = Session::new();
    run(&mut t, "open", json!({"path": path}));
    let i = run(&mut t, "inspect", json!({}));
    let ty = i["types"]["floor"].as_object().unwrap().values().find(|ty| ty["layers"].as_array().is_some_and(|l| l.len() == 2)).cloned();
    let ty = ty.unwrap_or_else(|| panic!("{}", i["types"]["floor"]));
    assert_eq!(
        (&ty["layers"][0]["material"], &ty["layers"][0]["thickness"], &ty["layers"][1]["function"]),
        (&json!("Oak"), &json!(20.0), &json!("structure"))
    );
    assert_eq!(i["materials"]["Oak"]["color"], json!([160, 112, 60]));
    let _ = std::fs::remove_file(path);
}

#[test]
fn a_materials_transparency_goes_to_ifc_in_its_surface_style_and_comes_back() {
    let mut s = Session::new();
    house(&mut s);
    run(&mut s, "material", json!({"name": "Tint", "color": "#a0c0e0", "transparency": 40}));
    run(&mut s, "material", json!({"name": "Paint", "color": "#a0703c"}));
    run(
        &mut s,
        "type",
        json!({"category": "floor", "name": "Tinted", "layers": [{"material": "Tint", "thickness": 20}, {"material": "Paint", "thickness": 180}]}),
    );
    run(&mut s, "floor", json!({"type": "Tinted"}));
    let path = temp("transparency.ifc");
    run(&mut s, "save", json!({"path": path}));
    let ifc = std::fs::read_to_string(&path).unwrap();
    assert!(ifc.contains("IFCSURFACESTYLESHADING(#") && ifc.contains(",0.4)"), "{ifc}");
    let mut t = Session::new();
    run(&mut t, "open", json!({"path": path}));
    let m = &t.model.materials;
    assert_eq!((m["Tint"].transparency, m["Paint"].transparency), (40, 0));
    let _ = std::fs::remove_file(path);
}

#[test]
fn a_project_from_before_the_library_brings_its_layers_materials_into_it() {
    let mut s = Session::new();
    cavity(&mut s);
    let path = temp("old.bimcraft");
    run(&mut s, "save", json!({"path": path}));
    // As an older BIMCraft saved it: no library, its layers' materials free text.
    let mut v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    v.as_object_mut().unwrap().remove("materials");
    v["types"]["wall"]["Cavity 300"]["layers"][1]["material"] = json!("Rock wool");
    std::fs::write(&path, v.to_string()).unwrap();
    let mut t = Session::new();
    run(&mut t, "open", json!({"path": path}));
    let lib = run(&mut t, "inspect", json!({}))["materials"].clone();
    assert!(lib["Rock wool"]["color"].is_array() && lib["Brick"]["color"].is_array(), "{lib}");
    let _ = std::fs::remove_file(path);
}

#[test]
fn a_new_project_has_doors_and_windows_of_its_own_that_build_in_every_language() {
    for l in crate::language::Language::ALL {
        let m = Model::new_in("x", l);
        let hosted: Vec<(&str, crate::family::Category)> =
            m.families.iter().filter(|(_, f)| f.category.hosted()).map(|(n, f)| (n.as_str(), f.category)).collect();
        let named = |english: &str| l.word(english).to_string();
        for (english, kind) in [
            ("Single Door", crate::family::Category::Door),
            ("Double Door", crate::family::Category::Door),
            ("Sliding Door", crate::family::Category::Door),
            ("Fixed Window", crate::family::Category::Window),
            ("Casement Window", crate::family::Category::Window),
            ("Sliding Window", crate::family::Category::Window),
        ] {
            assert!(hosted.contains(&(named(english).as_str(), kind)), "{l:?}: {english} in {hosted:?}");
        }
        // Each family builds, every type of it, with every material it names in the library.
        assert_eq!(m.check(), Ok(()), "{l:?}");
    }
}

/// An 8 x 6 m box of walls, a single door in its south wall and a fixed window in its east one, both of the project's families.
fn family_house() -> Session {
    let mut s = Session::new();
    s.script(
        r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
           door {"at": [2000, 0], "family": "Single Door", "type": "800 x 2100"}
           window {"at": [8000, 3000], "family": "Fixed Window", "type": "600 x 900"}"#,
    )
    .unwrap();
    s
}

#[test]
fn a_doors_family_sizes_its_opening_and_a_windows_gives_it_its_sill() {
    let s = family_house();
    // Elements 3 to 6 are the walls, 7 the door and 8 the window.
    let door = crate::element_json(&s.model, 100006);
    assert_eq!(
        (&door["family"], &door["type"], &door["width"], &door["height"], &door["sill"]),
        (&json!("Single Door"), &json!("800 x 2100"), &json!(800.0), &json!(2100.0), &json!(0.0))
    );
    assert_eq!((&door["values"]["Wall Thickness"], &door["values"]["Frame Width"]), (&json!(200.0), &json!(50.0)));
    let window = crate::element_json(&s.model, 100007);
    assert_eq!((&window["width"], &window["height"], &window["sill"]), (&json!(600.0), &json!(900.0), &json!(1100.0)), "its type's sill");
    // The wall's opening is the family's size: a 1000 wide type, as the family says, in place of the 800.
    let mut s = s;
    run(&mut s, "modify", json!({"id": 100006, "type": "1000 x 2100"}));
    assert_eq!(crate::element_json(&s.model, 100006)["width"], json!(1000.0));
    // A door of a family that isn't a door's, or of none, is refused; so is a type the family hasn't.
    for (p, says) in [
        (json!({"at": [4000, 0], "family": "Fixed Window"}), "no door family"),
        (json!({"at": [4000, 0], "family": "Nowhere"}), "no door family"),
        (json!({"at": [4000, 0], "family": "Single Door", "type": "9 x 9"}), "9 x 9"),
    ] {
        let e = s.execute("door", &p).unwrap_err();
        assert!(e.contains(says), "{p}: {e}");
    }
    // A door's family can't be placed as furniture, and while openings are of it, it can't go.
    let e = s.execute("component", &json!({"family": "Single Door", "at": [1000, 1000]})).unwrap_err();
    assert!(e.contains("door"), "{e}");
    let e = s.execute("family", &json!({"name": "Single Door", "delete": true})).unwrap_err();
    assert!(e.contains("100006"), "{e}");
}

#[test]
fn a_door_familys_instance_parameters_are_its_own_and_its_type_parameters_its_types() {
    let mut s = family_house();
    run(
        &mut s,
        "family",
        json!({"name": "Hatch", "category": "door", "parameters": [{"name": "Width", "instance": true, "value": 600}, {"name": "Height", "value": 700}],
               "types": {"A": {}}, "forms": [{"profile": [["-Width / 2", "-Wall Thickness / 2"], ["Width / 2", "-Wall Thickness / 2"], ["Width / 2", "Wall Thickness / 2"]], "to": "Height"}]}),
    );
    let id = run(&mut s, "door", json!({"at": [4000, 0], "family": "Hatch"}))["id"].as_u64().unwrap();
    assert_eq!(crate::element_json(&s.model, id)["width"], json!(600.0));
    run(&mut s, "modify", json!({"id": id, "params": {"Width": 900}}));
    assert_eq!(crate::element_json(&s.model, id)["width"], json!(900.0), "its own width, as a component's own parameter");
    // A type parameter isn't the opening's to set, and a family that isn't sound is refused, saying why.
    let e = s.execute("modify", &json!({"id": id, "params": {"Height": 5}})).unwrap_err();
    assert!(e.contains("type parameter"), "{e}");
    for (p, says) in [
        (json!({"name": "Bad", "category": "door", "types": {"A": {}}, "parameters": [{"name": "Width", "value": 1}]}), "Height"),
        (
            json!({"name": "Bad", "category": "window", "types": {"A": {}}, "parameters": [{"name": "Width", "value": 1}, {"name": "Height", "value": 1}, {"name": "Wall Thickness", "value": 1}]}),
            "Wall Thickness",
        ),
        (json!({"name": "Bad", "category": "furniture", "types": {"A": {}}, "symbols": [{"line": [[0, 0], [1, 1]]}]}), "symbols"),
        (
            json!({"name": "Bad", "category": "door", "types": {"A": {}}, "parameters": [{"name": "Width", "value": 1}, {"name": "Height", "value": 1}], "symbols": [{"arc": {"centre": [0, 0], "radius": 0, "to": 90}}]}),
            "radius",
        ),
    ] {
        let e = s.execute("family", &p).unwrap_err();
        assert!(e.contains(says), "{p}: {e}");
    }
}

#[test]
fn a_door_familys_swing_is_drawn_in_plan_and_flips_with_the_door() {
    let mut s = family_house();
    let level = 1;
    // The polylines the door draws that aren't closed: its leaf's swing, as lines of points in the plan's own millimetres.
    let swing = |s: &Session| -> Vec<Vec<P2>> {
        let d = crate::render::plan(&s.model, level, 1600, 1000);
        d.prims
            .iter()
            .zip(&d.owners)
            .filter(|(_, o)| **o == Some(100006))
            .filter_map(|(p, _)| match p {
                crate::render::Prim::Line { pts, closed: false, .. } if pts.len() > 10 => Some(pts.iter().map(|q| d.view.mm(*q)).collect()),
                _ => None,
            })
            .collect()
    };
    let arc = |s: &Session, hinge: P2, sign: P2| {
        let lines = swing(s);
        assert_eq!(lines.len(), 1, "one arc");
        for p in &lines[0] {
            // 700 from the hinge (the 800 opening less two 50 frames), on the side of the wall it swings to.
            assert!((p.dist(hinge) - 700.0).abs() < 1.5, "{p:?} from {hinge:?}");
            assert!((p.0 - hinge.0) * sign.0 >= -1.5 && (p.1 - hinge.1) * sign.1 >= -1.5, "{p:?} on the side {sign:?} of {hinge:?}");
        }
    };
    // Hinged on the jamb toward the wall's start, 50 in from its edge, at the wall's face (100 from its middle) toward +y.
    arc(&s, P2(1650.0, 100.0), P2(1.0, 1.0));
    run(&mut s, "modify", json!({"id": 100006, "flip_hand": true}));
    arc(&s, P2(2350.0, 100.0), P2(-1.0, 1.0));
    run(&mut s, "modify", json!({"id": 100006, "flip_facing": true}));
    arc(&s, P2(2350.0, -100.0), P2(-1.0, -1.0));
    run(&mut s, "modify", json!({"id": 100006, "flip_hand": false}));
    arc(&s, P2(1650.0, -100.0), P2(1.0, -1.0));
    // A door of the project's own types swings as it always did: 24 chords, drawn all the same.
    run(&mut s, "modify", json!({"id": 100006, "family": null, "type": "Single 800 x 2100"}));
    assert_eq!(swing(&s).len(), 1);
}

#[test]
fn a_family_door_flipped_either_way_keeps_its_forms_turned_out_and_the_same_size() {
    let mut s = family_house();
    // What each form encloses, from its triangles, in the opening's own axes: positive while they face out.
    let volumes = |s: &Session| -> Vec<f64> {
        let (pieces, _) = s.model.opening_local(&s.model.elements[&100006]).unwrap();
        let sixth = |p: &crate::family::Piece| {
            p.tris.iter().map(|[a, b, c]| crate::view3d::dot(*a, crate::view3d::cross(*b, *c))).sum::<f64>() / 6.0
        };
        pieces.iter().map(sixth).collect()
    };
    let straight = volumes(&s);
    assert!(straight.len() > 3 && straight.iter().all(|v| *v > 0.0), "{straight:?}");
    for (facing, hand) in [(true, false), (false, true), (true, true)] {
        run(&mut s, "modify", json!({"id": 100006, "flip_facing": facing, "flip_hand": hand}));
        for (a, b) in straight.iter().zip(volumes(&s)) {
            assert!((a - b).abs() < 1e-6 * a, "facing {facing}, hand {hand}: {a} against {b}");
        }
    }
}

#[test]
fn sliding_doors_and_windows_say_how_they_open_in_ifc_and_in_schedules() {
    let mut s = Session::new();
    s.script(
        r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
           door {"at": [2000, 0], "family": "Sliding Door", "type": "2000 x 2100"}
           door {"at": [5500, 0], "family": "Double Door"}
           window {"at": [8000, 3000], "family": "Sliding Window"}
           window {"at": [4000, 6000], "family": "Casement Window"}"#,
    )
    .unwrap();
    let doors: Vec<u64> = run(&mut s, "elements", json!({"category": "door"}))["elements"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["id"].as_u64().unwrap())
        .collect();
    assert_eq!(element(&mut s, doors[0])["operation"], "sliding");
    // Out to IFC: a sliding door slides toward its hand, a double door is a double door, a window of two leaves two panels.
    let file = crate::ifc::export(&s.model);
    assert!(file.contains(".SLIDING_TO_LEFT.") && file.contains(".DOUBLE_DOOR_SINGLE_SWING."), "door operations");
    assert_eq!(file.matches(".DOUBLE_PANEL_VERTICAL.").count(), 2, "both windows have two leaves");
    run(&mut s, "modify", json!({"id": doors[0], "flip_hand": true}));
    let file = crate::ifc::export(&s.model);
    assert!(file.contains(".SLIDING_TO_RIGHT."));
    // Back in, a door keeps how it opens and to which side, on a type of its own.
    let back = crate::import::ifc(file.as_bytes(), "t").unwrap();
    let mut t = Session::new();
    t.reset(back.model, None);
    let ops: Vec<(Option<crate::model::Operation>, bool)> = t
        .model
        .elements
        .values()
        .filter_map(|e| match e {
            crate::model::Element::Door(o) => Some((t.model.opening_type(e).and_then(|k| k.operation), o.flip_hand)),
            _ => None,
        })
        .collect();
    use crate::model::Operation::{DoubleSwing, Sliding};
    assert!(ops.contains(&(Some(Sliding), true)) && ops.contains(&(Some(DoubleSwing), false)), "{ops:?}");
    // Where the door says nothing, its type does: an IfcDoorType's (IFC4) operation, as Revit writes it.
    let door = file.lines().find(|l| l.contains("=IFCDOOR(") && l.contains(".SLIDING_TO_RIGHT.")).unwrap();
    let door_ref = door.split('=').next().unwrap();
    let typed = file.replacen(".SLIDING_TO_RIGHT.,$)", "$,$)", 1).replacen(
        "ENDSEC;\nEND-ISO",
        &format!(
            "#999990=IFCDOORTYPE('0000000000000000000T01',$,'Patio',$,$,$,$,$,$,.DOOR.,.SLIDING_TO_LEFT.,.F.,$);\n\
             #999991=IFCRELDEFINESBYTYPE('0000000000000000000T02',$,$,$,({door_ref}),#999990);\nENDSEC;\nEND-ISO"
        ),
        1,
    );
    assert!(!typed.contains(".SLIDING_TO_RIGHT.") && typed.contains("IFCDOORTYPE"));
    let back = crate::import::ifc(typed.as_bytes(), "t").unwrap();
    let patio = back.model.types.door.get("Patio").copied();
    assert_eq!(patio.and_then(|k| k.operation), Some(Sliding), "{:?}", back.model.types.door);
    // A schedule says it in the project's language.
    let r =
        run(&mut s, "schedule", json!({"name": "Esquadrias", "category": "doors", "fields": ["Family", "Operation"], "sort": ["Family"]}));
    assert_eq!(r["rows"], json!([["Double Door", "Double Swing"], ["Sliding Door", "Sliding"]]));
    let mut pt = Session::new();
    pt.reset(Model::new_in("x", crate::language::Language::Portuguese), None);
    pt.script(
        r#"wall {"points": [[0,0],[8000,0]]}
           door {"at": [2000, 0], "family": "Porta de correr"}"#,
    )
    .unwrap();
    let r = run(&mut pt, "schedule", json!({"name": "Esquadrias", "category": "doors", "fields": ["Operation"]}));
    assert_eq!(r["rows"], json!([["De correr"]]));
    // A project's own type says it too; a name that isn't one is refused.
    let made = run(&mut s, "type", json!({"category": "door", "name": "Correr 900", "width": 900, "height": 2100, "operation": "sliding"}));
    assert_eq!(made["type"]["operation"], "sliding");
    let e =
        s.execute("type", &json!({"category": "door", "name": "Odd", "width": 900, "height": 2100, "operation": "spinning"})).unwrap_err();
    assert!(e.contains("spinning") && e.contains("double-sliding"), "{e}");
}

#[test]
fn a_family_door_and_window_show_in_a_schedule_by_their_family() {
    let mut s = family_house();
    let r = run(&mut s, "schedule", json!({"name": "D", "category": "doors", "fields": ["Mark", "Family", "Type", "Width"]}));
    assert_eq!(r["rows"], json!([["1", "Single Door", "800 x 2100", 800.0]]));
    let r = run(&mut s, "schedule", json!({"name": "W", "category": "windows", "fields": ["Family", "Type", "Sill"]}));
    assert_eq!(r["rows"], json!([["Fixed Window", "600 x 900", 1100.0]]));
    // The Family field is there to pick, and not shown unless it is.
    assert_eq!(
        run(&mut s, "schedule", json!({"category": "doors"}))["columns"],
        json!(["Mark", "Type", "Width", "Height", "Level", "Wall"])
    );
}

/// A PNG file of `w` by `h` px in the scratch folder, every pixel `color`; its path.
fn picture_file(name: &str, w: u32, h: u32, color: [u8; 4]) -> std::path::PathBuf {
    let rgba: Vec<u8> = (0..w * h).flat_map(|_| color).collect();
    let png = tiny_skia::Pixmap::from_vec(rgba, tiny_skia::IntSize::from_wh(w, h).unwrap()).unwrap().encode_png().unwrap();
    let path = std::env::temp_dir().join(format!("bimcraft-{name}-{}.png", std::process::id()));
    std::fs::write(&path, png).unwrap();
    path
}

#[test]
fn a_materials_texture_is_set_changed_and_taken_away_and_a_picture_comes_into_the_project_once() {
    let mut s = Session::new();
    // A new project's library has textures: bricks, boards.
    let lib = run(&mut s, "inspect", json!({}))["materials"].clone();
    assert_eq!((&lib["Brick"]["texture"]["kind"], &lib["Brick"]["texture"]["size"]), (&json!("brick"), &json!([900.0, 600.0])));
    assert!(lib["Membrane"]["texture"].is_null() && lib["Timber"]["texture"]["kind"] == "boards");
    // A pattern of another kind, at its natural size; then over it a size, a turn and a move, which keep its kind.
    let t = |s: &Session| serde_json::to_value(&s.model.materials["Oak"].texture).unwrap();
    run(&mut s, "material", json!({"name": "Oak", "color": "#a0703c", "texture": {"kind": "boards"}}));
    assert_eq!(t(&s), json!({"kind": "boards", "size": [1200.0, 1120.0]}));
    run(&mut s, "material", json!({"name": "Oak", "texture": {"size": [600, 300], "angle": 90, "offset": [10, 20]}}));
    assert_eq!(t(&s), json!({"kind": "boards", "size": [600.0, 300.0], "angle": 90.0, "offset": [10.0, 20.0]}));
    run(&mut s, "material", json!({"name": "Oak", "texture": {"size": 250}}));
    assert_eq!(t(&s)["size"], json!([250.0, 250.0]), "one number is both");
    // Another kind starts again; null takes it away; the material's other fields stay as they were.
    run(&mut s, "material", json!({"name": "Oak", "texture": {"kind": "tile"}}));
    assert_eq!(t(&s), json!({"kind": "tile", "size": [1200.0, 1200.0]}));
    run(&mut s, "material", json!({"name": "Oak", "texture": null}));
    assert_eq!((t(&s), s.model.materials["Oak"].color), (json!(null), [160, 112, 60]));
    // A picture: its file, taken into the project (a tile a metre across, as tall as its proportions make it); the same file again, the same picture.
    let (wide, tall) = (picture_file("wide", 8, 4, [200, 100, 50, 255]), picture_file("tall", 4, 8, [20, 100, 250, 255]));
    run(&mut s, "material", json!({"name": "Oak", "texture": {"kind": "image", "path": wide}}));
    run(&mut s, "material", json!({"name": "Maple", "texture": {"kind": "image", "path": wide}}));
    assert_eq!((s.model.images.len(), t(&s)["size"].clone()), (1, json!([1000.0, 500.0])));
    let inspected = run(&mut s, "inspect", json!({}))["images"].clone();
    let id = s.model.images.keys().next().unwrap().clone();
    assert_eq!(
        (&inspected[&id]["size"], inspected[&id]["name"].as_str().map(|n| n.starts_with("bimcraft-wide"))),
        (&json!([8, 4]), Some(true))
    );
    // A picture is the project's while a material's texture is made of it: out of one, still there for the other; out of both, gone.
    run(&mut s, "material", json!({"name": "Oak", "texture": {"kind": "image", "path": tall}}));
    assert_eq!(s.model.images.len(), 2);
    run(&mut s, "material", json!({"name": "Oak", "texture": null}));
    assert_eq!(s.model.images.len(), 1);
    run(&mut s, "material", json!({"name": "Maple", "delete": true}));
    assert!(s.model.images.is_empty());
    // A project that holds one saves it with it, and opens with it.
    run(&mut s, "material", json!({"name": "Oak", "texture": {"kind": "image", "path": tall, "angle": 45}}));
    let project = temp("textured.bimcraft");
    run(&mut s, "save", json!({"path": project}));
    let mut t2 = Session::new();
    run(&mut t2, "open", json!({"path": project}));
    assert_eq!((t2.model.images.len(), &t2.model.materials["Oak"]), (1, &s.model.materials["Oak"]));
    assert_eq!(t2.model.images, s.model.images);
    // As a hand might spoil it: a picture's data cut short comes back with its name, and the project isn't opened.
    let mut v: Value = serde_json::from_str(&std::fs::read_to_string(&project).unwrap()).unwrap();
    let key = v["images"].as_object().unwrap().keys().next().unwrap().clone();
    v["images"][&key]["png"] = json!("iVBORw0KGgo=");
    std::fs::write(&project, v.to_string()).unwrap();
    let e = Session::new().execute("open", &json!({"path": project})).unwrap_err();
    assert!(e.contains("not a BIMCraft project") && e.contains("cut short"), "{e}");
    for f in [wide, tall, project] {
        let _ = std::fs::remove_file(f);
    }
}

#[test]
fn a_texture_that_isnt_sound_is_refused_and_says_why_and_the_project_is_as_it_was() {
    let mut s = Session::new();
    let before = s.model.clone();
    let wide = picture_file("refused", 8, 4, [1, 2, 3, 255]);
    let notes = temp("notes.txt");
    std::fs::write(&notes, "not a picture").unwrap();
    for (texture, says) in [
        (json!("brick"), "or null"),
        (json!({"kind": "marble"}), "isn't one of"),
        (json!({"size": [10, 10]}), "needs a `kind`"),
        (json!({"kind": "brick", "colour": 1}), "isn't one of kind"),
        (json!({"kind": "brick", "size": [0, 10]}), "each side"),
        (json!({"kind": "brick", "size": "big"}), "[a, b] numbers"),
        (json!({"kind": "brick", "angle": "up"}), "degrees"),
        (json!({"kind": "brick", "path": wide}), "only a texture of kind `image`"),
        (json!({"kind": "image"}), "needs its file"),
        (json!({"kind": "image", "image": "img-nothere"}), "doesn't hold"),
        (json!({"kind": "image", "path": "/nonexistent/wood.jpg"}), "wood.jpg"),
        (json!({"kind": "image", "path": notes}), "neither a PNG nor a JPEG"),
    ] {
        let e = s.execute("material", &json!({"name": "Bad", "texture": texture})).unwrap_err();
        assert!(e.contains(says), "{texture}: {e}");
        assert_eq!(*s.model, *before);
    }
    // At most 24 pictures; the 25th isn't taken in.
    for k in 0..crate::texture::MAX_IMAGES as u8 {
        let file = picture_file(&format!("many{k}"), 2, 2, [k, 7, 9, 255]);
        run(&mut s, "material", json!({"name": format!("M{k}"), "texture": {"kind": "image", "path": file.clone()}}));
        std::fs::remove_file(file).unwrap();
    }
    let extra = picture_file("extra", 2, 2, [250, 7, 9, 255]);
    let e = s.execute("material", &json!({"name": "M25", "texture": {"kind": "image", "path": extra}})).unwrap_err();
    assert!(e.contains("pictures already"), "{e}");
    for f in [wide, notes, extra] {
        let _ = std::fs::remove_file(f);
    }
}

#[test]
fn paint_gives_a_face_a_material_and_takes_it_off_and_a_material_painted_on_stays() {
    let mut s = Session::new();
    house(&mut s);
    let floor = run(&mut s, "floor", json!({}))["id"].as_u64().unwrap();
    let paint = |s: &Session, id: u64| crate::element_json(&s.model, id)["params"]["Paint"].clone();
    // A wall's faces are its exterior and its interior; paint a face, or all of them.
    run(&mut s, "paint", json!({"id": 100002, "face": "exterior", "material": "Brick"}));
    assert_eq!(paint(&s, 100002), json!({"exterior": "Brick"}));
    run(&mut s, "PT", json!({"ids": [100002, 100003], "material": "Plaster"}));
    assert_eq!(
        (paint(&s, 100002), paint(&s, 100003)),
        (json!({"exterior": "Plaster", "interior": "Plaster"}), json!({"exterior": "Plaster", "interior": "Plaster"}))
    );
    // A floor's: its top and its bottom.
    run(&mut s, "paint", json!({"id": floor, "face": "top", "material": "Ceramic tile"}));
    assert_eq!(paint(&s, floor), json!({"top": "Ceramic tile"}));
    // Taken off one face; off the last, the paint's gone.
    run(&mut s, "paint", json!({"id": 100002, "face": "interior", "material": null}));
    assert_eq!(paint(&s, 100002), json!({"exterior": "Plaster"}));
    run(&mut s, "paint", json!({"id": 100002, "material": null}));
    assert!(paint(&s, 100002).is_null() && crate::element_json(&s.model, 100002)["params"].is_null());
    // A material in use by paint can't go, and a rename follows it.
    let e = s.execute("material", &json!({"name": "Ceramic tile", "delete": true})).unwrap_err();
    assert!(e.contains("paint of floor"), "{e}");
    run(&mut s, "material", json!({"name": "Ceramic tile", "rename": "Glazed"}));
    assert_eq!(paint(&s, floor), json!({"top": "Glazed"}));
    // What can't be painted, or painted so, is refused; so is a hand-written value that isn't a paint.
    for (p, says) in [
        (json!({"id": 100002, "face": "top", "material": "Brick"}), "no face `top`"),
        (json!({"id": 100002, "material": "Nope"}), "no `Nope`"),
        (json!({"id": 100002}), "`material` is required"),
        (json!({"id": 100002, "material": 5}), "a material's name"),
        (json!({"id": 100000, "material": "Brick"}), "no face to paint"),
        (json!({"id": 999, "material": "Brick"}), "999"),
    ] {
        let e = s.execute("paint", &p).unwrap_err();
        assert!(e.contains(says), "{p}: {e}");
    }
    for (value, says) in
        [(json!("Brick"), "{face: material}"), (json!({"sky": "Brick"}), "no face"), (json!({"exterior": "Nope"}), "painted a material")]
    {
        let e = s.execute("modify", &json!({"id": 100002, "params": {"Paint": value}})).unwrap_err();
        assert!(e.contains(says), "{value}: {e}");
    }
    // A project parameter can't be called Paint, nor a family's parameter.
    let e = s.execute("parameter", &json!({"name": "Paint", "categories": ["wall"]})).unwrap_err();
    assert!(e.contains("paint"), "{e}");
    let e =
        s.execute("family", &json!({"name": "F", "types": {"A": {}}, "parameters": [{"name": "Paint", "kind": "number"}]})).unwrap_err();
    assert!(e.contains("paint"), "{e}");
    // A copy is painted as its original is.
    let copy = run(&mut s, "copy", json!({"ids": [floor], "by": [0, 0]}));
    let _ = copy;
}

#[test]
fn favorites_keep_materials_and_families_for_every_project() {
    let file = temp("favorites.json");
    let _ = std::fs::remove_file(&file);
    let picture = picture_file("fav", 8, 8, [10, 200, 30, 255]);
    let mut a = Session::new();
    run(
        &mut a,
        "material",
        json!({"name": "My Lawn", "color": "#33aa33", "texture": {"kind": "image", "path": picture, "size": [500, 500]}}),
    );
    run(&mut a, "material", json!({"name": "My Oak", "color": "#a0703c", "texture": {"kind": "boards", "angle": 30}}));
    // None to begin with; then two, a material with its picture and a family with the materials it's made of.
    assert_eq!(run(&mut a, "favorite", json!({"path": file}))["materials"], json!([]));
    run(&mut a, "favorite", json!({"action": "save", "name": "My Lawn", "path": file}));
    run(&mut a, "favorite", json!({"action": "save", "name": "My Oak", "as": "Oak, aged", "path": file}));
    run(
        &mut a,
        "family",
        json!({"name": "Bench", "parameters": [{"name": "Seat", "kind": "material", "value": "My Oak"}], "types": {"A": {}},
        "forms": [{"profile": [[0, 0], [1500, 0], [1500, 400], [0, 400]], "to": 450, "material": "Seat"}]}),
    );
    run(&mut a, "favorite", json!({"action": "save", "kind": "family", "name": "Bench", "path": file}));
    let listed = run(&mut a, "favorite", json!({"path": file}));
    assert_eq!(
        (listed["materials"].clone(), listed["families"].clone()),
        (
            json!([{"name": "My Lawn", "color": [51, 170, 51], "texture": "image"}, {"name": "Oak, aged", "color": [160, 112, 60], "texture": "boards"}]),
            json!([{"name": "Bench", "category": "furniture", "types": ["A"]}])
        )
    );
    // In another project, from the file alone: the lawn comes with its picture, the family with the oak it's made of.
    let mut b = Session::new();
    run(&mut b, "favorite", json!({"action": "load", "name": "My Lawn", "as": "Lawn", "path": file}));
    assert_eq!((b.model.images.len(), &b.model.materials["Lawn"].texture), (1, &a.model.materials["My Lawn"].texture));
    run(&mut b, "favorite", json!({"action": "load", "kind": "family", "name": "Bench", "path": file}));
    assert_eq!(b.model.materials["My Oak"].color, [160, 112, 60]);
    let bench = run(&mut b, "component", json!({"family": "Bench", "at": [0, 0]}))["id"].as_u64().unwrap();
    assert!(b.model.elements.contains_key(bench));
    // A name the project has is its own, unless the favorite is to replace it; the project's material of a family's keeps its own.
    let e = b.execute("favorite", &json!({"action": "load", "name": "My Lawn", "as": "Lawn", "path": file})).unwrap_err();
    assert!(e.contains("replace"), "{e}");
    run(&mut b, "material", json!({"name": "Lawn", "color": "#000000"}));
    run(&mut b, "favorite", json!({"action": "load", "name": "My Lawn", "as": "Lawn", "replace": true, "path": file}));
    assert_eq!(b.model.materials["Lawn"].color, [51, 170, 51]);
    run(&mut b, "material", json!({"name": "My Oak", "color": "#ffffff"}));
    run(&mut b, "favorite", json!({"action": "load", "kind": "family", "name": "Bench", "as": "Bench 2", "path": file}));
    assert_eq!(b.model.materials["My Oak"].color, [255, 255, 255]);
    // Loading is an undo step; saving and listing aren't, and nothing is left half done when it's refused.
    let steps = b.history().len();
    run(&mut b, "undo", json!({}));
    assert_eq!(b.history().len(), steps - 1);
    let before = b.model.clone();
    for (p, says) in [
        (json!({"action": "load", "name": "Nothing", "path": file}), "no favorite material"),
        (json!({"action": "load", "kind": "family", "name": "Nothing", "path": file}), "no favorite family"),
        (json!({"action": "save", "name": "Nothing", "path": file}), "no `Nothing`"),
        (json!({"action": "save", "path": file}), "`name` is required"),
        (json!({"action": "delete", "name": "Nothing", "path": file}), "no favorite"),
        (json!({"action": "keep", "path": file}), "list, save, load or delete"),
        (json!({"kind": "texture", "path": file}), "material or family"),
    ] {
        let e = b.execute("favorite", &p).unwrap_err();
        assert!(e.contains(says), "{p}: {e}");
    }
    assert_eq!(*b.model, *before);
    // Deleted, a favorite's gone; a file that isn't one is refused, not written over.
    run(&mut a, "favorite", json!({"action": "delete", "name": "My Lawn", "path": file}));
    assert_eq!(run(&mut a, "favorite", json!({"path": file}))["materials"].as_array().map(Vec::len), Some(1));
    std::fs::write(&file, "not favorites").unwrap();
    let e = a.execute("favorite", &json!({"action": "save", "name": "My Oak", "path": file})).unwrap_err();
    assert!(e.contains("isn't a favorites file"), "{e}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "not favorites");
    // A family goes to a file of its own, and comes back from it.
    let bench_file = temp("bench.json");
    run(&mut a, "family", json!({"name": "Bench", "save": bench_file}));
    let mut c = Session::new();
    run(&mut c, "material", json!({"name": "My Oak"}));
    run(&mut c, "family", json!({"path": bench_file}));
    assert_eq!(c.model.families["Bench"], a.model.families["Bench"]);
    for f in [file, picture, bench_file] {
        let _ = std::fs::remove_file(f);
    }
}

// ---------- massing ----------

fn block() -> Value {
    json!([[0, 0], [10000, 0], [10000, 6000], [0, 6000]])
}

/// Levels at 0, 3000, 6000 and 9000 mm, named Level 1 to 4 (1 and 2 are the project's), and a 10 x 6 m mass on the first up to the last.
fn tower(s: &mut Session) -> (u64, [u64; 4]) {
    let l3 = run(s, "level", json!({"elevation": 6000, "name": "Level 3"}))["id"].as_u64().unwrap();
    let l4 = run(s, "level", json!({"elevation": 9000, "name": "Level 4"}))["id"].as_u64().unwrap();
    let id = run(s, "mass", json!({"boundary": block(), "top_level": l4, "name": "Tower"}))["id"].as_u64().unwrap();
    (id, [100_000, 100_001, l3, l4])
}

#[test]
fn a_mass_is_a_footprint_extruded_from_a_level_up_to_a_level_or_a_height() {
    let mut s = Session::new();
    // Drawn clockwise it comes back counter-clockwise; with no top it goes up to the next level, as a shaft does.
    let r = run(&mut s, "mass", json!({"boundary": [[0, 0], [0, 6000], [10000, 6000], [10000, 0]], "name": "Block"}));
    let id = r["id"].as_u64().unwrap();
    assert_eq!((r["area_m2"].as_f64(), r["volume_m3"].as_f64(), r["height"].as_f64()), (Some(60.0), Some(180.0), Some(3000.0)));
    let e = element(&mut s, id);
    assert_eq!((id_of(&s, &e["level"]), id_of(&s, &e["top_level"]), e["name"].as_str()), (Some(100000), Some(100001), Some("Block")));
    assert_eq!(e["boundary"], json!([[10000.0, 0.0], [10000.0, 6000.0], [0.0, 6000.0], [0.0, 0.0]]));
    // Named in the project's language, 500 mm over its level and `height` tall.
    let tall = run(&mut s, "MS", json!({"boundary": block(), "height": 9000, "base_offset": 500}));
    assert_eq!(tall["volume_m3"], 540.0);
    let tall = tall["id"].as_u64().unwrap();
    assert_eq!(element(&mut s, tall)["name"], "Mass");
    assert_eq!(count(&mut s, "masses"), 2);
    // Changed like any element, and checked like the rest of the model.
    run(&mut s, "modify", json!({"id": id, "top_level": null, "height": 4500}));
    // 4.5 m up from Level 1: it crosses Levels 1 and 2, so its gross floor area is two floors of it.
    let e = element(&mut s, id);
    let facts = (e["volume_m3"].as_f64(), e["gross_floor_area_m2"].as_f64(), e["levels"].as_array().map(Vec::len));
    assert_eq!(facts, (Some(270.0), Some(120.0), Some(2)));
    for (p, says) in [
        (json!({"id": id, "height": -5}), "height"),
        (json!({"id": id, "top_level": 100000, "height": 0}), "above its base"),
        (json!({"id": id, "boundary": [[0, 0], [1000, 0]]}), "points"),
        (json!({"id": id, "level": 100098}), "level"),
        (json!({"id": id, "base_offset": 1e12}), "base_offset"),
    ] {
        let e = s.execute("modify", &p).unwrap_err();
        assert!(e.contains(says), "{p}: {e}");
    }
    assert_eq!(element(&mut s, id)["volume_m3"], 270.0, "refused, it's as it was");
    // It holds the project's parameters for masses.
    run(&mut s, "parameter", json!({"name": "Use", "kind": "text", "categories": ["mass"]}));
    run(&mut s, "modify", json!({"id": id, "params": {"Use": "Housing"}}));
    assert_eq!(element(&mut s, id)["params"]["Use"], "Housing");
    // Move, copy, mirror and rotate take its footprint along.
    run(&mut s, "move", json!({"ids": [id], "by": [1000, 500]}));
    assert_eq!(element(&mut s, id)["boundary"][0], json!([11000.0, 500.0]));
    let made = run(&mut s, "copy", json!({"ids": [id], "by": [0, 8000]}))["ids"][0].as_u64().unwrap();
    assert_eq!(element(&mut s, made)["boundary"][0], json!([11000.0, 8500.0]));
    assert_eq!(element(&mut s, made)["params"]["Use"], "Housing");
    let mirrored = run(&mut s, "MM", json!({"ids": [id], "axis": [[0, 0], [0, 1000]], "copy": true}))["ids"][0].as_u64().unwrap();
    assert_eq!(element(&mut s, mirrored)["area_m2"], 60.0, "still counter-clockwise");
    run(&mut s, "rotate", json!({"ids": [id], "about": [0, 0], "angle": 90}));
    assert_eq!(element(&mut s, id)["area_m2"], 60.0);
    // The project file keeps masses as they are.
    let file = temp("mass.bimcraft");
    run(&mut s, "save", json!({"path": file}));
    let mut t = Session::new();
    run(&mut t, "open", json!({"path": file}));
    let _ = std::fs::remove_file(&file);
    assert_eq!(t.model.elements, s.model.elements);
    // One undo step each: the rotation, the mirrored copy and the copy.
    for _ in 0..3 {
        run(&mut s, "undo", json!({}));
    }
    assert_eq!((count(&mut s, "mass"), element(&mut s, id)["boundary"][0].clone()), (2, json!([11000.0, 500.0])));
    // A level it stands on can't go while it does.
    assert!(s.execute("delete", &json!({"ids": [100000]})).unwrap_err().contains("level"));
}

#[test]
fn mass_floors_make_a_floor_for_each_level_the_mass_crosses() {
    let mut s = Session::new();
    let (id, [l1, l2, l3, l4]) = tower(&mut s);
    // At Levels 1, 2 and 3; the top level is the mass's roof, not a floor.
    let r = run(&mut s, "MF", json!({"mass": id}));
    let floors: Vec<u64> = r["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    assert_eq!(floors.len(), 3);
    assert_eq!(count(&mut s, "floor"), 3);
    for (f, level) in floors.iter().zip([l1, l2, l3]) {
        let e = element(&mut s, *f);
        assert_eq!((id_of(&s, &e["level"]), e["area_m2"].as_f64(), e["boundary"].clone()), (Some(level), Some(60.0), block_ccw()));
    }
    assert!(!floors.iter().any(|f| s.model.elements[f].category() != "floor"));
    let _ = l4;
    // Over its base, it starts at the first level it reaches; under a level, it makes no floor there.
    let high = run(&mut s, "mass", json!({"boundary": block(), "base_offset": 500, "height": 7000}))["id"].as_u64().unwrap();
    let r = run(&mut s, "mass_floors", json!({"mass": high}));
    let levels: Vec<u64> = r["ids"].as_array().unwrap().iter().map(|f| element_level(&s, f.as_u64().unwrap())).collect();
    assert_eq!(levels, [l2, l3]);
    // With no `mass`, every mass; with none, it says so.
    assert_eq!(run(&mut s, "mass_floors", json!({}))["ids"].as_array().map(Vec::len), Some(5));
    let mut empty = Session::new();
    assert!(empty.execute("mass_floors", &json!({})).unwrap_err().contains("no mass"));
    // The floor type is checked, a wall isn't a mass, and a failure leaves no floors behind.
    let wall = run(&mut s, "wall", json!({"points": [[0, 0], [1000, 0]]}))["ids"][0].as_u64().unwrap();
    let before = s.model.clone();
    assert!(s.execute("mass_floors", &json!({"mass": id, "type": "Nothing"})).unwrap_err().contains("type"));
    assert!(s.execute("mass_floors", &json!({"mass": wall})).unwrap_err().contains("isn't a mass"));
    assert_eq!(*s.model, *before);
}

fn ids_of(v: &Value) -> Vec<u64> {
    v.as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect()
}

#[test]
fn mass_floors_run_again_update_what_they_made_and_remove_the_stale_ones_only_when_asked() {
    let mut s = Session::new();
    let (id, [l1, l2, l3, _]) = tower(&mut s);
    let first = run(&mut s, "mass_floors", json!({"mass": id}));
    let floors = ids_of(&first["ids"]);
    // Each holds its mass and its level, under a reserved name.
    for (f, level) in floors.iter().zip([l1, l2, l3]) {
        assert_eq!(element(&mut s, *f)["params"]["Mass Floor"], json!({"mass": s.model.guid(id), "level": s.model.guid(level)}));
    }
    assert_eq!((ids_of(&first["made"]), first["updated"].clone()), (floors.clone(), json!([])));
    // Again: the same floors, and nothing made or changed.
    let again = run(&mut s, "MF", json!({"mass": id}));
    assert_eq!((ids_of(&again["ids"]), again["made"].clone(), again["updated"].clone()), (floors.clone(), json!([]), json!([])));
    assert_eq!(count(&mut s, "floor"), 3);
    // A floor of the same level nobody made from the mass stays as it is, and so does a mass's floor after `modify`.
    let own = run(&mut s, "floor", json!({"boundary": [[0, 0], [1000, 0], [0, 1000]]}))["id"].as_u64().unwrap();
    // The mass grows: its floors take the new outline.
    run(&mut s, "modify", json!({"id": id, "boundary": [[0, 0], [12000, 0], [12000, 6000], [0, 6000]]}));
    let again = run(&mut s, "mass_floors", json!({"mass": id}));
    assert_eq!((ids_of(&again["ids"]), ids_of(&again["updated"]), again["made"].clone()), (floors.clone(), floors.clone(), json!([])));
    assert_eq!((count(&mut s, "floor"), element(&mut s, floors[1])["area_m2"].as_f64()), (4, Some(72.0)));
    assert_eq!(element(&mut s, own)["area_m2"].as_f64(), Some(0.5));
    // `type` changes the floors' type; without it they keep theirs.
    run(&mut s, "type", json!({"category": "floor", "name": "Thin", "thickness": 100}));
    let r = run(&mut s, "mass_floors", json!({"mass": id, "type": "Thin"}));
    assert_eq!(ids_of(&r["updated"]), floors);
    assert_eq!(element(&mut s, floors[0])["type"], "Thin");
    assert_eq!(run(&mut s, "mass_floors", json!({"mass": id}))["updated"], json!([]));
    assert_eq!(element(&mut s, floors[0])["type"], "Thin");
    // Cut short, the mass no longer crosses Level 3: its floor there is stale, and stays until `prune` asks.
    run(&mut s, "modify", json!({"id": id, "top_level": l3}));
    let r = run(&mut s, "mass_floors", json!({"mass": id}));
    assert_eq!((ids_of(&r["ids"]), ids_of(&r["stale"]), r["removed"].clone()), (floors[..2].to_vec(), floors[2..].to_vec(), json!([])));
    assert_eq!(count(&mut s, "floor"), 4);
    let r = run(&mut s, "mass_floors", json!({"mass": id, "prune": true}));
    assert_eq!((ids_of(&r["removed"]), r["stale"].clone()), (floors[2..].to_vec(), json!([])));
    assert_eq!(count(&mut s, "floor"), 3);
    // Pruning with no `mass` prunes every mass; it never touches a floor that holds no mark.
    assert_eq!(run(&mut s, "mass_floors", json!({"prune": true}))["removed"], json!([]));
    assert!(s.model.elements.contains_key(own));
    // The reserved name is no parameter's, and a floor holds only a real mass and level under it.
    let e = s.execute("parameter", &json!({"name": "Mass Floor", "kind": "text", "categories": ["floor"]})).unwrap_err();
    assert!(e.contains("Mass Floor"), "{e}");
    for (value, says) in [
        (json!({"mass": 100000, "level": l1}), "isn't a mass"),
        (json!({"mass": id, "level": id}), "isn't a level"),
        // Merged into the mark there, one more name is one too many.
        (json!({"face": 1}), "{mass, level}"),
        (json!("x"), "{mass, level}"),
    ] {
        let e = s.execute("modify", &json!({"id": floors[0], "params": {"Mass Floor": value}})).unwrap_err();
        assert!(e.contains(says), "{value}: {e}");
    }
    let wall = run(&mut s, "wall", json!({"points": [[0, 0], [1000, 0]]}))["ids"][0].as_u64().unwrap();
    assert!(s.execute("modify", &json!({"id": wall, "params": {"Mass Floor": {"mass": id, "level": l1}}})).is_err());
    // A copy is an ordinary floor; and with the mass gone, the floors it made are too.
    let copy = run(&mut s, "copy", json!({"ids": [floors[0]], "by": [20000, 0]}))["ids"][0].as_u64().unwrap();
    assert!(element(&mut s, copy)["params"].get("Mass Floor").is_none());
    run(&mut s, "delete", json!({"ids": [id]}));
    assert!(element(&mut s, floors[0])["params"].get("Mass Floor").is_none());
    assert!(s.execute("mass_floors", &json!({"prune": true})).unwrap_err().contains("no mass"));
}

fn block_ccw() -> Value {
    json!([[0.0, 0.0], [10000.0, 0.0], [10000.0, 6000.0], [0.0, 6000.0]])
}

fn element_level(s: &Session, id: u64) -> u64 {
    match &s.model.elements[&id] {
        crate::model::Element::Floor(f) => s.model.elements.id(f.level).unwrap(),
        other => panic!("{other:?} isn't a floor"),
    }
}

#[test]
fn a_schedule_of_mass_floors_gives_each_masses_floor_area_and_volume_on_each_level() {
    let mut s = Session::new();
    let (_, _) = tower(&mut s);
    // Another, 7.5 m tall from Level 1: its last floor is only half a storey of volume.
    run(&mut s, "mass", json!({"boundary": [[20000, 0], [25000, 0], [25000, 4000], [20000, 4000]], "height": 7500, "name": "Annex"}));
    let r = run(&mut s, "schedule", json!({"category": "massfloors", "sort": ["Mass", "Level"]}));
    assert_eq!(r["columns"], json!(["Mass", "Level", "Area (m²)", "Perimeter", "Volume"]));
    assert_eq!(
        r["rows"],
        json!([
            ["Annex", "Level 1", 20.0, 18000.0, 60.0],
            ["Annex", "Level 2", 20.0, 18000.0, 60.0],
            ["Annex", "Level 3", 20.0, 18000.0, 30.0],
            ["Tower", "Level 1", 60.0, 32000.0, 180.0],
            ["Tower", "Level 2", 60.0, 32000.0, 180.0],
            ["Tower", "Level 3", 60.0, 32000.0, 180.0],
        ])
    );
    // Summed per mass, the gross floor area and the volume.
    let r = run(
        &mut s,
        "schedule",
        json!({"name": "Gross", "category": "massfloors", "fields": ["Mass", {"name": "Area", "total": true}, {"name": "Volume", "total": true}], "sort": ["Mass"], "grouped": true}),
    );
    assert_eq!(r["rows"], json!([["Annex", 60.0, 150.0], ["Tower", 180.0, 540.0]]));
    assert_eq!(r["totals"], json!([null, 240.0, 690.0]));
    // The masses themselves are a schedule too: one row each.
    let fields = json!(["Name", "Level", "Height", "Area", "Volume", "Gross Floor Area"]);
    let r = run(&mut s, "schedule", json!({"category": "masses", "fields": fields, "sort": ["Name"]}));
    assert_eq!(r["rows"], json!([["Annex", "Level 1", 7500.0, 20.0, 150.0, 60.0], ["Tower", "Level 1", 9000.0, 60.0, 540.0, 180.0]]));
}

#[test]
fn walls_floors_and_roofs_by_face_follow_a_mass() {
    let mut s = Session::new();
    let id = run(&mut s, "mass", json!({"boundary": block()}))["id"].as_u64().unwrap();
    // A wall on each edge, with its exterior finish on it and facing out, up to the mass's top: so together they
    // stand inside the footprint, filling it out to its edges.
    let walls: Vec<u64> = run(&mut s, "WB", json!({"mass": id}))["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    assert_eq!(walls.len(), 4);
    let w = element(&mut s, walls[0]);
    assert_eq!(
        (w["start"].clone(), w["end"].clone(), id_of(&s, &w["level"]), id_of(&s, &w["top_level"])),
        (json!([10000.0, 0.0]), json!([0.0, 0.0]), Some(100_000), Some(100_001))
    );
    let ring: Vec<P2> = walls.iter().flat_map(|w| crate::walls::outline(&s.model, *w)).collect();
    let (lo, hi) = (ring.iter().fold(f64::MAX, |a, p| a.min(p.0)), ring.iter().fold(f64::MIN, |a, p| a.max(p.0)));
    let (south, north) = (ring.iter().fold(f64::MAX, |a, p| a.min(p.1)), ring.iter().fold(f64::MIN, |a, p| a.max(p.1)));
    assert!((lo.abs() + (hi - 10000.0).abs() + south.abs() + (north - 6000.0).abs()) < 1e-6, "{lo} {hi} {south} {north}");
    // Chosen faces, of the wall type asked for; a mass with no top level gives its height.
    let tall = run(&mut s, "mass", json!({"boundary": block(), "height": 4000, "base_offset": 200}))["id"].as_u64().unwrap();
    let two = run(&mut s, "wall_by_face", json!({"mass": tall, "faces": [1, 3], "location": "centre"}));
    assert_eq!(two["ids"].as_array().map(Vec::len), Some(2));
    let w = element(&mut s, two["ids"][0].as_u64().unwrap());
    assert_eq!(
        (w["height"].as_f64(), w["base_offset"].as_f64(), w["start"].clone()),
        (Some(4000.0), Some(200.0), json!([10000.0, 6000.0]))
    );
    for (p, says) in [
        (json!({"mass": tall, "faces": [4]}), "face"),
        (json!({"mass": tall, "faces": [-1]}), "face"),
        (json!({"mass": tall, "faces": ["top"]}), "face"),
        (json!({"mass": tall, "type": "Nothing"}), "type"),
        (json!({"mass": 100000}), "isn't a mass"),
        (json!({}), "mass"),
    ] {
        let e = s.execute("wall_by_face", &p).unwrap_err();
        assert!(e.contains(says), "{p}: {e}");
    }
    // A floor on the top face: at the top level, or `height` over the base; a roof the same, flat unless it has a slope.
    let f = run(&mut s, "FB", json!({"mass": id}))["id"].as_u64().unwrap();
    let e = element(&mut s, f);
    assert_eq!((id_of(&s, &e["level"]), e["offset"].as_f64().unwrap_or(0.0), e["area_m2"].as_f64()), (Some(100001), 0.0, Some(60.0)));
    let f = run(&mut s, "floor_by_face", json!({"mass": tall}))["id"].as_u64().unwrap();
    let e = element(&mut s, f);
    assert_eq!((id_of(&s, &e["level"]), e["offset"].as_f64()), (Some(100000), Some(4200.0)));
    let r = run(&mut s, "RB", json!({"mass": id}))["id"].as_u64().unwrap();
    let e = element(&mut s, r);
    assert_eq!((id_of(&s, &e["level"]), e["slope"].as_f64().unwrap_or(0.0), e["boundary"].clone()), (Some(100001), 0.0, block_ccw()));
    let r = run(&mut s, "roof_by_face", json!({"mass": tall, "slope": 20}))["id"].as_u64().unwrap();
    let e = element(&mut s, r);
    assert_eq!((id_of(&s, &e["level"]), e["offset"].as_f64(), e["slope"].as_f64()), (Some(100000), Some(4200.0), Some(20.0)));
    // They are ordinary elements from then on: the mass can go and they stay.
    run(&mut s, "delete", json!({"ids": [id, tall]}));
    assert_eq!((count(&mut s, "wall"), count(&mut s, "roof")), (6, 2));
}

#[test]
fn masses_show_in_plans_sections_and_the_3d_view_and_a_category_hides_them() {
    use crate::render::{self, Prim};
    let mut s = Session::new();
    let walls = house(&mut s);
    let (id, levels) = tower(&mut s);
    let dashed = |d: &render::Drawing| {
        d.prims.iter().zip(&d.owners).filter(|(p, o)| **o == Some(id) && matches!(p, Prim::Line { dash: Some(_), .. })).count()
    };
    // Where the plan's cut plane passes through it: Levels 1 to 3, not 4, where it ends.
    let plans: Vec<usize> = levels.iter().map(|l| dashed(&render::plan(&s.model, *l, 800, 600))).collect();
    assert_eq!(plans.iter().map(|n| *n > 0).collect::<Vec<_>>(), [true, true, true, false], "{plans:?}");
    // The plan's cut plane, 1200 mm over Level 1: at a mass's base it passes through it, at its top it doesn't.
    let base = run(&mut s, "mass", json!({"boundary": block(), "base_offset": 1200, "height": 1800}))["id"].as_u64().unwrap();
    let top = run(&mut s, "mass", json!({"boundary": block(), "height": 1200}))["id"].as_u64().unwrap();
    let level_1 = render::plan(&s.model, 100000, 800, 600);
    let of = |who: u64| {
        level_1.prims.iter().zip(&level_1.owners).filter(|(p, o)| **o == Some(who) && matches!(p, Prim::Line { dash: Some(_), .. })).count()
    };
    assert_eq!((of(base) > 0, of(top)), (true, 0));
    // A section through it: where its line crosses the footprint, from the base to the top.
    let section = run(&mut s, "section", json!({"start": [5000, -4000], "end": [5000, 10000]}))["id"].as_u64().unwrap();
    let view = render::section_fit(&s.model, section, 800, 600);
    let d = render::section(&s.model, section, view);
    assert!(dashed(&d) > 0);
    assert!(d.prims.iter().zip(&d.owners).all(|(p, o)| *o != Some(id) || !matches!(p, Prim::Fill { .. })), "outlined, not filled in poché");
    // Its picture leaves the mass out, so only the outline says where the line cuts it; the walls beyond are there.
    let picture = render::section_image(&s.model, &crate::view3d::Scene::new(&s.model), section, view, &[]).unwrap();
    let seen = picture.shows();
    assert!(!seen.contains(&id) && walls.iter().any(|w| seen.contains(w)), "{seen:?}");
    // In 3D: a solid up to its top, seen through a little unless the view says otherwise.
    assert_eq!(crate::view3d::tops_at(&s.model, P2(5000.0, 3000.0)).get(&id), Some(&9000.0));
    assert!(crate::graphics::Look::of_3d(&s.model).transparency(id) > 0);
    // `mass` is a category a view may hide, in every drawing.
    run(&mut s, "graphics", json!({"view": 100000, "hide": ["masses"]}));
    run(&mut s, "graphics", json!({"view": section, "hide": ["mass"]}));
    assert_eq!((dashed(&render::plan(&s.model, 100000, 800, 600)), dashed(&render::section(&s.model, section, view))), (0, 0));
    assert!(dashed(&render::plan(&s.model, 100001, 800, 600)) > 0, "Level 2's plan still shows it");
}

#[test]
fn update_by_face_rebuilds_what_was_made_by_face_from_the_mass_as_it_is_now() {
    let mut s = Session::new();
    let (id, [_, _, l3, l4]) = tower(&mut s);
    let walls = ids_of(&run(&mut s, "WB", json!({"mass": id}))["ids"]);
    let floor = run(&mut s, "FB", json!({"mass": id}))["id"].as_u64().unwrap();
    let roof = run(&mut s, "RB", json!({"mass": id}))["id"].as_u64().unwrap();
    // Each holds its mass and its face: an edge number for a wall, "top" for a floor or a roof.
    for (i, w) in walls.iter().enumerate() {
        assert_eq!(element(&mut s, *w)["params"]["Mass Face"], json!({"mass": s.model.guid(id), "face": i}));
    }
    for e in [floor, roof] {
        assert_eq!(element(&mut s, e)["params"]["Mass Face"], json!({"mass": s.model.guid(id), "face": "top"}));
    }
    // Chosen faces keep their own numbers.
    let two = ids_of(&run(&mut s, "wall_by_face", json!({"mass": id, "faces": [1, 3]}))["ids"]);
    assert_eq!(element(&mut s, two[1])["params"]["Mass Face"], json!({"mass": s.model.guid(id), "face": 3}));
    run(&mut s, "delete", json!({"ids": two}));
    // Nothing follows the mass by itself.
    run(&mut s, "modify", json!({"id": id, "boundary": [[0, 0], [12000, 0], [12000, 6000], [0, 6000]], "top_level": l3}));
    assert_eq!(element(&mut s, walls[0])["start"], json!([10000.0, 0.0]));
    // Update to face: the walls stand on the edges as they are, as tall as the mass, the floor and the roof on its top.
    run(&mut s, "wall", json!({"points": [[0, 0], [100, 0]]}));
    let done = run(&mut s, "UF", json!({"mass": id}));
    let mut every: Vec<u64> = walls.iter().copied().chain([floor, roof]).collect();
    assert_eq!(ids_of(&done["updated"]), every);
    let w = element(&mut s, walls[0]);
    assert_eq!((w["start"].clone(), w["end"].clone(), id_of(&s, &w["top_level"])), (json!([12000.0, 0.0]), json!([0.0, 0.0]), Some(l3)));
    assert_eq!(element(&mut s, walls[1])["start"], json!([12000.0, 6000.0]));
    for e in [floor, roof] {
        let e = element(&mut s, e);
        assert_eq!((id_of(&s, &e["level"]), e["area_m2"].as_f64()), (Some(l3), Some(72.0)));
    }
    // Again: nothing to do, so no undo step.
    let steps = s.history().len();
    let done = run(&mut s, "update_by_face", json!({}));
    assert_eq!((done["updated"].clone(), ids_of(&done["unchanged"])), (json!([]), every.clone()));
    assert_eq!(s.history().len(), steps);
    // What was done to them since stays: their type, a roof's slope.
    run(&mut s, "modify", json!({"id": roof, "slope": 25}));
    run(&mut s, "modify", json!({"id": id, "top_level": l4}));
    run(&mut s, "update_by_face", json!({"ids": [roof, walls[2]]}));
    let r = element(&mut s, roof);
    assert_eq!((id_of(&s, &r["level"]), r["slope"].as_f64()), (Some(l4), Some(25.0)));
    let tops = (crate::element_json(&s.model, walls[2])["top_level"].clone(), crate::element_json(&s.model, walls[3])["top_level"].clone());
    assert_eq!((id_of(&s, &tops.0), id_of(&s, &tops.1)), (Some(l4), Some(l3)));
    // A wall split in two is two ordinary walls: neither is the mass's face now.
    let halves = ids_of(&run(&mut s, "split", json!({"id": walls[2], "at": [6000, 6000]}))["ids"]);
    assert_eq!(halves.len(), 2);
    for half in halves {
        assert!(element(&mut s, half)["params"].get("Mass Face").is_none());
    }
    // A mass with a height: the floor goes up with it.
    let low = run(&mut s, "mass", json!({"boundary": block(), "height": 4000, "base_offset": 200}))["id"].as_u64().unwrap();
    let f = run(&mut s, "floor_by_face", json!({"mass": low}))["id"].as_u64().unwrap();
    let w = ids_of(&run(&mut s, "wall_by_face", json!({"mass": low, "faces": [0]}))["ids"])[0];
    run(&mut s, "modify", json!({"id": low, "height": 5000}));
    run(&mut s, "update_by_face", json!({"mass": low}));
    assert_eq!((element(&mut s, f)["offset"].as_f64(), element(&mut s, w)["height"].as_f64()), (Some(5200.0), Some(5000.0)));
    // A mass mirrored in place runs round the other way: its walls are numbered again, so each follows its own edge.
    let flip = run(&mut s, "mass", json!({"boundary": block(), "height": 3000, "name": "Flip"}))["id"].as_u64().unwrap();
    let flipped = ids_of(&run(&mut s, "wall_by_face", json!({"mass": flip, "faces": [0, 1]}))["ids"]);
    run(&mut s, "mirror", json!({"ids": [flip], "axis": [[-1000, 0], [-1000, 1000]]}));
    assert_eq!(element(&mut s, flipped[0])["params"]["Mass Face"], json!({"mass": s.model.guid(flip), "face": 2}));
    assert_eq!(element(&mut s, flipped[1])["params"]["Mass Face"], json!({"mass": s.model.guid(flip), "face": 1}));
    run(&mut s, "update_by_face", json!({"mass": flip}));
    let w = element(&mut s, flipped[0]);
    assert_eq!((w["start"].clone(), w["end"].clone()), (json!([-2000.0, 0.0]), json!([-12000.0, 0.0])));
    run(&mut s, "delete", json!({"ids": [flip]}));
    // A face that's gone refuses the lot, and says which: the model is as it was.
    run(&mut s, "modify", json!({"id": id, "boundary": [[0, 0], [12000, 0], [0, 6000]]}));
    let before = s.model.clone();
    let e = s.execute("update_by_face", &json!({"mass": id})).unwrap_err();
    assert!(e.contains(&format!("wall {}", walls[3])) && e.contains("face 3") && e.contains("3 faces"), "{e}");
    assert_eq!(*s.model, *before);
    // The ones whose faces are there can be asked for by name.
    run(&mut s, "update_by_face", json!({"ids": [walls[0], walls[1], floor]}));
    assert_eq!(element(&mut s, floor)["area_m2"].as_f64(), Some(36.0));
    // Not made by face, or made from no mass any more.
    let plain = run(&mut s, "wall", json!({"points": [[0, 0], [100, 0]]}))["ids"][0].as_u64().unwrap();
    for (p, says) in [
        (json!({"ids": [plain]}), "isn't made by face"),
        (json!({"mass": plain}), "isn't a mass"),
        (json!({"mass": id, "ids": [walls[0]]}), "ids"),
        (json!({"ids": [9999]}), "9999"),
        (json!({"ids": "x"}), "ids"),
    ] {
        let e = s.execute("update_by_face", &p).unwrap_err();
        assert!(e.contains(says), "{p}: {e}");
    }
    every.push(plain);
    run(&mut s, "delete", json!({"ids": [id, low]}));
    assert!(element(&mut s, walls[0])["params"].get("Mass Face").is_none());
    assert!(s.execute("update_by_face", &json!({})).unwrap_err().contains("by face"));
    // The mark is a mass and its face, of the right kind for the element.
    let m2 = run(&mut s, "mass", json!({"boundary": block(), "height": 3000}))["id"].as_u64().unwrap();
    for (e, mark) in [
        (walls[0], json!({"mass": m2, "face": "top"})),
        (floor, json!({"mass": m2, "face": 0})),
        (walls[0], json!({"mass": plain, "face": 0})),
        (walls[0], json!({"mass": m2})),
        (plain, json!({"mass": m2, "face": -1})),
    ] {
        assert!(s.execute("modify", &json!({"id": e, "params": {"Mass Face": mark}})).is_err(), "{e} {mark}");
    }
    run(&mut s, "modify", json!({"id": plain, "params": {"Mass Face": {"mass": m2, "face": 7}}}));
    assert!(s.execute("update_by_face", &json!({})).unwrap_err().contains("face 7"), "a mark beyond the footprint is a face that's gone");
}

/// A mass 10 x 6 m at its base on Level 1 and 6 x 2 m at Level 3, 6 m up, the top outline centred: a prismatoid of rectangles.
fn frustum(s: &mut Session) -> (u64, [u64; 3]) {
    let l3 = run(s, "level", json!({"elevation": 6000, "name": "Level 3"}))["id"].as_u64().unwrap();
    let top = json!([[2000, 2000], [8000, 2000], [8000, 4000], [2000, 4000]]);
    let id = run(s, "mass", json!({"boundary": block(), "top_level": l3, "top": top, "name": "Frustum"}))["id"].as_u64().unwrap();
    (id, [1, 2, l3])
}

#[test]
fn a_mass_with_a_top_outline_is_lofted_straight_between_it_and_its_footprint() {
    let mut s = Session::new();
    let l3 = run(&mut s, "level", json!({"elevation": 6000, "name": "Level 3"}))["id"].as_u64().unwrap();
    let top = json!([[2000, 2000], [8000, 2000], [8000, 4000], [2000, 4000]]);
    let r = run(&mut s, "mass", json!({"boundary": block(), "top_level": l3, "top": top, "name": "Frustum"}));
    let id = r["id"].as_u64().unwrap();
    // 60 m² at its base, and 6 m x (60 + 4 x 32 + 12) / 6 = 200 m³, the 32 being the 8 x 4 m halfway up.
    assert_eq!((r["area_m2"].as_f64(), r["height"].as_f64(), r["volume_m3"].as_f64()), (Some(60.0), Some(6000.0), Some(200.0)));
    let e = element(&mut s, id);
    assert_eq!(e["top"], json!([[2000.0, 2000.0], [8000.0, 2000.0], [8000.0, 4000.0], [2000.0, 4000.0]]));
    assert_eq!((e["volume_m3"].as_f64(), e["gross_floor_area_m2"].as_f64()), (Some(200.0), Some(92.0)));
    // A floor at each level it crosses is the outline between the two there: Level 2 is halfway up.
    let floors = ids_of(&run(&mut s, "MF", json!({"mass": id}))["ids"]);
    assert_eq!(floors.len(), 2);
    let f = element(&mut s, floors[1]);
    assert_eq!(f["boundary"], json!([[1000.0, 1000.0], [9000.0, 1000.0], [9000.0, 5000.0], [1000.0, 5000.0]]));
    assert_eq!(f["area_m2"].as_f64(), Some(32.0));
    assert_eq!(element(&mut s, floors[0])["boundary"], block_ccw());
    // Run again after the top outline moves: its floors follow, the Level 1 one unchanged.
    run(&mut s, "modify", json!({"id": id, "top": [[0, 0], [10000, 0], [10000, 6000], [0, 6000]]}));
    let r = run(&mut s, "mass_floors", json!({"mass": id}));
    assert_eq!(ids_of(&r["updated"]), floors[1..].to_vec());
    // Its schedules: area, perimeter and the volume from each level to the next, from the cross section exactly.
    run(&mut s, "modify", json!({"id": id, "top": top}));
    run(&mut s, "mass_floors", json!({"mass": id}));
    let r = run(&mut s, "schedule", json!({"category": "massfloors", "sort": ["Level"]}));
    assert_eq!(r["rows"], json!([["Frustum", "Level 1", 60.0, 32000.0, 136.0], ["Frustum", "Level 2", 32.0, 24000.0, 64.0]]));
    // A top outline is a floor and a roof on its top face, at its own place, not the footprint's.
    let f = run(&mut s, "floor_by_face", json!({"mass": id}))["id"].as_u64().unwrap();
    assert_eq!(
        (element(&mut s, f)["boundary"].clone(), id_of(&s, &crate::element_json(&s.model, f)["level"])),
        (json!(top_ccw()), Some(l3))
    );
    let r = run(&mut s, "roof_by_face", json!({"mass": id}))["id"].as_u64().unwrap();
    assert_eq!(element(&mut s, r)["boundary"], json!(top_ccw()));
    // No wall stands plumb on a face that leans; every face of this one does.
    let e = s.execute("wall_by_face", &json!({"mass": id})).unwrap_err();
    assert!(e.contains("plumb") && e.contains("lean"), "{e}");
    let e = s.execute("wall_by_face", &json!({"mass": id, "faces": [1]})).unwrap_err();
    assert!(e.contains("face 1") && e.contains("plumb"), "{e}");
    // It follows when its top outline moves: update_by_face gives the floor the new one.
    run(&mut s, "modify", json!({"id": id, "top": [[0, 0], [10000, 0], [10000, 3000], [0, 3000]]}));
    run(&mut s, "update_by_face", json!({"mass": id}));
    assert_eq!(element(&mut s, f)["area_m2"].as_f64(), Some(30.0));
    // A sloped mass is the special case of a top outline moved sideways: `lean` makes one, and the volume is its base times its height.
    let lean = run(&mut s, "mass", json!({"boundary": block(), "top_level": l3, "lean": [3000, 0], "name": "Lean"}));
    let lean_id = lean["id"].as_u64().unwrap();
    assert_eq!((lean["volume_m3"].as_f64(), element(&mut s, lean_id)["top"][0].clone()), (Some(360.0), json!([3000.0, 0.0])));
    let floors = ids_of(&run(&mut s, "mass_floors", json!({"mass": lean_id}))["ids"]);
    assert_eq!(element(&mut s, floors[1])["boundary"][0], json!([1500.0, 0.0]));
    // A lean leaves no face plumb. Where only some corners move, the faces between those that don't are.
    assert!(s.execute("wall_by_face", &json!({"mass": lean_id})).unwrap_err().contains("leans"));
    let part = json!([[0, 0], [10000, 0], [8000, 6000], [0, 6000]]);
    let part = run(&mut s, "mass", json!({"boundary": block(), "height": 3000, "top": part}))["id"].as_u64().unwrap();
    let walls = ids_of(&run(&mut s, "wall_by_face", json!({"mass": part}))["ids"]);
    let faces: Vec<Value> = walls.iter().map(|w| s.model.elements[w].params().unwrap()["Mass Face"]["face"].clone()).collect();
    assert_eq!(faces, [json!(0), json!(3)]);
    run(&mut s, "modify", json!({"id": part, "top": [[0, 0], [10000, 0], [10000, 5000], [0, 5000]]}));
    let e = s.execute("update_by_face", &json!({"ids": walls})).unwrap_err();
    assert!(e.contains("leans now"), "{e}");
    // It moves, copies, rotates and mirrors with its top outline.
    let copy = ids_of(&run(&mut s, "copy", json!({"ids": [id], "by": [20000, 0]}))["ids"])[0];
    assert_eq!(element(&mut s, copy)["top"][0], json!([20000.0, 0.0]));
    run(&mut s, "mirror", json!({"ids": [id], "axis": [[-1000, 0], [-1000, 1000]]}));
    let e = element(&mut s, id);
    // 6 m x (60 + 4 x 45 + 30) / 6 = 270 m³ now; 60 + 45 m² of floor.
    assert_eq!((e["volume_m3"].as_f64(), e["gross_floor_area_m2"].as_f64()), (Some(270.0), Some(105.0)), "{e}");
    run(&mut s, "rotate", json!({"ids": [id], "about": [0, 0], "angle": 90}));
    assert_eq!(element(&mut s, id)["volume_m3"].as_f64(), Some(270.0));
    // `top: null` takes the loft away: a straight mass again.
    run(&mut s, "modify", json!({"id": id, "top": null}));
    let e = element(&mut s, id);
    assert!(e.get("top").is_none_or(Value::is_null));
    assert_eq!(e["volume_m3"].as_f64(), Some(360.0));
}

fn top_ccw() -> Value {
    json!([[2000.0, 2000.0], [8000.0, 2000.0], [8000.0, 4000.0], [2000.0, 4000.0]])
}

#[test]
fn a_top_outline_pairs_up_with_the_footprint_point_for_point_and_never_collapses_the_mass() {
    let mut s = Session::new();
    let before = s.model.clone();
    let l2 = 2;
    for (p, says) in [
        (json!({"boundary": block(), "height": 3000, "top": [[0, 0], [1, 0], [1, 1]]}), "points"),
        (json!({"boundary": block(), "height": 3000, "top": "x"}), "top"),
        (json!({"boundary": block(), "height": 3000, "top": [[0, 0], [1e300, 0], [1, 1], [0, 1]]}), "top"),
        // Clockwise where the footprint runs counter-clockwise, or collapsing halfway up: each corner meets the opposite one.
        (json!({"boundary": block(), "height": 3000, "top": [[0, 6000], [10000, 6000], [10000, 0], [0, 0]]}), "other way"),
        (json!({"boundary": block(), "height": 3000, "top": [[10000, 6000], [0, 6000], [0, 0], [10000, 0]]}), "collapses"),
        (json!({"boundary": block(), "height": 3000, "top": block(), "lean": [1, 1]}), "lean"),
        (json!({"boundary": block(), "height": 3000, "lean": 5}), "lean"),
        (json!({"boundary": block(), "top_level": l2, "lean": [1e300, 0]}), "lean"),
    ] {
        let e = s.execute("mass", &p).unwrap_err();
        assert!(e.contains(says), "{p}: {e}");
    }
    assert_eq!(*s.model, *before);
    // Given the way round that runs clockwise, the footprint is turned counter-clockwise and its top outline with it, still twin for twin.
    let cw = json!([[0, 0], [0, 6000], [10000, 6000], [10000, 0]]);
    let top = json!([[2000, 2000], [2000, 4000], [8000, 4000], [8000, 2000]]);
    let id = run(&mut s, "mass", json!({"boundary": cw, "height": 6000, "top": top}))["id"].as_u64().unwrap();
    let e = element(&mut s, id);
    assert_eq!(e["volume_m3"].as_f64(), Some(200.0));
    assert_eq!((e["boundary"][1].clone(), e["top"][1].clone()), (json!([10000.0, 6000.0]), json!([8000.0, 4000.0])));
    // A change to the footprint's corners alone leaves the top outline short.
    let e = s.execute("modify", &json!({"id": id, "boundary": [[0, 0], [100, 0], [0, 100]]})).unwrap_err();
    assert!(e.contains("points"), "{e}");
    // The same top outline as its footprint is no loft.
    let flat = run(&mut s, "mass", json!({"boundary": block(), "height": 3000, "top": block()}))["id"].as_u64().unwrap();
    assert!(element(&mut s, flat).get("top").is_none_or(Value::is_null));
}

#[test]
fn a_lofted_mass_is_drawn_in_plans_and_sections_where_the_view_cuts_it() {
    use crate::render::{self, Prim};
    let mut s = Session::new();
    let (id, _) = frustum(&mut s);
    let dashed = |d: &render::Drawing| -> Vec<Vec<(f32, f32)>> {
        d.prims
            .iter()
            .zip(&d.owners)
            .filter_map(|(p, o)| match p {
                Prim::Line { pts, dash: Some(_), .. } if *o == Some(id) => Some(pts.clone()),
                _ => None,
            })
            .collect()
    };
    let has = |lines: &[Vec<(f32, f32)>], want: (f32, f32)| {
        lines.iter().flatten().any(|p| (p.0 - want.0).abs() < 0.05 && (p.1 - want.1).abs() < 0.05)
    };
    // Level 1's plan cuts 1200 mm up, a fifth of the way: each corner that far along its way to its twin.
    let d = render::plan(&s.model, 100000, 800, 600);
    let lines = dashed(&d);
    assert_eq!(lines.len(), 1);
    for p in [P2(400.0, 400.0), P2(9600.0, 400.0), P2(9600.0, 5600.0), P2(400.0, 5600.0)] {
        assert!(has(&lines, d.view.px(p)), "{p:?} in {lines:?}");
    }
    // Level 2's, at 4200: seven tenths.
    let d = render::plan(&s.model, 100001, 800, 600);
    for p in [P2(1400.0, 1400.0), P2(8600.0, 1400.0), P2(8600.0, 4600.0), P2(1400.0, 4600.0)] {
        assert!(has(&dashed(&d), d.view.px(p)), "{p:?}");
    }
    // A section across it, along x = 5000 from y = -4000: the outline slants in from each side, to 2000 mm in at the top.
    let section = run(&mut s, "section", json!({"start": [5000, -4000], "end": [5000, 10000]}))["id"].as_u64().unwrap();
    let view = render::section_fit(&s.model, section, 800, 600);
    let d = render::section(&s.model, section, view);
    let lines = dashed(&d);
    for p in [P2(4000.0, 0.0), P2(10000.0, 0.0), P2(6000.0, 6000.0), P2(8000.0, 6000.0)] {
        assert!(has(&lines, d.view.px(p)), "{p:?} in {lines:?}");
    }
    assert!(d.prims.iter().zip(&d.owners).all(|(p, o)| *o != Some(id) || !matches!(p, Prim::Fill { .. })), "outlined, not filled in poché");
    // Every dashed point lies on the cut: left of the centre by 4000 + z/3 at most.
    let inside = |p: &(f32, f32)| {
        let z = ((f64::from(d.view.px(P2(0.0, 0.0)).1) - f64::from(p.1)) / d.view.scale).clamp(0.0, 6000.0);
        let s = (f64::from(p.0) - f64::from(d.view.px(P2(0.0, 0.0)).0)) / d.view.scale;
        s >= 4000.0 + z / 3.0 - 0.5 && s <= 10000.0 - z / 3.0 + 0.5
    };
    assert!(lines.iter().flatten().all(inside));
    // A view that hides masses draws none of it.
    run(&mut s, "graphics", json!({"view": section, "hide": ["mass"]}));
    assert!(dashed(&render::section(&s.model, section, view)).is_empty());
    // A plan that cuts it at the top, or above, shows nothing.
    run(&mut s, "modify", json!({"id": 100000, "view_range": {"cut": 6500}}));
    assert!(dashed(&render::plan(&s.model, 100000, 800, 600)).is_empty());
}

#[test]
fn show_mass_toggles_the_mass_category_in_one_view_in_one_step() {
    use crate::render::{self, Prim};
    let mut s = Session::new();
    let (id, _) = tower(&mut s);
    let dashed = |m: &crate::model::Model, view: u64| {
        let d = render::plan(m, view, 800, 600);
        d.prims.iter().zip(&d.owners).filter(|(p, o)| **o == Some(id) && matches!(p, Prim::Line { dash: Some(_), .. })).count()
    };
    assert!(dashed(&s.model, 100000) > 0 && dashed(&s.model, 100001) > 0);
    // With no say, it flips what the view does now: masses shown, so hidden, and shown again.
    let r = run(&mut s, "MH", json!({"view": 100000}));
    assert_eq!((r["shown"].clone(), r["hidden"].clone()), (json!(false), json!(["mass"])));
    assert_eq!((dashed(&s.model, 100000), dashed(&s.model, 100001) > 0), (0, true), "only the view it was asked for");
    let r = run(&mut s, "show_mass", json!({"view": 100000}));
    assert_eq!((r["shown"].clone(), r["hidden"].clone()), (json!(true), json!([])));
    assert!(dashed(&s.model, 100000) > 0);
    // `show` says which, whatever the view does now; one undo step each.
    run(&mut s, "show_mass", json!({"view": "Level 1", "show": false}));
    run(&mut s, "show_mass", json!({"view": "Level 1", "show": false}));
    assert_eq!(dashed(&s.model, 100000), 0);
    run(&mut s, "show_mass", json!({"view": 100000, "show": true}));
    assert!(dashed(&s.model, 100000) > 0);
    run(&mut s, "undo", json!({}));
    assert_eq!(dashed(&s.model, 100000), 0);
    // The 3D view makes its own on the first change, a section and an elevation are views too.
    let r = run(&mut s, "show_mass", json!({"view": "{3D}", "show": false}));
    assert_eq!(r["hidden"], json!(["mass"]));
    let section = run(&mut s, "section", json!({"start": [5000, -4000], "end": [5000, 10000]}))["id"].as_u64().unwrap();
    assert_eq!(run(&mut s, "show_mass", json!({"view": section}))["shown"], json!(false));
    // A view that follows a template with categories takes no change of its own, and `show` is a yes or no.
    run(&mut s, "template", json!({"name": "T", "controls": ["categories"]}));
    run(&mut s, "graphics", json!({"view": 100001, "template": "T"}));
    let e = s.execute("show_mass", &json!({"view": 100001})).unwrap_err();
    assert!(e.contains("template"), "{e}");
    for (p, says) in [
        (json!({"view": 100000, "show": "yes"}), "show"),
        (json!({"view": "nowhere"}), "view"),
        (json!({"view": 100000, "show": 5}), "show"),
    ] {
        let e = s.execute("show_mass", &p).unwrap_err();
        assert!(e.contains(says), "{p}: {e}");
    }
}

#[test]
fn a_level_reaches_as_far_as_its_extent_in_sections_elevations_and_the_3d_view() {
    use crate::view3d::{AZIMUTH, ELEVATION, Scene};
    let mut s = Session::new();
    house(&mut s);
    let sec = run(&mut s, "section", json!({"start": [-1000, 3000], "end": [9000, 3000]}))["id"].as_u64().unwrap();
    // A level stretches over a rectangle in plan: by default, the model's, a metre past it.
    let mezz = run(&mut s, "level", json!({"elevation": 1500, "name": "Mezzanine"}))["id"].as_u64().unwrap();
    assert_eq!(s.model.level_extent(mezz), Some([P2(-1100.0, -1100.0), P2(9100.0, 7100.0)]));
    run(&mut s, "modify", json!({"id": mezz, "extent": [[0, 0], [4000, 6000]]}));
    assert_eq!(s.model.level_extent(mezz), Some([P2(0.0, 0.0), P2(4000.0, 6000.0)]));
    // The section draws it only over its extent: 1000 to 5000 along its line.
    let at = |s: &Session, along: f64, z: f64| {
        let view = crate::render::section_fit(&s.model, sec, 1600, 1000);
        let d = crate::render::section(&s.model, sec, view);
        d.pick(view.px(P2(along, z)), 2.0)
    };
    assert_eq!(at(&s, 3000.0, 1500.0), Some(mezz));
    assert_ne!(at(&s, 7000.0, 1500.0), Some(mezz));
    // An extent wholly behind the section's line: not in it at all.
    run(&mut s, "modify", json!({"id": mezz, "extent": [[0, 0], [4000, 2000]]}));
    assert_ne!(at(&s, 3000.0, 1500.0), Some(mezz));
    // Hostile or back to front: refused, with what to fix.
    for bad in [json!([[4000, 0], [0, 6000]]), json!([[0, 0], [1e300, 6000]])] {
        let e = s.execute("modify", &json!({"id": mezz, "extent": bad})).unwrap_err();
        assert!(e.contains("extent"), "{e}");
    }
    let e = s.execute("level", &json!({"elevation": 6000, "extent": [[0, 0], [0, 0]]})).unwrap_err();
    assert!(e.contains("extent"), "{e}");
    // In 3D, a plane: its outline is drawn, and clicking it picks the level.
    let top = run(&mut s, "level", json!({"elevation": 9000, "name": "Top", "extent": [[0, 0], [8000, 6000]]}))["id"].as_u64().unwrap();
    let scene = Scene::new(&s.model);
    let cam = scene.fit(AZIMUTH, ELEVATION, 1600, 1000);
    let image = scene.render(&cam, &[]);
    // Its far north edge: the near ones show only when it is selected.
    let (x, y, _) = cam.project([4000.0, 6000.0, 9000.0]);
    let near: Vec<_> = (-2..=2)
        .flat_map(|dx| (-2..=2).map(move |dy| (dx, dy)))
        .filter_map(|(dx, dy)| image.pick(x + f64::from(dx), y + f64::from(dy)))
        .collect();
    assert!(near.contains(&top), "{near:?}");
}

#[test]
fn a_section_uncropped_shows_all_there_is_along_its_line_and_cropped_only_its_region() {
    let mut s = Session::new();
    let w = house(&mut s);
    let sec = run(&mut s, "section", json!({"start": [-1000, 3000], "end": [9000, 3000]}))["id"].as_u64().unwrap();
    // Cropped from 2000 along its line: the west wall, at 1000, is left out.
    run(&mut s, "crop", json!({"view": sec, "region": [[2000, -600], [10000, 3600]]}));
    assert_eq!(element(&mut s, sec)["start"], json!([1000.0, 3000.0]));
    let west = w[3];
    let shows = |s: &Session| {
        let view = crate::render::section_fit(&s.model, sec, 1600, 1000);
        let d = crate::render::section(&s.model, sec, view);
        d.pick(view.px(P2(-1000.0, 1500.0)), 3.0)
    };
    assert_ne!(shows(&s), Some(west));
    assert!(crate::render::crop_region(&s.model, sec).is_some());
    // Uncropped: no crop region, and the wall before its start is cut again.
    run(&mut s, "crop", json!({"view": sec, "region": null}));
    assert!(crate::render::crop_region(&s.model, sec).is_none());
    assert_eq!(shows(&s), Some(west));
    // Cropped again to what it shows: its line and the model's heights.
    let r = run(&mut s, "crop", json!({"view": sec}));
    assert_eq!(r["crop"], json!([-600.0, 3600.0]));
}

#[test]
fn a_levels_ends_in_a_section_drag_its_extent_and_its_edges_drag_in_3d() {
    let mut s = Session::new();
    house(&mut s);
    let sec = run(&mut s, "section", json!({"start": [-1000, 3000], "end": [9000, 3000]}))["id"].as_u64().unwrap();
    let mezz = run(&mut s, "level", json!({"elevation": 1500, "extent": [[0, 0], [4000, 6000]]}))["id"].as_u64().unwrap();
    // Its ends in the section, along its line: where the extent's west and east edges cross it.
    assert_eq!(crate::render::level_ends(&s.model, mezz, sec), Some((1000.0, 5000.0)));
    // Its far end dragged to 7000 along: the east edge goes to 6000; its near end back to 0, the west to -1000.
    assert_eq!(s.model.level_extent_along(mezz, sec, true, 7000.0), Some([P2(0.0, 0.0), P2(6000.0, 6000.0)]));
    assert_eq!(s.model.level_extent_along(mezz, sec, false, 0.0), Some([P2(-1000.0, 0.0), P2(4000.0, 6000.0)]));
    // Past its other end: no less than 100 mm across.
    assert_eq!(s.model.level_extent_along(mezz, sec, true, -5000.0), Some([P2(0.0, 0.0), P2(100.0, 6000.0)]));
    // A section drawn the other way: its far end is the extent's west edge.
    let back = run(&mut s, "section", json!({"start": [9000, 3000], "end": [-1000, 3000]}))["id"].as_u64().unwrap();
    assert_eq!(s.model.level_extent_along(mezz, back, true, 10000.0), Some([P2(-1000.0, 0.0), P2(4000.0, 6000.0)]));
    // In 3D, a grip at the middle of each edge: west, east, south, north; one dragged moves its edge.
    let extent = [P2(0.0, 0.0), P2(4000.0, 6000.0)];
    assert_eq!(crate::geom::edge_grips(extent), [P2(0.0, 3000.0), P2(4000.0, 3000.0), P2(2000.0, 0.0), P2(2000.0, 6000.0)]);
    assert_eq!(crate::geom::move_edge(extent, 1, P2(5000.0, 999.0)), [P2(0.0, 0.0), P2(5000.0, 6000.0)]);
    // The camera finds where on the level's plane a pixel lies.
    let cam = crate::view3d::Scene::new(&s.model).fit(crate::view3d::AZIMUTH, crate::view3d::ELEVATION, 800, 600);
    let (x, y, _) = cam.project([2500.0, 1200.0, 1500.0]);
    let p = cam.on_plane((x, y), 1500.0).unwrap();
    assert!((p[0] - 2500.0).abs() < 1e-6 && (p[1] - 1200.0).abs() < 1e-6 && (p[2] - 1500.0).abs() < 1e-6, "{p:?}");
}

#[test]
fn a_levels_plane_in_3d_hides_its_near_edges_until_selected_and_names_itself_on_a_head() {
    use crate::view3d::{AZIMUTH, ELEVATION, Scene};
    let mut s = Session::new();
    house(&mut s);
    let top = run(&mut s, "level", json!({"elevation": 9000, "name": "Top", "extent": [[0, 0], [8000, 6000]]}))["id"].as_u64().unwrap();
    let scene = Scene::new(&s.model);
    let cam = scene.fit(AZIMUTH, ELEVATION, 1600, 1000);
    let near = |img: &crate::view3d::Image, p: [f64; 3]| {
        let (x, y, _) = cam.project(p);
        (-2..=2).flat_map(|dx| (-2..=2).map(move |dy| (dx, dy))).any(|(dx, dy)| img.pick(x + f64::from(dx), y + f64::from(dy)) == Some(top))
    };
    // From the south-east: the south and east edges are the nearer two, left out; the far two drawn.
    let plain = scene.render(&cam, &[]);
    assert!(!near(&plain, [4000.0, 0.0, 9000.0]) && !near(&plain, [8000.0, 3000.0, 9000.0]));
    assert!(near(&plain, [4000.0, 6000.0, 9000.0]) && near(&plain, [0.0, 3000.0, 9000.0]));
    // Selected, all four.
    let picked = scene.render(&cam, &[(top, crate::render::Rgb(0, 120, 255))]);
    assert!(near(&picked, [4000.0, 0.0, 9000.0]) && near(&picked, [8000.0, 3000.0, 9000.0]));
    // Its head: a vertical plane past the right end of its far edges, on the north edge's line, its name written on it.
    let heads = scene.level_heads(&cam);
    let head = heads.iter().find(|h| h.level == top).unwrap();
    assert_eq!(head.name, "Top");
    assert!(head.corners.iter().all(|c| (c[1] - 6000.0).abs() < 1e-6 && c[0] > 8000.0), "{:?}", head.corners);
    assert_eq!((head.corners[0][2], head.corners[1][2]), (9000.0, 9000.0));
    assert!(head.corners[2][2] > 9000.0 && head.corners[3][2] > 9000.0);
    // Clicking on it picks the level.
    let mid = head.corners.iter().fold([0.0; 3], |a, c| [a[0] + c[0] / 4.0, a[1] + c[1] / 4.0, a[2] + c[2] / 4.0]);
    let (x, y, _) = cam.project(mid);
    assert_eq!(plain.pick(x, y), Some(top));
    // Its name is drawn there: pixels in the level's colour inside the head.
    let lettered = (-30..=30)
        .flat_map(|dx| (-6..=6).map(move |dy| (dx, dy)))
        .filter(|(dx, dy)| plain.color(x + f64::from(*dx), y + f64::from(*dy)) != crate::render::PAPER)
        .count();
    assert!(lettered > 10, "{lettered}");
}

/// `v` as a file from before GUIDs: no `format`, elements numbered from 1, what points at one holding its number.
fn before_guids(v: &Value) -> Value {
    fn walk(v: &mut Value, old: &std::collections::HashMap<String, u64>) {
        match v {
            Value::String(s) if old.contains_key(s.as_str()) => *v = json!(old[s.as_str()]),
            Value::Array(a) => a.iter_mut().for_each(|x| walk(x, old)),
            Value::Object(o) => {
                o.remove("guid");
                let keys: Vec<String> = o.keys().cloned().collect();
                for k in keys {
                    let mut x = o.remove(&k).unwrap();
                    walk(&mut x, old);
                    o.insert(old.get(&k).map_or(k, |n| n.to_string()), x);
                }
            }
            _ => {}
        }
    }
    let mut v = v.clone();
    let old: std::collections::HashMap<String, u64> = v["elements"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(id, e)| (e["guid"].as_str().unwrap().to_string(), id.parse::<u64>().unwrap() - 99_999))
        .collect();
    let elements: serde_json::Map<String, Value> = v["elements"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(id, e)| {
            let mut e = e.clone();
            walk(&mut e, &old);
            ((id.parse::<u64>().unwrap() - 99_999).to_string(), e)
        })
        .collect();
    v["elements"] = Value::Object(elements);
    v["next_id"] = json!(v["next_id"].as_u64().unwrap() - 99_999);
    v.as_object_mut().unwrap().remove("format");
    v
}

#[test]
fn every_element_has_a_six_digit_id_and_a_guid_and_points_at_others_by_guid() {
    let mut s = Session::new();
    let walls = house(&mut s);
    run(&mut s, "door", json!({"at": [2000, 0]}));
    assert_eq!(s.model.levels()[0].0, 100_000, "the first id");
    assert!(s.model.elements.keys().all(|id| (100_000..1_000_000).contains(id)));
    let v = serde_json::to_value(&*s.model).unwrap();
    assert_eq!(v["format"], crate::model::FORMAT);
    let wall = &v["elements"][walls[0].to_string()];
    assert_eq!(wall["level"], json!(s.model.guid(100_000).to_string()), "a wall holds its level's GUID");
    assert_eq!(wall["guid"], json!(s.model.guid(walls[0]).to_string()));
    let e = run(&mut s, "elements", json!({"category": "walls"}));
    assert_eq!(e["elements"][0]["guid"], wall["guid"]);
    // Commands take an element by its id or its GUID.
    let g = s.model.guid(walls[1]).to_string();
    run(&mut s, "modify", json!({"id": g, "height": 2500}));
    run(&mut s, "door", json!({"wall": g, "offset": 3000}));
    run(&mut s, "modify", json!({"id": walls[2], "top_level": null, "height": 2700, "level": 100_000}));
    let Some(crate::model::Element::Wall(w)) = s.model.elements.get(walls[2]) else { panic!() };
    assert_eq!(w.level, s.model.guid(100_000), "an id given where a reference goes is held as the GUID");
    // A copy is a new element: a new id and a new GUID.
    let copy = run(&mut s, "copy", json!({"ids": [walls[0]], "by": [0, 10000]}))["ids"][0].as_u64().unwrap();
    assert_ne!(s.model.guid(copy), s.model.guid(walls[0]));
}

#[test]
fn a_file_from_before_guids_opens_with_guids_and_six_digit_ids() {
    let mut s = Session::new();
    let walls = house(&mut s);
    run(&mut s, "door", json!({"at": [2000, 0]}));
    run(&mut s, "graphics", json!({"view": 100_000, "hide_elements": [walls[1]]}));
    let old = before_guids(&serde_json::to_value(&*s.model).unwrap());
    assert_eq!(old["elements"]["3"]["level"], 1, "{}", old["elements"]["3"]);
    let path = std::env::temp_dir().join(format!("bimcraft-old-{}.bimcraft", std::process::id()));
    std::fs::write(&path, old.to_string()).unwrap();
    let mut t = Session::new();
    run(&mut t, "open", json!({"path": path.to_str().unwrap()}));
    let _ = std::fs::remove_file(&path);
    // The same elements under the same ids, each with a GUID of its own, pointing at each other by them.
    assert_eq!(t.model.elements.keys().collect::<Vec<_>>(), s.model.elements.keys().collect::<Vec<_>>());
    assert_eq!(t.model.format, crate::model::FORMAT);
    assert_eq!(t.model.next_id, s.model.next_id);
    let Some(crate::model::Element::Wall(w)) = t.model.elements.get(walls[0]) else { panic!() };
    assert_eq!(w.level, t.model.guid(100_000));
    let hidden = crate::graphics::graphics_of(&t.model, 100_000).unwrap();
    assert!(hidden.elements.contains_key(&t.model.guid(walls[1])));
    // A newer file says so.
    let mut newer = serde_json::to_value(&*s.model).unwrap();
    newer["format"] = json!(crate::model::FORMAT + 1);
    newer["something_new"] = json!(true);
    std::fs::write(&path, newer.to_string()).unwrap();
    let e = t.execute("open", &json!({"path": path.to_str().unwrap()})).unwrap_err();
    let _ = std::fs::remove_file(&path);
    assert!(e.contains("newer BIMCraft"), "{e}");
}

#[test]
fn a_project_is_saved_as_format_11_and_one_of_format_4_to_10_opens_as_it_is_and_one_of_format_12_is_refused() {
    use crate::model::Model;
    // 5 added the scans a project holds and the point clouds that place them; 6 added detailing (lines, regions, groups,
    // repeating details, line and fill styles); 7 its CAD drawings and the imports that place them; 8 text types; 9 graphic line patterns; 10 dimensions to points and linear ones. Nothing in a file of 4
    // to 7 that holds no notes changes.
    let mut s = Session::new();
    let empty = serde_json::to_value(&*s.model).unwrap();
    house(&mut s);
    run(&mut s, "detail_line", json!({"start": [0, 0], "end": [1000, 0]}));
    let full = serde_json::to_value(&*s.model).unwrap();
    for v in [&empty, &full] {
        assert_eq!(v["format"], 11, "a project is saved as format 11");
        for old in [4, 5, 6, 7, 8, 9, 10] {
            let mut was = v.clone();
            was["format"] = json!(old);
            let (m, left_out) = Model::read(&was.to_string()).unwrap();
            assert_eq!((m.format, left_out.len(), m.elements.len()), (11, 0, v["elements"].as_object().unwrap().len()));
        }
    }
    assert!(full["elements"].as_object().unwrap().len() > 5);
    // A newer file says what it is and what to do, even when nothing else in it is wrong.
    let mut newer = empty.clone();
    newer["format"] = json!(12);
    let e = Model::read(&newer.to_string()).unwrap_err();
    assert!(
        e.contains("newer BIMCraft") && e.contains("file format 12") && e.contains("reads up to 11") && e.contains("update BIMCraft"),
        "{e}"
    );
    // A file of this format with a field it doesn't know is read without it, and says which.
    let mut odd = full.clone();
    odd["something_new"] = json!(true);
    let (m, left_out) = Model::read(&odd.to_string()).unwrap();
    assert_eq!((m.format, left_out), (11, vec!["/something_new".to_string()]));
}

#[test]
fn a_format_5_file_that_holds_elements_and_scans_migrates_to_11_and_keeps_both() {
    use crate::model::Model;
    let mut s = Session::new();
    house(&mut s);
    run(&mut s, "detail_line", json!({"start": [0, 0], "end": [1000, 0]}));
    let scan = std::env::temp_dir().join(format!("bimcraft-format5-{}.xyz", std::process::id()));
    std::fs::write(&scan, "0 0 0\n1 0 0\n").unwrap();
    run(&mut s, "point_cloud", json!({"path": scan.to_str().unwrap(), "name": "dot"}));
    let _ = std::fs::remove_file(&scan);
    let mut file: serde_json::Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
    file["format"] = json!(5);
    let (m, left_out) = Model::read(&file.to_string()).unwrap();
    assert_eq!((m.format, m.clouds.len(), left_out.len(), m.elements.len()), (11, 1, 0, s.model.elements.len()));
    // Written again, it says 11.
    assert_eq!(serde_json::from_str::<serde_json::Value>(&m.to_json().unwrap()).unwrap()["format"], 11);
    // A file of 6 (detailing, no CAD drawings) opens as 10 likewise.
    file["format"] = json!(6);
    let (m, left_out) = Model::read(&file.to_string()).unwrap();
    assert_eq!((m.format, left_out.len(), m.elements.len()), (11, 0, s.model.elements.len()));
}

#[test]
fn a_file_with_fields_this_bimcraft_does_not_know_opens_without_them_and_says_which() {
    let mut s = Session::new();
    s.script(crate::sample::SAMPLE).unwrap();
    let mut v = serde_json::to_value(&*s.model).unwrap();
    let first = |cat: &str| v["elements"].as_object().unwrap().iter().find(|(_, e)| e["category"] == cat).unwrap().0.clone();
    let (wall, level, schedule) = (first("wall"), first("level"), first("schedule"));
    let family = v["families"].as_object().unwrap().keys().next().unwrap().clone();
    // What a newer BIMCraft might add, at every depth: the project, an element, a struct inside one, a type's layer,
    // a family's form, a material, and a key with a `/` in it.
    let added = [
        "/weather".to_string(),
        format!("/elements/{wall}/finish_date"),
        format!("/elements/{level}/view_range/glow"),
        "/types/wall/Exterior - 300mm/layers/0/u_value".into(),
        format!("/families/{}/forms/0/smoothing", family.replace('/', "~1")),
        "/materials/Brick/roughness".into(),
        format!("/elements/{schedule}/totals~1grand"),
    ];
    for p in &added {
        let (parent, key) = p.rsplit_once('/').unwrap();
        let parent = v.pointer_mut(parent).unwrap_or_else(|| panic!("{parent}"));
        parent[key.replace("~1", "/")] = json!(42);
    }
    let path = std::env::temp_dir().join(format!("bimcraft-newer-fields-{}.bimcraft", std::process::id()));
    std::fs::write(&path, v.to_string()).unwrap();
    let mut t = Session::new();
    let r = t.execute("open", &json!({"path": path.to_str().unwrap()}));
    let _ = std::fs::remove_file(&path);
    let r = r.unwrap();
    let mut said: Vec<String> = serde_json::from_value(r["left_out"].clone()).unwrap();
    let mut want = added.to_vec();
    said.sort();
    want.sort();
    assert_eq!(said, want);
    // The rest is the project as it was.
    assert_eq!(t.model.elements, s.model.elements);
    assert_eq!(t.model.types, s.model.types);
    assert_eq!(t.model.families, s.model.families);
    assert_eq!(t.model.materials, s.model.materials);
    // Commands stay strict: a misspelled key is an error, not something quietly left out.
    let e = t.execute("modify", &json!({"id": wall.parse::<u64>().unwrap(), "heigth": 2500})).unwrap_err();
    assert!(e.contains("unknown field `heigth`"), "{e}");
}

#[test]
fn a_file_with_a_great_many_unknown_fields_lists_the_first_hundred_and_counts_them_all() {
    let mut s = Session::new();
    s.execute("wall", &json!({"points": [[0, 0], [4000, 0]]})).unwrap();
    let mut v = serde_json::to_value(&*s.model).unwrap();
    for i in 0..500 {
        v[format!("made_later_{i}")] = json!(i);
    }
    let path = std::env::temp_dir().join(format!("bimcraft-many-fields-{}.bimcraft", std::process::id()));
    std::fs::write(&path, v.to_string()).unwrap();
    let r = Session::new().execute("open", &json!({"path": path.to_str().unwrap()}));
    let _ = std::fs::remove_file(&path);
    let r = r.unwrap();
    assert_eq!((r["left_out"].as_array().map(Vec::len), r["left_out_count"].as_u64()), (Some(100), Some(500)));
}

#[test]
fn a_project_as_written_reads_back_with_nothing_left_out() {
    let mut s = Session::new();
    s.script(crate::sample::SAMPLE).unwrap();
    run(&mut s, "phase", json!({"name": "Demolition", "before": "New Construction"}));
    run(&mut s, "option", json!({"set": "Entrance", "options": ["Canopy", "Porch"]}));
    run(&mut s, "phase", json!({"overrides": {"new": {"cut": {"color": [0, 0, 200]}}}}));
    let (m, left_out) = crate::model::Model::read(&s.model.to_json().unwrap()).unwrap();
    assert_eq!(left_out, Vec::<String>::new());
    assert_eq!(m, *s.model);
    // And a file that isn't a project at all is still refused.
    assert!(crate::model::Model::read("[1, 2]").is_err());
    assert!(crate::model::Model::read(r#"{"name": "x"}"#).is_err());
}

#[test]
fn each_undo_step_says_what_it_made_or_took_away_and_undo_and_redo_go_back_several_at_once() {
    let mut s = Session::new();
    let walls = house(&mut s);
    run(&mut s, "door", json!({"wall": walls[0], "offset": 2000}));
    run(&mut s, "delete", json!({"ids": [walls[0]]}));
    run(&mut s, "move", json!({"ids": [walls[1]], "by": [0, 100]}));
    // Newest first, as Undo's list shows them.
    let steps = s.steps(false);
    assert_eq!(steps.iter().map(|st| st.label).collect::<Vec<_>>(), ["Move", "Delete", "Door", "Wall"]);
    assert!(steps[0].made.is_empty() && steps[0].removed.is_empty());
    assert_eq!(steps[1].removed, [("wall", 1), ("door", 1)], "by the first of each, in id order");
    assert_eq!((steps[2].made.as_slice(), steps[3].made.as_slice()), (&[("door", 1)][..], &[("wall", 4)][..]));
    assert_eq!(run(&mut s, "undo", json!({"count": 3})), json!({"undone": "Door", "count": 3}));
    assert_eq!((count(&mut s, "wall"), count(&mut s, "door")), (4, 0));
    assert_eq!(s.steps(true).iter().map(|st| st.label).collect::<Vec<_>>(), ["Door", "Delete", "Move"]);
    assert_eq!(run(&mut s, "redo", json!({"count": 2})), json!({"redone": "Delete", "count": 2}));
    assert_eq!((count(&mut s, "wall"), count(&mut s, "door")), (3, 0));
    assert_eq!(run(&mut s, "undo", json!({})), json!({"undone": "Delete", "count": 1}));
    // More than there is, or none, is refused and changes nothing.
    let before = s.model.clone();
    for count in [json!(3), json!(0), json!(-1), json!("two")] {
        assert!(s.execute("undo", &json!({"count": count})).is_err(), "{count}");
    }
    assert!(s.execute("undo", &json!({"steps": 1})).is_err(), "an unknown parameter");
    assert_eq!(s.model, before);
    assert_eq!(s.steps(false).len(), 2);
}

#[test]
fn room_separators_split_rooms_without_walls_and_survive_ifc() {
    let mut s = Session::new();
    house(&mut s);
    let r = run(&mut s, "room", json!({"point": [2000, 3000], "name": "Living"}));
    assert_eq!(r["area_m2"], 48.0);
    // An open plan split into a living room and a kitchen by a line, not a wall.
    let made = run(&mut s, "room_separator", json!({"points": [[5000, 0], [5000, 6000]]}));
    assert_eq!(made["ids"].as_array().map(Vec::len), Some(1));
    let rooms = run(&mut s, "elements", json!({"category": "room"}));
    assert_eq!(rooms["elements"][0]["area_m2"], 30.0);
    assert_eq!(run(&mut s, "room", json!({"point": [7000, 3000], "name": "Kitchen"}))["area_m2"], 18.0);
    let lines = run(&mut s, "elements", json!({"category": "roomseparator"}));
    assert_eq!((lines["count"].as_u64(), lines["elements"][0]["length"].as_f64()), (Some(1), Some(6000.0)));
    // Floors still follow the walls: a separator is no edge of a slab.
    assert_eq!(run(&mut s, "floor", json!({}))["area_m2"], 48.0);
    // A chain makes one line per segment; a point repeated or too few points is an error.
    let chain = run(&mut s, "room_separator", json!({"points": [[0, 3000], [2000, 3000], [2000, 6000]]}));
    assert_eq!(chain["ids"].as_array().map(Vec::len), Some(2));
    run(&mut s, "undo", json!({}));
    for p in [json!({"points": [[0, 0]]}), json!({"points": [[0, 0], [0, 0]]}), json!({"points": [[0, 0], [1000, 0]], "level": "Nowhere"})]
    {
        assert!(s.execute("room_separator", &p).is_err(), "{p}");
    }

    // IFC: an IfcVirtualElement with its axis on the storey, read back as the same line.
    let file = crate::ifc::export(&s.model);
    assert!(file.contains("IFCVIRTUALELEMENT(") && file.contains("'RoomSeparation'"), "{file}");
    let back = crate::import::ifc(file.as_bytes(), "t").unwrap();
    assert_eq!(back.skipped, Vec::<String>::new());
    let lines: Vec<_> = back
        .model
        .elements
        .values()
        .filter_map(|e| match e {
            crate::model::Element::RoomSeparator(rs) => Some((rs.start, rs.end)),
            _ => None,
        })
        .collect();
    assert_eq!(lines, vec![(P2(5000.0, 0.0), P2(5000.0, 6000.0))]);
    let mut t = Session::new();
    t.reset(back.model, None);
    let rooms = run(&mut t, "elements", json!({"category": "room"}));
    let mut areas: Vec<f64> = rooms["elements"].as_array().unwrap().iter().filter_map(|r| r["area_m2"].as_f64()).collect();
    areas.sort_by(f64::total_cmp);
    assert_eq!(areas, vec![18.0, 30.0]);
}

#[test]
fn a_separator_drawn_to_the_wall_faces_splits_rooms_measured_at_the_centre_lines_too() {
    let mut s = Session::new();
    house(&mut s);
    let half = run(&mut s, "elements", json!({"category": "wall"}))["elements"][0]["thickness"].as_f64().unwrap() / 2.0;
    // Drawn from face to face, as on a plan, and one more at a slant from a face to the first line.
    run(&mut s, "room_separator", json!({"points": [[5000, half], [5000, 6000.0 - half]]}));
    run(&mut s, "room_separator", json!({"points": [[8000.0 - half, 4000.0 - half / 3.0], [5000, 3000]]}));
    for (point, name) in [([2000, 3000], "Living"), ([7000, 1000], "Kitchen"), ([7000, 5500], "Pantry")] {
        run(&mut s, "room", json!({"point": point, "name": name}));
    }
    let areas = |s: &mut Session| -> Vec<f64> {
        run(s, "elements", json!({"category": "room"}))["elements"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|r| r["area_m2"].as_f64())
            .collect()
    };
    // At the centre lines: 5 m by 6 m, and the 3 m by 6 m rest cut by the slant from (8000, 4000) to (5000, 3000).
    let centre = areas(&mut s);
    let want = [30.0, 3.0 * 3.5, 3.0 * 2.5];
    assert!(centre.iter().zip(want).all(|(a, w)| (a - w).abs() < 1e-6), "{centre:?}");
    assert!((centre.iter().sum::<f64>() - 48.0).abs() < 1e-6);
    // The lines themselves stay as drawn.
    let lines = run(&mut s, "elements", json!({"category": "roomseparator"}));
    assert_eq!(lines["elements"][0]["length"].as_f64(), Some(6000.0 - 2.0 * half));
    // At the faces they split the same way, and the parts add up to the floor inside the walls.
    run(&mut s, "room_area", json!({"at": "finish"}));
    let inside = (8000.0 - 2.0 * half) * (6000.0 - 2.0 * half) / 1e6;
    assert!((areas(&mut s).iter().sum::<f64>() - inside).abs() < 1e-6, "{:?}", areas(&mut s));
}

#[test]
fn rooms_measure_at_the_wall_centre_lines_or_at_their_faces() {
    let mut s = Session::new();
    house(&mut s);
    let t = run(&mut s, "elements", json!({"category": "wall"}))["elements"][0]["thickness"].as_f64().unwrap();
    run(&mut s, "room", json!({"point": [4000, 3000], "name": "Living"}));
    let area = |s: &mut Session| run(s, "elements", json!({"category": "room"}))["elements"][0]["area_m2"].as_f64().unwrap();
    assert_eq!(run(&mut s, "room_area", json!({}))["at"], "centre", "as a project starts");
    assert_eq!(area(&mut s), 48.0);
    // At the faces: the floor inside the walls, half a wall in from each centre line.
    assert_eq!(run(&mut s, "room_area", json!({"at": "finish"}))["at"], "finish");
    let inside = (8000.0 - t) * (6000.0 - t) / 1e6;
    assert!((area(&mut s) - inside).abs() < 1e-6, "{} vs {inside}", area(&mut s));
    // A partition takes its faces too.
    run(&mut s, "wall", json!({"points": [[5000, 0], [5000, 6000]], "type": "Interior - 100mm"}));
    let left = (5000.0 - t / 2.0 - 50.0) * (6000.0 - t) / 1e6;
    assert!((area(&mut s) - left).abs() < 1e-6, "{} vs {left}", area(&mut s));
    // It is kept with the project and undone like any change.
    let file = serde_json::to_value(&*s.model).unwrap();
    assert_eq!(file["room_area"], "finish");
    assert!(s.execute("room_area", &json!({"at": "core"})).unwrap_err().contains("centre or finish"));
    assert!(s.execute("room_area", &json!({"to": "finish"})).is_err());

    // IFC: the space follows the setting; its quantities give both, gross at the centre lines and net at the faces.
    let ifc = crate::ifc::export(&s.model);
    assert!(ifc.contains("'Qto_SpaceBaseQuantities'"), "{ifc}");
    let q = |name: &str| -> f64 {
        let at = ifc.find(&format!("IFCQUANTITYAREA('{name}',$,$,")).unwrap_or_else(|| panic!("no {name}"));
        let rest = &ifc[at..];
        let v = rest.split(',').nth(3).unwrap();
        v.parse().unwrap()
    };
    assert!((q("GrossFloorArea") - 30.0).abs() < 1e-6);
    assert!((q("NetFloorArea") - left).abs() < 1e-4);
    assert!(ifc.contains("IFCQUANTITYLENGTH('Height',$,$,3000.,$)"), "{ifc}");

    run(&mut s, "undo", json!({}));
    run(&mut s, "undo", json!({}));
    assert_eq!(area(&mut s), 48.0, "back at the centre lines");
    assert!(serde_json::to_value(&*s.model).unwrap().get("room_area").is_none(), "a project at the centre lines doesn't say so");
}

#[test]
fn rooms_measured_at_the_faces_stop_at_room_separators_too() {
    let mut s = Session::new();
    house(&mut s);
    let t = run(&mut s, "elements", json!({"category": "wall"}))["elements"][0]["thickness"].as_f64().unwrap();
    run(&mut s, "room_area", json!({"at": "finish"}));
    run(&mut s, "room_separator", json!({"points": [[5000, 0], [5000, 6000]]}));
    let living = run(&mut s, "room", json!({"point": [2000, 3000], "name": "Living"}))["area_m2"].as_f64().unwrap();
    let kitchen = run(&mut s, "room", json!({"point": [7000, 3000], "name": "Kitchen"}))["area_m2"].as_f64().unwrap();
    // The line has no thickness: the two share the floor inside the walls between them.
    let (left, right) = ((5000.0 - t / 2.0) * (6000.0 - t) / 1e6, (3000.0 - t / 2.0) * (6000.0 - t) / 1e6);
    assert!((living - left).abs() < 1e-6 && (kitchen - right).abs() < 1e-6, "{living} {kitchen} vs {left} {right}");
}

#[test]
fn technical_standards_configure_line_weights_sheets_and_styles() {
    let mut s = Session::new();
    house(&mut s);

    // Initial inspection reports current standard
    let r = run(&mut s, "standard", json!({}));
    assert_eq!(r["standard"], "iso");
    assert!(r["all"].as_array().unwrap().len() >= 4);

    // Switch to NBR (ABNT / Brasil)
    let nbr = run(&mut s, "standard", json!({"set": "nbr", "adapt_all": true}));
    assert_eq!(nbr["standard"], "nbr");
    assert_eq!(nbr["region"], "Brasil");
    assert_eq!(s.model.standard(), crate::standards::TechnicalStandard::Nbr);
    assert_eq!(s.history(), ["Wall", "Technical Standards"]);

    // Verify object styles adapted to NBR 6492
    let wall_style = s.model.object_style("wall");
    assert_eq!(wall_style.cut, crate::pen::Pen(10), "Cut wall is 0.50mm in NBR 6492");
    let col_style = s.model.object_style("column");
    assert_eq!(col_style.cut, crate::pen::Pen(12), "Cut structural column is 0.70mm in NBR 6492");

    // Verify sheet margins follow NBR 16752 (25mm binding margin) and folding marks
    let margins_a1 = s.model.standard().sheet_margins("A1");
    assert_eq!(margins_a1.left, 25.0);
    assert_eq!(margins_a1.title_width, 178.0);
    assert!(margins_a1.folding_marks);

    // Sheets can be created in both ISO and Arch/ANSI formats
    let sh_a1 = run(&mut s, "sheet", json!({"name": "Planta Baixa", "size": "A1"}))["id"].as_u64().unwrap();
    assert_eq!(s.model.sheet(sh_a1).unwrap().size, "A1");

    // Switch to ANSI (United States)
    let ansi = run(&mut s, "standard", json!({"set": "ansi", "adapt_all": true}));
    assert_eq!(ansi["standard"], "ansi");
    assert_eq!(s.model.standard(), crate::standards::TechnicalStandard::Ansi);
    assert_eq!(s.model.units, crate::units::Units::FeetInches, "ANSI adapts units to Imperial");

    // Creating US Architectural sheet Arch D
    let sh_arch_d = run(&mut s, "sheet", json!({"name": "Floor Plans", "size": "ARCH D"}))["id"].as_u64().unwrap();
    assert_eq!(s.model.sheet(sh_arch_d).unwrap().size, "ARCH D");

    // Drawing sheet works with both formats
    let drawing = crate::sheet::draw(&s.model, sh_a1);
    assert!(drawing.is_some());
    let drawing_d = crate::sheet::draw(&s.model, sh_arch_d);
    assert!(drawing_d.is_some());

    // Undoing sheet then standard reverts back to NBR
    run(&mut s, "undo", json!({}));
    run(&mut s, "undo", json!({}));
    assert_eq!(s.model.standard(), crate::standards::TechnicalStandard::Nbr);
}

#[test]
fn make_fill_pattern_makes_a_pattern_of_the_detail_lines_picked_cut_to_the_tile_model_or_drafting_at_the_plans_scale() {
    let mut s = Session::new();
    house(&mut s);
    // A brick of 200 x 100 drawn in the plan: its bed joint, its head joint, and a line that hangs out of the tile.
    let bed = run(&mut s, "detail_line", json!({"start": [0, 0], "end": [200, 0]}))["id"].clone();
    let head = run(&mut s, "detail_line", json!({"start": [0, 0], "end": [0, 300]}))["id"].clone();
    let made = run(&mut s, "make_fill_pattern", json!({"ids": [bed, head], "from": [0, 0], "to": [200, 100], "name": "Brick"}));
    let p = &s.model.fill_patterns["Brick"];
    assert_eq!((made["name"].as_str(), p.target), (Some("Brick"), crate::fills::Target::Model));
    assert_eq!(p.lines.len(), 2);
    assert!(p.lines.iter().all(|l| l.dashes.is_empty()), "both cross the tile whole once the head joint is cut: {p:?}");
    // A drafting one is the drawing on paper: the plan's 1 : 100 makes the 100 mm course 1 mm.
    let view = s.model.elements.values().find_map(|e| if let crate::model::Element::DetailLine(l) = e { Some(l.view) } else { None });
    let scale = crate::graphics::scale_of(&s.model, view.unwrap());
    assert!(scale > 1.0);
    run(&mut s, "make_fill_pattern", json!({"ids": [bed], "from": [0, 0], "to": [200, 100], "name": "Brick paper", "target": "drafting"}));
    let paper = &s.model.fill_patterns["Brick paper"];
    assert_eq!(paper.target, crate::fills::Target::Drafting);
    assert!((paper.lines[0].spacing - 100.0 / scale).abs() < 1e-9, "{paper:?}");
    // A name in use, a wall, nothing inside the tile: refused, and the project as it was.
    for (p, why) in [
        (json!({"ids": [bed], "from": [0, 0], "to": [200, 100], "name": "Brick"}), "already"),
        (json!({"ids": [bed], "from": [5000, 5000], "to": [5200, 5100], "name": "None"}), "no line"),
    ] {
        let e = s.execute("make_fill_pattern", &p).unwrap_err();
        assert!(e.contains(why), "{e}");
    }
    let wall = s.model.elements.iter().find(|(_, e)| matches!(e, crate::model::Element::Wall(_))).map(|(id, _)| *id).unwrap();
    let e = s.execute("make_fill_pattern", &json!({"ids": [wall], "from": [0, 0], "to": [200, 100], "name": "W"})).unwrap_err();
    assert!(e.contains("not a detail line"), "{e}");
}
