//! Stage 3 starts with advisory physical capacity checks after catalogue replacement.
use bimcraft_core::{engine::Session, ods};
use serde_json::{Value, json};

#[allow(clippy::unwrap_used)] // Test fixture: failures should stop the test.
fn setup(linked: bool, diameter: f64) -> Session {
    let mut s = Session::new();
    s.execute("type", &json!({"category":"wire","name":"Test cable","width":diameter,"depth":diameter,"round":true})).unwrap();
    s.execute("type", &json!({"category":"cablecarrier","name":"Test box","width":10,"depth":10,"round":false})).unwrap();
    let carrier = s.execute("cable_carrier", &json!({"type":"Test box","points":[[0,0],[3000,0]]})).unwrap();
    let mut params = json!({"type":"Test cable","points":[[0,0],[3000,0]]});
    if linked {
        params["carrier"] = carrier.get("id").or_else(|| carrier["ids"].get(0)).unwrap().clone();
    }
    s.execute("wire", &params).unwrap();
    s
}

fn carrier_warning(reply: &Value) -> Option<&str> {
    reply["warnings"].as_array()?.iter().filter_map(Value::as_str).find(|s| s.contains("cable carrier") && s.contains("exceeds"))
}

#[test]
fn closed_ods_diameter_change_warns_carrier_overfill() {
    let temp = tempfile::tempdir().unwrap();
    let catalogue = temp.path().join("catalogue.ods");
    let project = temp.path().join("project.bimcraft");
    let mut s = setup(true, 6.0);
    assert!(carrier_warning(&s.execute("inspect", &json!({})).unwrap()).is_none());
    s.execute("catalogue_link", &json!({"path":catalogue})).unwrap();
    s.execute("save", &json!({"path":project})).unwrap();
    let before = s.model.elements.clone();
    drop(s);
    let mut rows = ods::read(&std::fs::read(&catalogue).unwrap()).unwrap();
    let row = rows.iter_mut().find(|row| row[2] == "Test cable").unwrap();
    row[3] = "20".into();
    row[4] = "20".into();
    std::fs::write(&catalogue, ods::write(&rows).unwrap()).unwrap();
    let mut reopened = Session::new();
    let reply = reopened.execute("open", &json!({"path":project})).unwrap();
    assert!(reply.get("catalogue_not_loaded").is_none(), "{reply}");
    assert_eq!(reopened.model.types.runs["wire"]["Test cable"].width, 20.0);
    assert_eq!(reopened.model.elements, before, "warnings must preserve geometry, identity and references");
    let warning = carrier_warning(&reply).expect("opening must report the oversized linked cable");
    let inspected = reopened.execute("inspect", &json!({})).unwrap();
    assert_eq!(carrier_warning(&inspected), Some(warning));
    reopened.model.check().unwrap();
}

#[test]
fn unlinked_oversized_wire_does_not_warn_about_carrier() {
    let mut s = setup(false, 20.0);
    assert!(carrier_warning(&s.execute("inspect", &json!({})).unwrap()).is_none());
}

#[test]
fn moving_carrier_moves_linked_wire_once_and_undo_restores_both() {
    let mut s = setup(true, 6.0);
    let carrier = *s.model.elements.iter().find(|(_, e)| e.category() == "cablecarrier").unwrap().0;
    let wire = *s.model.elements.iter().find(|(_, e)| e.category() == "wire").unwrap().0;
    let before = s.model.elements.clone();
    let original = s.model.elements.get(wire).unwrap().run().unwrap().clone();
    let guid = s.model.elements.guid(wire);
    s.execute("move", &json!({"ids":[carrier],"by":[200,300]})).unwrap();
    let moved = s.model.elements.get(wire).unwrap().run().unwrap();
    assert_eq!(moved.start.0, original.start.0 + 200.0);
    assert_eq!(moved.start.1, original.start.1 + 300.0);
    assert_eq!(moved.end.0, original.end.0 + 200.0);
    assert_eq!(moved.end.1, original.end.1 + 300.0);
    assert_eq!(s.model.elements.guid(wire), guid);
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(s.model.elements, before);
    s.execute("redo", &json!({})).unwrap();
    let after = s.model.elements.get(wire).unwrap().run().unwrap().clone();
    s.execute("move", &json!({"ids":[carrier,wire],"by":[100,0]})).unwrap();
    let together = s.model.elements.get(wire).unwrap().run().unwrap();
    assert_eq!(together.start.0, after.start.0 + 100.0, "selected wire must not move twice");
    assert_eq!(s.model.elements.guid(wire), guid);
}

