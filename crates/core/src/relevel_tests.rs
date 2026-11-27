//! Change Base Level (`change_level`): an element put on another level stays where it stands.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::Element;

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

fn id(v: &Value) -> u64 {
    v["id"].as_u64().or_else(|| v["ids"][0].as_u64()).unwrap_or_else(|| panic!("no id in {v}"))
}

/// Level 1 at 0, Level 2 at 3000 and a third at 6000.
fn levels(s: &mut Session) -> (u64, u64) {
    let two = run(s, "level", json!({"elevation": 3000, "name": "Two"}));
    let three = run(s, "level", json!({"elevation": 6000, "name": "Three"}));
    (id(&two), id(&three))
}

#[test]
fn a_wall_put_on_the_level_above_keeps_its_base_and_top_where_they_were() {
    let mut s = Session::new();
    let (two, three) = levels(&mut s);
    let w = id(&run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]], "height": 2500, "base_offset": 200})));
    let before = s.model.elements.get(w).cloned().unwrap();
    let Element::Wall(old) = &before else { panic!() };
    let (bottom, top) = (s.model.base_z(old), s.model.top_z(old));
    let r = run(&mut s, "change_level", json!({"ids": [w], "level": "Two"}));
    assert_eq!(r["changed"], json!([w]));
    let Some(Element::Wall(new)) = s.model.elements.get(w).cloned() else { panic!() };
    assert_eq!(new.level, s.model.guid(two));
    assert_eq!(new.base_offset, -2800.0);
    assert_eq!((s.model.base_z(&new), s.model.top_z(&new)), (bottom, top));
    // A wall whose top is tied to a level keeps it tied there.
    let tied = id(&run(&mut s, "wall", json!({"points": [[0, 2000], [4000, 2000]], "top_level": three})));
    let Some(Element::Wall(old)) = s.model.elements.get(tied).cloned() else { panic!() };
    let (bottom, top) = (s.model.base_z(&old), s.model.top_z(&old));
    run(&mut s, "change_level", json!({"ids": [tied], "level": two}));
    let Some(Element::Wall(new)) = s.model.elements.get(tied).cloned() else { panic!() };
    assert_eq!((new.top_level, s.model.base_z(&new), s.model.top_z(&new)), (old.top_level, bottom, top));
    // One undo step puts it back.
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(s.model.elements.get(tied), Some(&Element::Wall(old)));
}

#[test]
fn floors_components_columns_runs_and_stairs_stay_where_they_stand() {
    let mut s = Session::new();
    let (two, _) = levels(&mut s);
    let floor = id(&run(&mut s, "floor", json!({"boundary": [[0, 0], [3000, 0], [3000, 3000], [0, 3000]]})));
    let column = id(&run(&mut s, "column", json!({"at": [500, 500]})));
    let beam = id(&run(&mut s, "beam", json!({"points": [[0, 0], [3000, 0]], "level": two, "offset": -200})));
    let stair = id(&run(&mut s, "stair", json!({"start": [1000, 1000], "angle": 0})));
    let ids = [floor, column, beam, stair];
    // How low and how high what it builds reaches, from the 3D view's triangles.
    let z = |s: &Session, id: u64| -> (f64, f64) {
        let scene = crate::view3d::Scene::new(&s.model);
        let (lo, hi) =
            scene.boxes(&std::collections::HashSet::from([id])).get(&id).copied().unwrap_or_else(|| panic!("{id} builds nothing"));
        (lo[2], hi[2])
    };
    let before: Vec<(f64, f64)> = ids.iter().map(|i| z(&s, *i)).collect();
    // Up to Two, and the beam down to Level 1: each where it was.
    run(&mut s, "change_level", json!({"ids": [floor, column, stair], "level": two}));
    run(&mut s, "change_level", json!({"ids": [beam], "level": "Level 1"}));
    for (i, id) in ids.iter().enumerate() {
        let (lo, hi) = z(&s, *id);
        assert!((lo - before[i].0).abs() < 1e-6 && (hi - before[i].1).abs() < 1e-6, "{id}: {:?} then {:?}", before[i], (lo, hi));
    }
    let level_of = |s: &Session, id: u64| s.model.elements.get(id).and_then(|e| s.model.level_of(e));
    assert_eq!(level_of(&s, floor), Some(s.model.guid(two)));
    assert_eq!(level_of(&s, beam), s.model.levels().first().map(|(l, _)| s.model.guid(*l)));
}

