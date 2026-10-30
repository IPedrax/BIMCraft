//! Electrical circuits: a set of devices assigned to a distribution board, with a number, a breaker, poles, a voltage and a
//! classification, through the commands every front end drives. A device's voltage, poles and load are its family's connector's.

use serde_json::{Value, json};

use crate::element_json;
use crate::engine::Session;
use crate::model::{Element, Id};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn fails(s: &mut Session, id: &str, p: Value) -> String {
    let before = s.model.clone();
    let e = s.execute(id, &p).expect_err(&format!("{id} {p} should fail"));
    assert_eq!(*s.model, *before, "a refused {id} changes nothing");
    e
}

fn place(s: &mut Session, family: &str, at: [i32; 2]) -> Id {
    run(s, "component", json!({"family": family, "at": at}))["id"].as_u64().unwrap()
}

/// A distribution board on `system` (127/220 V unless said), taking 24 single-pole breakers and 63 A.
fn board_on(s: &mut Session, system: &str) -> Id {
    let b = place(s, "Distribution Board", [0, 0]);
    run(s, "panel", json!({"id": b, "distribution": system, "mains": 63, "slots": 24}));
    b
}

fn board(s: &mut Session) -> Id {
    board_on(s, "127/220 V")
}

/// Ceiling luminaires of 12 VA each (127 V, one pole), placed in a row.
fn lamps(s: &mut Session, n: i32) -> Vec<Id> {
    (0..n).map(|i| place(s, "Ceiling Luminaire", [1000 + 1000 * i, 1000])).collect()
}

fn circuits(s: &Session) -> Vec<Id> {
    s.model.elements.iter().filter(|(_, e)| matches!(e, Element::Circuit(_))).map(|(id, _)| *id).collect()
}

fn make(s: &mut Session, p: Value) -> Id {
    run(s, "circuit", p)["id"].as_u64().unwrap()
}

#[test]
fn a_circuit_assigns_devices_to_a_board_and_carries_a_number_a_breaker_poles_and_a_voltage() {
    let mut s = Session::new();
    let (b, d) = (board(&mut s), lamps(&mut s, 3));
    let c = make(&mut s, json!({"devices": d, "panel": b, "name": "Iluminação sala", "rating": 10}));
    let e = element_json(&s.model, c);
    assert_eq!(e["category"], json!("circuit"));
    assert_eq!(
        (e["panel_id"].as_u64(), e["number"].as_u64(), e["rating_a"].as_f64(), e["poles"].as_u64()),
        (Some(b), Some(1), Some(10.0), Some(1))
    );
    assert_eq!(e["name"], json!("Iluminação sala"));
    // What the poles imply: the system's line to neutral. And what its devices say they are for.
    assert_eq!(
        (e["voltage_name"].as_str(), e["voltage_v"].as_f64(), e["classification"].as_str()),
        (Some("127 V"), Some(127.0), Some("lighting"))
    );
    assert_eq!(e["device_ids"], json!(d));
    assert_eq!(e["demand_factor"], json!(1.0));
    // Its load is its devices': 3 × 12 VA at 127 V.
    assert_eq!((e["installed_va"].as_f64(), e["demand_va"].as_f64()), (Some(36.0), Some(36.0)));
    assert!((e["current_a"].as_f64().unwrap() - 36.0 / 127.0).abs() < 1e-3, "{e}");
    assert_eq!(e["pole_va"], json!([36.0]));
    // The next circuit is the next number, and a number is a board's own.
    let c2 = make(&mut s, json!({"devices": [], "panel": b}));
    assert_eq!(element_json(&s.model, c2)["number"], json!(2));
    let other = board(&mut s);
    let c3 = make(&mut s, json!({"devices": [], "panel": other}));
    assert_eq!(element_json(&s.model, c3)["number"], json!(1));
}

