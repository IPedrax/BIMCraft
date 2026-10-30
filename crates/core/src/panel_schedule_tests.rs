//! The panel schedule (quadro de cargas): a view of a distribution board with its slots, its load summary and notes, that goes on a
//! sheet and prints with it, and is edited through the circuits it shows.

use serde_json::{Value, json};

use crate::circuit_tests::{board_on, fails, lamps, make, place, run};
use crate::element_json;
use crate::engine::Session;
use crate::ids::Id;
use crate::model::Element;
use crate::render::{Prim, Rgb};

fn view(s: &mut Session, board: Id, extra: Value) -> Id {
    let mut p = json!({"panel": board});
    if let (Some(o), Some(e)) = (p.as_object_mut(), extra.as_object()) {
        o.extend(e.clone());
    }
    run(s, "panel_schedule", p)["id"].as_u64().unwrap()
}

fn rows(s: &mut Session, v: Id) -> Vec<Value> {
    run(s, "panel_schedule", json!({"id": v}))["rows"].as_array().unwrap().clone()
}

fn kinds(rows: &[Value]) -> Vec<String> {
    rows.iter().map(|r| r["kind"].as_str().unwrap().to_string()).collect()
}

/// A board on 127/220 V with 24 slots and the lamps 12 VA each (127 V), the first on slot 1.
fn small() -> (Session, Id, Vec<Id>) {
    let mut s = Session::new();
    let b = board_on(&mut s, "127/220 V");
    let d = lamps(&mut s, 4);
    (s, b, d)
}

#[test]
fn a_panel_schedule_is_a_view_of_a_board_listed_by_inspect() {
    let (mut s, b, d) = small();
    make(&mut s, json!({"devices": [d[0]], "panel": b, "name": "Sala"}));
    let r = run(&mut s, "panel_schedule", json!({"panel": b, "name": "QD-1"}));
    let id = r["id"].as_u64().unwrap();
    assert_eq!((r["name"].as_str(), r["panel"].as_u64(), r["template"].as_str()), (Some("QD-1"), Some(b), Some("branch")));
    assert_eq!(s.model.view_name(id), Some("QD-1"), "it is a view, with a name");
    let i = run(&mut s, "inspect", json!({}));
    assert_eq!(i["counts"]["panelschedule"], json!(1));
    let listed = i["panel_schedules"].as_array().unwrap().iter().find(|v| v["id"] == json!(id)).cloned().expect("inspect lists it");
    assert_eq!((listed["name"].as_str(), listed["panel_id"].as_u64()), (Some("QD-1"), Some(b)));
    // The header says the board, its system, voltage, phases and mains.
    let header: Vec<(String, String)> = serde_json::from_value(r["header"].clone()).unwrap();
    let get = |k: &str| header.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str());
    assert_eq!(
        (get("Distribution System"), get("Voltage"), get("Phases"), get("Mains")),
        (Some("127/220 V"), Some("220/127 V"), Some("3"), Some("63 A"))
    );
    // Without a name, one that is free; a name taken is refused; what isn't a board is refused.
    let other = run(&mut s, "panel_schedule", json!({"panel": b}));
    assert_ne!(other["name"], r["name"]);
    assert!(fails(&mut s, "panel_schedule", json!({"panel": b, "name": "QD-1"})).contains("QD-1"));
    assert!(fails(&mut s, "panel_schedule", json!({"panel": d[0]})).contains("not a distribution board"));
    assert!(fails(&mut s, "panel_schedule", json!({})).contains("panel"));
}

