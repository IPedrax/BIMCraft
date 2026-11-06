//! The electrical settings: voltages, distribution systems and wire types, as the project's tables, through the commands every
//! front end drives.

use serde_json::{Value, json};

use crate::electrical;
use crate::engine::Session;
use crate::library::Kind;
use crate::nbr5410;
use crate::units::Units;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn fails(s: &mut Session, id: &str, p: Value) -> String {
    let before = s.model.clone();
    let e = s.execute(id, &p).expect_err(&format!("{id} {p} should fail"));
    assert_eq!(*s.model, *before, "a refused {id} changes nothing");
    e
}

/// The `name` entry of `kind` is in the library, with an id and a GUID.
fn in_library(s: &Session, kind: Kind, name: &str) -> bool {
    s.model.library.entries().any(|(_, e)| e.kind == kind && e.name == name)
}

#[test]
fn a_new_project_has_the_starter_settings() {
    let s = Session::new();
    let m = &s.model;
    assert_eq!(m.check(), Ok(()));
    let v = &m.voltages["230 V"];
    assert_eq!((v.nominal, v.min, v.max), (230.0, 207.0, 253.0));
    let sys = &m.distribution_systems["230/400 V"];
    assert_eq!((sys.phases, sys.wires), (3, 4));
    assert_eq!((sys.l_l.as_deref(), sys.l_n.as_deref(), sys.ground.as_deref()), (Some("400 V"), Some("230 V"), Some("TN-S")));
    for system in ["127/220 V", "220/380 V", "277/480 V"] {
        assert!(m.distribution_systems.contains_key(system), "{system}");
    }
    for wire in ["Cu PVC 70 °C", "Cu XLPE/EPR 90 °C", "Cu THHN 90 °C", "Cu THW 75 °C"] {
        assert!(m.wire_types.contains_key(wire), "{wire}");
        assert!(in_library(&s, Kind::WireType, wire), "{wire} is an entry of the library");
    }
    assert!(m.library_in_step(), "each has an id and a GUID");
    assert!(in_library(&s, Kind::Voltage, "230 V") && in_library(&s, Kind::DistributionSystem, "230/400 V"));
}

#[test]
fn the_starter_data_agrees_with_the_sizing_tables() {
    let s = Session::new();
    let m = &s.model;
    // The voltage systems the circuit sizing knows are voltages the project has.
    for (name, ln, ll) in nbr5410::SYSTEMS {
        let sys = m.distribution_systems.get(&format!("{name} V")).unwrap_or_else(|| panic!("{name}"));
        let volts = |v: &Option<String>| v.as_deref().and_then(|n| m.voltages.get(n)).map(|v| v.nominal);
        assert_eq!((volts(&sys.l_n), volts(&sys.l_l)), (Some(*ln), Some(*ll)), "{name}");
    }
    // The wire types seeded from NBR 5410 hold its tables' reference method B1, two loaded conductors (the fifth column).
    for (wire, table) in [("Cu PVC 70 °C", nbr5410::CU_PVC), ("Cu XLPE/EPR 90 °C", nbr5410::CU_EPR)] {
        let sizes = &m.wire_types[wire].sizes;
        assert_eq!(sizes.len(), table.len(), "{wire}");
        for (size, (section, row)) in sizes.iter().zip(table) {
            assert_eq!((size.section, size.ampacity), (*section, row[4]), "{wire} {section} mm²");
        }
    }
    // The American ones are sized in AWG and kept in mm², their AWG name beside.
    let thhn = m.wire_types["Cu THHN 90 °C"].sizes.iter().find(|x| x.label.as_deref() == Some("12 AWG")).expect("12 AWG");
    assert!((thhn.section - 3.31).abs() < 0.01 && thhn.ampacity == 30.0, "{thhn:?}");
    let thw = m.wire_types["Cu THW 75 °C"].sizes.iter().find(|x| x.label.as_deref() == Some("12 AWG")).expect("12 AWG");
    assert_eq!(thw.ampacity, 25.0);
    assert!(m.wire_types["Cu THW 75 °C"].sizes.iter().any(|x| x.label.as_deref() == Some("250 kcmil")));
}

#[test]
fn a_new_project_starts_in_the_settings_of_its_units() {
    let s = Session::new();
    for units in Units::ALL {
        assert!(s.model.distribution_systems.contains_key(electrical::start_system(units)), "{units:?}");
        assert!(s.model.wire_types.contains_key(electrical::start_wire(units)), "{units:?}");
    }
    assert_eq!(electrical::start_system(Units::Millimetres), "230/400 V");
    assert_eq!(electrical::start_system(Units::FeetInches), "277/480 V");
}

