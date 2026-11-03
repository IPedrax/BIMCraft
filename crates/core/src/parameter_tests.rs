//! Project parameters as a set, and between projects: driven through commands, as a front end drives them.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::library::Kind;
use crate::model::{Element, ParameterKind};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn file(name: &str) -> String {
    let dir = tempfile::tempdir().unwrap().keep();
    dir.join(name).to_string_lossy().into_owned()
}

/// A project with one wall; its id.
fn walled() -> (Session, u64) {
    let mut s = Session::new();
    let r = run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]]}));
    let id = r["id"].as_u64().or_else(|| r["ids"][0].as_u64()).unwrap();
    (s, id)
}

/// `s` saved; its path.
fn saved(s: &mut Session, name: &str) -> String {
    let path = file(name);
    run(s, "save", json!({"path": path}));
    path
}

fn params_of(s: &Session, id: u64) -> serde_json::Map<String, Value> {
    s.model.elements.get(id).and_then(Element::params).map(|p| p.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default()
}

/// The walls of `s`, last made last.
fn walls(s: &Session) -> Vec<u64> {
    s.model.elements.iter().filter(|(_, e)| matches!(e, Element::Wall(_))).map(|(id, _)| *id).collect()
}

#[test]
fn a_parameter_renamed_in_another_copy_is_still_itself_when_its_elements_come_in() {
    let (mut here, wall) = walled();
    run(&mut here, "parameter", json!({"name": "Fire Rating", "categories": ["wall"]}));
    run(&mut here, "modify", json!({"id": wall, "params": {"Fire Rating": "EI60"}}));
    let mut there = Session::new();
    there.reset((*here.model).clone(), None);
    run(&mut there, "parameter", json!({"name": "Fire Rating", "rename": "Fire Class"}));
    run(&mut there, "modify", json!({"id": wall, "params": {"Fire Class": "EI90"}}));
    let path = saved(&mut there, "there.bimcraft");
    let r = run(&mut here, "insert", json!({"path": path, "ids": [wall]}));
    assert_eq!(r["parameters"]["Fire Class"], "Fire Rating", "{r}");
    assert_eq!(here.model.parameters.len(), 1, "one parameter: {:?}", here.model.parameters.keys());
    let copy = *walls(&here).last().unwrap();
    assert_eq!(params_of(&here, copy)["Fire Rating"], "EI90");
}

#[test]
fn two_parameters_of_one_name_and_different_guids_stay_apart_unless_said_to_be_the_same() {
    let (mut here, wall) = walled();
    run(&mut here, "parameter", json!({"name": "Rating", "categories": ["wall"]}));
    run(&mut here, "modify", json!({"id": wall, "params": {"Rating": "fire"}}));
    let (mut there, other) = walled();
    run(&mut there, "parameter", json!({"name": "Rating", "categories": ["wall"]}));
    run(&mut there, "modify", json!({"id": other, "params": {"Rating": "acoustic"}}));
    let path = saved(&mut there, "there.bimcraft");

    let mut apart = Session::new();
    apart.reset((*here.model).clone(), None);
    let r = run(&mut apart, "insert", json!({"path": path, "ids": [other]}));
    assert_eq!(r["parameters"]["Rating"], "Rating 2", "{r}");
    let new = *walls(&apart).last().unwrap();
    assert_eq!(params_of(&apart, new)["Rating 2"], "acoustic");
    assert_eq!(params_of(&apart, wall)["Rating"], "fire");
    let there_guid = there.model.library.guid(&Kind::Parameter, "Rating").unwrap();
    assert_eq!(apart.model.library.guid(&Kind::Parameter, "Rating 2"), Some(there_guid), "it keeps its GUID");

    let r = run(&mut here, "insert", json!({"path": path, "ids": [other], "same_parameters": ["Rating"]}));
    assert_eq!(r["parameters"], json!({}), "{r}");
    assert_eq!(here.model.parameters.len(), 1);
    assert_eq!(here.model.parameters["Rating"].same_as, vec![there_guid], "remembered");
    // Remembered: asked no more, whatever it's called by then.
    run(&mut here, "parameter", json!({"name": "Rating", "rename": "Grade"}));
    let r = run(&mut here, "insert", json!({"path": path, "ids": [other]}));
    assert_eq!(r["parameters"]["Rating"], "Grade", "{r}");
}

#[test]
fn only_what_comes_in_uses_comes_along_and_a_parameter_takes_on_the_categories_it_is_held_for() {
    let (mut there, wall) = walled();
    run(&mut there, "parameter", json!({"name": "Used", "categories": ["wall"]}));
    run(&mut there, "parameter", json!({"name": "Unused", "kind": "number", "categories": ["roof"]}));
    run(&mut there, "modify", json!({"id": wall, "params": {"Used": "x"}}));
    let path = saved(&mut there, "there.bimcraft");
    let mut here = Session::new();
    run(&mut here, "insert", json!({"path": path, "ids": [wall]}));
    assert_eq!(here.model.parameters.keys().collect::<Vec<_>>(), ["Used"]);
    // A whole project brings all of them.
    let mut all = Session::new();
    run(&mut all, "insert", json!({"path": path}));
    assert_eq!(all.model.parameters.len(), 2);

    // Here for doors, same GUID, and a wall comes in holding it.
    let mut doors = Session::new();
    doors.reset((*there.model).clone(), None);
    run(&mut doors, "modify", json!({"id": wall, "params": {"Used": null}}));
    run(&mut doors, "parameter", json!({"name": "Used", "categories": ["door"]}));
    run(&mut doors, "insert", json!({"path": path, "ids": [wall]}));
    assert_eq!(doors.model.parameters["Used"].categories, ["door", "wall"]);
}

#[test]
fn a_value_a_parameter_here_cant_hold_is_left_off_and_said_and_the_rest_comes_in() {
    let (mut there, wall) = walled();
    run(&mut there, "parameter", json!({"name": "Rate", "kind": "text", "categories": ["wall"]}));
    let mut here = Session::new();
    here.reset((*there.model).clone(), None);
    run(&mut there, "modify", json!({"id": wall, "params": {"Rate": "high"}}));
    run(&mut here, "parameter", json!({"name": "Rate", "kind": "number"}));
    let n = walls(&here).len();
    let path = saved(&mut there, "there.bimcraft");
    let r = run(&mut here, "insert", json!({"path": path, "ids": [wall]}));
    assert_eq!(walls(&here).len(), n + 1);
    assert!(r["left_out"].to_string().contains("Rate"), "{r}");
}

#[test]
fn renaming_a_parameter_reaches_tags_formulas_and_files() {
    let (mut s, wall) = walled();
    run(&mut s, "parameter", json!({"name": "Finish Height", "kind": "number", "categories": ["wall"]}));
    run(&mut s, "parameter", json!({"name": "Finish Height 2", "kind": "number", "categories": ["wall"]}));
    run(&mut s, "modify", json!({"id": wall, "params": {"Finish Height": 2}}));
    let r = run(&mut s, "tag", json!({"ids": [wall], "label": "Finish Height"}));
    let tag = r["id"].as_u64().or_else(|| r["ids"][0].as_u64()).unwrap();
    let r = run(
        &mut s,
        "schedule",
        json!({"name": "S", "category": "wall", "fields": [{"name": "Finish Height"}, {"name": "Double", "formula": "Finish Height * 2 + Finish Height 2"}]}),
    );
    let sc = r["id"].as_u64().unwrap();
    run(&mut s, "parameter", json!({"name": "Finish Height", "rename": "Dado"}));
    let Some(Element::Tag(t)) = s.model.elements.get(tag) else { panic!() };
    assert_eq!(t.label.as_deref(), Some("Dado"));
    let Some(Element::Schedule(table)) = s.model.elements.get(sc) else { panic!() };
    assert_eq!(table.fields[1].formula.as_deref(), Some("Dado * 2 + Finish Height 2"), "whole words only");
    // A tag's label is the parameter's GUID in a file, and its name again when read.
    let text = s.model.to_json().unwrap();
    let guid = s.model.library.guid(&Kind::Parameter, "Dado").unwrap().to_string();
    assert!(text.contains(&format!("\"label\": \"{guid}\"")), "by GUID in a file");
    let (back, _) = crate::model::Model::read(&text).unwrap();
    let Some(Element::Tag(t)) = back.elements.get(tag) else { panic!() };
    assert_eq!(t.label.as_deref(), Some("Dado"));
}

#[test]
fn the_new_kinds_hold_their_values_and_go_through_a_file_and_ifc() {
    let (mut s, wall) = walled();
    for (name, kind) in [("Panels", "integer"), ("Paint Area", "area"), ("Fill", "volume"), ("Pitch", "angle")] {
        run(&mut s, "parameter", json!({"name": name, "kind": kind, "categories": ["wall"]}));
    }
    assert!(s.execute("modify", &json!({"id": wall, "params": {"Panels": 1.5}})).is_err(), "a whole number");
    run(&mut s, "modify", json!({"id": wall, "params": {"Panels": 3, "Paint Area": 12.5, "Fill": 2.0, "Pitch": 30.0}}));
    let path = saved(&mut s, "kinds.bimcraft");
    let mut again = Session::new();
    run(&mut again, "open", json!({"path": path}));
    assert_eq!(again.model.parameters["Paint Area"].kind, ParameterKind::Area);

    let ifc = file("kinds.ifc");
    run(&mut s, "save", json!({"path": ifc}));
    let text = std::fs::read_to_string(&ifc).unwrap();
    for needle in ["IFCINTEGER(3)", "IFCAREAMEASURE(12.5", "IFCVOLUMEMEASURE(2", "IFCPLANEANGLEMEASURE(0.5235"] {
        assert!(text.contains(needle), "{needle}");
    }
    let mut back = Session::new();
    run(&mut back, "open", json!({"path": ifc}));
    assert_eq!(back.model.parameters["Pitch"].kind, ParameterKind::Angle);
    let w = walls(&back)[0];
    assert_eq!(params_of(&back, w)["Pitch"].as_f64().unwrap().round(), 30.0);
    assert_eq!(params_of(&back, w)["Paint Area"], 12.5);
}

const SHARED: &str = "# This is a Revit shared parameter file.\r\n\
# Do not edit manually.\r\n\
*META\tVERSION\tMINVERSION\r\n\
META\t2\t1\r\n\
*GROUP\tID\tNAME\r\n\
GROUP\t1\tIdentity\r\n\
GROUP\t2\tFire\r\n\
*PARAM\tGUID\tNAME\tDATATYPE\tDATACATEGORY\tGROUP\tVISIBLE\tDESCRIPTION\tUSERMODIFIABLE\tHIDEWHENNOVALUE\r\n\
PARAM\t1f2e3d4c-5b6a-4789-8abc-def012345678\tFire Rating\tTEXT\t\t2\t1\tResistance, as the permit says\t1\t0\r\n\
PARAM\t2f2e3d4c-5b6a-4789-8abc-def012345679\tPanel Count\tINTEGER\t\t1\t0\t\t0\t1\r\n\
PARAM\t3f2e3d4c-5b6a-4789-8abc-def01234567a\tDepth (mm)\tLENGTH\t\t1\t1\t\t1\t0\r\n\
PARAM\t4f2e3d4c-5b6a-4789-8abc-def01234567b\tLoad\tFORCE\t\t1\t1\t\t1\t0\r\n\
PARAM\t5f2e3d4c-5b6a-4789-8abc-def01234567c\tArea Tag\tFAMILYTYPE\t-2005020\t1\t1\t\t1\t0\r\n\
PARAM\tnot-a-guid\tBroken\tTEXT\t\t1\t1\t\t1\t0\r\n";

#[test]
fn a_revit_shared_parameter_file_is_read_with_its_guids_and_given_categories_and_never_written() {
    let path = file("office.txt");
    // Revit writes UTF-16 as often as not.
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(SHARED.encode_utf16().flat_map(u16::to_le_bytes));
    std::fs::write(&path, bytes).unwrap();
    let mut s = Session::new();
    let r = run(&mut s, "parameter_import", json!({"path": path, "dry_run": true}));
    assert!(s.model.parameters.is_empty(), "a dry run changes nothing");
    assert_eq!(r["conflict"], 4, "a file with no categories needs them: {r}");
    assert!(r["left_out"].to_string().contains("Area Tag") && r["left_out"].to_string().contains("Broken"), "{r}");
    assert!(s.execute("parameter_import", &json!({"path": path})).is_err(), "a conflict stops it");

    let r = run(&mut s, "parameter_import", json!({"path": path, "categories": ["wall"]}));
    assert_eq!(r["create"], 4, "{r}");
    let m = &s.model;
    let rating = &m.parameters["Fire Rating"];
    assert_eq!((rating.group.as_str(), rating.description.as_str()), ("Fire", "Resistance, as the permit says"));
    assert_eq!(rating.dictionary.as_deref(), Some("office"), "from a file, so shared");
    assert_eq!(m.library.guid(&Kind::Parameter, "Fire Rating").unwrap().to_string(), "1f2e3d4c-5b6a-4789-8abc-def012345678");
    let panels = &m.parameters["Panel Count"];
    assert!(panels.hidden && panels.read_only && panels.hide_when_empty);
    assert_eq!(panels.kind, ParameterKind::Integer);
    assert!(m.parameters.contains_key("Depth mm"), "made nameable");
    assert_eq!(m.parameters["Load"].kind, ParameterKind::Number, "said in left_out");
    assert!(r["left_out"].to_string().contains("FORCE"), "{r}");

    let e = s.execute("parameter_export", &json!({"path": file("out.txt")})).unwrap_err();
    assert!(e.contains("doesn't write"), "{e}");
}

#[test]
fn a_table_goes_out_and_comes_back_and_its_categories_are_mapped() {
    let (mut s, wall) = walled();
    run(
        &mut s,
        "parameter",
        json!({"name": "Fire Rating", "categories": ["wall", "door"], "description": "=rm -rf, \"quoted\"", "group": "Fire"}),
    );
    run(&mut s, "modify", json!({"id": wall, "params": {"Fire Rating": "EI60"}}));
    let guid = s.model.library.guid(&Kind::Parameter, "Fire Rating").unwrap();
    for name in ["out.csv", "out.xlsx"] {
        let path = file(name);
        run(&mut s, "parameter_export", json!({"path": path}));
        let mut other = Session::new();
        let r = run(&mut other, "parameter_import", json!({"path": path}));
        assert_eq!(r["create"], 1, "{name}: {r}");
        let p = &other.model.parameters["Fire Rating"];
        assert_eq!(p.categories, ["door", "wall"], "{name}");
        assert_eq!(p.description, "=rm -rf, \"quoted\"", "{name}");
        assert_eq!(other.model.library.guid(&Kind::Parameter, "Fire Rating"), Some(guid), "{name}");
        assert_eq!(p.dictionary, None, "a table isn't a dictionary");
        // In itself, nothing changes.
        let r = run(&mut s, "parameter_import", json!({"path": path}));
        assert_eq!(r["existing"], 1, "{name}: {r}");
    }

    let path = file("map.csv");
    std::fs::write(&path, "Name;Kind;Categories\nA;Length;Walls; Doors\nB;Text;OST_StructuralFraming\nC;Number;Furniture\nD;Text;Gizmos\nE;Yes/No;Gizmos; Walls\n").unwrap();
    // `;` separates the columns here, so the file's categories go one to a column: a table that wants several quotes them.
    std::fs::write(&path, "Name;Kind;Categories\nA;Length;\"Walls; Doors\"\nB;Text;OST_StructuralFraming\nC;Number;Furniture\nD;Text;Gizmos\nE;Yes/No;\"Gizmos; Walls\"\n").unwrap();
    let mut t = Session::new();
    let r = run(&mut t, "parameter_import", json!({"path": path, "dry_run": true}));
    assert_eq!(r["unmapped"], json!(["Gizmos"]), "{r}");
    assert_eq!(r["broad"], json!(["Furniture"]), "{r}");
    assert_eq!(r["create"], 4, "E has Walls: {r}");
    assert_eq!(r["conflict"], 1, "D has none: {r}");
    let r = run(&mut t, "parameter_import", json!({"path": path, "map": {"Gizmos": "column"}}));
    assert_eq!(r["create"], 5, "{r}");
    let m = &t.model.parameters;
    assert_eq!((m["A"].categories.clone(), m["A"].kind), (vec!["door".to_string(), "wall".to_string()], ParameterKind::Length));
    assert_eq!(m["B"].categories, ["beam"]);
    assert_eq!(m["C"].categories, ["component"]);
    assert_eq!(m["D"].categories, ["column"]);
    assert_eq!(m["E"].categories, ["column", "wall"]);
    assert_eq!(m["E"].kind, ParameterKind::YesNo);
    // The mapping stays with the project, and in its file.
    assert_eq!(t.model.parameter_map["Gizmos"].as_deref(), Some("column"));
    let again = file("again.csv");
    std::fs::write(&again, "Name,Categories\nF,Gizmos\n").unwrap();
    let r = run(&mut t, "parameter_import", json!({"path": again}));
    assert_eq!(r["create"], 1, "{r}");
}

#[test]
fn a_row_of_a_name_taken_by_another_guid_waits_for_a_decision() {
    let (mut s, wall) = walled();
    run(&mut s, "parameter", json!({"name": "Rating", "categories": ["wall"]}));
    run(&mut s, "modify", json!({"id": wall, "params": {"Rating": "x"}}));
    let path = file("office.csv");
    let other = "7f2e3d4c-5b6a-4789-8abc-def012345678";
    std::fs::write(&path, format!("GUID,Name,Kind,Categories\n{other},Rating,text,Walls\n")).unwrap();
    let r = run(&mut s, "parameter_import", json!({"path": path, "dry_run": true}));
    assert_eq!(r["conflict"], 1, "{r}");
    assert!(s.execute("parameter_import", &json!({"path": path})).is_err());

    let mut apart = Session::new();
    apart.reset((*s.model).clone(), None);
    let r = run(&mut apart, "parameter_import", json!({"path": path, "resolve": {"Rating": "separate"}}));
    assert_eq!(r["create"], 1, "{r}");
    assert_eq!(apart.model.library.guid(&Kind::Parameter, "Rating 2").unwrap().to_string(), other);

    let r = run(&mut s, "parameter_import", json!({"path": path, "resolve": {"Rating": "same"}}));
    assert_eq!(s.model.parameters.len(), 1, "{r}");
    assert_eq!(s.model.parameters["Rating"].same_as[0].to_string(), other);
    let r = run(&mut s, "parameter_import", json!({"path": path, "dry_run": true}));
    assert_eq!((r["conflict"].clone(), r["existing"].clone()), (json!(0), json!(1)), "asked once: {r}");

    // A kind that would strand values is a conflict, whatever else.
    std::fs::write(&path, format!("GUID,Name,Kind,Categories\n{other},Rating,number,Walls\n")).unwrap();
    let r = run(&mut s, "parameter_import", json!({"path": path, "dry_run": true}));
    assert_eq!(r["conflict"], 1, "{r}");
}

#[test]
fn a_dictionary_is_made_shared_followed_and_opened_without_its_file() {
    let (mut s, wall) = walled();
    let path = file("office.bimparams");
    let r = run(
        &mut s,
        "dictionary",
        json!({"path": path, "version": "1.0.0", "add": [
            {"name": "Fire Rating", "kind": "text", "description": "Resistance", "group": "Fire", "categories": ["Walls"]},
            {"name": "Cost", "kind": "number", "categories": ["wall"]},
        ]}),
    );
    assert_eq!(r["parameters"].as_array().unwrap().len(), 2, "{r}");
    assert!(s.model.parameters.is_empty(), "the project isn't touched");
    let rating = r["parameters"][1]["guid"].as_str().unwrap().to_string();
    assert_eq!(r["parameters"][1]["name"], "Fire Rating", "{r}");

    let r = run(&mut s, "parameter_import", json!({"path": path}));
    assert_eq!(r["create"], 2, "{r}");
    assert_eq!(s.model.parameters["Cost"].dictionary.as_deref(), Some("office"));
    assert_eq!(s.model.dictionaries["office"].version, "1.0.0");
    let dict_guid = s.model.library.guid(&Kind::Dictionary, "office");
    assert!(dict_guid.is_some());
    assert_eq!(s.model.library.guid(&Kind::Parameter, "Fire Rating").unwrap().to_string(), rating);
    run(&mut s, "modify", json!({"id": wall, "params": {"Fire Rating": "EI60"}}));

    // The project opens with the dictionary gone, and its name goes by GUID in the file.
    let saved_at = saved(&mut s, "p.bimcraft");
    let text = std::fs::read_to_string(&saved_at).unwrap();
    assert!(text.contains(&format!("\"dictionary\": \"{}\"", dict_guid.unwrap())), "by GUID in a file");
    let moved = file("elsewhere.bimparams");
    std::fs::rename(&path, &moved).unwrap();
    let mut opened = Session::new();
    run(&mut opened, "open", json!({"path": saved_at}));
    assert_eq!(opened.model.parameters["Cost"].dictionary.as_deref(), Some("office"));

    // A new version: nothing changes until it's imported, then a rename and a new description follow, by GUID.
    let d: Value = serde_json::from_str(&std::fs::read_to_string(&moved).unwrap()).unwrap();
    let cost = d["parameters"].as_array().unwrap().iter().find(|p| p["name"] == "Cost").unwrap()["guid"].clone();
    run(
        &mut opened,
        "dictionary",
        json!({"path": moved, "version": "1.1.0", "add": [
            {"guid": rating, "name": "Fire Class", "kind": "text", "description": "As the permit says", "group": "Fire"},
            {"guid": cost, "name": "Cost", "kind": "number"},
        ]}),
    );
    assert!(opened.model.parameters.contains_key("Fire Rating"));
    let r = run(&mut opened, "parameter_import", json!({"path": moved, "dry_run": true}));
    assert_eq!(r["modify"], 1, "{r}");
    run(&mut opened, "parameter_import", json!({"path": moved}));
    assert!(opened.model.parameters.contains_key("Fire Class"));
    assert_eq!(params_of(&opened, wall)["Fire Class"], "EI60");
    assert_eq!(opened.model.dictionaries["office"].version, "1.1.0");
    // A dictionary of a newer format is refused, not guessed at.
    let newer = file("newer.bimparams");
    std::fs::write(&newer, r#"{"format": 99, "name": "x", "parameters": []}"#).unwrap();
    assert!(opened.execute("parameter_import", &json!({"path": newer})).unwrap_err().contains("newer"));
}

#[test]
fn a_dictionary_merges_other_files_and_a_parameter_can_leave_it() {
    let shared = file("revit.txt");
    std::fs::write(&shared, SHARED).unwrap();
    let path = file("office.bimparams");
    let mut s = Session::new();
    let r = run(&mut s, "dictionary", json!({"path": path, "merge": [shared], "remove": ["Load"]}));
    let names: Vec<&str> = r["parameters"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Depth (mm)", "Fire Rating", "Panel Count"], "{r}");
    assert_eq!(r["parameters"][1]["guid"], "1f2e3d4c-5b6a-4789-8abc-def012345678");
    // Another GUID under a name it has stops it.
    let e = s.execute("dictionary", &json!({"path": path, "add": [{"name": "Fire Rating", "kind": "text"}]})).unwrap_err();
    assert!(e.contains("Fire Rating"), "{e}");

    run(&mut s, "parameter_import", json!({"path": path, "categories": ["wall"]}));
    run(&mut s, "parameter", json!({"name": "Fire Rating", "dictionary": null}));
    assert_eq!(s.model.parameters["Fire Rating"].dictionary, None, "a project parameter now, its GUID kept");
    assert!(s.model.library.guid(&Kind::Parameter, "Fire Rating").is_some());
}

#[test]
fn a_batch_is_all_or_none_and_one_undo_step() {
    let (mut s, wall) = walled();
    run(&mut s, "parameter", json!({"name": "Old One", "categories": ["wall"]}));
    run(&mut s, "parameter", json!({"name": "Old Two", "categories": ["wall"]}));
    run(&mut s, "modify", json!({"id": wall, "params": {"Old One": "v"}}));

    let ops = json!([
        {"op": "rename", "names": ["Old One", "Old Two"], "prefix": "AE_", "find": "Old", "replace": "New"},
        {"name": "Made", "kind": "length", "categories": ["door"], "group": "Dimensions", "read_only": true},
        {"op": "duplicate", "name": "AE_New One"},
    ]);
    let r = run(&mut s, "parameter_batch", json!({"ops": ops, "dry_run": true}));
    assert_eq!(r["done"].as_array().unwrap().len(), 4, "{r}");
    assert!(s.model.parameters.contains_key("Old One"), "a dry run changes nothing");

    let before = s.model.parameters.clone();
    run(&mut s, "parameter_batch", json!({"ops": ops}));
    assert_eq!(params_of(&s, wall)["AE_New One"], "v");
    assert!(s.model.parameters["Made"].read_only);
    let copy = s.model.library.guid(&Kind::Parameter, "AE_New One Copy");
    assert!(copy.is_some() && copy != s.model.library.guid(&Kind::Parameter, "AE_New One"), "a GUID of its own");
    run(&mut s, "undo", json!({}));
    assert_eq!(s.model.parameters, before, "one undo step");
    // A dry run leaves no undo step to take.
    let mut fresh = Session::new();
    run(&mut fresh, "parameter_batch", json!({"ops": [{"name": "X", "categories": ["wall"]}], "dry_run": true}));
    assert!(fresh.execute("undo", &json!({})).is_err(), "nothing to undo");

    let e =
        s.execute("parameter_batch", &json!({"ops": [{"op": "delete", "name": "Old One"}, {"op": "delete", "name": "Nope"}]})).unwrap_err();
    assert!(e.contains("change 2"), "{e}");
    assert_eq!(s.model.parameters, before, "none of it");
    assert!(s.execute("parameter_batch", &json!({"ops": [{"op": "new_guid", "name": "Old One"}]})).is_err(), "a GUID is made once");
}

#[test]
fn merging_moves_values_and_what_names_it_and_a_file_of_either_guid_means_the_one_left() {
    let (mut s, wall) = walled();
    run(&mut s, "parameter", json!({"name": "Room Type", "categories": ["wall"]}));
    run(&mut s, "parameter", json!({"name": "RoomCategory", "categories": ["wall", "door"]}));
    run(&mut s, "parameter", json!({"name": "Size", "kind": "number", "categories": ["wall"]}));
    run(&mut s, "modify", json!({"id": wall, "params": {"RoomCategory": "A"}}));
    let tag = run(&mut s, "tag", json!({"ids": [wall], "label": "RoomCategory"}));
    let tag = tag["id"].as_u64().or_else(|| tag["ids"][0].as_u64()).unwrap();
    let sc = run(&mut s, "schedule", json!({"name": "S", "category": "wall", "fields": [{"name": "Room Type"}, {"name": "RoomCategory"}]}))
        ["id"]
        .as_u64()
        .unwrap();
    let gone = s.model.library.guid(&Kind::Parameter, "RoomCategory").unwrap();

    assert!(s.execute("parameter_merge", &json!({"from": "RoomCategory", "into": "Size"})).unwrap_err().contains("holds"));
    let mut differ = Session::new();
    differ.reset((*s.model).clone(), None);
    run(&mut differ, "modify", json!({"id": wall, "params": {"Room Type": "B"}}));
    assert!(differ.execute("parameter_merge", &json!({"from": "RoomCategory", "into": "Room Type"})).unwrap_err().contains("different"));
    run(&mut differ, "parameter_merge", json!({"from": "RoomCategory", "into": "Room Type", "overwrite": true}));
    assert_eq!(params_of(&differ, wall)["Room Type"], "A");

    run(&mut s, "parameter_merge", json!({"from": "RoomCategory", "into": "Room Type"}));
    assert!(!s.model.parameters.contains_key("RoomCategory"));
    assert_eq!(params_of(&s, wall)["Room Type"], "A");
    assert_eq!(s.model.parameters["Room Type"].categories, ["door", "wall"]);
    let Some(Element::Tag(t)) = s.model.elements.get(tag) else { panic!() };
    assert_eq!(t.label.as_deref(), Some("Room Type"));
    let Some(Element::Schedule(table)) = s.model.elements.get(sc) else { panic!() };
    assert_eq!(table.fields.iter().filter(|f| f.name == "Room Type").count(), 1, "one column");

    let path = file("old.csv");
    std::fs::write(&path, format!("GUID,Name,Kind,Categories\n{gone},RoomCategory,text,Walls\n")).unwrap();
    let r = run(&mut s, "parameter_import", json!({"path": path, "dry_run": true}));
    assert_eq!(r["rows"][0]["here"], "Room Type", "{r}");
}

#[test]
fn what_uses_a_parameter_is_listed_and_what_nothing_uses_is_purged() {
    let (mut s, wall) = walled();
    for name in ["Valued", "In Schedule", "In Filter", "Tagged", "In Formula", "Nothing"] {
        run(&mut s, "parameter", json!({"name": name, "kind": "number", "categories": ["wall"]}));
    }
    run(&mut s, "modify", json!({"id": wall, "params": {"Valued": 1}}));
    run(&mut s, "tag", json!({"ids": [wall], "label": "Tagged"}));
    run(
        &mut s,
        "schedule",
        json!({"name": "S", "category": "wall", "fields": [{"name": "In Schedule"}, {"name": "F", "formula": "In Formula * 2"}]}),
    );
    run(
        &mut s,
        "filter",
        json!({"name": "VF", "categories": ["wall"], "rules": [{"field": "In Filter", "op": "greater-or-equal", "value": 1}]}),
    );
    let r = run(&mut s, "parameter_list", json!({}));
    let of = |n: &str| r["parameters"].as_array().unwrap().iter().find(|p| p["name"] == n).unwrap()["usage"].clone();
    assert_eq!(of("Valued")["values"], 1);
    assert_eq!(of("In Schedule")["schedules"], 1);
    assert_eq!(of("In Filter")["filters"], 1);
    assert_eq!(of("Tagged")["tags"], 1);
    assert_eq!(of("In Formula")["formulas"], 1);
    assert_eq!(r["parameters"][0]["shared"], false);
    assert!(r["parameters"][0]["guid"].is_string());
    let r = run(&mut s, "parameter_purge", json!({"dry_run": true}));
    assert_eq!(r["purged"], json!(["Nothing"]));
    assert!(s.model.parameters.contains_key("Nothing"));
    run(&mut s, "parameter_purge", json!({}));
    assert_eq!(s.model.parameters.len(), 5);
}

#[test]
fn a_familys_parameters_go_to_others_with_what_their_formulas_read() {
    let mut s = Session::new();
    let family = |name: &str| {
        json!({"name": name, "category": "furniture", "parameters": [
            {"name": "Width", "kind": "length", "value": 900.0},
            {"name": "Half", "kind": "length", "formula": "Width / 2"},
        ], "types": {"T": {}}})
    };
    run(&mut s, "family", family("Source"));
    run(&mut s, "family", json!({"name": "Target", "category": "furniture", "types": {"T": {}}}));
    run(
        &mut s,
        "family",
        json!({"name": "Has Width", "category": "furniture", "parameters": [{"name": "Width", "kind": "length", "value": 1.0}], "types": {"T": {}}}),
    );
    let r = run(&mut s, "family_parameter_transfer", json!({"from": "Source", "to": ["Target", "Has Width"], "names": ["Half"]}));
    let names = |s: &Session, f: &str| s.model.families[f].parameters.iter().map(|p| p.name.clone()).collect::<Vec<_>>();
    assert_eq!(names(&s, "Target"), ["Half", "Width"], "its formula's too: {r}");
    assert_eq!(r["families"][1]["skipped"], json!(["Width"]), "{r}");
    run(&mut s, "family_parameter_transfer", json!({"from": "Source", "to": ["Target"], "names": ["Half"], "prefix": "Top "}));
    let top = s.model.families["Target"].parameters.iter().find(|p| p.name == "Top Half").unwrap();
    assert_eq!(top.formula.as_deref(), Some("Top Width / 2"));
    assert!(s.execute("family_parameter_transfer", &json!({"from": "Source", "to": ["Nope"], "names": ["Half"]})).is_err());
    assert!(s.execute("family_parameter_transfer", &json!({"from": "Source", "to": ["Target"], "names": ["Nope"]})).is_err());
}

#[test]
fn hostile_files_are_refused_or_read_as_far_as_they_go() {
    let mut s = Session::new();
    for (name, bytes) in [
        ("empty.csv", &b""[..]),
        ("nonsense.csv", &b"a,b,c\n\"unterminated"[..]),
        ("empty.xlsx", &b""[..]),
        ("junk.xlsx", &b"PK\x05\x06junk that is not a workbook at all, no directory to be found"[..]),
        ("junk.txt", &b"\xFF\xFE\x00"[..]),
        ("junk.bimparams", &b"{not json"[..]),
        ("no-format.bimparams", &br#"{"name": "x"}"#[..]),
        ("bad-rows.bimparams", &br#"{"format": 1, "name": "x", "parameters": [5, {"name": ""}, {"name": "ok", "kind": "plasma"}]}"#[..]),
    ] {
        let path = file(name);
        std::fs::write(&path, bytes).unwrap();
        let _ = s.execute("parameter_import", &json!({"path": path, "dry_run": true}));
        let _ = s.execute("parameter_import", &json!({"path": path, "categories": ["wall"], "skip_conflicts": true}));
        let _ = s.execute("dictionary", &json!({"path": file("d.bimparams"), "merge": [path], "skip_conflicts": true}));
    }
    assert!(s.model.check().is_ok());
    // A name with nothing nameable in it is skipped, and a row of one that's reserved waits.
    let path = file("names.csv");
    std::fs::write(&path, "Name,Categories\n123,Walls\nPhase Created,Walls\nLength,Walls\n").unwrap();
    let r = run(&mut s, "parameter_import", json!({"path": path, "dry_run": true}));
    assert_eq!((r["skip"].clone(), r["conflict"].clone()), (json!(1), json!(2)), "{r}");
    assert!(s.execute("parameter_export", &json!({"path": "/nonexistent/dir/out.csv"})).is_err());
    assert!(s.execute("parameter_export", &json!({"path": file("out.doc")})).is_err());
}
