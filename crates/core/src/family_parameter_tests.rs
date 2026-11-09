//! A family's parameters as Revit's Parameter Properties describe them: their kinds (text, integer, area, volume, URL and
//! family type besides lengths, numbers, angles, yes/no and materials), the group each is listed under, Family or Shared
//! (a GUID of its own), Type or Instance switched without losing what components hold, and reporting parameters. Driven
//! through `Session::execute`, as every front end drives it.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::family::Group;
use crate::library::Kind as LibKind;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn err(s: &mut Session, id: &str, p: Value) -> String {
    match s.execute(id, &p) {
        Ok(r) => panic!("{id} {p} should have been refused, and did {r}"),
        Err(e) => e,
    }
}

const FIRE: &str = "6f1c2a3b-4d5e-4f60-8a71-92b3c4d5e6f7";
const ACOUSTIC: &str = "0a1b2c3d-4e5f-4a6b-9c7d-8e9fa0b1c2d3";

/// A cabinet: a type parameter Width, an instance parameter Height, and a box of them.
fn cabinet(s: &mut Session) {
    run(
        s,
        "family",
        json!({
            "name": "Cabinet",
            "parameters": [
                {"name": "Width", "value": 600},
                {"name": "Depth", "value": 500},
                {"name": "Height", "instance": true, "value": 900}
            ],
            "types": {"Small": {"Width": 500}, "Large": {"Width": 800}, "Spare": {"Width": 1000}},
            "forms": [{"profile": [[0, 0], ["Width", 0], ["Width", "Depth"], [0, "Depth"]], "to": "Height"}]
        }),
    );
}

fn place(s: &mut Session, ty: &str, params: Value) -> u64 {
    let id = run(s, "component", json!({"family": "Cabinet", "type": ty, "at": [0, 0]}))["id"].as_u64().unwrap_or_default();
    if params.as_object().is_some_and(|p| !p.is_empty()) {
        run(s, "modify", json!({"id": id, "params": params}));
    }
    id
}

fn values(s: &Session, id: u64) -> Value {
    crate::element_json(&s.model, id)["values"].clone()
}

#[test]
fn a_family_holds_text_integer_area_volume_url_and_family_type_parameters() {
    let mut s = Session::new();
    let r = run(
        &mut s,
        "family",
        json!({
            "name": "Tagged Box",
            "parameters": [
                {"name": "Width", "value": 2000},
                {"name": "Depth", "value": 500},
                {"name": "Maker", "kind": "text", "value": "Acme"},
                {"name": "Shelves", "kind": "integer", "value": 3},
                {"name": "Footprint", "kind": "area", "formula": "Width * Depth"},
                {"name": "Space", "kind": "volume", "value": 2.5e8},
                {"name": "Datasheet", "kind": "url", "value": "https://example.org/box"},
                {"name": "Handle", "kind": "familytype", "of": "furniture"}
            ],
            "types": {"A": {"Maker": "Other Co", "Shelves": 4}},
            "forms": [{"profile": [[0, 0], ["Width", 0], ["Width", "Depth"], [0, "Depth"]], "to": 100}]
        }),
    );
    assert_eq!(r["warnings"], json!([]), "{r}");
    let read = run(&mut s, "family", json!({"name": "Tagged Box"}));
    let a = &read["values"]["A"];
    assert_eq!(a["Maker"], json!("Other Co"));
    assert_eq!(a["Shelves"], json!(4.0));
    assert_eq!(a["Footprint"], json!(1.0e6), "an area in mm², as its lengths are in mm");
    assert_eq!(a["Space"], json!(2.5e8));
    assert_eq!(a["Datasheet"], json!("https://example.org/box"));
    assert_eq!(a["Handle"], json!(""), "no family type is chosen until nested families exist");
}