#[test]
fn its_slots_follow_max_1_pole_breakers_and_a_circuit_takes_one_for_each_pole() {
    let (mut s, b, d) = small();
    let v = view(&mut s, b, json!({}));
    // 24 slots, all spaces.
    let all = rows(&mut s, v);
    assert_eq!(all.len(), 24);
    assert!(kinds(&all).iter().all(|k| k == "space"));
    // A one-pole circuit at 1, a three-pole at 3 (3, 5, 7).
    make(&mut s, json!({"devices": [d[0]], "panel": b}));
    let three = place(&mut s, "Three-Phase Outlet", [4000, 2000]);
    let c3 = make(&mut s, json!({"devices": [three], "panel": b, "number": 3}));
    assert_eq!(element_json(&s.model, c3)["poles"], json!(3));
    let all = rows(&mut s, v);
    assert_eq!(
        kinds(&all)[..8],
        ["circuit", "space", "circuit", "space", "continues", "space", "continues", "space"].map(String::from),
        "{all:?}"
    );
    // A new circuit takes the first free slot it fits in: 2, not past the last.
    let c2 = make(&mut s, json!({"devices": [d[1]], "panel": b}));
    assert_eq!(element_json(&s.model, c2)["number"], json!(2));
    // Slots overlapping are refused, naming the slot and the circuit; so are slots past the board's.
    let e = fails(&mut s, "circuit", json!({"id": c2, "number": 5}));
    assert!(e.contains("slot 5") && e.contains(&c3.to_string()), "{e}");
    let e = fails(&mut s, "circuit", json!({"id": c3, "number": 21}));
    assert!(e.contains("slot 25") && e.contains("24"), "{e}");
    // Without `Max #1 Pole Breakers`, the schedule runs to the last slot a circuit takes, made even.
    run(&mut s, "panel", json!({"id": b, "slots": ""}));
    assert_eq!(rows(&mut s, v).len(), 8);
}

#[test]
fn a_spare_and_a_space_are_different_rows_and_can_be_left_out() {
    let (mut s, b, d) = small();
    make(&mut s, json!({"devices": [d[0]], "panel": b}));
    make(&mut s, json!({"devices": [], "panel": b, "number": 2}));
    let v = view(&mut s, b, json!({}));
    let all = rows(&mut s, v);
    assert_eq!(kinds(&all)[..3], ["circuit", "spare", "space"].map(String::from));
    assert_eq!(all[1]["cells"][1], json!("Spare"), "a spare says so");
    run(&mut s, "panel_schedule", json!({"id": v, "show_space": false}));
    assert!(!kinds(&rows(&mut s, v)).contains(&"space".to_string()));
    run(&mut s, "panel_schedule", json!({"id": v, "show_spare": false}));
    assert_eq!(kinds(&rows(&mut s, v)), ["circuit"]);
}

#[test]
fn phase_loads_add_up_to_the_boards_and_the_current_is_the_phases_over_its_voltage() {
    let (mut s, b, d) = small();
    // Slots 1 (A), 3 (B) and 5 (C): 24 VA, 12 VA and 12 VA; the second lamp of the first.
    make(&mut s, json!({"devices": [d[0], d[1]], "panel": b, "number": 1}));
    make(&mut s, json!({"devices": [d[2]], "panel": b, "number": 3}));
    make(&mut s, json!({"devices": [d[3]], "panel": b, "number": 5}));
    let v = view(&mut s, b, json!({}));
    let r = run(&mut s, "panel_schedule", json!({"id": v}));
    let phases = r["summary"]["phases"].as_array().unwrap();
    let va: Vec<f64> = phases.iter().map(|p| p["installed_va"].as_f64().unwrap()).collect();
    assert_eq!((phases.len(), va.clone()), (3, vec![24.0, 12.0, 12.0]));
    assert_eq!(phases[0]["phase"], json!("A"));
    let total = r["summary"]["installed_va"].as_f64().unwrap();
    assert!((va.iter().sum::<f64>() - total).abs() < 0.01 && total == 48.0, "{r}");
    // The current of a phase: its demand over the phase voltage (127 V).
    for p in phases {
        let want = p["demand_va"].as_f64().unwrap() / 127.0;
        assert!((p["current_a"].as_f64().unwrap() - want).abs() < 1e-3, "{p}");
    }
    // The pole's load sits in its phase's column of its row.
    let all = rows(&mut s, v);
    let phase_cells = |r: &Value| r["cells"].as_array().unwrap()[5..8].to_vec();
    assert_eq!(phase_cells(&all[0]), [json!("24"), json!(""), json!("")]);
    assert_eq!(phase_cells(&all[2]), [json!(""), json!("12"), json!("")]);
}