#[test]
fn carrier_height_edit_takes_linked_wire_with_it() {
    let mut s = setup(true, 6.0);
    let carrier = *s.model.elements.iter().find(|(_, e)| e.category() == "cablecarrier").unwrap().0;
    let wire = *s.model.elements.iter().find(|(_, e)| e.category() == "wire").unwrap().0;
    let before = s.model.elements.get(wire).unwrap().run().unwrap().offset;
    let host_before = s.model.elements.get(carrier).unwrap().run().unwrap().offset;
    s.execute("modify", &json!({"id":carrier,"offset":host_before+500.0})).unwrap();
    assert_eq!(s.model.elements.get(wire).unwrap().run().unwrap().offset, before + 500.0);
    s.model.check().unwrap();
}

#[test]
fn editing_carrier_axis_takes_wire_to_new_axis() {
    let mut s = setup(true, 6.0);
    let carrier = *s.model.elements.iter().find(|(_, e)| e.category() == "cablecarrier").unwrap().0;
    let wire = *s.model.elements.iter().find(|(_, e)| e.category() == "wire").unwrap().0;
    s.execute("modify", &json!({"id":carrier,"start":[100,200],"end":[100,6200]})).unwrap();
    let run = s.model.elements.get(wire).unwrap().run().unwrap();
    assert_eq!(run.start.0, 100.0);
    assert_eq!(run.start.1, 200.0);
    assert_eq!(run.end.0, 100.0);
    assert_eq!(run.end.1, 6200.0);
}

#[test]
fn simultaneous_cables_exceed_carrier_together() {
    let mut s = setup(true, 9.0);
    assert!(carrier_warning(&s.execute("inspect", &json!({})).unwrap()).is_none());
    let carrier = *s.model.elements.iter().find(|(_, e)| e.category() == "cablecarrier").unwrap().0;
    s.execute("wire", &json!({"type":"Test cable","points":[[0,0],[3000,0]],"carrier":carrier})).unwrap();
    assert!(carrier_warning(&s.execute("inspect", &json!({})).unwrap()).is_some(), "two 9 mm cables exceed a 100 mm2 gross section");
}

#[test]
fn disjoint_cable_intervals_do_not_add_their_sections() {
    let mut s = setup(true, 9.0);
    let carrier = *s.model.elements.iter().find(|(_, e)| e.category() == "cablecarrier").unwrap().0;
    let wire = *s.model.elements.iter().find(|(_, e)| e.category() == "wire").unwrap().0;
    s.execute("modify", &json!({"id":wire,"end":[1400,0]})).unwrap();
    s.execute("wire", &json!({"type":"Test cable","points":[[1500,0],[3000,0]],"carrier":carrier})).unwrap();
    assert!(carrier_warning(&s.execute("inspect", &json!({})).unwrap()).is_none());
}