#[test]
fn a_formula_reads_no_text_and_text_takes_no_formula() {
    let mut s = Session::new();
    let base = json!([{"name": "Width", "value": 600}, {"name": "Maker", "kind": "text", "value": "Acme"}]);
    let with = |extra: Value| {
        let mut ps = base.as_array().cloned().unwrap_or_default();
        ps.push(extra);
        json!({"name": "T", "parameters": ps, "types": {"A": {}}})
    };
    let e = err(&mut s, "family", with(json!({"name": "Odd", "formula": "Width + Maker"})));
    assert!(e.contains("`Maker`") && e.contains("text"), "{e}");
    for kind in ["text", "url", "familytype"] {
        let e = err(&mut s, "family", with(json!({"name": "Said", "kind": kind, "of": "furniture", "formula": "Width"})));
        assert!(e.contains("Said") && e.contains("no formula"), "{kind}: {e}");
    }
    let e = err(&mut s, "family", with(json!({"name": "Link", "kind": "url", "value": "https://a.b/\nx"})));
    assert!(e.contains("Link"), "{e}");
}

#[test]
fn an_integer_is_whole_and_its_formula_rounds() {
    let mut s = Session::new();
    let fam = |shelves: Value| {
        json!({
            "name": "Bay Rack",
            "parameters": [{"name": "Width", "value": 1000}, {"name": "Bays", "kind": "integer", "formula": "Width / 300"}, shelves],
            "types": {"A": {}}
        })
    };
    run(&mut s, "family", fam(json!({"name": "Shelves", "kind": "integer", "value": 2})));
    assert_eq!(run(&mut s, "family", json!({"name": "Bay Rack"}))["values"]["A"]["Bays"], json!(3.0));
    run(&mut s, "family", json!({"name": "Bay Rack", "types": {"A": {"Width": 1050}}}));
    assert_eq!(run(&mut s, "family", json!({"name": "Bay Rack"}))["values"]["A"]["Bays"], json!(4.0), "3.5 rounds away from zero");
    let e = err(&mut s, "family", fam(json!({"name": "Shelves", "kind": "integer", "value": 2.5})));
    assert!(e.contains("Shelves") && e.contains("whole number"), "{e}");
    let e = err(&mut s, "family", json!({"name": "Bay Rack", "types": {"A": {"Shelves": 1.5}}}));
    assert!(e.contains("whole number"), "{e}");
}

#[test]
fn schedules_show_a_familys_area_in_square_metres_and_its_volume_in_cubic_metres() {
    let mut s = Session::new();
    run(
        &mut s,
        "family",
        json!({
            "name": "Plinth",
            "parameters": [
                {"name": "Width", "value": 2000}, {"name": "Depth", "value": 1500},
                {"name": "Footprint", "kind": "area", "formula": "Width * Depth"},
                {"name": "Bulk", "kind": "volume", "formula": "Width * Depth * 1000"},
                {"name": "Pieces", "kind": "integer", "value": 2},
                {"name": "Maker", "kind": "text", "value": "Acme"}
            ],
            "types": {"A": {}},
            "forms": [{"profile": [[0, 0], ["Width", 0], ["Width", "Depth"], [0, "Depth"]], "to": 1000}],
            "symbols": [{"text": {"at": [0, 0], "height": 100, "value": "{Footprint} M2"}}]
        }),
    );
    let f = &s.model.families["Plinth"];
    let marks = f.values("A", &Default::default()).and_then(|v| f.marks(&v)).unwrap_or_default();
    let label = marks.iter().find_map(|m| match m {
        crate::family::Mark::Label { text, .. } => Some(text.clone()),
        _ => None,
    });
    assert_eq!(label.as_deref(), Some("3 M2"), "a label says an area in m²");
    run(&mut s, "component", json!({"family": "Plinth", "at": [0, 0]}));
    let r =
        run(&mut s, "schedule", json!({"name": "Plinths", "category": "component", "fields": ["Footprint", "Bulk", "Pieces", "Maker"]}));
    assert_eq!(r["columns"][0], json!("Footprint (m²)"), "{r}");
    assert_eq!(r["rows"], json!([[3.0, 3.0, 2.0, "Acme"]]), "{r}");
}