#[test]
fn voltages_are_made_edited_renamed_and_deleted() {
    let mut s = Session::new();
    let r = run(&mut s, "voltage", json!({"name": "690 V", "nominal": 690}));
    assert_eq!(r["name"], json!("690 V"));
    // The band is 10 per cent either way unless it is given.
    let v = &s.model.voltages["690 V"];
    assert_eq!((v.nominal, v.min, v.max), (690.0, 621.0, 759.0));
    assert!(in_library(&s, Kind::Voltage, "690 V"));
    run(&mut s, "voltage", json!({"name": "690 V", "min": 600, "max": 760}));
    let v = &s.model.voltages["690 V"];
    assert_eq!((v.nominal, v.min, v.max), (690.0, 600.0, 760.0), "what is not given stays");
    // Without a name, they are listed.
    assert_eq!(run(&mut s, "voltage", json!({}))["voltages"]["690 V"]["max"], json!(760.0));
    run(&mut s, "voltage", json!({"name": "690 V", "rename": "690 V IT"}));
    assert!(s.model.voltages.contains_key("690 V IT") && !s.model.voltages.contains_key("690 V"));
    assert!(in_library(&s, Kind::Voltage, "690 V IT") && !in_library(&s, Kind::Voltage, "690 V"));
    run(&mut s, "voltage", json!({"name": "690 V IT", "delete": true}));
    assert!(!s.model.voltages.contains_key("690 V IT") && !in_library(&s, Kind::Voltage, "690 V IT"));
    // One undo step each.
    assert_eq!(s.history().iter().filter(|l| **l == "Voltage Definitions").count(), 4);
    run(&mut s, "undo", json!({}));
    assert!(s.model.voltages.contains_key("690 V IT"));
}

#[test]
fn a_voltage_that_is_not_one_is_refused() {
    let mut s = Session::new();
    for (p, says) in [
        (json!({"name": "X"}), "nominal"),
        (json!({"name": "X", "nominal": 0}), "nominal"),
        (json!({"name": "X", "nominal": -230}), "nominal"),
        (json!({"name": "X", "nominal": 1e300}), "nominal"),
        (json!({"name": "X", "nominal": "high"}), "nominal"),
        (json!({"name": "X", "nominal": 230, "min": 250}), "min"),
        (json!({"name": "X", "nominal": 230, "max": 200}), "max"),
        (json!({"name": "X", "nominal": 230, "min": -5}), "min"),
        (json!({"name": "230 V", "nominal": 230, "rename": "400 V"}), "400 V"),
        (json!({"name": "nope", "delete": true}), "nope"),
        (json!({"name": "230 V", "volts": 230}), "volts"),
    ] {
        let e = fails(&mut s, "voltage", p.clone());
        assert!(e.contains(says), "{p}: {e}");
    }
}

#[test]
fn a_distribution_system_names_voltages_the_project_has() {
    let mut s = Session::new();
    let r = run(
        &mut s,
        "distribution_system",
        json!({"name": "Bifásico", "phases": 2, "wires": 3, "l_l": "220 V", "l_n": "127 V", "ground": "TT"}),
    );
    assert_eq!(r["name"], json!("Bifásico"));
    let sys = &s.model.distribution_systems["Bifásico"];
    assert_eq!(
        (sys.phases, sys.wires, sys.l_l.as_deref(), sys.l_n.as_deref(), sys.l_g.as_deref()),
        (2, 3, Some("220 V"), Some("127 V"), None)
    );
    assert!(in_library(&s, Kind::DistributionSystem, "Bifásico"));
    // What is not given stays; "" takes a voltage or the earthing away.
    run(&mut s, "distribution_system", json!({"name": "Bifásico", "wires": 2, "l_n": "", "ground": ""}));
    let sys = &s.model.distribution_systems["Bifásico"];
    assert_eq!((sys.wires, sys.l_l.as_deref(), sys.l_n.as_deref(), sys.ground.as_deref()), (2, Some("220 V"), None, None));
    assert_eq!(run(&mut s, "distribution_system", json!({}))["distribution_systems"]["Bifásico"]["l_l"], json!("220 V"));
    // The voltages a system supplies, which a circuit's has to be one of.
    assert_eq!(s.model.system_voltages("230/400 V"), [230.0, 400.0]);
    assert_eq!(s.model.system_voltages("Bifásico"), [220.0]);
    assert!(s.model.system_voltages("nope").is_empty());
    assert_eq!(s.model.voltage_range("230 V"), Some((207.0, 253.0)));
    assert_eq!(s.model.voltage_range("nope"), None);
    run(&mut s, "distribution_system", json!({"name": "Bifásico", "delete": true}));
    assert!(!s.model.distribution_systems.contains_key("Bifásico") && !in_library(&s, Kind::DistributionSystem, "Bifásico"));
}