#[test]
fn a_breaker_is_chosen_from_the_load_when_none_is_given() {
    let mut s = Session::new();
    let b = board(&mut s);
    let outlets: Vec<Id> = (0..30).map(|i| place(&mut s, "Outlet", [1000 + 200 * i, 2000])).collect();
    // 30 outlets of 100 VA at 127 V draw 23.6 A: the next standard breaker up is 25 A.
    let c = make(&mut s, json!({"devices": outlets, "panel": b}));
    assert_eq!(element_json(&s.model, c)["rating_a"], json!(25.0));
    // None at all: a 16 A circuit to start from.
    let spare = make(&mut s, json!({"devices": [], "panel": b}));
    assert_eq!(element_json(&s.model, spare)["rating_a"], json!(16.0));
}

#[test]
fn a_device_is_in_one_circuit_and_the_second_assignment_is_refused() {
    let mut s = Session::new();
    let (b, d) = (board(&mut s), lamps(&mut s, 3));
    let first = make(&mut s, json!({"devices": [d[0], d[1]], "panel": b}));
    let e = fails(&mut s, "circuit", json!({"devices": [d[1], d[2]], "panel": b}));
    assert!(e.contains(&d[1].to_string()) && e.contains(&first.to_string()), "it names the device and the circuit that has it: {e}");
    // Adding it to another is refused the same way; and nothing moved.
    let second = make(&mut s, json!({"devices": [d[2]], "panel": b}));
    let e = fails(&mut s, "circuit", json!({"id": second, "add": [d[0]]}));
    assert!(e.contains(&d[0].to_string()) && e.contains(&first.to_string()), "{e}");
    // Taken out of the first, it may go into the second.
    run(&mut s, "circuit", json!({"id": first, "remove": [d[0]]}));
    run(&mut s, "circuit", json!({"id": second, "add": [d[0]]}));
    assert_eq!(element_json(&s.model, second)["device_ids"], json!([d[2], d[0]]));
    // Twice in one list is no better.
    let e = fails(&mut s, "circuit", json!({"devices": [d[0], d[0]], "panel": b}));
    assert!(e.contains("twice") || e.contains(&d[0].to_string()), "{e}");
}

#[test]
fn a_voltage_the_board_does_not_supply_is_refused_naming_both_voltages_and_nothing_changes() {
    let mut s = Session::new();
    // The starter luminaires draw 127 V; this board's single-pole circuits are 230 V (207 to 253 V).
    let (b, d) = (board_on(&mut s, "230/400 V"), lamps(&mut s, 2));
    let e = fails(&mut s, "circuit", json!({"devices": d, "panel": b}));
    for says in ["127 V", "230 V", "230/400 V", &d[0].to_string(), &b.to_string()] {
        assert!(e.contains(says), "it names {says}: {e}");
    }
    assert!(circuits(&s).is_empty());
    // The board's band is the voltage's own: 253 V is in 230 V's, and a device at 220 V is not.
    let family = run(
        &mut s,
        "family",
        json!({"name": "Ceiling Luminaire", "connectors": [{"kind": "power", "system": "power-balanced", "poles": 1, "voltage": 230, "load_va": 12, "power_factor": 0.95, "classification": "lighting"}]}),
    );
    assert_eq!(family["name"], json!("Ceiling Luminaire"));
    let ok = make(&mut s, json!({"devices": d, "panel": b}));
    assert_eq!(element_json(&s.model, ok)["voltage_v"], json!(230.0));
}

