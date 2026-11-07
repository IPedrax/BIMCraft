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

#[test]
fn reload_preserves_reference_planes_lines_and_form_hosts() {
    use bimcraft_core::family::{Expr, Family, Form};
    use bimcraft_core::family::{Face, PlaneRef, RefLine, Reference};
    use bimcraft_core::ids::Guid;
    use bimcraft_core::model::Model;
    let plane_guid = Guid::from_u128(0xcccccccc_cccc_4ccc_8ccc_cccccccccc11);
    let line_guid = Guid::from_u128(0xcccccccc_cccc_4ccc_8ccc_cccccccccc12);
    let mut f = Family::default();
    f.types.insert("A".into(), Default::default());
    let mut plane = f.planes[0].clone();
    plane.guid = plane_guid;
    plane.name = "Offset plane".into();
    plane.start = [0., 0., 100.];
    plane.end = [100., 0., 100.];
    plane.up = [0., 1., 0.];
    plane.level = false;
    plane.defines_origin = false;
    plane.reference = Reference::Weak;
    f.planes.push(plane);
    f.lines.push(RefLine {
        guid: line_guid,
        start: [0., 0., 100.],
        end: [100., 0., 100.],
        host: PlaneRef::Plane(plane_guid),
        ..Default::default()
    });
    f.forms.push(Form {
        host: PlaneRef::Line(line_guid, Face::Vertical),
        profile: vec![[0., 0.], [10., 0.], [10., 10.], [0., 10.]].into_iter().map(|p| p.map(Expr::Number)).collect(),
        to: Expr::Number(20.),
        ..Default::default()
    });
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("hosted.json");
    let mut source = serde_json::to_value(&f).unwrap();
    source["name"] = json!("Hosted reload");
    std::fs::write(&file, source.to_string()).unwrap();
    let mut s = Session::new();
    run(&mut s, json!({"path":file}));
    let before = s.model.families["Hosted reload"].clone();
    let old_pieces = before.pieces(&before.values("A", &Default::default()).unwrap()).unwrap();
    run(&mut s, json!({"name":"Hosted reload","save":file}));
    let mut source: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    source["forms"][0]["to"] = json!(40);
    std::fs::write(&file, source.to_string()).unwrap();
    run(&mut s, json!({"path":file,"overwrite":"version"}));
    let after = &s.model.families["Hosted reload"];
    assert_eq!(after.planes, before.planes);
    assert_eq!(after.lines, before.lines);
    assert_eq!(after.forms[0].host, before.forms[0].host);
    let new_pieces = after.pieces(&after.values("A", &Default::default()).unwrap()).unwrap();
    assert!(!new_pieces.is_empty());
    assert_ne!(old_pieces, new_pieces);
    let (restored, left_out) = Model::read(&s.model.to_json().unwrap()).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    restored.check().unwrap();
    assert_eq!(restored.families["Hosted reload"], *after);
}

#[test]
fn reload_replaces_all_family_options_and_resets_omitted_defaults() {
    use bimcraft_core::family::{Expr, Family};
    let old_options = json!({"work_plane_based":true,"always_vertical":false,"default_elevation":450,"cut_with_voids_when_loaded":true,"shared":true,
        "room_calculation_point":["Width / 2",0,0],"cut_in_views":false,"omniclass":{"number":"23.30","title":"Original"}});
    for policy in [None, Some("no"), Some("version"), Some("version_and_values")] {
        let mut s = Session::new();
        run(
            &mut s,
            json!({"name":"Options reload","parameters":[{"name":"Width","value":100}],"types":{"A":{}},
            "forms":[{"profile":[[0,0],[10,0],[10,10],[0,10]],"to":20}],"options":old_options}),
        );
        let before = s.model.families["Options reload"].clone();
        let mut incoming = before.clone();
        incoming.merge_options(&Family::default().options_json()).unwrap();
        incoming.forms[0].to = Expr::Number(40.);
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("options.json");
        let mut value = serde_json::to_value(&incoming).unwrap();
        value["name"] = json!("Options reload");
        std::fs::write(&file, value.to_string()).unwrap();
        let mut p = json!({"path":file});
        if let Some(policy) = policy {
            p["overwrite"] = json!(policy);
        }
        run(&mut s, p);
        let f = &s.model.families["Options reload"];
        if matches!(policy, Some("version" | "version_and_values")) {
            assert_eq!(f.options_json(), Family::default().options_json());
            assert_eq!(f.forms[0].to, Expr::Number(40.));
        } else {
            assert_eq!(f.options_json(), before.options_json());
        }
    }
}