#[test]
fn a_distribution_system_with_an_unknown_voltage_is_refused() {
    let mut s = Session::new();
    for key in ["l_l", "l_n", "l_g"] {
        let e = fails(&mut s, "distribution_system", json!({"name": "S", "phases": 3, "wires": 4, "l_l": "400 V", key: "nope"}));
        assert!(e.contains("nope") && e.contains(key) && e.contains("230 V"), "the refusal names the voltage and the ones there are: {e}");
    }
}

#[test]
fn a_distribution_system_that_is_not_one_is_refused() {
    let mut s = Session::new();
    let ok = json!({"name": "S", "phases": 3, "wires": 4, "l_l": "400 V", "l_n": "230 V"});
    let with = |k: &str, v: Value| {
        let mut p = ok.clone();
        p[k] = v;
        p
    };
    for (p, says) in [
        (with("phases", json!(4)), "phases"),
        (with("phases", json!(0)), "phases"),
        (with("phases", json!("three")), "phases"),
        (with("wires", json!(2)), "wires"),
        (with("wires", json!(6)), "wires"),
        (with("wires", json!(1.5)), "wires"),
        (with("ground", json!("PE")), "TN-S"),
        // Three phases at 230 V line to neutral are 400 V apart, not 300.
        (with("l_l", json!("380 V")), "230 V"),
        (with("l_l", json!("")), "l_l"),
        (json!({"name": "S", "phases": 1, "wires": 2}), "voltage"),
        (json!({"name": "S", "phases": 3, "wires": 4}), "voltage"),
        (json!({"name": "S", "phases": 1}), "wires"),
        (json!({"name": "S", "wires": 2}), "phases"),
        (json!({"name": "230/400 V", "rename": "127/220 V"}), "127/220 V"),
        (json!({"name": "nope", "delete": true}), "nope"),
        (with("volts", json!(230)), "volts"),
    ] {
        let e = fails(&mut s, "distribution_system", p.clone());
        assert!(e.contains(says), "{p}: {e}");
    }
}

#[test]
fn a_voltage_a_system_names_stays_and_follows_its_rename() {
    let mut s = Session::new();
    let e = fails(&mut s, "voltage", json!({"name": "230 V", "delete": true}));
    assert!(e.contains("230 V") && e.contains("230/400 V"), "the refusal names the system that holds it: {e}");
    run(&mut s, "voltage", json!({"name": "230 V", "rename": "230 V (EU)"}));
    let sys = &s.model.distribution_systems["230/400 V"];
    assert_eq!((sys.l_n.as_deref(), sys.l_g.as_deref()), (Some("230 V (EU)"), Some("230 V (EU)")));
    assert_eq!(s.model.check(), Ok(()));
}

#[test]
fn a_wire_type_holds_sections_in_square_millimetres_and_their_ampacity() {
    let mut s = Session::new();
    let r = run(
        &mut s,
        "wire_type",
        json!({"name": "Al XLPE 90 °C", "material": "aluminium", "temperature": 90, "insulation": "XLPE", "note": "Method C",
               "sizes": [{"section": 16, "ampacity": 61}, {"section": 25, "ampacity": 79, "label": "AA 25"}, {"section": 35, "ampacity": 96}]}),
    );
    assert_eq!(r["name"], json!("Al XLPE 90 °C"));
    let w = &s.model.wire_types["Al XLPE 90 °C"];
    assert_eq!((w.material.as_str(), w.temperature, w.insulation.as_str(), w.note.as_str()), ("aluminium", 90.0, "XLPE", "Method C"));
    assert_eq!(w.sizes.len(), 3);
    assert_eq!(s.model.wire_ampacity("Al XLPE 90 °C", 25.0), Some(79.0));
    assert_eq!(s.model.wire_ampacity("Al XLPE 90 °C", 50.0), None, "a section the table lacks has none");
    assert_eq!(s.model.wire_ampacity("nope", 25.0), None);
    // The sizes are the table: given again, they replace it; what else is not given stays.
    run(&mut s, "wire_type", json!({"name": "Al XLPE 90 °C", "sizes": [{"section": 50, "ampacity": 115}]}));
    let w = &s.model.wire_types["Al XLPE 90 °C"];
    assert_eq!((w.sizes.len(), w.insulation.as_str()), (1, "XLPE"));
    assert_eq!(run(&mut s, "wire_type", json!({}))["wire_types"]["Al XLPE 90 °C"]["sizes"][0]["section"], json!(50.0));
    run(&mut s, "wire_type", json!({"name": "Al XLPE 90 °C", "rename": "Al 90"}));
    assert!(in_library(&s, Kind::WireType, "Al 90") && !in_library(&s, Kind::WireType, "Al XLPE 90 °C"));
    run(&mut s, "wire_type", json!({"name": "Al 90", "delete": true}));
    assert!(!s.model.wire_types.contains_key("Al 90"));
}