#[test]
fn poles_follow_the_devices_and_a_voltage_is_one_the_poles_can_have() {
    let mut s = Session::new();
    let b = board(&mut s);
    let (lamp, three) = (lamps(&mut s, 1)[0], place(&mut s, "Three-Phase Outlet", [4000, 2000]));
    // A three-pole device makes a three-pole circuit, at the system's line-to-line voltage: 220 V.
    let c = make(&mut s, json!({"devices": [three], "panel": b, "rating": 20}));
    let e = element_json(&s.model, c);
    assert_eq!((e["poles"].as_u64(), e["voltage_v"].as_f64(), e["voltage_name"].as_str()), (Some(3), Some(220.0), Some("220 V")));
    // 6000 VA over three phases at 220 V: 6000 / (√3 × 220) = 15.7 A, and 2000 VA a pole.
    assert!((e["current_a"].as_f64().unwrap() - 15.746).abs() < 0.01, "{e}");
    assert_eq!(e["pole_va"], json!([2000.0, 2000.0, 2000.0]));
    // A one-pole device on it, or a three-pole one on a one-pole circuit, is refused.
    let (lamp2, three2) = (lamps(&mut s, 1)[0], place(&mut s, "Three-Phase Outlet", [5000, 2000]));
    let err = fails(&mut s, "circuit", json!({"id": c, "add": [lamp2]}));
    assert!(err.contains("pole"), "{err}");
    let err = fails(&mut s, "circuit", json!({"devices": [three2], "panel": b, "poles": 1}));
    assert!(err.contains("pole"), "{err}");
    // Mixed devices without saying: refused, naming the poles of each.
    let err = fails(&mut s, "circuit", json!({"devices": [lamp2, three2], "panel": b}));
    assert!(err.contains("pole") && err.contains('1') && err.contains('3'), "{err}");
    let _ = lamp;
    // A circuit of more poles than the system has phases is refused, naming the system.
    let single = run(&mut s, "distribution_system", json!({"name": "Mono", "phases": 1, "wires": 2, "l_n": "127 V"}));
    assert_eq!(single["name"], json!("Mono"));
    let mono = board_on(&mut s, "Mono");
    let err = fails(&mut s, "circuit", json!({"devices": [], "panel": mono, "poles": 3}));
    assert!(err.contains("Mono") && err.contains("phase"), "{err}");
    // Changing a board's system under circuits it cannot hold is refused too, and nothing changes.
    let err = fails(&mut s, "panel", json!({"id": b, "distribution": "Mono"}));
    assert!(err.contains(&c.to_string()), "{err}");
}

#[test]
fn a_breaker_poles_and_slots_are_positive_and_fit_the_board() {
    let mut s = Session::new();
    let (b, d) = (board(&mut s), lamps(&mut s, 2));
    for (p, says) in [
        (json!({"devices": [], "panel": b, "rating": 0}), "rating"),
        (json!({"devices": [], "panel": b, "rating": -16}), "rating"),
        (json!({"devices": [], "panel": b, "rating": 1e300}), "rating"),
        (json!({"devices": [], "panel": b, "rating": "big"}), "rating"),
        (json!({"devices": [], "panel": b, "rating": 80}), "mains"),
        (json!({"devices": [], "panel": b, "poles": 0}), "pole"),
        (json!({"devices": [], "panel": b, "poles": 4}), "pole"),
        (json!({"devices": [], "panel": b, "poles": 1.5}), "pole"),
        (json!({"devices": [], "panel": b, "number": 0}), "number"),
        (json!({"devices": [], "panel": b, "demand_factor": 0}), "demand"),
        (json!({"devices": [], "panel": b, "demand_factor": 1.5}), "demand"),
        (json!({"devices": [], "panel": b, "name": "x".repeat(300)}), "name"),
        (json!({"devices": [], "panel": b, "classification": "x".repeat(300)}), "classification"),
        (json!({"devices": [], "panel": b, "volts": 127}), "volts"),
    ] {
        let e = fails(&mut s, "circuit", p.clone());
        assert!(e.to_lowercase().contains(says), "{p}: {e}");
    }
    // The board takes 24 single-pole breakers: one circuit more is refused, saying so.
    for _ in 0..24 {
        make(&mut s, json!({"devices": [], "panel": b}));
    }
    let e = fails(&mut s, "circuit", json!({"devices": [d[0]], "panel": b}));
    assert!(e.contains("24") && e.contains(&b.to_string()), "{e}");
    // A number a board has is not given again.
    let first = circuits(&s)[0];
    let e = fails(&mut s, "circuit", json!({"id": first, "number": 2}));
    assert!(e.contains('2') && e.contains(&b.to_string()), "{e}");
}