#[test]
fn an_unbalanced_circuit_puts_each_pole_on_the_phase_of_its_slot() {
    let mut s = Session::new();
    let b = board_on(&mut s, "127/220 V");
    let o = run(&mut s, "component", json!({"family": "Three-Phase Outlet", "type": "Unbalanced 1100", "at": [4000, 2000]}))["id"]
        .as_u64()
        .unwrap();
    let c = make(&mut s, json!({"devices": [o], "panel": b, "number": 3}));
    let v = view(&mut s, b, json!({}));
    let r = run(&mut s, "panel_schedule", json!({"id": v}));
    // 3000, 2000 and 1000 VA on slots 3 (B), 5 (C) and 7 (A).
    let va: Vec<f64> = r["summary"]["phases"].as_array().unwrap().iter().map(|p| p["installed_va"].as_f64().unwrap()).collect();
    assert_eq!(va, vec![1000.0, 3000.0, 2000.0]);
    // Moved to start on phase A, it is on 1, 3 and 5: the loads land on A, B and C.
    run(&mut s, "panel_schedule", json!({"id": v, "circuit": c, "phase": "A"}));
    assert_eq!(element_json(&s.model, c)["number"], json!(1));
    let r = run(&mut s, "panel_schedule", json!({"id": v}));
    let va: Vec<f64> = r["summary"]["phases"].as_array().unwrap().iter().map(|p| p["installed_va"].as_f64().unwrap()).collect();
    assert_eq!(va, vec![3000.0, 2000.0, 1000.0]);
}

#[test]
fn demand_factors_by_classification_are_applied_to_the_summary_and_the_boards_totals() {
    let (mut s, b, d) = small();
    run(&mut s, "circuit", json!({"devices": [d[0], d[1]], "panel": b, "classification": "lighting"}));
    run(&mut s, "circuit", json!({"devices": [d[2]], "panel": b, "classification": "receptacle", "demand_factor": 0.5}));
    run(&mut s, "demand_factors", json!({"class": "lighting", "factor": 0.8}));
    assert_eq!(run(&mut s, "demand_factors", json!({}))["demand_factors"]["lighting"], json!(0.8));
    let v = view(&mut s, b, json!({}));
    let sum = run(&mut s, "panel_schedule", json!({"id": v}))["summary"].clone();
    let class = |name: &str| sum["classes"].as_array().unwrap().iter().find(|c| c["class"] == json!(name)).cloned().unwrap();
    // 24 VA of lighting counts 0.8; the receptacle's own 0.5 of 12 VA counts 6.
    assert_eq!((class("lighting")["demand_va"].as_f64(), class("lighting")["factor"].as_f64()), (Some(19.2), Some(0.8)));
    assert_eq!(class("receptacle")["demand_va"].as_f64(), Some(6.0));
    assert_eq!((sum["installed_va"].as_f64(), sum["demand_va"].as_f64()), (Some(36.0), Some(25.2)));
    let panel = run(&mut s, "panel", json!({}))["panels"].as_array().unwrap().iter().find(|p| p["id"] == json!(b)).cloned().unwrap();
    assert_eq!(panel["demand_va"].as_f64(), Some(25.2), "the board says what its schedule does");
    // A factor out of range is refused; one taken away counts whole again.
    assert!(fails(&mut s, "demand_factors", json!({"class": "lighting", "factor": 1.5})).contains("up to 1"));
    run(&mut s, "demand_factors", json!({"class": "lighting", "delete": true}));
    assert_eq!(run(&mut s, "panel_schedule", json!({"id": v}))["summary"]["demand_va"].as_f64(), Some(30.0));
}

#[test]
fn a_circuit_is_edited_through_the_view_and_the_totals_follow() {
    let (mut s, b, d) = small();
    let c = make(&mut s, json!({"devices": [d[0], d[1]], "panel": b, "name": "Luz"}));
    let v = view(&mut s, b, json!({}));
    run(&mut s, "panel_schedule", json!({"id": v, "circuit": c, "description": "Iluminação sala", "breaker": 10, "demand_factor": 0.5}));
    let e = element_json(&s.model, c);
    assert_eq!((e["name"].as_str(), e["rating_a"].as_f64(), e["demand_factor"].as_f64()), (Some("Iluminação sala"), Some(10.0), Some(0.5)));
    assert_eq!(run(&mut s, "panel_schedule", json!({"id": v}))["summary"]["demand_va"].as_f64(), Some(12.0));
    // Moved to a slot, and to a phase: the first free slot that starts on it.
    run(&mut s, "panel_schedule", json!({"id": v, "circuit": c, "slot": 7}));
    assert_eq!(element_json(&s.model, c)["number"], json!(7));
    run(&mut s, "panel_schedule", json!({"id": v, "circuit": c, "phase": "B"}));
    assert_eq!(element_json(&s.model, c)["number"], json!(3));
    // Refused edits say why and change nothing.
    assert!(fails(&mut s, "panel_schedule", json!({"id": v, "circuit": c, "phase": "D"})).contains("A, B, C"));
    assert!(fails(&mut s, "panel_schedule", json!({"id": v, "circuit": c, "slot": 99})).contains("slot"));
    assert!(fails(&mut s, "panel_schedule", json!({"id": v, "circuit": c, "breaker": 100})).contains("mains"));
    assert!(fails(&mut s, "panel_schedule", json!({"id": v, "circuit": c, "slot": 1, "phase": "A"})).contains("not both"));
    assert!(fails(&mut s, "panel_schedule", json!({"id": v, "description": "x"})).contains("circuit"));
    // Undone as one step each.
    let before = element_json(&s.model, c)["number"].clone();
    run(&mut s, "panel_schedule", json!({"id": v, "circuit": c, "slot": 1}));
    run(&mut s, "undo", json!({}));
    assert_eq!(element_json(&s.model, c)["number"], before);
}