#[test]
fn a_wire_type_that_is_not_one_is_refused() {
    let mut s = Session::new();
    let ok =
        json!({"name": "W", "material": "copper", "temperature": 70, "insulation": "PVC", "sizes": [{"section": 1.5, "ampacity": 17.5}]});
    let with = |k: &str, v: Value| {
        let mut p = ok.clone();
        p[k] = v;
        p
    };
    let sizes = |v: Value| with("sizes", v);
    for (p, says) in [
        (with("material", json!("gold")), "copper"),
        (with("temperature", json!(0)), "temperature"),
        (with("temperature", json!(1e300)), "temperature"),
        (with("insulation", json!("x".repeat(300))), "insulation"),
        (sizes(json!([])), "sizes"),
        (sizes(json!("x")), "sizes"),
        (sizes(json!([{"section": 0, "ampacity": 10}])), "section"),
        (sizes(json!([{"section": -1.5, "ampacity": 10}])), "section"),
        (sizes(json!([{"section": 1.5, "ampacity": 0}])), "ampacity"),
        (sizes(json!([{"section": 1.5, "ampacity": 1e300}])), "ampacity"),
        (sizes(json!([{"section": 1.5}])), "ampacity"),
        (sizes(json!([{"section": 1.5, "ampacity": 10, "gauge": 5}])), "gauge"),
        // The sections go up, and a larger one never carries less.
        (sizes(json!([{"section": 2.5, "ampacity": 24}, {"section": 1.5, "ampacity": 17.5}])), "section"),
        (sizes(json!([{"section": 1.5, "ampacity": 17.5}, {"section": 1.5, "ampacity": 17.5}])), "section"),
        (sizes(json!([{"section": 1.5, "ampacity": 17.5}, {"section": 2.5, "ampacity": 10}])), "ampacity"),
        (json!({"name": "W2", "temperature": 70, "insulation": "PVC", "sizes": [{"section": 1.5, "ampacity": 17.5}]}), "material"),
        (json!({"name": "Cu PVC 70 °C", "rename": "Cu THHN 90 °C"}), "Cu THHN 90 °C"),
        (json!({"name": "nope", "delete": true}), "nope"),
        (with("amps", json!(1)), "amps"),
    ] {
        let e = fails(&mut s, "wire_type", p.clone());
        assert!(e.contains(says), "{p}: {e}");
    }
}

#[test]
fn the_tables_hold_so_many_and_no_more() {
    let mut s = Session::new();
    let rows: Vec<Value> = (1..=electrical::MAX_SIZES + 1).map(|i| json!({"section": i, "ampacity": i})).collect();
    let e = fails(&mut s, "wire_type", json!({"name": "W", "material": "copper", "temperature": 70, "insulation": "PVC", "sizes": rows}));
    assert!(e.contains(&electrical::MAX_SIZES.to_string()), "{e}");
    for i in s.model.voltages.len()..electrical::MAX_VOLTAGES {
        run(&mut s, "voltage", json!({"name": format!("V{i}"), "nominal": 100 + i}));
    }
    let e = fails(&mut s, "voltage", json!({"name": "one too many", "nominal": 50}));
    assert!(e.contains(&electrical::MAX_VOLTAGES.to_string()), "{e}");
}