#[test]
fn only_a_device_with_a_power_connector_is_a_member_and_only_a_board_a_panel() {
    let mut s = Session::new();
    let (b, d) = (board(&mut s), lamps(&mut s, 1));
    let switch = place(&mut s, "Switch", [3000, 3000]);
    let wall = run(&mut s, "wall", json!({"points": [[0, 5000], [4000, 5000]]}))["ids"][0].as_u64().unwrap();
    let e = fails(&mut s, "circuit", json!({"devices": [switch], "panel": b}));
    assert!(e.contains(&switch.to_string()) && e.contains("power connector"), "{e}");
    let e = fails(&mut s, "circuit", json!({"devices": [wall], "panel": b}));
    assert!(e.contains(&wall.to_string()), "{e}");
    let e = fails(&mut s, "circuit", json!({"devices": [b], "panel": b}));
    assert!(e.contains(&b.to_string()), "{e}");
    for (p, says) in [
        (json!({"devices": d.clone(), "panel": d[0]}), "board"),
        (json!({"devices": d.clone(), "panel": wall}), "board"),
        (json!({"devices": d.clone(), "panel": 999_999}), "999999"),
        (json!({"devices": [999_999], "panel": b}), "999999"),
        (json!({"devices": d.clone()}), "panel"),
        (json!({"panel": b, "name": "no devices said"}), "devices"),
    ] {
        let e = fails(&mut s, "circuit", p.clone());
        assert!(e.contains(says), "{p}: {e}");
    }
    // A board that has no distribution system has no voltage to give a circuit.
    let bare = place(&mut s, "Distribution Board", [6000, 0]);
    let e = fails(&mut s, "circuit", json!({"devices": d, "panel": bare}));
    assert!(e.contains(&bare.to_string()) && e.contains("distribution system"), "{e}");
}

#[test]
fn wires_are_sections_of_a_wire_type_and_a_section_it_has_not_is_refused_naming_the_table() {
    let mut s = Session::new();
    let (b, d) = (board(&mut s), lamps(&mut s, 2));
    let wires = json!({"hot": {"wire_type": "Cu PVC 70 °C", "section": 1.5}, "neutral": {"wire_type": "Cu PVC 70 °C", "section": 1.5},
                       "ground": {"wire_type": "Cu PVC 70 °C", "section": 1.5}});
    let c = make(&mut s, json!({"devices": d, "panel": b, "rating": 10, "wires": wires}));
    assert_eq!(element_json(&s.model, c)["wires"]["hot"]["section"], json!(1.5));
    let e = fails(&mut s, "circuit", json!({"id": c, "wires": {"hot": {"wire_type": "Cu PVC 70 °C", "section": 2.0}}}));
    assert!(e.contains("Cu PVC 70 °C") && e.contains("2 mm²") && e.contains("1.5"), "it names the table and what it has: {e}");
    let e = fails(&mut s, "circuit", json!({"id": c, "wires": {"hot": {"wire_type": "Gold", "section": 1.5}}}));
    assert!(e.contains("Gold") && e.contains("Cu PVC 70 °C"), "{e}");
    let e = fails(&mut s, "circuit", json!({"id": c, "wires": {"neutral": {"wire_type": "Cu PVC 70 °C", "section": 1.5}}}));
    assert!(e.contains("hot"), "{e}");
    // The wire type is the project's: renamed, the circuit follows; in use, it stays.
    run(&mut s, "wire_type", json!({"name": "Cu PVC 70 °C", "rename": "PVC 70"}));
    assert_eq!(element_json(&s.model, c)["wires"]["hot"]["wire_type"], json!("PVC 70"));
    assert_eq!(s.model.check(), Ok(()));
    // A breaker that the wire cannot carry is said, not refused.
    run(&mut s, "circuit", json!({"id": c, "rating": 25}));
    assert!(s.model.warnings().iter().any(|w| w.contains(&c.to_string()) && w.contains("25")), "{:?}", s.model.warnings());
    run(&mut s, "circuit", json!({"id": c, "wires": ""}));
    assert!(element_json(&s.model, c).get("wires").is_none_or(Value::is_null));
}