#[test]
fn a_slot_is_marked_a_spare_or_a_space() {
    let (mut s, b, d) = small();
    let v = view(&mut s, b, json!({}));
    run(&mut s, "panel_schedule", json!({"id": v, "slot": 4, "mark": "spare"}));
    let spare = s
        .model
        .elements
        .iter()
        .find_map(|(i, e)| matches!(e, Element::Circuit(c) if c.number == 4).then_some(*i))
        .expect("a spare is a circuit");
    assert_eq!(kinds(&rows(&mut s, v))[3], "spare");
    run(&mut s, "panel_schedule", json!({"id": v, "slot": 4, "mark": "spare"}));
    run(&mut s, "panel_schedule", json!({"id": v, "slot": 4, "mark": "space"}));
    assert!(s.model.elements.get(spare).is_none(), "a space is a slot with no breaker");
    assert_eq!(kinds(&rows(&mut s, v))[3], "space");
    // A circuit with devices is not turned into either.
    make(&mut s, json!({"devices": [d[0]], "panel": b, "number": 2}));
    let e = fails(&mut s, "panel_schedule", json!({"id": v, "slot": 2, "mark": "space"}));
    assert!(e.contains("devices"), "{e}");
    assert!(fails(&mut s, "panel_schedule", json!({"id": v, "slot": 2, "mark": "bogus"})).contains("spare"));
}

fn dark(png: &[u8], at: (f32, f32)) -> bool {
    let pm = tiny_skia::Pixmap::decode_png(png).unwrap();
    let c = pm.pixel(at.0 as u32, at.1 as u32).unwrap();
    c.red() < 200 && c.green() < 200 && c.blue() < 200
}