#[test]
fn doors_hosted_things_and_a_missing_level_are_refused_and_change_nothing() {
    let mut s = Session::new();
    levels(&mut s);
    let wall = id(&run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]]})));
    let door = id(&run(&mut s, "door", json!({"at": [2000, 0]})));
    let before = s.model.clone();
    for (p, says) in [
        (json!({"ids": [wall, door], "level": "Two"}), "stands on its wall's level"),
        (json!({"ids": [wall], "level": "Nowhere"}), "Nowhere"),
        (json!({"ids": [wall]}), "`level`"),
        (json!({"ids": [wall], "level": "Two", "base": 0}), "base"),
    ] {
        let e = s.execute("change_level", &p).unwrap_err();
        assert!(e.contains(says), "{p}: {e}");
    }
    assert_eq!(s.model.elements, before.elements);
    assert!(crate::relevel::can_change_level(&s.model, &[wall]));
    assert!(!crate::relevel::can_change_level(&s.model, &[wall, door]));
    assert!(!crate::relevel::can_change_level(&s.model, &[]));
    // Pinned: unpin it first.
    run(&mut s, "pin", json!({"ids": [wall]}));
    assert!(s.execute("change_level", &json!({"ids": [wall], "level": "Two"})).unwrap_err().contains("pinned"));
}

fn base_z(s: &Session, wall: u64) -> f64 {
    let w = s.model.wall(wall).unwrap();
    s.model.base_z(w)
}

#[test]
fn move_and_copy_raise_what_they_move_by_a_dz_and_levels_go_up_and_down_only() {
    let mut s = Session::new();
    let (two, _) = levels(&mut s);
    let wall = id(&run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]], "height": 2500})));
    let door = id(&run(&mut s, "door", json!({"at": [2000, 0]})));
    let other = id(&run(&mut s, "wall", json!({"points": [[0, 3000], [4000, 3000]], "height": 2500})));
    let window = id(&run(&mut s, "window", json!({"at": [2000, 3000], "sill": 900})));
    let sill = |s: &Session, id: u64| match s.model.elements.get(id) {
        Some(Element::Door(o) | Element::Window(o)) => o.sill,
        _ => panic!("{id} is no opening"),
    };
    // Up 500 and across 100: the wall's base and top, and its door with it, on its sill.
    run(&mut s, "move", json!({"ids": [wall, door], "by": [100, 0, 500]}));
    assert_eq!(base_z(&s, wall), 500.0);
    assert_eq!(s.model.wall(wall).unwrap().start, crate::geom::P2(100.0, 0.0));
    assert_eq!(sill(&s, door), 0.0, "on its wall as it was");
    // A window alone goes up its wall.
    run(&mut s, "move", json!({"ids": [window], "by": [0, 0, 300]}));
    assert_eq!(sill(&s, window), 1200.0);
    // A level goes up, and what stands on it with it; never across.
    let lvl1 = s.model.levels()[0].0;
    run(&mut s, "move", json!({"ids": [lvl1], "by": [0, 0, -200]}));
    assert_eq!((s.model.elevation(s.model.guid(lvl1)), base_z(&s, other)), (-200.0, -200.0));
    assert!(s.execute("move", &json!({"ids": [lvl1], "by": [10, 0, 0]})).unwrap_err().contains("only up or down"));
    // Copied up a storey: a copy of the wall 3000 higher; a level copied is a new level, named after it.
    let made = run(&mut s, "copy", json!({"ids": [other], "by": [0, 0, 3000]}));
    let copy = made["ids"][0].as_u64().unwrap();
    assert_eq!(base_z(&s, copy), base_z(&s, other) + 3000.0);
    let made = run(&mut s, "copy", json!({"ids": [two], "by": [0, 0, 6000]}));
    let new = made["ids"][0].as_u64().unwrap();
    let Some(Element::Level(l)) = s.model.elements.get(new) else { panic!("{made}") };
    assert_eq!((l.name.as_str(), l.elevation), ("Two 2", 9000.0));
    assert!(s.execute("copy", &json!({"ids": [two], "by": [0, 0]})).unwrap_err().contains("only up or down"));
    // Pinned: unpin it first.
    run(&mut s, "pin", json!({"ids": [other]}));
    assert!(s.execute("move", &json!({"ids": [other], "by": [0, 0, 10]})).unwrap_err().contains("pinned"));
}
