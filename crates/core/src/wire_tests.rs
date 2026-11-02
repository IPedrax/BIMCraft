//! Physical wires carry their source, ordered devices, cable make and reserve on the same element.

use serde_json::{Value, json};

use crate::element_json;
use crate::engine::Session;
use crate::geom::P2;
use crate::model::{Element, Model};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn fails(s: &mut Session, id: &str, p: Value) -> String {
    let before = s.model.clone();
    let e = s.execute(id, &p).expect_err(&format!("{id} {p} should fail"));
    assert_eq!(*s.model, *before, "a refused {id} changes nothing");
    e
}

fn made(s: &mut Session, id: &str, p: Value) -> u64 {
    let r = run(s, id, p);
    r["id"].as_u64().or_else(|| r["ids"][0].as_u64()).unwrap()
}

fn wire(s: &mut Session, mut p: Value) -> u64 {
    p["points"] = json!([[0, 0], [3000, 0]]);
    made(s, "wire", p)
}

/// A device of one family and type everywhere, so their heights differ only by their offsets.
fn device(s: &mut Session, at: [f64; 2], offset: f64) -> u64 {
    let ty = s.model.families["Voice Outlet"].types.keys().next().cloned().unwrap();
    made(s, "component", json!({"family": "Voice Outlet", "type": ty, "at": at, "offset": offset}))
}

/// A source and three devices: the third a metre higher than the rest.
fn loop_of_three(s: &mut Session) -> (u64, [u64; 3]) {
    let source = device(s, [0.0, 0.0], 0.0);
    let a = device(s, [3000.0, 0.0], 0.0);
    let b = device(s, [3000.0, 4000.0], 1000.0);
    let c = device(s, [0.0, 4000.0], 0.0);
    (source, [a, b, c])
}

#[test]
fn a_wire_has_physical_geometry_and_its_ordered_devices_with_reserve() {
    let mut s = Session::new();
    let (source, [a, b, c]) = loop_of_three(&mut s);
    let id = wire(&mut s, json!({"source": source, "devices": [a, b, c], "system": "L1", "cable": "КПС 1x2x0,5", "reserve": 10}));
    let e = element_json(&s.model, id);
    assert_eq!(e["category"], "wire");
    assert_eq!(e["system"], "L1");
    assert_eq!(e["cable"], "КПС 1x2x0,5");
    assert_eq!(e["source_id"], json!(source));
    assert_eq!(e["device_ids"], json!([a, b, c]));
    // 3000 along; 4000 across and 1000 up; 3000 back and 1000 down: 12000, and a tenth more.
    assert_eq!(e["length"], json!(13200.0));
    assert!(s.model.run_geom(s.model.elements.get(id).unwrap()).is_some());
    run(&mut s, "modify", json!({"id": id, "reserve": 0}));
    assert_eq!(element_json(&s.model, id)["length"], json!(12000.0));
    assert_eq!(element_json(&s.model, a)["address"], "L1.1");
    assert_eq!(element_json(&s.model, c)["address"], "L1.3");
    assert_eq!(element_json(&s.model, source)["address"], Value::Null);
}

#[test]
fn a_wire_refuses_invalid_device_and_carrier_references_atomically() {
    let mut s = Session::new();
    let (source, [a, b, _]) = loop_of_three(&mut s);
    let pipe = made(&mut s, "pipe", json!({"points": [[0, 0], [1000, 0]]}));
    for mut p in [
        json!({"source": source, "devices": []}),
        json!({"source": source, "devices": [a, a]}),
        json!({"source": source, "devices": [a, source]}),
        json!({"source": source, "devices": [a, pipe]}),
        json!({"source": pipe, "devices": [a]}),
        json!({"source": source, "devices": [a], "reserve": 150}),
        json!({"devices": [a]}),
        json!({"carrier": pipe}),
    ] {
        p["points"] = json!([[0, 0], [1000, 0]]);
        fails(&mut s, "wire", p);
    }
    let id = wire(&mut s, json!({"source": source, "devices": [a, b]}));
    fails(&mut s, "modify", json!({"id": id, "devices": [a, a]}));
    fails(&mut s, "modify", json!({"id": id, "reserve": -1}));
    fails(&mut s, "pipe", json!({"points": [[0, 0], [1000, 0]], "source": source}));
}

