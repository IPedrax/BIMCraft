#![cfg(test)]

use bimcraft_core::engine::Session;
use serde_json::{Value, json};

#[allow(clippy::panic)] // Command assertion helper used only by these integration tests.
fn run(s: &mut Session, p: Value) -> Value {
    s.execute("family", &p).unwrap_or_else(|e| panic!("{p}: {e}"))
}

#[allow(clippy::unwrap_used)] // Test fixture creation must succeed.
fn setup() -> (Session, tempfile::TempDir, std::path::PathBuf) {
    let mut s = Session::new();
    run(
        &mut s,
        json!({"name":"Reload", "parameters":[
        {"name":"Width","kind":"length","value":100},
        {"name":"Height","kind":"length","instance":true,"value":50}
    ], "types":{"A":{"Width":150},"Project only":{"Width":175}},
    "forms":[{"profile":[[0,0],["Width",0],["Width",20],[0,20]],"to":"Height"}],
    "symbols":[{"line":[[0,0],[100,0]]}]}),
    );
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("reload.json");
    std::fs::write(
        &file,
        json!({"name":"Reload", "parameters":[
            {"name":"Width","kind":"length","value":200},
            {"name":"Height","kind":"length","instance":true,"value":80},
            {"name":"New","kind":"number","value":5}
        ], "types":{"A":{"Width":250},"New type":{"Width":300}},
        "forms":[{"profile":[[0,0],["Width",0],["Width",40],[0,40]],"to":"Height"}]
        })
        .to_string(),
    )
    .unwrap();
    (s, dir, file)
}

#[test]
fn no_keeps_project_definition_and_history() {
    let (mut s, _dir, file) = setup();
    let before = s.model.to_json().unwrap();
    let reply = run(&mut s, json!({"path":file,"overwrite":"no"}));
    assert!(s.model.to_json().unwrap() == before, "the project snapshot changed");
    assert_eq!(reply["overwritten"], false);
    s.execute("undo", &json!({})).unwrap();
    assert!(!s.model.families.contains_key("Reload"), "a skipped load must not add an undo step");
}

#[test]
fn version_replaces_definition_but_keeps_project_values() {
    let (mut s, _dir, file) = setup();
    let before = s.model.to_json().unwrap();
    let old_form = s.model.families["Reload"].forms[0].guid;
    run(&mut s, json!({"path":file,"overwrite":"version"}));
    let f = &s.model.families["Reload"];
    assert_eq!(f.parameters[0].value, json!(100));
    assert_eq!(f.parameters[1].value, json!(50));
    assert_eq!(f.parameters[2].value, json!(5));
    assert_eq!(f.types["A"]["Width"], json!(150));
    assert_eq!(f.types["Project only"]["Width"], json!(175));
    assert_eq!(f.types["New type"]["Width"], json!(300));
    assert!(f.symbols.is_empty(), "omitted definition fields reset on explicit replacement");
    assert_eq!(f.forms[0].guid, old_form);
    assert_eq!(serde_json::to_value(&f.forms[0]).unwrap()["profile"][2][1], 40.0);
    s.execute("undo", &json!({})).unwrap();
    assert!(s.model.to_json().unwrap() == before, "the project snapshot changed");
}

#[test]
fn version_and_values_uses_incoming_values() {
    let (mut s, _dir, file) = setup();
    run(&mut s, json!({"path":file,"overwrite":"version_and_values"}));
    let f = &s.model.families["Reload"];
    assert_eq!(f.parameters[0].value, json!(200));
    assert_eq!(f.parameters[1].value, json!(80));
    assert_eq!(f.types["A"]["Width"], json!(250));
    assert!(f.types.contains_key("Project only"));
    assert!(f.symbols.is_empty());
}

#[test]
fn omitted_policy_keeps_legacy_merge_and_file_cannot_choose_policy() {
    let (mut s, _dir, file) = setup();
    let mut data: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    data["overwrite"] = json!("no");
    std::fs::write(&file, data.to_string()).unwrap();
    run(&mut s, json!({"path":file}));
    assert_eq!(s.model.families["Reload"].types["A"]["Width"], json!(250));
    assert!(!s.model.families["Reload"].symbols.is_empty());
}