#[test]
fn a_parameter_is_grouped_by_its_kind_unless_told_and_keeps_its_group_in_files() {
    let mut s = Session::new();
    cabinet(&mut s);
    run(&mut s, "family", json!({"parameter": {"name": "Finish", "kind": "material"}, "name": "Cabinet"}));
    run(&mut s, "family", json!({"name": "Cabinet", "parameter": {"name": "Maker", "kind": "text", "group": "identity-data"}}));
    let group = |s: &Session, n: &str| s.model.families["Cabinet"].parameters.iter().find(|p| p.name == n).map(|p| p.group());
    assert_eq!(group(&s, "Width"), Some(Group::Dimensions));
    assert_eq!(group(&s, "Finish"), Some(Group::MaterialsAndFinishes));
    assert_eq!(group(&s, "Maker"), Some(Group::IdentityData));
    run(&mut s, "family", json!({"name": "Cabinet", "parameter": {"name": "Width", "group": "constraints"}}));
    assert_eq!(group(&s, "Width"), Some(Group::Constraints));
    let e = err(&mut s, "family", json!({"name": "Cabinet", "parameter": {"name": "Width", "group": "nowhere"}}));
    assert!(e.contains("group"), "{e}");
    // In a project file, and in a family file another project loads.
    let (back, left_out) = crate::model::Model::read(&s.model.to_json().unwrap_or_default()).unwrap_or_else(|e| panic!("{e}"));
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(back.families["Cabinet"], s.model.families["Cabinet"]);
    let path = std::env::temp_dir().join(format!("bimcraft-cabinet-params-{}.json", std::process::id()));
    run(&mut s, "family", json!({"name": "Cabinet", "save": path}));
    let mut other = Session::new();
    run(&mut other, "family", json!({"path": path}));
    let _ = std::fs::remove_file(&path);
    assert_eq!(other.model.families["Cabinet"].parameters, s.model.families["Cabinet"].parameters);
}

#[test]
fn a_type_parameter_made_instance_moves_each_types_value_onto_its_components() {
    let mut s = Session::new();
    cabinet(&mut s);
    let a = place(&mut s, "Small", json!({}));
    let b = place(&mut s, "Small", json!({}));
    let c = place(&mut s, "Large", json!({}));
    let before: Vec<Value> = [a, b, c].iter().map(|id| values(&s, *id)).collect();
    let r = run(&mut s, "family", json!({"name": "Cabinet", "parameter": {"name": "Width", "instance": true}}));
    assert_eq!(r["warnings"], json!([]), "{r}");
    let after: Vec<Value> = [a, b, c].iter().map(|id| values(&s, *id)).collect();
    assert_eq!(after, before, "every component keeps the width it had");
    let f = &s.model.families["Cabinet"];
    assert!(f.types.values().all(|t| !t.contains_key("Width")), "a type holds no instance value");
    assert_eq!(crate::element_json(&s.model, c)["params"]["Width"], json!(800));
}

#[test]
fn an_instance_parameter_made_type_takes_what_its_components_agree_on() {
    let mut s = Session::new();
    cabinet(&mut s);
    let a = place(&mut s, "Small", json!({"Height": 1000}));
    let b = place(&mut s, "Small", json!({"Height": 1000}));
    let c = place(&mut s, "Large", json!({}));
    let d = place(&mut s, "Large", json!({"Height": 700}));
    let e = err(&mut s, "family", json!({"name": "Cabinet", "parameter": {"name": "Height", "instance": false}}));
    assert!(e.contains(&c.to_string()) && e.contains(&d.to_string()) && e.contains("Large"), "{e}");
    run(&mut s, "modify", json!({"id": d, "params": {"Height": null}}));
    run(&mut s, "family", json!({"name": "Cabinet", "parameter": {"name": "Height", "instance": false}}));
    let f = &s.model.families["Cabinet"];
    assert_eq!(f.types["Small"]["Height"], json!(1000));
    assert!(!f.types["Large"].contains_key("Height"), "the default they share stays the family's");
    for (id, h) in [(a, 1000.0), (b, 1000.0), (c, 900.0), (d, 900.0)] {
        assert_eq!(values(&s, id)["Height"], json!(h));
        assert!(crate::element_json(&s.model, id)["params"].get("Height").is_none());
    }
}