#[test]
fn a_circuit_is_edited_its_members_changed_deleted_undone_and_redone() {
    let mut s = Session::new();
    let (b, d) = (board(&mut s), lamps(&mut s, 3));
    let steps = s.history().len();
    let c = make(&mut s, json!({"devices": [d[0]], "panel": b, "rating": 10}));
    assert_eq!(s.history().len(), steps + 1, "one undo step");
    run(
        &mut s,
        "circuit",
        json!({"id": c, "add": [d[1], d[2]], "name": "Sala", "rating": 16, "demand_factor": 0.5, "classification": "lighting"}),
    );
    let e = element_json(&s.model, c);
    assert_eq!((e["device_ids"].clone(), e["name"].clone(), e["rating_a"].clone()), (json!(d), json!("Sala"), json!(16.0)));
    assert_eq!((e["installed_va"].as_f64(), e["demand_va"].as_f64()), (Some(36.0), Some(18.0)));
    run(&mut s, "circuit", json!({"id": c, "remove": [d[1]]}));
    assert_eq!(element_json(&s.model, c)["device_ids"], json!([d[0], d[2]]));
    let e = fails(&mut s, "circuit", json!({"id": c, "remove": [d[1]]}));
    assert!(e.contains(&d[1].to_string()), "a device that is not in it cannot be taken out: {e}");
    // modify merges fields, as it does for every element, and its refusals are the circuit's.
    run(&mut s, "modify", json!({"id": c, "name": "Sala de estar"}));
    assert_eq!(element_json(&s.model, c)["name"], json!("Sala de estar"));
    let e = fails(&mut s, "modify", json!({"id": c, "rating_a": -3}));
    assert!(e.contains(&c.to_string()), "{e}");
    // Deleted, undone, redone.
    run(&mut s, "delete", json!({"ids": [c]}));
    assert!(circuits(&s).is_empty() && s.model.elements.get(d[0]).is_some(), "its devices stay");
    run(&mut s, "undo", json!({}));
    assert_eq!(circuits(&s), [c]);
    run(&mut s, "redo", json!({}));
    assert!(circuits(&s).is_empty());
    run(&mut s, "undo", json!({}));
    run(&mut s, "circuit", json!({"id": c, "delete": true}));
    assert!(circuits(&s).is_empty());
}

#[test]
fn what_hangs_on_what_goes_goes_a_device_leaves_its_circuit_and_a_board_takes_its_circuits() {
    let mut s = Session::new();
    let (b, d) = (board(&mut s), lamps(&mut s, 2));
    let c = make(&mut s, json!({"devices": d, "panel": b}));
    run(&mut s, "delete", json!({"ids": [d[0]]}));
    assert_eq!(element_json(&s.model, c)["device_ids"], json!([d[1]]));
    assert_eq!(s.model.check(), Ok(()));
    // The last device: the circuit stays, a spare, and is said.
    run(&mut s, "delete", json!({"ids": [d[1]]}));
    assert_eq!(circuits(&s), [c]);
    assert!(s.model.warnings().iter().any(|w| w.contains(&c.to_string()) && w.contains("no device")), "{:?}", s.model.warnings());
    // A board that goes takes its circuits.
    run(&mut s, "delete", json!({"ids": [b]}));
    assert!(circuits(&s).is_empty());
    run(&mut s, "undo", json!({}));
    assert_eq!(circuits(&s), [c]);
}