#[test]
fn loading_and_reloading_a_file_keeps_nondefault_family_options() {
    use bimcraft_core::family::{Expr, Family};
    let mut f = Family::default();
    f.types.insert("A".into(), Default::default());
    let options = json!({"work_plane_based":true,"always_vertical":false,"default_elevation":300,"cut_with_voids_when_loaded":true,"shared":true,
        "room_calculation_point":[10,20,30],"cut_in_views":false,"omniclass":{"number":"23.30","title":"Incoming"}});
    f.merge_options(&options).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("incoming.json");
    let mut value = serde_json::to_value(&f).unwrap();
    value["name"] = json!("Incoming options");
    std::fs::write(&file, value.to_string()).unwrap();
    let mut s = Session::new();
    run(&mut s, json!({"path":file}));
    assert_eq!(s.model.families["Incoming options"].options_json(), f.options_json());
    run(&mut s, json!({"name":"Incoming options","save":file}));
    let mut value: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert!(value.get("options").is_none());
    assert_eq!(value["omniclass"]["title"], "Incoming");
    value["room_calculation_point"] = json!([11, 22, 33]);
    std::fs::write(&file, value.to_string()).unwrap();
    run(&mut s, json!({"path":file,"overwrite":"version"}));
    let after = &s.model.families["Incoming options"];
    assert_eq!(after.room_calculation_point, Some([Expr::Number(11.), Expr::Number(22.), Expr::Number(33.)]));
    assert_eq!(after.omniclass, f.omniclass);
    assert!(after.shared && after.work_plane_based && !after.always_vertical);
    assert_eq!(after.default_elevation, 300.0);
}

/// A file holding the project's family `name` as `save` writes it, after `change` has edited it.
#[allow(clippy::unwrap_used)] // Test fixture creation must succeed.
fn saved(s: &mut Session, name: &str, dir: &tempfile::TempDir, change: impl FnOnce(&mut Value)) -> std::path::PathBuf {
    let file = dir.path().join(format!("{name}.json"));
    run(s, json!({"name":name,"save":file}));
    let mut value: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    change(&mut value);
    std::fs::write(&file, value.to_string()).unwrap();
    file
}

#[test]
fn reload_keeps_components_hosted_on_planes_and_faces_where_they_are() {
    use bimcraft_core::model::Element;
    for policy in [None, Some("version"), Some("version_and_values")] {
        let mut s = Session::new();
        run(
            &mut s,
            json!({"name":"Light","category":"generic","parameters":[{"name":"Size","kind":"length","value":400}],"types":{"A":{}},
            "options":{"work_plane_based":true,"always_vertical":false,"default_elevation":300},
            "forms":[{"profile":[[0,0],["Size",0],["Size","Size"],[0,"Size"]],"to":100}]}),
        );
        s.execute("reference_plane", &json!({"name":"Ramp","start":[0,0,0],"end":[10000,0,0],"up":[0,0.8,0.6]})).unwrap();
        let wall = s.execute("wall", &json!({"points":[[0,-5000],[6000,-5000]],"height":3000})).unwrap()["ids"][0].as_u64().unwrap();
        let ids: Vec<u64> = [
            json!({"family":"Light","at":[2000,2000],"host":{"plane":"Ramp"},"angle":15}),
            json!({"family":"Light","at":[3000,-5000,1500],"host":{"face":{"element":wall,"face":"exterior"}}}),
            json!({"family":"Light","at":[1000,1000]}),
        ]
        .into_iter()
        .map(|p| s.execute("component", &p).unwrap()["id"].as_u64().unwrap())
        .collect();
        let placed: Vec<Element> = ids.iter().map(|id| s.model.elements.get(*id).unwrap().clone()).collect();
        let hosted = |e: &Element| matches!(e, Element::Component(c) if c.host.is_some());
        assert!(placed[..2].iter().all(hosted), "{placed:?}");
        let dir = tempfile::tempdir().unwrap();
        let file = saved(&mut s, "Light", &dir, |v| {
            v["forms"][0]["to"] = json!(250);
            v["parameters"][0]["value"] = json!(500);
        });
        let mut p = json!({"path":file});
        if let Some(policy) = policy {
            p["overwrite"] = json!(policy);
        }
        run(&mut s, p);
        let f = &s.model.families["Light"];
        assert!(f.work_plane_based && !f.always_vertical && f.default_elevation == 300.0, "{policy:?}");
        for (id, was) in ids.iter().zip(&placed) {
            assert_eq!(s.model.elements.get(*id).unwrap(), was, "{policy:?}: component {id} moved or lost its host");
        }
        s.model.check().unwrap();

        // A version that is no longer work plane-based can't strand them: the reload is refused and nothing moves. (A
        // legacy merge's `dry_run` checks the family alone, as it always has; only its real run meets the model's check.)
        let file = saved(&mut s, "Light", &dir, |v| v["work_plane_based"] = json!(false));
        let before = s.model.to_json().unwrap();
        for dry in if policy.is_some() { vec![true, false] } else { vec![false] } {
            let mut p = json!({"path":file,"dry_run":dry});
            if let Some(policy) = policy {
                p["overwrite"] = json!(policy);
            }
            let e = s.execute("family", &p).unwrap_err();
            assert!(e.contains("work plane-based"), "{policy:?}: {e}");
            assert!(s.model.to_json().unwrap() == before, "{policy:?}: a refused reload changed the project");
        }
    }
}