#[test]
fn carrier_autosize_uses_same_manufacturer_and_supports_undo() {
    use bimcraft_core::library::{Kind, Product};
    let mut s = setup(true, 20.0);
    for (name, width, manufacturer) in [("Same maker", 30, "Maker A"), ("Other maker", 25, "Maker B")] {
        s.execute("type", &json!({"category":"cablecarrier","name":name,"width":width,"depth":width,"round":false})).unwrap();
        s.model_mut()
            .library
            .set_product(
                &Kind::Type("cablecarrier".into()),
                name,
                Some(Product { manufacturer: manufacturer.into(), ..Default::default() }),
            )
            .unwrap();
    }
    s.model_mut()
        .library
        .set_product(&Kind::Type("cablecarrier".into()), "Test box", Some(Product { manufacturer: "Maker A".into(), ..Default::default() }))
        .unwrap();
    let carrier = *s.model.elements.iter().find(|(_, e)| e.category() == "cablecarrier").unwrap().0;
    let before = s.model.elements.clone();
    s.execute("cable_carrier_check", &json!({"mode":"auto","fill_percent":100})).unwrap();
    assert_eq!(s.model.elements.get(carrier).unwrap().run().unwrap().type_name, "Same maker");
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(s.model.elements, before);
}

#[test]
fn autosize_keeps_one_size_on_a_straight_carrier_run() {
    use bimcraft_core::library::{Kind, Product};
    let mut s = setup(true, 20.0);
    s.execute("type", &json!({"category":"cablecarrier","name":"Large box","width":30,"depth":30,"round":false})).unwrap();
    for name in ["Test box", "Large box"] {
        s.model_mut()
            .library
            .set_product(&Kind::Type("cablecarrier".into()), name, Some(Product { manufacturer: "Maker A".into(), ..Default::default() }))
            .unwrap();
    }
    s.execute("cable_carrier", &json!({"type":"Test box","points":[[3000,0],[6000,0]]})).unwrap();
    s.execute("cable_carrier_check", &json!({"mode":"auto","fill_percent":100})).unwrap();
    for e in s.model.elements.values().filter(|e| e.category() == "cablecarrier") {
        assert_eq!(e.run().unwrap().type_name, "Large box");
    }
}

#[test]
fn manually_locked_carrier_is_not_autosized_and_lock_survives_open() {
    use bimcraft_core::library::{Kind, Product};
    let mut s = setup(true, 20.0);
    s.execute("type", &json!({"category":"cablecarrier","name":"Large box","width":30,"depth":30,"round":false})).unwrap();
    for name in ["Test box", "Large box"] {
        s.model_mut()
            .library
            .set_product(&Kind::Type("cablecarrier".into()), name, Some(Product { manufacturer: "Maker A".into(), ..Default::default() }))
            .unwrap();
    }
    let carrier = *s.model.elements.iter().find(|(_, e)| e.category() == "cablecarrier").unwrap().0;
    s.execute("modify", &json!({"id":carrier,"size_locked":true})).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("locked.bimcraft");
    s.execute("save", &json!({"path":path})).unwrap();
    let mut reopened = Session::new();
    reopened.execute("open", &json!({"path":path})).unwrap();
    let reply = reopened.execute("cable_carrier_check", &json!({"mode":"auto","fill_percent":100})).unwrap();
    assert!(reply["warnings"].as_array().unwrap().iter().any(|v| v.as_str().is_some_and(|s| s.contains("locked"))));
    assert_eq!(reopened.model.elements.get(carrier).unwrap().run().unwrap().type_name, "Test box");
}

#[test]
fn project_autosize_setting_survives_open_and_applies_to_type_edits() {
    use bimcraft_core::library::{Kind, Product};
    let mut s = setup(true, 6.0);
    s.execute("type", &json!({"category":"cablecarrier","name":"Large box","width":30,"depth":30,"round":false})).unwrap();
    for name in ["Test box", "Large box"] {
        s.model_mut()
            .library
            .set_product(&Kind::Type("cablecarrier".into()), name, Some(Product { manufacturer: "Maker A".into(), ..Default::default() }))
            .unwrap();
    }
    s.execute("cable_carrier_check", &json!({"mode":"auto","fill_percent":100,"configure":true})).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("policy.bimcraft");
    s.execute("save", &json!({"path":path})).unwrap();
    let mut reopened = Session::new();
    reopened.execute("open", &json!({"path":path})).unwrap();
    reopened.execute("type", &json!({"category":"wire","name":"Test cable","width":20,"depth":20,"round":true})).unwrap();
    let carrier = reopened.model.elements.values().find(|e| e.category() == "cablecarrier").unwrap();
    assert_eq!(carrier.run().unwrap().type_name, "Large box");
}