#[test]
fn a_copy_of_a_device_is_in_no_circuit_and_a_circuit_is_not_copied() {
    let mut s = Session::new();
    let (b, d) = (board(&mut s), lamps(&mut s, 1));
    let c = make(&mut s, json!({"devices": d, "panel": b}));
    let r = run(&mut s, "copy", json!({"ids": [d[0]], "by": [0, 1000]}));
    let copy = r["ids"][0].as_u64().unwrap();
    assert_eq!(element_json(&s.model, c)["device_ids"], json!(d), "the circuit has the original only");
    assert_eq!(s.model.check(), Ok(()));
    // Copied together with its board and device, the copies are circuits of their own? No: a circuit is not copied.
    let e = fails(&mut s, "copy", json!({"ids": [c], "by": [0, 1000]}));
    assert!(e.contains("circuit"), "{e}");
    let _ = copy;
    // Moving what it holds moves nothing of the circuit's.
    run(&mut s, "move", json!({"ids": [d[0], b], "by": [500, 0]}));
    assert_eq!(s.model.check(), Ok(()));
}

#[test]
fn circuits_are_saved_by_guid_and_open_again_in_the_inspect_with_their_load() {
    let mut s = Session::new();
    let (b, d) = (board(&mut s), lamps(&mut s, 2));
    let wires = json!({"hot": {"wire_type": "Cu PVC 70 °C", "section": 1.5}});
    let c = make(&mut s, json!({"devices": d, "panel": b, "name": "Luz", "wires": wires, "voltage": "127 V"}));
    let r = run(&mut s, "inspect", json!({}));
    let listed = r["circuits"].as_array().unwrap().iter().find(|x| x["id"] == json!(c)).cloned().expect("inspect lists it");
    assert_eq!((listed["installed_va"].as_f64(), listed["panel_id"].as_u64(), listed["number"].as_u64()), (Some(24.0), Some(b), Some(1)));
    assert_eq!(r["counts"]["circuit"], json!(1));
    let path = std::env::temp_dir().join(format!("bimcraft-circuit-{}.bimcraft", std::process::id()));
    run(&mut s, "save", json!({"path": path.to_str().unwrap()}));
    let file: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(file["format"], json!(crate::model::FORMAT));
    let mut t = Session::new();
    let r = run(&mut t, "open", json!({"path": path.to_str().unwrap()}));
    assert!(r["left_out"].as_array().is_none_or(Vec::is_empty), "nothing is left out: {r}");
    let (a, z) = (element_json(&s.model, c), element_json(&t.model, c));
    for k in ["name", "number", "rating_a", "poles", "device_ids", "panel_id", "voltage_name", "installed_va", "wires"] {
        assert_eq!(a[k], z[k], "{k}");
    }
    assert_eq!(t.model.check(), Ok(()));
    // The voltage and the wire type are the file's GUIDs, so a rename in the project follows.
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains("\"voltage\": \"127 V\""), "a name of the library is its GUID in a file");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_board_says_what_its_circuits_load() {
    let mut s = Session::new();
    let (b, d) = (board(&mut s), lamps(&mut s, 4));
    make(&mut s, json!({"devices": [d[0], d[1]], "panel": b, "demand_factor": 1.0}));
    make(&mut s, json!({"devices": [d[2], d[3]], "panel": b, "demand_factor": 0.5}));
    let r = run(&mut s, "panel", json!({}));
    let board_json = r["panels"].as_array().unwrap().iter().find(|p| p["id"] == json!(b)).cloned().unwrap();
    assert_eq!((board_json["installed_va"].as_f64(), board_json["demand_va"].as_f64()), (Some(48.0), Some(36.0)));
    assert_eq!(board_json["circuits"].as_array().map(Vec::len), Some(2));
    // The balanced three-phase current of the demand: 36 / (√3 × 220).
    assert!((board_json["current_a"].as_f64().unwrap() - 36.0 / (3f64.sqrt() * 220.0)).abs() < 1e-3, "{board_json}");
    assert_eq!(element_json(&s.model, b)["installed_va"], json!(48.0), "a board's element says it too");
}