#[test]
fn deleting_devices_or_a_carrier_keeps_the_physical_wire_valid() {
    let mut s = Session::new();
    let (source, [a, b, c]) = loop_of_three(&mut s);
    let carrier = made(&mut s, "cable_carrier", json!({"points": [[0, 0], [3000, 0]]}));
    let id = wire(&mut s, json!({"source": source, "devices": [a, b, c], "carrier": carrier}));
    run(&mut s, "delete", json!({"ids": [b]}));
    assert_eq!(element_json(&s.model, id)["device_ids"], json!([a, c]));
    run(&mut s, "delete", json!({"ids": [carrier]}));
    assert!(s.model.elements.get(id).unwrap().run().unwrap().carrier.is_none());
    run(&mut s, "delete", json!({"ids": [source]}));
    let w = s.model.elements.get(id).unwrap().run().unwrap();
    assert!(w.source.is_none() && w.devices.is_empty());
    let other = wire(&mut s, json!({"source": a, "devices": [c]}));
    run(&mut s, "delete", json!({"ids": [c]}));
    let w = s.model.elements.get(other).unwrap().run().unwrap();
    assert!(w.source.is_none() && w.devices.is_empty());
    assert_eq!(s.model.check(), Ok(()));
}

#[test]
fn wires_draw_on_their_level_and_have_a_cable_schedule_and_file_roundtrip() {
    let mut s = Session::new();
    let (source, [a, b, c]) = loop_of_three(&mut s);
    let id = wire(&mut s, json!({"source": source, "devices": [a, b, c], "system": "L1", "cable": "A"}));
    wire(&mut s, json!({"source": source, "devices": [c], "system": "L2", "cable": "B"}));
    let r = run(
        &mut s,
        "schedule",
        json!({"name": "Cables", "category": "wire", "fields": ["Cable", {"name": "Length", "total": true}], "sort": ["Cable"], "grouped": true}),
    );
    // The loop: 12000; the speaker line, from the source straight to the last device: 4000.
    assert_eq!(r["rows"], json!([["A", 12000.0], ["B", 4000.0]]));
    let (back, left_out) = Model::read(&s.model.to_json().unwrap()).unwrap();
    assert!(left_out.is_empty());
    assert_eq!(back.check(), Ok(()));
    assert_eq!(element_json(&back, id)["device_ids"], json!([a, b, c]));
    let level = s.model.levels()[0].0;
    assert!(crate::render::plan(&s.model, level, 800, 600).owners.contains(&Some(id)));
    run(&mut s, "graphics", json!({"view": "Level 1", "hide": ["wire"]}));
    assert!(!crate::render::plan(&s.model, level, 800, 600).owners.contains(&Some(id)));
}

#[test]
fn a_wires_logical_route_follows_walls_when_requested() {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0], [4000, 3000], [0, 3000]], "closed": true}));
    let source = device(&mut s, [1500.0, 500.0], 0.0);
    let one = device(&mut s, [3500.0, 2000.0], 0.0);
    let id = wire(&mut s, json!({"source": source, "devices": [one], "along_walls": true}));
    let Some(Element::Wire(w)) = s.model.elements.get(id) else { panic!("no wire") };
    // Out to the wall it leans on, along it, and off to the device.
    assert_eq!(
        s.model.circuit_route(w, s.model.guid(s.model.levels()[0].0)),
        vec![vec![P2(1500.0, 500.0), P2(1500.0, 0.0), P2(3500.0, 0.0), P2(3500.0, 2000.0)]]
    );
    // 500 + 2000 + 2000, where square across the room would make 3500.
    assert_eq!(element_json(&s.model, id)["length"], json!(4500.0));
    let other = wire(&mut s, json!({"source": source, "devices": [one]}));
    assert_eq!(element_json(&s.model, other)["length"], json!(3500.0));
}