#[test]
fn it_goes_on_a_sheet_and_prints_with_it_a_space_struck_through_and_a_spare_grey() {
    let (mut s, b, d) = small();
    make(&mut s, json!({"devices": [d[0]], "panel": b, "name": "Sala"}));
    make(&mut s, json!({"devices": [], "panel": b, "number": 2}));
    let v = view(&mut s, b, json!({"name": "QD-1", "notes": "Cabos em eletroduto"}));
    run(&mut s, "sheet", json!({"name": "Quadros"}));
    let vp = run(&mut s, "place", json!({"sheet": "A101", "view": "QD-1", "at": [200, 150]}))["id"].as_u64().unwrap();
    let sheet = s.model.elements.iter().find_map(|(i, e)| matches!(e, Element::Sheet(_)).then_some(*i)).unwrap();
    let (frame, (ox, oy)) = crate::sheet::frame(&s.model, vp).unwrap();
    assert!(frame.width > 300 && frame.height > 300, "its tables make its size: {frame:?}");
    let d1 = crate::sheet::draw(&s.model, sheet).unwrap();
    // Picked as the viewport; no scale note for a view that has none.
    assert_eq!(d1.pick((ox + 4.0, oy + 4.0), 2.0), Some(vp));
    let png = crate::render::png(&d1).unwrap();
    assert!(dark(&png, (ox + 0.5, oy + 0.5)), "its outline");
    // A space is a diagonal rule across a row; a spare, grey lettering.
    let grey = Rgb(128, 128, 128);
    let diagonals: Vec<&Prim> = d1
        .prims
        .iter()
        .filter(|p| matches!(p, Prim::Line { pts, .. } if pts.len() == 2 && (pts[0].0 - pts[1].0).abs() > 20.0 && (pts[0].1 - pts[1].1).abs() > 5.0))
        .collect();
    assert!(!diagonals.is_empty(), "the spaces are struck through");
    if let Prim::Line { pts, .. } = diagonals[0] {
        let mid = ((pts[0].0 + pts[1].0) / 2.0, (pts[0].1 + pts[1].1) / 2.0);
        // A hairline is anti-aliased: some pixel beside its middle is inked.
        let near = |dx: f32, dy: f32| dark(&png, (mid.0 + dx, mid.1 + dy));
        assert!([-1.0, 0.0, 1.0].iter().any(|dx| [-1.0, 0.0, 1.0].iter().any(|dy| near(*dx, *dy))), "the strike is drawn");
    }
    assert!(d1.prims.iter().any(|p| matches!(p, Prim::Line { color, .. } if *color == grey)), "the spare is lettered grey");
    // Hide them and neither is there.
    run(&mut s, "panel_schedule", json!({"id": v, "show_space": false, "show_spare": false}));
    let d2 = crate::sheet::draw(&s.model, sheet).unwrap();
    assert!(!d2.prims.iter().any(|p| matches!(p, Prim::Line { color, .. } if *color == grey)));
    assert!(d2.prims.len() < d1.prims.len());
    // A larger one is larger on paper.
    let small_size = crate::sheet::size(&s.model, v, 1.0).unwrap();
    run(&mut s, "panel_schedule", json!({"id": v, "scale": 2}));
    let big = crate::sheet::size(&s.model, v, 1.0).unwrap();
    assert!(big.0 > small_size.0 * 1.9 && big.1 > small_size.1 * 1.9, "{small_size:?} {big:?}");
    // Printed with the sheets.
    let pdf = std::env::temp_dir().join(format!("bimcraft-qd-{}.pdf", std::process::id()));
    run(&mut s, "save", json!({"path": pdf.to_str().unwrap()}));
    let bytes = std::fs::read(&pdf).unwrap();
    assert!(bytes.starts_with(b"%PDF-") && bytes.len() > 2000);
    let _ = std::fs::remove_file(pdf);
}

#[test]
fn it_is_saved_with_its_board_and_goes_with_it() {
    let (mut s, b, d) = small();
    make(&mut s, json!({"devices": [d[0]], "panel": b}));
    let v = view(&mut s, b, json!({"template": "list", "show_space": false, "notes": "n", "scale": 1.5}));
    run(&mut s, "sheet", json!({}));
    run(&mut s, "place", json!({"sheet": "A101", "view": v}));
    let path = std::env::temp_dir().join(format!("bimcraft-qd-{}.bimcraft", std::process::id()));
    run(&mut s, "save", json!({"path": path.to_str().unwrap()}));
    let mut t = Session::new();
    let r = run(&mut t, "open", json!({"path": path.to_str().unwrap()}));
    assert!(r["left_out"].as_array().is_none_or(Vec::is_empty), "{r}");
    assert_eq!(t.model.elements.get(v), s.model.elements.get(v));
    assert_eq!(t.model.check(), Ok(()));
    let _ = std::fs::remove_file(path);
    // Copying leaves it; deleting its board takes it and its viewport.
    assert!(s.execute("copy", &json!({"ids": [v], "offset": [100, 0]})).is_err());
    run(&mut s, "delete", json!({"ids": [b]}));
    let n = |s: &Session, f: fn(&Element) -> bool| s.model.elements.values().filter(|e| f(e)).count();
    assert_eq!(n(&s, |e| matches!(e, Element::PanelSchedule(_))), 0);
    assert_eq!(n(&s, |e| matches!(e, Element::Viewport(_))), 0);
    assert_eq!(n(&s, |e| matches!(e, Element::Circuit(_))), 0);
}

#[test]
fn it_speaks_the_projects_language() {
    let (mut s, b, d) = small();
    make(&mut s, json!({"devices": [d[0]], "panel": b}));
    let v = view(&mut s, b, json!({}));
    run(&mut s, "language", json!({"code": "pt"}));
    let r = run(&mut s, "panel_schedule", json!({"id": v}));
    let header: Vec<(String, String)> = serde_json::from_value(r["header"].clone()).unwrap();
    assert!(header.iter().any(|(k, _)| k == "Sistema de distribuição"), "{header:?}");
    assert_eq!(kinds(&rows(&mut s, v))[1], "space");
    assert_eq!(rows(&mut s, v)[1]["cells"][1], json!("Espaço"));
}