#[test]
fn reload_keeps_adaptive_definitions_and_placed_points() {
    use bimcraft_core::adaptive::{Operation, four_point_panel};
    use bimcraft_core::family::Expr;
    use bimcraft_core::model::Element;
    let distance = |s: &Session| {
        let d = s.model.families["Panel"].adaptive.as_ref().expect("the family is still adaptive");
        d.nodes
            .iter()
            .find_map(|n| match &n.operation {
                Operation::Extrude { distance, .. } => Some(distance.clone()),
                _ => None,
            })
            .expect("its panel")
    };
    for policy in [None, Some("no"), Some("version"), Some("version_and_values")] {
        let mut s = Session::new();
        run(
            &mut s,
            json!({"name":"Panel","parameters":[{"name":"Thickness","kind":"length","value":60}],"types":{"A":{}},
            "adaptive":four_point_panel()}),
        );
        let preview = s.execute("component", &json!({"family":"Panel","at":[100,200],"angle":30})).unwrap()["id"].as_u64().unwrap();
        let sources: Vec<Value> = (1..=4).map(|n| json!({"id":preview,"host":{"type":"frame","point":format!("p{n}")}})).collect();
        let capture = s.execute("adaptive_capture", &json!({"sources":sources})).unwrap();
        let framed = s.execute("adaptive_place", &json!({"family":"Panel","placements":[{"at":[3000,-500],"adaptive":capture}]})).unwrap()
            ["ids"][0]
            .as_u64()
            .unwrap();
        let placed: Vec<Element> = [preview, framed].iter().map(|id| s.model.elements.get(*id).unwrap().clone()).collect();
        assert!(matches!(&placed[1], Element::Component(c) if c.adaptive.is_some()));
        let definition = s.model.families["Panel"].adaptive.clone().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let file = saved(&mut s, "Panel", &dir, |v| {
            let panel = v["adaptive"]["nodes"].as_array_mut().unwrap().iter_mut().find(|n| n["id"] == "panel").unwrap();
            let extrude = &mut panel["operation"];
            assert_eq!(extrude["distance"].as_f64(), Some(60.0), "{extrude}");
            extrude["distance"] = json!("Thickness");
        });
        let mut p = json!({"path":file});
        if let Some(policy) = policy {
            p["overwrite"] = json!(policy);
        }
        run(&mut s, p);
        if policy == Some("no") {
            assert_eq!(s.model.families["Panel"].adaptive.as_ref(), Some(&definition));
        } else {
            assert_eq!(distance(&s), Expr::Formula("Thickness".into()), "{policy:?}");
        }
        for (id, was) in [preview, framed].iter().zip(&placed) {
            assert_eq!(s.model.elements.get(*id).unwrap(), was, "{policy:?}: adaptive component {id} changed");
            let builds = matches!(s.model.elements.get(*id), Some(Element::Component(c)) if !s.model.component_pieces(c).is_empty());
            assert!(builds, "{policy:?}: component {id} no longer builds");
        }
        s.model.check().unwrap();
        let (restored, left_out) = bimcraft_core::model::Model::read(&s.model.to_json().unwrap()).unwrap();
        assert!(left_out.is_empty(), "{left_out:?}");
        assert_eq!(restored.families["Panel"].adaptive, s.model.families["Panel"].adaptive);
    }
}