#[test]
fn a_renamed_parameter_keeps_its_guid_and_what_names_it_follows() {
    let mut s = Session::new();
    cabinet(&mut s);
    run(
        &mut s,
        "family",
        json!({"name": "Cabinet", "parameter": {"name": "Top", "formula": "Height - 20"},
               "symbols": [{"text": {"at": [0, 0], "height": 100, "value": "H {Height}"}}]}),
    );
    let id = place(&mut s, "Small", json!({"Height": 1100}));
    let guid = s.model.library.guid(&LibKind::FamilyParameter, "Cabinet: Height");
    assert!(guid.is_some());
    run(&mut s, "family", json!({"name": "Cabinet", "parameter": {"name": "Height", "rename": "Tall Height"}}));
    assert_eq!(s.model.library.guid(&LibKind::FamilyParameter, "Cabinet: Tall Height"), guid);
    let f = &s.model.families["Cabinet"];
    let top = f.parameters.iter().find(|p| p.name == "Top").and_then(|p| p.formula.clone());
    assert_eq!(top.as_deref(), Some("Tall Height - 20"));
    assert_eq!(f.forms[0].to, crate::family::Expr::Formula("Tall Height".into()));
    assert_eq!(f.symbols[0].text.as_ref().map(|t| t.value.as_str()), Some("H {Tall Height}"));
    assert_eq!(values(&s, id)["Tall Height"], json!(1100.0));
    let e = err(&mut s, "family", json!({"name": "Cabinet", "parameter": {"name": "Width", "rename": "Depth"}}));
    assert!(e.contains("Depth"), "{e}");
}

#[test]
fn a_shared_parameter_keeps_its_name_and_kind() {
    let mut s = Session::new();
    cabinet(&mut s);
    run(&mut s, "family", json!({"name": "Cabinet", "parameter": {"name": "Fire Rating", "kind": "text", "shared": FIRE}}));
    let p = s.model.families["Cabinet"].parameters.iter().find(|p| p.name == "Fire Rating").cloned().unwrap_or_default();
    assert_eq!(p.shared.map(|g| g.to_string()).as_deref(), Some(FIRE));
    let e = err(&mut s, "family", json!({"name": "Cabinet", "parameter": {"name": "Fire Rating", "rename": "Rating"}}));
    assert!(e.contains("shared"), "{e}");
    let e = err(&mut s, "family", json!({"name": "Cabinet", "parameter": {"name": "Fire Rating", "kind": "number"}}));
    assert!(e.contains("shared"), "{e}");
    // Its group and Type/Instance are the family's to choose; and it may stop being shared.
    run(&mut s, "family", json!({"name": "Cabinet", "parameter": {"name": "Fire Rating", "instance": true, "group": "identity-data"}}));
    run(&mut s, "family", json!({"name": "Cabinet", "parameter": {"name": "Fire Rating", "shared": null}}));
    assert!(s.model.families["Cabinet"].parameters.iter().all(|p| p.shared.is_none()));
    // A family parameter stays in schedules, as it always was.
    run(&mut s, "component", json!({"family": "Cabinet", "at": [0, 0]}));
    let r = run(&mut s, "schedule", json!({"name": "Cabinets", "category": "component", "fields": ["Width"]}));
    assert_eq!(r["rows"], json!([[800.0]]), "the first type, Large: {r}");
}