#[test]
fn circuits_come_in_from_another_project_with_their_board_and_devices() {
    let mut from = Session::new();
    let (b, d) = (board(&mut from), lamps(&mut from, 2));
    let c = make(&mut from, json!({"devices": d, "panel": b, "name": "Luz"}));
    let path = std::env::temp_dir().join(format!("bimcraft-circuit-insert-{}.bimcraft", std::process::id()));
    run(&mut from, "save", json!({"path": path.to_str().unwrap()}));
    let mut to = Session::new();
    let r = run(&mut to, "insert", json!({"path": path.to_str().unwrap(), "ids": [c]}));
    let here = r["ids"][c.to_string()].as_u64().expect("the circuit came");
    let e = element_json(&to.model, here);
    assert_eq!((e["name"].as_str(), e["device_ids"].as_array().map(Vec::len)), (Some("Luz"), Some(2)));
    assert!(to.model.elements.get(e["panel_id"].as_u64().unwrap()).is_some(), "its board came with it");
    assert_eq!(to.model.check(), Ok(()));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn circuit_is_two_commands_in_one_a_model_circuit_and_the_nbr_5410_sizing() {
    let mut s = Session::new();
    let (b, d) = (board(&mut s), lamps(&mut s, 1));
    // The sizing is as it was, and makes no undo step.
    let steps = s.history().len();
    let r = run(&mut s, "circuit", json!({"load": 1800}));
    assert!(r["section_mm2"].is_number() && r["breaker_a"].is_number(), "{r}");
    assert_eq!(s.history().len(), steps, "sizing changes nothing");
    // Keys of both are refused, saying which are whose.
    let e = fails(&mut s, "circuit", json!({"load": 1800, "devices": d.clone(), "panel": b}));
    assert!(e.contains("load") && e.contains("devices") && e.contains("sizing"), "{e}");
    let e = fails(&mut s, "circuit", json!({"devices": d, "panel": b, "breaker": 16}));
    assert!(e.contains("breaker") && e.contains("devices"), "{e}");
    // Listing: of the board's, and of every one.
    let c = make(&mut s, json!({"devices": [], "panel": b}));
    let r = run(&mut s, "circuit", json!({"list": true}));
    assert_eq!(r["circuits"].as_array().map(Vec::len), Some(1));
    let r = run(&mut s, "circuit", json!({"panel": b}));
    assert_eq!(r["circuits"][0]["id"], json!(c));
    assert!(crate::engine::find("circuit").is_some_and(|c| c.undoable), "a model circuit is an undo step");
}

#[test]
fn every_circuit_payload_is_safe() {
    let mut s = Session::new();
    let (b, d) = (board(&mut s), lamps(&mut s, 2));
    let c = make(&mut s, json!({"devices": [d[0]], "panel": b}));
    for p in [
        json!({"devices": "x", "panel": b}),
        json!({"devices": [[], null, {"a": 1}], "panel": b}),
        json!({"devices": d.clone(), "panel": {"x": 1}}),
        json!({"id": c, "add": "x", "remove": 5}),
        json!({"id": c, "add": [u64::MAX], "remove": [u64::MAX]}),
        json!({"id": u64::MAX, "delete": true}),
        json!({"id": b, "delete": true}),
        json!({"id": c, "rating": f64::MAX, "poles": u64::MAX, "number": u64::MAX, "demand_factor": f64::NAN}),
        json!({"id": c, "wires": [1, 2], "voltage": 5}),
        json!({"id": c, "wires": {"hot": {"wire_type": [], "section": "x"}, "neutral": 5}}),
        json!({"id": c, "voltage": "nope"}),
        json!({"id": c, "voltage": ""}),
        json!({"devices": (0..5000).collect::<Vec<u64>>(), "panel": b}),
        json!({"list": "yes", "panel": null}),
    ] {
        let _ = s.execute("circuit", &p);
        assert_eq!(s.model.check(), Ok(()), "{p}");
    }
}