#[test]
fn old_circuits_and_carrier_types_migrate_without_losing_ids_or_references() {
    let mut s = Session::new();
    let (source, [a, _, _]) = loop_of_three(&mut s);
    let carrier = made(&mut s, "cable_carrier", json!({"points": [[0, 0], [3000, 0]]}));
    let tray = made(&mut s, "cable_carrier", json!({"points": [[0, 1000], [3000, 1000]], "type": "Tray 100 x 50"}));
    let id = wire(&mut s, json!({"source": source, "devices": [a], "system": "L1"}));
    let schedule = made(
        &mut s,
        "schedule",
        json!({"name": "Legacy cables", "category": "wire", "fields": ["Name", "Source", "Devices", "Cable", "Length"]}),
    );
    let mut file = serde_json::to_value(&*s.model).unwrap();
    file["format"] = json!(7);
    let old = file["elements"][id.to_string()].as_object_mut().unwrap();
    old.insert("category".into(), json!("circuit"));
    old.insert("name".into(), json!("L1"));
    old.insert("system".into(), json!("low-voltage"));
    for k in ["type", "level", "start", "end", "offset", "end_offset", "justify"] {
        old.remove(k);
    }
    file["elements"][carrier.to_string()]["category"] = json!("conduit");
    file["elements"][tray.to_string()]["category"] = json!("cabletray");
    file["elements"][schedule.to_string()]["of"] = json!("circuit");
    file["elements"]["100000"]["graphics"]["categories"] = json!({"conduit": {"hidden": true}, "circuit": {"halftone": true}});
    file["object_styles"] = json!({"cabletray": {"projection": 4, "cut": 10}});
    let types = file["types"]["runs"].as_object_mut().unwrap();
    let carriers = types.remove("cablecarrier").unwrap();
    types.insert("conduit".into(), json!({}));
    types.insert("cabletray".into(), json!({}));
    for (name, ty) in carriers.as_object().unwrap() {
        let category = if ty["round"] == true { "conduit" } else { "cabletray" };
        types.get_mut(category).unwrap()[name] = ty.clone();
    }
    for entry in file["library"].as_object_mut().unwrap().values_mut() {
        if entry["kind"] == "type:cablecarrier" {
            let name = entry["name"].as_str().unwrap();
            entry["kind"] = json!(if s.model.types.runs["cablecarrier"][name].round { "type:conduit" } else { "type:cabletray" });
        }
    }
    let (m, left_out) = Model::read(&file.to_string()).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(m.check(), Ok(()));
    assert_eq!(m.guid(id), s.model.guid(id));
    assert_eq!(element_json(&m, id)["source_id"], source);
    assert_eq!(element_json(&m, id)["system"], "L1");
    assert_eq!(m.elements.get(carrier).unwrap().category(), "cablecarrier");
    assert!(!m.types.runs.contains_key("conduit"));
    assert!(!m.types.runs.contains_key("cabletray"));
    assert!(m.run_geom(m.elements.get(tray).unwrap()).is_some_and(|g| !g.round));
    assert!(m.object_styles.contains_key("cablecarrier"));
    let (again, _) = Model::read(&m.to_json().unwrap()).unwrap();
    assert_eq!(again.check(), Ok(()));
}

/// Real wire and detector keys for the hostile sweep, with valid targets and invalid references and values.
pub(crate) fn sweep_payloads(s: &mut Session) -> Vec<(&'static str, Value)> {
    let room = made(s, "room", json!({"point": [4000, 3000]}));
    let smoke = made(s, "detectors", json!({"room": room}));
    let heat = run(s, "detectors", json!({"room": room, "kind": "heat"}));
    let (one, two) = (heat["ids"][0].as_u64().unwrap(), heat["ids"][1].as_u64().unwrap());
    let id = wire(s, json!({"source": smoke, "devices": [one]}));
    vec![
        ("wire", json!({"points": [[0, 0], [1000, 0]], "source": smoke, "devices": [one, two], "reserve": 10})),
        ("wire", json!({"points": [[0, 0], [1000, 0]], "source": room, "devices": [one]})),
        ("wire", json!({"points": [[0, 0], [1000, 0]], "source": smoke, "devices": [smoke]})),
        ("wire", json!({"points": [[0, 0], [1000, 0]], "source": one, "devices": [two], "along_walls": "yes"})),
        ("wire", json!({"points": [[0, 0], [1000, 0]], "carrier": room})),
        ("detectors", json!({"room": room, "kind": "heat"})),
        ("detectors", json!({"room": room, "kind": "x"})),
        ("detectors", json!({"room": room, "radius": 1})),
        ("modify", json!({"id": id, "devices": [one, two]})),
        ("modify", json!({"id": id, "along_walls": true})),
        ("modify", json!({"id": id, "along_walls": "no"})),
    ]
}