#[test]
fn a_shared_guid_is_one_name_and_one_kind_across_the_project() {
    let mut s = Session::new();
    cabinet(&mut s);
    let e = err(
        &mut s,
        "family",
        json!({"name": "Cabinet", "parameters": [
            {"name": "Width", "value": 600}, {"name": "Depth", "value": 500}, {"name": "Height", "instance": true, "value": 900},
            {"name": "A", "kind": "text", "shared": FIRE}, {"name": "B", "kind": "text", "shared": FIRE}]}),
    );
    assert!(e.contains("`A`") && e.contains("`B`"), "{e}");
    for bad in [json!("00000000-0000-0000-0000-000000000000"), json!(12)] {
        let e = err(&mut s, "family", json!({"name": "Cabinet", "parameter": {"name": "R", "kind": "text", "shared": bad}}));
        assert!(e.contains("GUID"), "{e}");
    }
    run(&mut s, "family", json!({"name": "Cabinet", "parameter": {"name": "Fire Rating", "kind": "text", "shared": FIRE}}));
    let other = |p: Value| json!({"name": "Shelf", "parameters": [p], "types": {"A": {}}});
    let e = err(&mut s, "family", other(json!({"name": "Fire Rate", "kind": "text", "shared": FIRE})));
    assert!(e.contains("Fire Rate") && e.contains("Fire Rating") && e.contains("Cabinet"), "{e}");
    let e = err(&mut s, "family", other(json!({"name": "Fire Rating", "kind": "number", "shared": FIRE})));
    assert!(e.contains("Fire Rating") && e.contains("Cabinet"), "{e}");
    let e = err(&mut s, "family", other(json!({"name": "Fire Rating", "kind": "text", "shared": ACOUSTIC})));
    assert!(e.contains(FIRE) && e.contains(ACOUSTIC), "{e}");
    run(&mut s, "family", other(json!({"name": "Fire Rating", "kind": "text", "shared": FIRE})));
    // A family parameter of the same name, not shared, is a parameter of its own.
    run(&mut s, "family", json!({"name": "Plain", "parameters": [{"name": "Fire Rating", "kind": "text"}], "types": {"A": {}}}));
}

#[test]
fn a_reporting_parameter_is_a_length_or_an_angle_nothing_sets() {
    let mut s = Session::new();
    cabinet(&mut s);
    let r = run(&mut s, "family", json!({"name": "Cabinet", "parameter": {"name": "Reach", "reporting": true, "value": 450}}));
    assert!(r["warnings"].as_array().is_some_and(|w| w.iter().any(|w| w.as_str().is_some_and(|w| w.contains("Reach")))), "{r}");
    run(&mut s, "family", json!({"name": "Cabinet", "parameter": {"name": "Half Reach", "formula": "Reach / 2"}}));
    assert_eq!(run(&mut s, "family", json!({"name": "Cabinet"}))["values"]["Small"]["Half Reach"], json!(225.0));
    let e = err(&mut s, "family", json!({"name": "Cabinet", "parameter": {"name": "Reach", "formula": "Width"}}));
    assert!(e.contains("Reach") && e.contains("reporting"), "{e}");
    let e = err(&mut s, "family", json!({"name": "Cabinet", "types": {"Small": {"Reach": 10}}}));
    assert!(e.contains("Reach") && e.contains("reporting"), "{e}");
    let e = err(&mut s, "family", json!({"name": "Cabinet", "parameter": {"name": "Count", "kind": "number", "reporting": true}}));
    assert!(e.contains("Count") && e.contains("length or an angle"), "{e}");
    run(&mut s, "family", json!({"name": "Cabinet", "parameter": {"name": "Reach", "instance": true}}));
    let id = place(&mut s, "Small", json!({}));
    let e = err(&mut s, "modify", json!({"id": id, "params": {"Reach": 5}}));
    assert!(e.contains("Reach") && e.contains("reporting"), "{e}");
}