#[test]
fn the_settings_are_saved_with_the_project_and_come_back() {
    let mut s = Session::new();
    run(&mut s, "voltage", json!({"name": "690 V", "nominal": 690}));
    run(&mut s, "distribution_system", json!({"name": "Mina", "phases": 3, "wires": 3, "l_l": "690 V"}));
    run(
        &mut s,
        "wire_type",
        json!({"name": "W", "material": "copper", "temperature": 70, "insulation": "PVC", "sizes": [{"section": 4, "ampacity": 32}]}),
    );
    let guid = |s: &Session, k: Kind, n: &str| s.model.library.entries().find(|(_, e)| e.kind == k && e.name == n).map(|(_, e)| e.guid);
    let path = std::env::temp_dir().join(format!("bimcraft-electrical-{}.bimcraft", std::process::id()));
    run(&mut s, "save", json!({"path": path.to_str().unwrap()}));
    // A file holds them by GUID, and a system names its voltages by theirs.
    let file: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let g = guid(&s, Kind::Voltage, "690 V").unwrap().to_string();
    assert_eq!(file["distribution_systems"]["Mina"]["l_l"], json!(g), "{}", file["distribution_systems"]["Mina"]);
    let mut t = Session::new();
    run(&mut t, "open", json!({"path": path.to_str().unwrap()}));
    assert_eq!(t.model.voltages, s.model.voltages);
    assert_eq!(t.model.distribution_systems, s.model.distribution_systems);
    assert_eq!(t.model.wire_types, s.model.wire_types);
    for (k, n) in [(Kind::Voltage, "690 V"), (Kind::DistributionSystem, "Mina"), (Kind::WireType, "W")] {
        assert_eq!(guid(&t, k.clone(), n), guid(&s, k, n), "{n} keeps its GUID");
    }
    // A file from before the tables opens with the starter set, and nothing left out.
    let mut old = file;
    for k in ["voltages", "distribution_systems", "wire_types"] {
        old.as_object_mut().unwrap().remove(k);
    }
    std::fs::write(&path, old.to_string()).unwrap();
    let mut u = Session::new();
    let r = run(&mut u, "open", json!({"path": path.to_str().unwrap()}));
    assert!(r["left_out"].as_array().is_none_or(Vec::is_empty), "nothing is left out: {r}");
    assert!(u.model.voltages.contains_key("230 V") && u.model.distribution_systems.contains_key("230/400 V"));
    assert_eq!(u.model.check(), Ok(()));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn inspect_lists_the_settings() {
    let mut s = Session::new();
    let r = run(&mut s, "inspect", json!({}));
    assert_eq!(r["voltages"]["230 V"]["nominal"], json!(230.0));
    assert_eq!(r["distribution_systems"]["230/400 V"]["l_l"], json!("400 V"));
    assert_eq!(r["wire_types"]["Cu PVC 70 °C"]["sizes"][0]["section"], json!(0.5));
}

#[test]
fn the_settings_come_in_with_insert_from_file() {
    let mut from = Session::new();
    run(&mut from, "voltage", json!({"name": "690 V", "nominal": 690}));
    run(&mut from, "distribution_system", json!({"name": "Mina", "phases": 3, "wires": 3, "l_l": "690 V"}));
    run(
        &mut from,
        "wire_type",
        json!({"name": "W", "material": "copper", "temperature": 70, "insulation": "PVC", "sizes": [{"section": 4, "ampacity": 32}]}),
    );
    run(&mut from, "wall", json!({"points": [[0, 0], [3000, 0]]}));
    let path = std::env::temp_dir().join(format!("bimcraft-electrical-insert-{}.bimcraft", std::process::id()));
    run(&mut from, "save", json!({"path": path.to_str().unwrap()}));
    let mut to = Session::new();
    let r = run(&mut to, "insert", json!({"path": path.to_str().unwrap()}));
    let came: Vec<&str> = r["library"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    for what in ["voltage: 690 V", "distribution system: Mina", "wire type: W"] {
        assert!(came.contains(&what), "{what} in {came:?}");
    }
    assert_eq!(to.model.distribution_systems["Mina"].l_l.as_deref(), Some("690 V"));
    assert_eq!(to.model.check(), Ok(()));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn the_starter_set_is_data_that_passes_its_own_checks() {
    let (v, d, w) = (electrical::voltages(), electrical::distribution_systems(), electrical::wire_types());
    assert!(!v.is_empty() && !d.is_empty() && !w.is_empty());
    for (n, x) in &v {
        assert_eq!(x.check(n), Ok(()));
    }
    for (n, x) in &d {
        assert_eq!(x.check(n, &v), Ok(()));
    }
    for (n, x) in &w {
        assert_eq!(x.check(n), Ok(()));
    }
}
