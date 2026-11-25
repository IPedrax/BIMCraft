//! The `renovation` command: the status of the work (existing, demolished, new) what a session draws is in, and the one it
//! gives elements, as a renovation palette sets them, over the project's phases.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::graphics::testing::{house, run};
use crate::model::Element;

/// The parameters an element holds, null when none.
fn params(s: &Session, id: u64) -> Value {
    s.model.elements.get(id).and_then(Element::params).filter(|p| !p.is_empty()).map_or(Value::Null, |p| json!(p))
}

fn id(v: &Value) -> u64 {
    v["id"].as_u64().or_else(|| v["ids"][0].as_u64()).unwrap_or_default()
}

#[test]
fn a_wall_drawn_to_be_demolished_stands_in_the_existing_phase_and_comes_down_in_the_new() {
    let mut s = Session::new();
    let r = run(&mut s, "renovation", json!({"status": "demolished"}));
    assert_eq!(r["status"], "demolished");
    assert_eq!((r["existing_phase"].clone(), r["new_phase"].clone()), (json!("Existing"), json!("New Construction")));
    let wall = id(&run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]]})));
    // Made in the first phase, which holds no value, and taken down in the new one.
    assert_eq!(params(&s, wall), json!({"Phase Demolished": "New Construction"}));
    // A door drawn meanwhile names its phase, or it would follow its wall.
    let door = id(&run(&mut s, "door", json!({"wall": wall, "offset": 1000})));
    assert_eq!(params(&s, door), json!({"Phase Created": "Existing", "Phase Demolished": "New Construction"}));
    assert_eq!(run(&mut s, "renovation", json!({}))["status"], "demolished");

    run(&mut s, "renovation", json!({"status": "new"}));
    let new = id(&run(&mut s, "wall", json!({"points": [[0, 2000], [4000, 2000]]})));
    assert_eq!(params(&s, new), json!({"Phase Created": "New Construction"}));
    run(&mut s, "renovation", json!({"status": "existing"}));
    let old = id(&run(&mut s, "wall", json!({"points": [[0, 4000], [4000, 4000]]})));
    assert_eq!(params(&s, old), Value::Null);
    // Following the view again: the session makes things in no phase of its own.
    let r = run(&mut s, "renovation", json!({"status": null}));
    assert_eq!((r["status"].clone(), r["now"].clone(), r["demolish"].clone()), (Value::Null, Value::Null, Value::Null));
    // Choosing a phase with `phase {now}` leaves the demolition behind.
    run(&mut s, "renovation", json!({"status": "demolished"}));
    run(&mut s, "phase", json!({"now": "New Construction"}));
    assert_eq!(run(&mut s, "renovation", json!({}))["status"], "new");
}

#[test]
fn with_more_phases_new_is_the_phase_given_and_existing_the_one_before() {
    let mut s = Session::new();
    run(&mut s, "phase", json!({"name": "Phase 3"}));
    let r = run(&mut s, "renovation", json!({"status": "demolished", "phase": "New Construction"}));
    assert_eq!((r["existing_phase"].clone(), r["new_phase"].clone()), (json!("Existing"), json!("New Construction")));
    let wall = id(&run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]]})));
    assert_eq!(params(&s, wall), json!({"Phase Demolished": "New Construction"}));
    // With no phase, the project's last.
    let r = run(&mut s, "renovation", json!({"status": "new"}));
    assert_eq!((r["existing_phase"].clone(), r["new_phase"].clone()), (json!("New Construction"), json!("Phase 3")));
    let e = s.execute("renovation", &json!({"status": "new", "phase": "Existing"})).unwrap_err();
    assert!(e.contains("Existing") && e.contains("before"), "{e}");
    let e = s.execute("renovation", &json!({"phase": "Nowhere"})).unwrap_err();
    assert!(e.contains("Nowhere"), "{e}");
}