#[test]
fn a_family_type_parameter_names_a_category_and_holds_no_value_yet() {
    let mut s = Session::new();
    cabinet(&mut s);
    let e = err(&mut s, "family", json!({"name": "Cabinet", "parameter": {"name": "Handle", "kind": "familytype"}}));
    assert!(e.contains("Handle") && e.contains("`of`"), "{e}");
    let e = err(
        &mut s,
        "family",
        json!({"name": "Cabinet", "parameter": {"name": "Handle", "kind": "familytype", "of": "furniture", "value": "Knob: A"}}),
    );
    assert!(e.contains("Handle") && e.contains("value"), "{e}");
    let e = err(&mut s, "family", json!({"name": "Cabinet", "parameter": {"name": "Plain", "kind": "text", "of": "furniture"}}));
    assert!(e.contains("Plain") && e.contains("`of`"), "{e}");
    run(&mut s, "family", json!({"name": "Cabinet", "parameter": {"name": "Handle", "kind": "familytype", "of": "furniture"}}));
}

#[test]
fn a_parameter_changes_kind_only_when_its_values_read_as_the_new_one() {
    let mut s = Session::new();
    cabinet(&mut s);
    run(&mut s, "family", json!({"name": "Cabinet", "parameter": {"name": "Depth", "kind": "integer"}}));
    run(&mut s, "family", json!({"name": "Cabinet", "parameter": {"name": "Depth", "value": 512.5, "kind": "number"}}));
    let e = err(&mut s, "family", json!({"name": "Cabinet", "parameter": {"name": "Depth", "kind": "integer"}}));
    assert!(e.contains("Depth") && e.contains("whole number"), "{e}");
    let e = err(&mut s, "family", json!({"name": "Cabinet", "parameter": {"name": "Depth", "kind": "text"}}));
    assert!(e.contains("Depth"), "{e}");
}

#[test]
fn a_deleted_parameter_takes_its_values_and_is_refused_while_named() {
    let mut s = Session::new();
    cabinet(&mut s);
    run(&mut s, "family", json!({"name": "Cabinet", "parameter": {"name": "Note", "kind": "text", "instance": true}}));
    let id = place(&mut s, "Small", json!({"Note": "keep dry"}));
    let e = err(&mut s, "family", json!({"name": "Cabinet", "delete_parameter": ["Height"]}));
    assert!(e.contains("Height"), "{e}");
    let e = err(&mut s, "family", json!({"name": "Cabinet", "delete_parameter": ["Nope"]}));
    assert!(e.contains("Nope"), "{e}");
    let r = run(&mut s, "family", json!({"name": "Cabinet", "delete_parameter": ["Note"], "dry_run": true}));
    assert_eq!(r["changes"]["parameters"]["removed"], json!(["Note"]), "{r}");
    run(&mut s, "family", json!({"name": "Cabinet", "delete_parameter": ["Note"]}));
    assert!(s.model.families["Cabinet"].parameters.iter().all(|p| p.name != "Note"));
    assert!(crate::element_json(&s.model, id)["params"].get("Note").is_none());
}

#[test]
fn a_file_of_the_format_before_reads_every_parameter_as_a_family_one() {
    let mut s = Session::new();
    cabinet(&mut s);
    let mut v: Value = serde_json::from_str(&s.model.to_json().unwrap_or_default()).unwrap_or_default();
    v["format"] = json!(17);
    let (m, left_out) = crate::model::Model::read(&v.to_string()).unwrap_or_else(|e| panic!("{e}"));
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(m.format, crate::model::FORMAT);
    let f = &m.families["Cabinet"];
    assert!(f.parameters.iter().all(|p| p.shared.is_none() && !p.reporting && p.group.is_none()));
    assert_eq!(f.parameters.iter().find(|p| p.name == "Height").map(|p| p.group()), Some(Group::Dimensions));
    const { assert!(crate::model::FORMAT >= 19) };
}