#[test]
fn overwrite_is_strict_and_only_for_loading() {
    let (mut s, _dir, file) = setup();
    for value in [json!("typo"), json!(false), json!(3), Value::Null] {
        let before = s.model.to_json().unwrap();
        assert!(s.execute("family", &json!({"path":file,"overwrite":value})).is_err());
        assert!(s.model.to_json().unwrap() == before, "the project snapshot changed");
    }
    assert!(s.execute("family", &json!({"name":"Reload","overwrite":"no"})).is_err());
}

#[test]
fn overwrite_dry_run_is_nonmutating_and_new_family_loads_with_no() {
    let (mut s, _dir, file) = setup();
    let before = s.model.to_json().unwrap();
    let reply = run(&mut s, json!({"path":file,"overwrite":"version","dry_run":true}));
    assert!(s.model.to_json().unwrap() == before, "the project snapshot changed");
    assert_eq!(reply["family"]["types"]["A"]["Width"], json!(150));
    run(&mut s, json!({"path":file,"name":"New family","overwrite":"no"}));
    assert_eq!(s.model.families["New family"].types["A"]["Width"], json!(250));
}

#[test]
fn reload_keeps_explicit_instance_values_and_rolls_back_incompatible_changes() {
    for policy in ["version", "version_and_values"] {
        let (mut s, _dir, file) = setup();
        let placed = s.execute("component", &json!({"family":"Reload", "type":"A", "at":[0,0], "params":{"Height":65}})).unwrap();
        let id = placed["id"].as_u64().unwrap();
        let instance = s.model.elements.get(id).unwrap().clone();
        run(&mut s, json!({"path":file, "overwrite":policy}));
        assert_eq!(s.model.elements.get(id).unwrap(), &instance);
        let before = s.model.to_json().unwrap();
        let mut data: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        data["parameters"][1]["instance"] = json!(false);
        std::fs::write(&file, data.to_string()).unwrap();
        for dry in [true, false] {
            assert!(s.execute("family", &json!({"path":file,"overwrite":policy,"dry_run":dry})).is_err());
            assert!(s.model.to_json().unwrap() == before, "a failed reload changed the project");
        }
    }
}

#[test]
fn incoming_formulas_win_and_removed_parameters_do_not_survive_in_types() {
    let (mut s, _dir, file) = setup();
    run(
        &mut s,
        json!({"name":"Reload", "parameters":[
        {"name":"Width","kind":"length","value":100},
        {"name":"Height","kind":"length","instance":true,"value":50},
        {"name":"Removed","kind":"number","value":7}
    ], "types":{"A":{"Removed":9}}}),
    );
    let mut data: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    data["parameters"][0]["formula"] = json!("Height * 2");
    data["types"]["A"] = json!({});
    data["types"]["New type"] = json!({});
    std::fs::write(&file, data.to_string()).unwrap();
    run(&mut s, json!({"path":file, "overwrite":"version"}));
    let f = &s.model.families["Reload"];
    assert_eq!(f.parameters[0].formula.as_deref(), Some("Height * 2"));
    for values in f.types.values() {
        assert!(!values.contains_key("Removed"));
        assert!(!values.contains_key("Width"), "formula values must not be overridden");
    }
}

#[test]
fn new_types_keep_incoming_defaults_while_existing_types_keep_project_defaults() {
    let (mut s, _dir, file) = setup();
    run(&mut s, json!({"name":"Reload", "types":{"A":{"Width":null}}}));
    let mut data: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    data["types"]["New type"] = json!({});
    std::fs::write(&file, data.to_string()).unwrap();
    run(&mut s, json!({"path":file,"overwrite":"version"}));
    let f = &s.model.families["Reload"];
    assert_eq!(f.types["A"]["Width"], json!(100));
    assert_eq!(f.types["New type"]["Width"], json!(200));
}