#[test]
fn reload_refuses_formulas_naming_what_the_incoming_family_lacks() {
    for policy in [None, Some("version"), Some("version_and_values")] {
        for (at, formula) in [("parameter", "Width * 2"), ("form", "Depth + 10")] {
            let (mut s, _dir, file) = setup();
            let mut data: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
            if at == "parameter" {
                // The project family has Width; the incoming one drops it while a formula still says it.
                data["parameters"][0] = json!({"name":"Length","kind":"length","value":200});
                data["parameters"][2]["formula"] = json!(formula);
                data["types"] = json!({"A":{"Length":250}});
                data["forms"][0]["profile"] = json!([[0, 0], ["Length", 0], ["Length", 40], [0, 40]]);
            } else {
                data["forms"][0]["to"] = json!(formula);
            }
            std::fs::write(&file, data.to_string()).unwrap();
            let unknown = if at == "parameter" { "Width" } else { "Depth" };
            let before = s.model.to_json().unwrap();
            for dry in [true, false] {
                let mut p = json!({"path":file,"dry_run":dry});
                if let Some(policy) = policy {
                    p["overwrite"] = json!(policy);
                }
                let e = s.execute("family", &p).unwrap_err();
                assert!(e.contains(&format!("not a valid parameter: {unknown}")) && e.contains(formula), "{policy:?} {at}: {e}");
                assert!(s.model.to_json().unwrap() == before, "{policy:?} {at}: a refused reload changed the project");
            }
            // `no` keeps the project's own definition and never takes the file's formulas in.
            let reply = run(&mut s, json!({"path":file,"overwrite":"no"}));
            assert_eq!(reply["overwritten"], false);
            assert!(s.model.to_json().unwrap() == before);
        }
    }
}

/// `definition` loads a family as its file holds it, given inline: the same policies, read strictly, and never with `path`.
#[test]
fn a_definition_loads_as_its_file_would_under_every_policy() {
    let (mut s, _dir, file) = setup();
    let definition: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    let before = s.model.to_json().unwrap();
    let reply = run(&mut s, json!({"definition":definition,"overwrite":"no"}));
    assert_eq!(reply["overwritten"], false);
    assert!(s.model.to_json().unwrap() == before, "`no` keeps the project's");
    run(&mut s, json!({"definition":definition,"overwrite":"version"}));
    assert_eq!(s.model.families["Reload"].types["A"]["Width"], json!(150));
    assert_eq!(s.model.families["Reload"].types["New type"]["Width"], json!(300));
    s.execute("undo", &json!({})).unwrap();
    run(&mut s, json!({"definition":definition,"overwrite":"version_and_values"}));
    assert_eq!(s.model.families["Reload"].types["A"]["Width"], json!(250));
    // The caller's name wins over the definition's; a new family loads whatever the policy.
    run(&mut s, json!({"definition":definition,"name":"Inline","overwrite":"version"}));
    assert_eq!(s.model.families["Inline"].types["A"]["Width"], json!(250));
    // Loaded again as the project holds it, it is the same: skipped.
    let same = serde_json::to_value(&s.model.families["Inline"]).unwrap();
    let reply = run(&mut s, json!({"definition":same,"name":"Inline","overwrite":"version_and_values"}));
    assert_eq!(reply["overwritten"], false);
    let before = s.model.to_json().unwrap();
    let mut typo = definition.clone();
    typo["paramters"] = json!([]);
    for bad in [
        json!({"definition":typo,"overwrite":"version"}),
        json!({"definition":definition,"path":file,"overwrite":"version"}),
        json!({"definition":[1, 2],"overwrite":"version"}),
        json!({"definition":{"types":{}},"overwrite":"version"}),
    ] {
        assert!(s.execute("family", &bad).is_err(), "{bad}");
        assert!(s.model.to_json().unwrap() == before, "the project snapshot changed");
    }
}