#[test]
fn straight_run_uses_uniform_size_even_if_each_segment_already_fits() {
    use bimcraft_core::library::{Kind, Product};
    let mut s = setup(true, 6.0);
    s.execute("type", &json!({"category":"cablecarrier","name":"Large box","width":30,"depth":30,"round":false})).unwrap();
    for name in ["Test box", "Large box"] {
        s.model_mut()
            .library
            .set_product(&Kind::Type("cablecarrier".into()), name, Some(Product { manufacturer: "Maker A".into(), ..Default::default() }))
            .unwrap();
    }
    s.execute("cable_carrier", &json!({"type":"Large box","points":[[3000,0],[6000,0]]})).unwrap();
    s.execute("cable_carrier_check", &json!({"mode":"auto","fill_percent":100})).unwrap();
    let names: std::collections::BTreeSet<_> =
        s.model.elements.values().filter(|e| e.category() == "cablecarrier").map(|e| e.run().unwrap().type_name.clone()).collect();
    assert_eq!(names.len(), 1, "one straight run must have one sufficient size");
}

#[test]
fn inspect_respects_the_saved_fill_limit() {
    let mut s = setup(true, 9.0);
    s.execute("cable_carrier_check", &json!({"mode":"warn","fill_percent":50,"configure":true})).unwrap();
    assert!(carrier_warning(&s.execute("inspect", &json!({})).unwrap()).is_some());
}