#[test]
fn renamed_family_loads_as_new_and_renamed_type_is_added_without_moving_instances() {
    let (mut s, _dir, file) = setup();
    let placed = s.execute("component", &json!({"family":"Reload","type":"A","at":[0,0]})).unwrap();
    let id = placed["id"].as_u64().unwrap();
    let instance = s.model.elements.get(id).unwrap().clone();
    let original = s.model.families["Reload"].clone();
    run(&mut s, json!({"name":"Reload","save":file}));
    let mut data: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    data["name"] = json!("Renamed family");
    std::fs::write(&file, data.to_string()).unwrap();
    run(&mut s, json!({"path":file,"overwrite":"no"}));
    assert_eq!(s.model.families["Reload"], original);
    assert!(s.model.families.contains_key("Renamed family"));
    assert_ne!(s.model.families["Reload"].forms[0].guid, s.model.families["Renamed family"].forms[0].guid);
    data["name"] = json!("Reload");
    let values = data["types"].as_object_mut().unwrap().remove("A").unwrap();
    data["types"]["Renamed type"] = values;
    std::fs::write(&file, data.to_string()).unwrap();
    run(&mut s, json!({"path":file,"overwrite":"version_and_values"}));
    assert!(s.model.families["Reload"].types.contains_key("A"));
    assert!(s.model.families["Reload"].types.contains_key("Renamed type"));
    assert_eq!(s.model.elements.get(id).unwrap(), &instance);
    s.model.check().unwrap();
}

#[test]
fn unchanged_family_load_is_skipped_without_an_undo_step() {
    for policy in ["no", "version", "version_and_values"] {
        let (mut s, _dir, file) = setup();
        run(&mut s, json!({"name":"Reload", "types":{"A":{"Width":null}}}));
        let before = s.model.to_json().unwrap();
        run(&mut s, json!({"name":"Reload","save":file}));
        let reply = run(&mut s, json!({"path":file,"overwrite":policy}));
        assert_eq!(reply["overwritten"], false);
        assert!(s.model.to_json().unwrap() == before);
        s.execute("undo", &json!({})).unwrap();
        assert_eq!(s.model.families["Reload"].types["A"]["Width"], json!(150));
    }
}

#[test]
fn reload_only_carries_values_between_matching_shared_identities() {
    let guid_a = json!("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa");
    let guid_b = json!("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb");
    for (old_shared, new_shared, expected) in [
        (Value::Null, Value::Null, 100),
        (guid_a.clone(), guid_a.clone(), 100),
        (guid_a.clone(), guid_b.clone(), 200),
        (guid_a.clone(), Value::Null, 200),
        (Value::Null, guid_a.clone(), 200),
    ] {
        let mut s = Session::new();
        run(&mut s, json!({"name":"Identity", "parameters":[{"name":"Size","value":100,"shared":old_shared}],"types":{"A":{"Size":150}}}));
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("identity.json");
        std::fs::write(
            &file,
            json!({"name":"Identity","parameters":[{"name":"Size","value":200,"shared":new_shared}],"types":{"A":{"Size":250}}})
                .to_string(),
        )
        .unwrap();
        run(&mut s, json!({"path":file,"overwrite":"version"}));
        let f = &s.model.families["Identity"];
        assert_eq!(f.parameters[0].value, json!(expected));
        assert_eq!(f.types["A"]["Size"], json!(if expected == 100 { 150 } else { 250 }));
    }
}

#[test]
fn reporting_values_are_never_carried_over_on_reload() {
    let mut s = Session::new();
    run(&mut s, json!({"name":"Reporting","parameters":[{"name":"Measured","value":10,"reporting":true}],"types":{"A":{}}}));
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("reporting.json");
    std::fs::write(
        &file,
        json!({"name":"Reporting","parameters":[{"name":"Measured","value":20,"reporting":true}],"types":{"A":{},"New":{}}}).to_string(),
    )
    .unwrap();
    run(&mut s, json!({"path":file,"overwrite":"version"}));
    let f = &s.model.families["Reporting"];
    assert_eq!(f.parameters[0].value, json!(20));
    assert!(f.types.values().all(|t| !t.contains_key("Measured")));
}

#[test]
fn family_type_reload_keeps_values_null_and_uses_incoming_category() {
    for of in ["door", "furniture"] {
        let mut s = Session::new();
        run(
            &mut s,
            json!({"name":"Family types","parameters":[{"name":"Part","kind":"familytype","of":"door"}],"types":{"A":{"Part":null}}}),
        );
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("familytypes.json");
        std::fs::write(&file,json!({"name":"Family types","parameters":[{"name":"Part","kind":"familytype","of":of,"group":"constraints"}],"types":{"A":{}}}).to_string()).unwrap();
        run(&mut s, json!({"path":file,"overwrite":"version"}));
        let f = &s.model.families["Family types"];
        assert!(f.parameters[0].value.is_null());
        assert_eq!(serde_json::to_value(&f.parameters[0]).unwrap()["of"], json!(of));
        assert!(f.types.values().all(|t| t.get("Part").is_none_or(Value::is_null)));
    }
}