#[test]
fn new_given_to_a_wall_its_door_and_its_window_is_one_step_and_undo_takes_it_back() {
    let mut s = Session::new();
    let walls = house(&mut s);
    let door = id(&run(&mut s, "door", json!({"wall": walls[0], "offset": 1000})));
    let window = id(&run(&mut s, "window", json!({"wall": walls[0], "offset": 4000})));
    let r = run(&mut s, "renovation", json!({"ids": [walls[0], door, window]}));
    assert_eq!(r["selection"], "existing");
    assert_eq!(r["elements"][1], json!({"id": door, "status": "existing"}));

    let r = run(&mut s, "renovation", json!({"status": "new", "ids": [walls[0], door, window]}));
    assert_eq!(r["selection"], "new");
    for e in [walls[0], door, window] {
        assert_eq!(params(&s, e), json!({"Phase Created": "New Construction"}), "{e}");
    }
    // Giving a status to elements leaves what the session draws in as it was.
    assert_eq!(r["status"], Value::Null);
    let r = run(&mut s, "renovation", json!({"status": "demolished", "ids": [walls[1]]}));
    assert_eq!(params(&s, walls[1]), json!({"Phase Demolished": "New Construction"}));
    assert_eq!(r["selection"], "demolished");
    assert_eq!(run(&mut s, "renovation", json!({"ids": [walls[0], walls[1]]}))["selection"], "mixed");

    run(&mut s, "undo", json!({}));
    run(&mut s, "undo", json!({}));
    for e in [walls[0], walls[1]] {
        assert_eq!(params(&s, e), Value::Null, "{e}");
    }
    assert_eq!(params(&s, door), Value::Null, "the door follows its wall again");
    // Existing given back takes the demolition away.
    run(&mut s, "renovation", json!({"status": "demolished", "ids": [walls[2]]}));
    run(&mut s, "renovation", json!({"status": "existing", "ids": [walls[2]]}));
    assert_eq!(params(&s, walls[2]), Value::Null);
}

#[test]
fn an_element_with_no_phase_or_no_status_is_refused_and_nothing_changes() {
    let mut s = Session::new();
    let walls = house(&mut s);
    let e = s.execute("renovation", &json!({"status": null, "ids": [walls[0]]})).unwrap_err();
    assert!(e.contains("status"), "{e}");
    let e = s.execute("renovation", &json!({"status": "rubble"})).unwrap_err();
    assert!(e.contains("rubble") && e.contains("existing"), "{e}");
    // A level holds no phase.
    let e = s.execute("renovation", &json!({"status": "new", "ids": [walls[0], 100000]})).unwrap_err();
    assert!(e.contains("100000"), "{e}");
    assert_eq!(params(&s, walls[0]), Value::Null);
}

#[test]
fn a_project_of_one_phase_has_no_renovation() {
    let mut s = Session::new();
    run(&mut s, "phase", json!({"name": "Existing", "delete": true}));
    for p in [json!({}), json!({"status": "new"}), json!({"status": null})] {
        let e = s.execute("renovation", &p).unwrap_err();
        assert!(e.contains("one phase") && e.contains("New Construction"), "{p}: {e}");
    }
}

#[test]
fn the_phase_a_session_demolishes_in_follows_a_phase_renamed_and_holds_it_against_deleting() {
    let mut s = Session::new();
    run(&mut s, "phase", json!({"name": "Phase 3"}));
    run(&mut s, "renovation", json!({"status": "demolished"}));
    run(&mut s, "phase", json!({"name": "Phase 3", "rename": "Works"}));
    assert_eq!(run(&mut s, "renovation", json!({}))["demolish"], "Works");
    let e = s.execute("phase", &json!({"name": "Works", "delete": true})).unwrap_err();
    assert!(e.contains("in use"), "{e}");
    // A refused command leaves the session drawing as it was.
    assert!(s.execute("renovation", &json!({"status": "new", "ids": [999]})).is_err());
    assert_eq!(run(&mut s, "renovation", json!({}))["status"], "demolished");
}