#[test]
fn narrow_carrier_reports_geometric_conflict_despite_sufficient_area() {
    let mut s = setup(true, 20.0);
    s.execute("type", &json!({"category":"cablecarrier","name":"Test box","width":10,"depth":100,"round":false})).unwrap();
    let reply = s.execute("inspect", &json!({})).unwrap();
    assert!(
        reply["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_str().is_some_and(|s| s.contains("cable carrier") && s.contains("geometric conflict"))),
        "{reply}"
    );
}

#[test]
fn autosize_rejects_an_area_sufficient_but_too_narrow_type() {
    use bimcraft_core::library::{Kind, Product};
    let mut s = setup(true, 20.0);
    for (name, width, depth) in [("Narrow box", 10, 40), ("Good box", 30, 30)] {
        s.execute("type", &json!({"category":"cablecarrier","name":name,"width":width,"depth":depth,"round":false})).unwrap();
    }
    for name in ["Test box", "Narrow box", "Good box"] {
        s.model_mut()
            .library
            .set_product(&Kind::Type("cablecarrier".into()), name, Some(Product { manufacturer: "Maker A".into(), ..Default::default() }))
            .unwrap();
    }
    s.execute("cable_carrier_check", &json!({"mode":"auto","fill_percent":100})).unwrap();
    let e = s.model.elements.values().find(|e| e.category() == "cablecarrier").unwrap();
    assert_eq!(e.run().unwrap().type_name, "Good box");
}

#[test]
fn compartment_fill_and_system_separation_are_checked_and_persisted() {
    let mut s = setup(true, 6.0);
    let carrier = *s.model.elements.iter().find(|(_, e)| e.category() == "cablecarrier").unwrap().0;
    let wire = *s.model.elements.iter().find(|(_, e)| e.category() == "wire").unwrap().0;
    s.execute("modify",&json!({"id":carrier,"compartments":{"data":{"width":4,"depth":10,"offset":-3,"systems":["Data"]},"power":{"width":4,"depth":10,"offset":3,"systems":["Power"]}}})).unwrap();
    s.execute("modify", &json!({"id":wire,"compartment":"data","system":"Power"})).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("compartments.bimcraft");
    s.execute("save", &json!({"path":path})).unwrap();
    let mut reopened = Session::new();
    reopened.execute("open", &json!({"path":path})).unwrap();
    let reply = reopened.execute("inspect", &json!({})).unwrap();
    let warnings = reply["warnings"].as_array().unwrap();
    assert!(warnings.iter().any(|v| v.as_str().is_some_and(|s| s.contains("compartment") && s.contains("geometric conflict"))));
    assert!(warnings.iter().any(|v| v.as_str().is_some_and(|s| s.contains("separation"))));
}

#[test]
fn simultaneous_compartment_fill_is_checked_separately_from_gross_fill() {
    let mut s = setup(true, 3.0);
    let carrier = *s.model.elements.iter().find(|(_, e)| e.category() == "cablecarrier").unwrap().0;
    let wire = *s.model.elements.iter().find(|(_, e)| e.category() == "wire").unwrap().0;
    s.execute("modify", &json!({"id":carrier,"compartments":{"data":{"width":4,"depth":4}}})).unwrap();
    s.execute("modify", &json!({"id":wire,"compartment":"data"})).unwrap();
    for _ in 0..2 {
        s.execute("wire", &json!({"type":"Test cable","points":[[0,0],[3000,0]],"carrier":carrier,"compartment":"data"})).unwrap();
    }
    let reply = s.execute("inspect", &json!({})).unwrap();
    assert!(
        reply["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_str().is_some_and(|s| s.contains("compartment data") && s.contains("exceeds"))),
        "compartment sum must exceed its own usable section"
    );
}

#[test]
fn overlapping_compartment_measurements_are_rejected_without_model_changes() {
    let mut s = setup(true, 3.0);
    let carrier = *s.model.elements.iter().find(|(_, e)| e.category() == "cablecarrier").unwrap().0;
    let before = s.model.clone();
    assert!(
        s.execute(
            "modify",
            &json!({"id":carrier,"compartments":{"a":{"width":6,"depth":10,"offset":-1},"b":{"width":6,"depth":10,"offset":1}}})
        )
        .is_err()
    );
    assert_eq!(s.model, before);
}

#[test]
fn warning_only_carrier_check_does_not_add_an_undo_step() {
    let mut s = setup(true, 6.0);
    s.execute("cable_carrier_check", &json!({"mode":"warn","fill_percent":100})).unwrap();
    s.execute("undo", &json!({})).unwrap();
    assert!(
        !s.model.elements.values().any(|e| e.category() == "wire"),
        "Undo should remove the last created wire, not undo a read-only check"
    );
}

#[test]
fn project_default_carrier_type_survives_open() {
    let mut s = Session::new();
    s.execute("type", &json!({"category":"cablecarrier","name":"Project tray","width":150,"depth":75,"round":false})).unwrap();
    s.execute("cable_carrier_check", &json!({"mode":"warn","fill_percent":100,"configure":true,"default_type":"Project tray"})).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("default.bimcraft");
    s.execute("save", &json!({"path":path})).unwrap();
    let mut reopened = Session::new();
    reopened.execute("open", &json!({"path":path})).unwrap();
    reopened.execute("cable_carrier", &json!({"points":[[0,0],[3000,0]]})).unwrap();
    assert_eq!(reopened.model.elements.values().find(|e| e.category() == "cablecarrier").unwrap().run().unwrap().type_name, "Project tray");
}

#[test]
fn autosize_does_not_convert_an_adjacent_conduit_into_a_rectangular_tray() {
    use bimcraft_core::library::{Kind, Product};
    let mut s = setup(true, 20.0);
    for (name, width, round) in [("Large box", 30, false), ("Conduit", 10, true)] {
        s.execute("type", &json!({"category":"cablecarrier","name":name,"width":width,"depth":width,"round":round})).unwrap();
    }
    for name in ["Test box", "Large box", "Conduit"] {
        s.model_mut()
            .library
            .set_product(&Kind::Type("cablecarrier".into()), name, Some(Product { manufacturer: "Maker A".into(), ..Default::default() }))
            .unwrap();
    }
    let reply = s.execute("cable_carrier", &json!({"type":"Conduit","points":[[3000,0],[6000,0]]})).unwrap();
    let id = reply.get("id").or_else(|| reply["ids"].get(0)).unwrap().clone();
    s.execute("cable_carrier_check", &json!({"mode":"auto","fill_percent":100})).unwrap();
    let guid = bimcraft_core::ids::Guid::parse(id.as_str().unwrap_or_default()).unwrap_or_else(|| serde_json::from_value(id).unwrap());
    assert!(s.model.run_type(s.model.elements.get(guid).unwrap()).unwrap().round);
}

#[test]
fn autosize_preserves_measured_compartment_space() {
    use bimcraft_core::library::{Kind, Product};
    let mut s = setup(true, 20.0);
    for (name, width, depth) in [("Test box", 10, 100), ("Shallow", 40, 30), ("Deep", 40, 100)] {
        s.execute("type", &json!({"category":"cablecarrier","name":name,"width":width,"depth":depth,"round":false})).unwrap();
        s.model_mut()
            .library
            .set_product(&Kind::Type("cablecarrier".into()), name, Some(Product { manufacturer: "Maker A".into(), ..Default::default() }))
            .unwrap();
    }
    let carrier = *s.model.elements.iter().find(|(_, e)| e.category() == "cablecarrier").unwrap().0;
    s.execute("modify", &json!({"id":carrier,"compartments":{"tall":{"width":8,"depth":100,"offset":0}}})).unwrap();
    s.execute("cable_carrier_check", &json!({"mode":"auto","fill_percent":100})).unwrap();
    assert_eq!(s.model.elements.get(carrier).unwrap().run().unwrap().type_name, "Deep");
}

#[test]
fn stretching_a_vertical_carrier_preserves_cable_position_in_its_section() {
    let mut s = setup(true, 6.0);
    let carrier = *s.model.elements.iter().find(|(_, e)| e.category() == "cablecarrier").unwrap().0;
    let wire = *s.model.elements.iter().find(|(_, e)| e.category() == "wire").unwrap().0;
    s.execute("modify", &json!({"id":carrier,"end":[0,0],"offset":0,"end_offset":3000})).unwrap();
    s.execute("modify", &json!({"id":wire,"start":[2,1],"end":[2,1]})).unwrap();
    let before = s.model.elements.clone();
    s.execute("modify", &json!({"id":carrier,"offset":100,"end_offset":6100})).unwrap();
    let r = s.model.elements.get(wire).unwrap().run().unwrap();
    assert_eq!((r.start.0, r.start.1, r.end.0, r.end.1), (2.0, 1.0, 2.0, 1.0));
    assert_eq!(r.offset, 100.0);
    assert_eq!(r.end_offset, Some(6100.0));
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(s.model.elements, before);
}

#[test]
fn tilting_a_vertical_carrier_takes_its_cable_to_the_new_axis() {
    let mut s = setup(true, 6.0);
    let carrier = *s.model.elements.iter().find(|(_, e)| e.category() == "cablecarrier").unwrap().0;
    let wire = *s.model.elements.iter().find(|(_, e)| e.category() == "wire").unwrap().0;
    s.execute("modify", &json!({"id":carrier,"end":[0,0],"offset":0,"end_offset":3000})).unwrap();
    s.execute("modify", &json!({"id":carrier,"start":[100,0],"end":[3100,0]})).unwrap();
    let r = s.model.elements.get(wire).unwrap().run().unwrap();
    assert_eq!((r.start.0, r.start.1, r.end.0, r.end.1), (100.0, 0.0, 3100.0, 0.0));
    assert_eq!(r.offset, 0.0);
    assert_eq!(r.end_offset, Some(3000.0));
}

#[test]
fn straight_run_ignores_perpendicular_branches_but_respects_a_manual_boundary() {
    use bimcraft_core::library::{Kind, Product};
    for boundary in [false, true] {
        let mut s = setup(true, 20.0);
        s.execute("type", &json!({"category":"cablecarrier","name":"Large box","width":30,"depth":30,"round":false})).unwrap();
        for name in ["Test box", "Large box"] {
            s.model_mut()
                .library
                .set_product(
                    &Kind::Type("cablecarrier".into()),
                    name,
                    Some(Product { manufacturer: "Maker A".into(), ..Default::default() }),
                )
                .unwrap();
        }
        s.execute("cable_carrier", &json!({"type":"Test box","points":[[3000,0],[6000,0]],"size_locked":boundary})).unwrap();
        s.execute("cable_carrier", &json!({"type":"Test box","points":[[6000,0],[9000,0]]})).unwrap();
        s.execute("cable_carrier", &json!({"type":"Test box","points":[[3000,0],[3000,1000]]})).unwrap();
        s.execute("cable_carrier_check", &json!({"mode":"auto","fill_percent":100})).unwrap();
        for e in s.model.elements.values().filter(|e| e.category() == "cablecarrier") {
            let r = e.run().unwrap();
            let expected = if r.end.1 != 0.0 || (boundary && r.start.0 >= 3000.0) { "Test box" } else { "Large box" };
            assert_eq!(r.type_name, expected, "boundary={boundary}, start={:?}", r.start);
        }
    }
}

#[test]
fn autosizing_a_top_justified_carrier_takes_the_cable_with_its_section() {
    use bimcraft_core::library::{Kind, Product};
    let mut s = setup(true, 20.0);
    let carrier = *s.model.elements.iter().find(|(_, e)| e.category() == "cablecarrier").unwrap().0;
    let wire = *s.model.elements.iter().find(|(_, e)| e.category() == "wire").unwrap().0;
    s.execute("modify", &json!({"id":carrier,"justify":"top"})).unwrap();
    assert_eq!(s.model.elements.get(wire).unwrap().run().unwrap().offset, -5.0);
    s.execute("type", &json!({"category":"cablecarrier","name":"Large box","width":30,"depth":30,"round":false})).unwrap();
    for name in ["Test box", "Large box"] {
        s.model_mut()
            .library
            .set_product(&Kind::Type("cablecarrier".into()), name, Some(Product { manufacturer: "Maker A".into(), ..Default::default() }))
            .unwrap();
    }
    let before = s.model.elements.clone();
    s.execute("cable_carrier_check", &json!({"mode":"auto","fill_percent":100})).unwrap();
    assert_eq!(s.model.elements.get(wire).unwrap().run().unwrap().offset, -15.0);
    let report = s.execute("inspect", &json!({})).unwrap();
    assert!(!report["warnings"].as_array().unwrap().iter().any(|w| w.as_str().is_some_and(|w| w.contains("geometric conflict"))));
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(s.model.elements, before);
}

#[test]
fn locking_a_sloped_carrier_does_not_change_its_cable_geometry() {
    let mut s = setup(true, 6.0);
    let carrier = *s.model.elements.iter().find(|(_, e)| e.category() == "cablecarrier").unwrap().0;
    let wire = *s.model.elements.iter().find(|(_, e)| e.category() == "wire").unwrap().0;
    s.execute("modify", &json!({"id":carrier,"start":[123.4,567.8],"end":[5000.7,8000.9],"offset":100.23,"end_offset":1523.45})).unwrap();
    let before = s.model.elements.get(wire).unwrap().clone();
    s.execute("modify", &json!({"id":carrier,"size_locked":true})).unwrap();
    assert_eq!(s.model.elements.get(wire), Some(&before));
}

#[test]
fn editing_carrier_type_depth_keeps_the_cable_in_its_physical_section() {
    for (justify, sign) in [("top", -1.0), ("bottom", 1.0)] {
        let mut s = setup(true, 6.0);
        let carrier = *s.model.elements.iter().find(|(_, e)| e.category() == "cablecarrier").unwrap().0;
        let wire = *s.model.elements.iter().find(|(_, e)| e.category() == "wire").unwrap().0;
        s.execute("modify", &json!({"id":carrier,"justify":justify})).unwrap();
        let before = s.model.elements.clone();
        s.execute("type", &json!({"category":"cablecarrier","name":"Test box","width":30,"depth":30,"round":false})).unwrap();
        assert_eq!(s.model.elements.get(wire).unwrap().run().unwrap().offset, sign * 15.0);
        let after = s.model.elements.clone();
        s.execute("undo", &json!({})).unwrap();
        assert_eq!(s.model.elements, before);
        s.execute("redo", &json!({})).unwrap();
        assert_eq!(s.model.elements, after);
    }
}

#[test]
fn closed_ods_carrier_depth_change_takes_the_cable_with_its_section() {
    for (justify, sign) in [("top", -1.0), ("bottom", 1.0)] {
        let temp = tempfile::tempdir().unwrap();
        let catalogue = temp.path().join("catalogue.ods");
        let project = temp.path().join("project.bimcraft");
        let mut s = setup(true, 6.0);
        let carrier = *s.model.elements.iter().find(|(_, e)| e.category() == "cablecarrier").unwrap().0;
        let wire = *s.model.elements.iter().find(|(_, e)| e.category() == "wire").unwrap().0;
        s.execute("modify", &json!({"id":carrier,"justify":justify})).unwrap();
        let guid = s.model.elements.guid(wire);
        s.execute("catalogue_link", &json!({"path":catalogue})).unwrap();
        s.execute("save", &json!({"path":project})).unwrap();
        drop(s);
        let mut rows = ods::read(&std::fs::read(&catalogue).unwrap()).unwrap();
        let row = rows.iter_mut().find(|row| row[2] == "Test box").unwrap();
        row[3] = "30".into();
        row[4] = "30".into();
        std::fs::write(&catalogue, ods::write(&rows).unwrap()).unwrap();
        let mut reopened = Session::new();
        reopened.execute("open", &json!({"path":project})).unwrap();
        assert_eq!(reopened.model.elements.get(wire).unwrap().run().unwrap().offset, sign * 15.0);
        assert_eq!(reopened.model.elements.guid(wire), guid);
        let reply = reopened.execute("inspect", &json!({})).unwrap();
        assert!(!reply["warnings"].as_array().unwrap().iter().any(|w| w.as_str().is_some_and(|w| w.contains("geometric conflict"))));
    }
}

#[test]
fn rotating_and_mirroring_carriers_take_linked_cables_exactly_once() {
    use bimcraft_core::geom::P2;
    for (command, mut params, start, end) in [
        ("rotate", json!({"about":[0,0],"angle":90}), P2(-2.0, 300.0), P2(-2.0, 2700.0)),
        ("mirror", json!({"axis":[[0,0],[0,1000]]}), P2(-300.0, 2.0), P2(-2700.0, 2.0)),
    ] {
        for together in [false, true] {
            let mut s = setup(true, 6.0);
            let carrier = *s.model.elements.iter().find(|(_, e)| e.category() == "cablecarrier").unwrap().0;
            let wire = *s.model.elements.iter().find(|(_, e)| e.category() == "wire").unwrap().0;
            s.execute("modify", &json!({"id":wire,"start":[300,2],"end":[2700,2],"offset":1})).unwrap();
            let before = s.model.elements.clone();
            params["ids"] = if together { json!([carrier, wire]) } else { json!([carrier]) };
            s.execute(command, &params).unwrap();
            let cable = s.model.elements.get(wire).unwrap().run().unwrap();
            assert_eq!((cable.start, cable.end, cable.offset), (start, end, 1.0), "{command}, together={together}");
            let after = s.model.elements.clone();
            s.execute("undo", &json!({})).unwrap();
            assert_eq!(s.model.elements, before);
            s.execute("redo", &json!({})).unwrap();
            assert_eq!(s.model.elements, after);
        }
    }
}
