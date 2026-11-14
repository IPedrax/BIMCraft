//! A shared parameters file, as Revit's Manage > Shared Parameters keeps one: a plain-text file outside any project or family,
//! groups of parameters each with a GUID, a name, a data type, a description and whether it is shown and may be changed. Read
//! and written in Revit's tab-separated layout (versions 2 and 1), and driven through `Session::execute`, as every front end
//! drives it: a parameter from the file goes into a family as a shared parameter, and a family's shared parameters go out to one.

use std::path::PathBuf;

use serde_json::{Value, json};

use crate::engine::Session;
use crate::family::{Category, Group, Kind, Under};
use crate::shared_parameters::{self, SharedFile};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn err(s: &mut Session, id: &str, p: Value) -> String {
    match s.execute(id, &p) {
        Ok(r) => panic!("{id} {p} should have been refused, and did {r}"),
        Err(e) => e,
    }
}

/// A folder of the test's own, emptied first.
fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("bimcraft-shared-params-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap_or_else(|e| panic!("{e}"));
    d
}

const FIRE: &str = "6f1c2a3b-4d5e-4f60-8a71-92b3c4d5e6f7";
const MAKER: &str = "0a1b2c3d-4e5f-4a6b-9c7d-8e9fa0b1c2d3";
const SPAN: &str = "11112222-3333-4444-8555-666677778888";
const AREA: &str = "21112222-3333-4444-8555-666677778888";
const DOCS: &str = "31112222-3333-4444-8555-666677778888";
const LOAD: &str = "41112222-3333-4444-8555-666677778888";
const HANDLE: &str = "51112222-3333-4444-8555-666677778888";
const COST: &str = "61112222-3333-4444-8555-666677778888";

/// A cabinet family with a width, as the family parameter tests make one, and a shelf with nothing shared.
fn families(s: &mut Session) {
    run(
        s,
        "family",
        json!({"name": "Cabinet", "parameters": [{"name": "Width", "value": 600}], "types": {"Small": {"Width": 500}},
               "forms": [{"profile": [[0, 0], ["Width", 0], ["Width", 400], [0, 400]], "to": 900}]}),
    );
    run(s, "family", json!({"name": "Shelf", "parameters": [{"name": "Depth", "value": 300}], "types": {"A": {}}}));
}

/// A file made by commands: two groups, a parameter of every kind BIMCraft has.
fn made(s: &mut Session, path: &PathBuf) {
    run(s, "shared_parameters", json!({"action": "create", "path": path}));
    run(s, "shared_parameters", json!({"action": "add_group", "path": path, "group": "Identity"}));
    run(s, "shared_parameters", json!({"action": "add_group", "path": path, "group": "Dimensions"}));
    let add = |s: &mut Session, group: &str, name: &str, kind: &str, guid: &str, extra: Value| {
        let mut p = json!({"action": "add", "path": path, "group": group, "name": name, "kind": kind});
        if !guid.is_empty() {
            p["guid"] = json!(guid);
        }
        if let (Some(p), Value::Object(extra)) = (p.as_object_mut(), extra) {
            p.extend(extra);
        }
        run(s, "shared_parameters", p)
    };
    add(s, "Identity", "Fire Rating", "text", FIRE, json!({"description": "Resistance to fire, in minutes — EI 60"}));
    add(s, "Identity", "Maker", "text", MAKER, json!({"visible": false, "modifiable": false}));
    add(s, "Dimensions", "Clear Span", "length", SPAN, json!({}));
    add(s, "Dimensions", "Net Area", "area", AREA, json!({}));
    add(s, "Identity", "Datasheet", "url", DOCS, json!({}));
    add(s, "Identity", "Load Class", "integer", LOAD, json!({}));
    add(s, "Identity", "Handle", "familytype", HANDLE, json!({"of": "furniture"}));
    for (name, kind) in [("Cost Factor", "number"), ("Tilt", "angle"), ("Bulk", "volume"), ("Fixed", "yesno"), ("Finish", "material")] {
        add(s, "Dimensions", name, kind, "", json!({}));
    }
}

#[test]
fn a_file_written_by_commands_reads_back_as_it_was_written() {
    let d = dir("round-trip");
    let path = d.join("shared.txt");
    let mut s = Session::new();
    let steps = s.history().len();
    made(&mut s, &path);
    assert_eq!(s.history().len(), steps, "a file of the user's own is no undo step of the project's");
    let listed = run(&mut s, "shared_parameters", json!({"path": path}));
    let groups: Vec<&str> = listed["groups"].as_array().into_iter().flatten().filter_map(|g| g["name"].as_str()).collect();
    assert_eq!(groups, ["Identity", "Dimensions"], "{listed}");
    let identity = &listed["groups"][0]["parameters"];
    let fire = identity.as_array().and_then(|ps| ps.iter().find(|p| p["name"] == "Fire Rating")).cloned().unwrap_or_default();
    assert_eq!(fire["guid"], json!(FIRE));
    assert_eq!(fire["kind"], json!("text"));
    assert_eq!(fire["datatype"], json!("TEXT"));
    assert_eq!(fire["description"], json!("Resistance to fire, in minutes — EI 60"));
    assert_eq!((fire["visible"].clone(), fire["modifiable"].clone()), (json!(true), json!(true)));
    let maker = identity.as_array().and_then(|ps| ps.iter().find(|p| p["name"] == "Maker")).cloned().unwrap_or_default();
    assert_eq!((maker["visible"].clone(), maker["modifiable"].clone()), (json!(false), json!(false)), "{maker}");
    let handle = identity.as_array().and_then(|ps| ps.iter().find(|p| p["name"] == "Handle")).cloned().unwrap_or_default();
    assert_eq!((handle["kind"].clone(), handle["of"].clone()), (json!("familytype"), json!("furniture")), "{handle}");
    // Each one made without a GUID was given a new one of its own.
    let all: Vec<&Value> = listed["groups"].as_array().into_iter().flatten().flat_map(|g| g["parameters"].as_array()).flatten().collect();
    assert_eq!(all.len(), 12);
    let mut guids: Vec<&str> = all.iter().filter_map(|p| p["guid"].as_str()).collect();
    guids.sort_unstable();
    guids.dedup();
    assert_eq!(guids.len(), 12, "{listed}");
    assert!(guids.iter().all(|g| crate::ids::Guid::parse(g).is_some_and(|g| g.pending().is_none())));
    // The file is Revit's layout: a BOM, the META, GROUP and PARAM headers, tab-separated rows, lines ended as Windows ends them.
    let bytes = std::fs::read(&path).unwrap_or_default();
    assert!(bytes.starts_with(&[0xEF, 0xBB, 0xBF]), "UTF-8 with a byte order mark");
    let text = String::from_utf8_lossy(&bytes[3..]).into_owned();
    assert!(text.contains("*META\tVERSION\tMINVERSION\r\nMETA\t2\t1\r\n"), "{text}");
    assert!(text.contains("*GROUP\tID\tNAME\r\nGROUP\t1\tIdentity\r\nGROUP\t2\tDimensions\r\n"), "{text}");
    assert!(
        text.contains("*PARAM\tGUID\tNAME\tDATATYPE\tDATACATEGORY\tGROUP\tVISIBLE\tDESCRIPTION\tUSERMODIFIABLE\tHIDEWHENNOVALUE\r\n"),
        "{text}"
    );
    assert!(
        text.contains(&format!("PARAM\t{FIRE}\tFire Rating\tTEXT\t\t1\t1\tResistance to fire, in minutes — EI 60\t1\t0\r\n")),
        "{text}"
    );
    assert!(text.contains(&format!("PARAM\t{MAKER}\tMaker\tTEXT\t\t1\t0\t\t0\t0\r\n")), "{text}");
    assert!(text.contains(&format!("PARAM\t{HANDLE}\tHandle\tFAMILYTYPE\t-2000080\t1\t1\t\t1\t0\r\n")), "{text}");
    // Read and written again, it is the same text, byte for byte.
    let file = SharedFile::parse(&text).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(file.to_text(), text);
    assert_eq!(SharedFile::parse(&file.to_text()).map(|f| f.to_text()), Ok(text));
    let _ = std::fs::remove_dir_all(d);
}

/// A file in the layout Revit writes, version 2, by hand: comments, every column, Windows line ends, the data types
/// written as names and as the newer spec ids, a family type of a category by its number, and a type BIMCraft has no kind for.
fn sample_v2() -> String {
    [
        "# A shared parameters file, written for this test.",
        "# Tab-separated: a header line starting with * names the columns of the lines after it.",
        "*META\tVERSION\tMINVERSION",
        "META\t2\t1",
        "*GROUP\tID\tNAME",
        "GROUP\t1\tDoors",
        "GROUP\t7\tGeneral",
        "*PARAM\tGUID\tNAME\tDATATYPE\tDATACATEGORY\tGROUP\tVISIBLE\tDESCRIPTION\tUSERMODIFIABLE\tHIDEWHENNOVALUE",
        &format!("PARAM\t{FIRE}\tFire Rating\tTEXT\t\t1\t1\tMinutes it resists fire\t1\t0"),
        &format!("PARAM\t{SPAN}\tClear Span\tautodesk.spec.aec:length-2.0.0\t\t1\t1\t\t1\t1"),
        &format!("PARAM\t{LOAD}\tLoad Class\tautodesk.spec:spec.int64-2.0.0\t\t7\t0\tFrom the schedule\t0\t0"),
        &format!("PARAM\t{HANDLE}\tHandle\tFAMILYTYPE\t-2000080\t7\t1\t\t1\t0"),
        &format!("PARAM\t{COST}\tCost\tCURRENCY\t\t7\t1\t\t1\t0"),
        &format!("PARAM\t{DOCS}\tDatasheet\tURL\t\t7\t1\t\t1\t0"),
        "",
    ]
    .join("\r\n")
}

/// The same in version 1's columns (no description, no flag for changing it) as UTF-16, as a Unicode text file is saved.
fn sample_v1_utf16() -> Vec<u8> {
    let text = [
        "# Version 1: fewer columns.",
        "*META\tVERSION\tMINVERSION",
        "META\t1\t1",
        "*GROUP\tID\tNAME",
        "GROUP\t3\tÉquipement",
        "*PARAM\tGUID\tNAME\tDATATYPE\tDATACATEGORY\tGROUP\tVISIBLE",
        &format!("PARAM\t{AREA}\tNet Area\tAREA\t\t3\t1"),
        &format!("PARAM\t{MAKER}\tMaker\tTEXT\t\t3\t0"),
        "",
    ]
    .join("\r\n");
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    bytes
}

#[test]
fn a_hand_written_file_in_revits_layout_is_read_in_versions_2_and_1() {
    let d = dir("sample");
    let mut s = Session::new();
    let v2 = d.join("v2.txt");
    std::fs::write(&v2, sample_v2()).unwrap_or_else(|e| panic!("{e}"));
    let r = run(&mut s, "shared_parameters", json!({"path": v2}));
    assert_eq!(r["version"], json!(2), "{r}");
    let general = &r["groups"][1];
    assert_eq!((general["id"].clone(), general["name"].clone()), (json!(7), json!("General")), "{r}");
    let find = |r: &Value, name: &str| {
        r["groups"].as_array().into_iter().flatten().flat_map(|g| g["parameters"].as_array()).flatten().find(|p| p["name"] == name).cloned()
    };
    let span = find(&r, "Clear Span").unwrap_or_default();
    assert_eq!((span["kind"].clone(), span["datatype"].clone()), (json!("length"), json!("autodesk.spec.aec:length-2.0.0")));
    let load = find(&r, "Load Class").unwrap_or_default();
    assert_eq!(load["kind"], json!("integer"));
    assert_eq!(
        (load["visible"].clone(), load["modifiable"].clone(), load["description"].clone()),
        (json!(false), json!(false), json!("From the schedule"))
    );
    let handle = find(&r, "Handle").unwrap_or_default();
    assert_eq!((handle["kind"].clone(), handle["of"].clone()), (json!("familytype"), json!("furniture")));
    // A type BIMCraft has no kind for is listed as it is, with its line, and kept when the file is written again.
    let cost = find(&r, "Cost").unwrap_or_default();
    assert_eq!(
        (cost["kind"].clone(), cost["datatype"].clone(), cost["line"].clone()),
        (Value::Null, json!("CURRENCY"), json!(13)),
        "{cost}"
    );
    run(&mut s, "shared_parameters", json!({"action": "add_group", "path": v2, "group": "More"}));
    let again = run(&mut s, "shared_parameters", json!({"path": v2}));
    assert_eq!(find(&again, "Cost").map(|c| c["datatype"].clone()), Some(json!("CURRENCY")));
    assert_eq!(again["groups"][2], json!({"id": 8, "name": "More", "parameters": []}), "a new group takes the next id");
    // Version 1, UTF-16: what it doesn't say is what Revit takes then, a parameter that may be changed and has no description.
    let v1 = d.join("v1.txt");
    std::fs::write(&v1, sample_v1_utf16()).unwrap_or_else(|e| panic!("{e}"));
    let r = run(&mut s, "shared_parameters", json!({"path": v1}));
    assert_eq!((r["version"].clone(), r["groups"][0]["name"].clone()), (json!(1), json!("Équipement")), "{r}");
    let area = find(&r, "Net Area").unwrap_or_default();
    assert_eq!((area["kind"].clone(), area["description"].clone(), area["modifiable"].clone()), (json!("area"), json!(""), json!(true)));
    assert_eq!(find(&r, "Maker").map(|m| m["visible"].clone()), Some(json!(false)));
    // The same through the module: its kinds.
    let file = SharedFile::parse(&sample_v2()).unwrap_or_else(|e| panic!("{e}"));
    let kinds: Vec<Option<(Kind, Option<Category>)>> = file.parameters.iter().map(|p| p.kind().ok()).collect();
    assert_eq!(
        kinds,
        [
            Some((Kind::Text, None)),
            Some((Kind::Length, None)),
            Some((Kind::Integer, None)),
            Some((Kind::FamilyType, Some(Category::Furniture))),
            None,
            Some((Kind::Url, None))
        ]
    );
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn every_data_type_name_reads_as_its_kind() {
    for (datatype, kind) in [
        ("TEXT", Kind::Text),
        ("INTEGER", Kind::Integer),
        ("NUMBER", Kind::Number),
        ("LENGTH", Kind::Length),
        ("AREA", Kind::Area),
        ("VOLUME", Kind::Volume),
        ("ANGLE", Kind::Angle),
        ("YESNO", Kind::YesNo),
        ("URL", Kind::Url),
        ("MATERIAL", Kind::Material),
        ("autodesk.spec:spec.string-2.0.0", Kind::Text),
        ("autodesk.spec:spec.int64-2.0.0", Kind::Integer),
        ("autodesk.spec:spec.double-2.0.0", Kind::Number),
        ("autodesk.spec:spec.bool-1.0.0", Kind::YesNo),
        ("autodesk.spec.aec:number-2.0.0", Kind::Number),
        ("autodesk.spec.aec:area-2.0.0", Kind::Area),
        ("autodesk.spec.aec:volume-2.0.0", Kind::Volume),
        ("autodesk.spec.aec:angle-2.0.0", Kind::Angle),
        ("autodesk.spec.string:url-2.0.0", Kind::Url),
        ("autodesk.spec.reference:material-1.0.0", Kind::Material),
    ] {
        assert_eq!(shared_parameters::kind_of(datatype, ""), Some((kind, None)), "{datatype}");
    }
    for kind in [
        Kind::Text,
        Kind::Integer,
        Kind::Number,
        Kind::Length,
        Kind::Area,
        Kind::Volume,
        Kind::Angle,
        Kind::YesNo,
        Kind::Url,
        Kind::Material,
    ] {
        let (datatype, category) = shared_parameters::datatype(kind, None).unwrap_or_default();
        assert_eq!(shared_parameters::kind_of(&datatype, &category), Some((kind, None)), "{kind:?} written as {datatype}");
    }
    for cat in [Category::Furniture, Category::Door, Category::Window, Category::Generic, Category::Casework, Category::Lighting] {
        let (datatype, category) = shared_parameters::datatype(Kind::FamilyType, Some(cat)).unwrap_or_default();
        assert_eq!(shared_parameters::kind_of(&datatype, &category), Some((Kind::FamilyType, Some(cat))), "{cat:?}");
    }
    for unknown in ["CURRENCY", "HVAC_AIR_FLOW", "autodesk.spec.aec.hvac:airFlow-2.0.0", "", "text"] {
        assert_eq!(shared_parameters::kind_of(unknown, ""), None, "{unknown}");
    }
    // A family type of a category the file says no number BIMCraft knows: the kind, and no category.
    assert_eq!(shared_parameters::kind_of("FAMILYTYPE", "-1"), Some((Kind::FamilyType, None)));
    assert!(shared_parameters::datatype(Kind::FamilyType, Some(Category::Rack)).is_err());
}

#[test]
fn a_file_that_isnt_one_is_refused_naming_its_line() {
    let head = "*META\tVERSION\tMINVERSION\nMETA\t2\t1\n*GROUP\tID\tNAME\nGROUP\t1\tG\n*PARAM\tGUID\tNAME\tDATATYPE\tDATACATEGORY\tGROUP\tVISIBLE\tDESCRIPTION\tUSERMODIFIABLE\n";
    let param = |row: &str| format!("{head}{row}\n");
    for (text, said) in [
        (String::new(), "META"),
        ("META\t2\t1\n".to_string(), "line 1"),
        ("*META\tVERSION\tMINVERSION\nMETA\t3\t3\n".to_string(), "version 3"),
        ("*META\tVERSION\tMINVERSION\nMETA\tx\t1\n".to_string(), "line 2"),
        ("*META\tVERSION\tMINVERSION\nMETA\tNaN\t1\n".to_string(), "line 2"),
        ("*META\tVERSION\tMINVERSION\nMETA\t2\t1\nMETA\t2\t1\n".to_string(), "line 3"),
        ("*META\tVERSION\tMINVERSION\nMETA\t2\t1\nWHAT\t1\n".to_string(), "line 3"),
        ("*META\tVERSION\tMINVERSION\nMETA\t2\t1\n*GROUP\tID\tNAME\nGROUP\t0\tG\n".to_string(), "line 4"),
        ("*META\tVERSION\tMINVERSION\nMETA\t2\t1\n*GROUP\tID\tNAME\nGROUP\t1\tG\nGROUP\t1\tH\n".to_string(), "line 5"),
        ("*META\tVERSION\tMINVERSION\nMETA\t2\t1\n*GROUP\tID\tNAME\nGROUP\t1\tG\nGROUP\t2\tG\n".to_string(), "line 5"),
        ("*META\tVERSION\tMINVERSION\nMETA\t2\t1\n*GROUP\tID\nGROUP\t1\n".to_string(), "line 3"),
        ("*META\tVERSION\tMINVERSION\nMETA\t2\t1\n*GROUP\tID\tNAME\nGROUP\t99999999999\tG\n".to_string(), "line 4"),
        ("*META\tVERSION\tMINVERSION\nMETA\t2\t1\nGROUP\t1\tG\n".to_string(), "line 3"),
        (param("PARAM\tnot-a-guid\tA\tTEXT\t\t1\t1\t\t1"), "line 6"),
        (param("PARAM\t12345\tA\tTEXT\t\t1\t1\t\t1"), "GUID"),
        (param("PARAM\t00000000-0000-0000-0000-000000000000\tA\tTEXT\t\t1\t1\t\t1"), "line 6"),
        (param(&format!("PARAM\t{FIRE}\tA\tTEXT\t\t1\t1\t\t1\nPARAM\t{FIRE}\tB\tTEXT\t\t1\t1\t\t1")), "line 7"),
        (param(&format!("PARAM\t{FIRE}\tA\tTEXT\t\t2\t1\t\t1")), "line 6"),
        (param(&format!("PARAM\t{FIRE}\tA\tTEXT\t\t1\t2\t\t1")), "line 6"),
        (param(&format!("PARAM\t{FIRE}\tA\tTEXT\t\t1\t1\t\tyes")), "line 6"),
        (param(&format!("PARAM\t{FIRE}\t\tTEXT\t\t1\t1\t\t1")), "line 6"),
        (param(&format!("PARAM\t{FIRE}\tA\u{7}\tTEXT\t\t1\t1\t\t1")), "line 6"),
        (param(&format!("PARAM\t{FIRE}\t{}\tTEXT\t\t1\t1\t\t1", "N".repeat(crate::model::MAX_TEXT + 1))), "line 6"),
        (param(&format!("PARAM\t{FIRE}\tA\tTEXT\t\t1\t1\t{}\t1", "d".repeat(shared_parameters::MAX_DESCRIPTION + 1))), "line 6"),
        (param(&format!("PARAM\t{FIRE}\tA\t\t\t1\t1\t\t1")), "line 6"),
        (param(&format!("PARAM\t{FIRE}\tA")), "line 6"),
        (param(&format!("PARAM\t{FIRE}{}", "\tx".repeat(100))), "line 6"),
        ("*META\tVERSION\tMINVERSION\nMETA\t2\t1\n*PARAM\tNAME\tDATATYPE\tGROUP\n".to_string(), "line 3"),
        ("*META\tVERSION\tMINVERSION\nMETA\t2\t1\nPARAM\tx\n".to_string(), "line 3"),
        ("\n".repeat(shared_parameters::MAX_LINES + 1), "lines"),
    ] {
        match SharedFile::parse(&text) {
            Ok(f) => panic!("{text:?} should have been refused, and read {f:?}"),
            Err(e) => assert!(e.contains(said), "{text:?}: {e}"),
        }
    }
    // Bytes that aren't text: UTF-16 cut in half, and a lone surrogate.
    assert!(shared_parameters::decode(&[0xFF, 0xFE, 0x41]).is_err());
    assert!(shared_parameters::decode(&[0xFF, 0xFE, 0x00, 0xD8, 0x41, 0x00]).is_err());
    // Latin-1, as an older editor saves it, reads too.
    assert_eq!(shared_parameters::decode(b"caf\xe9").as_deref(), Ok("café"));
}

#[test]
fn the_commands_refuse_what_they_cant_do_and_leave_the_file_as_it_was() {
    let d = dir("refusals");
    let path = d.join("shared.txt");
    let mut s = Session::new();
    families(&mut s);
    made(&mut s, &path);
    let before = std::fs::read(&path).unwrap_or_default();
    let unknown = d.join("unknown.txt");
    std::fs::write(&unknown, sample_v2()).unwrap_or_else(|e| panic!("{e}"));
    let garbage = d.join("garbage.txt");
    std::fs::write(&garbage, "not a shared parameters file").unwrap_or_else(|e| panic!("{e}"));
    let big = d.join("big.txt");
    std::fs::write(&big, vec![b'#'; shared_parameters::MAX_FILE as usize + 1]).unwrap_or_else(|e| panic!("{e}"));
    let twice = d.join("twice.txt");
    let text = sample_v2().replace(&format!("{DOCS}\tDatasheet"), &format!("{DOCS}\tFire Rating"));
    std::fs::write(&twice, text).unwrap_or_else(|e| panic!("{e}"));
    let sp = |p: Value| ("shared_parameters", p);
    for ((cmd, p), said) in [
        (sp(json!({})), "`path`"),
        (sp(json!({"path": d.join("none.txt")})), "none.txt"),
        (sp(json!({"path": big})), "too large"),
        (sp(json!({"path": garbage})), "line 1"),
        (sp(json!({"path": path, "action": "explode"})), "explode"),
        (sp(json!({"path": path, "wat": 1})), "`wat`"),
        (sp(json!({"path": path, "action": "create"})), "replace"),
        (sp(json!({"path": path, "action": "add_group", "group": "Identity"})), "Identity"),
        (sp(json!({"path": path, "action": "add_group", "group": " "})), "group"),
        (sp(json!({"path": path, "action": "add_group", "group": "a\tb"})), "group"),
        (sp(json!({"path": garbage, "action": "add_group", "group": "New"})), "line 1"),
        (sp(json!({"path": path, "action": "add", "group": "Nowhere", "name": "X", "kind": "text"})), "add_group"),
        (sp(json!({"path": path, "action": "add", "group": "Identity", "name": "Fire Rating", "kind": "text"})), "Fire Rating"),
        (sp(json!({"path": path, "action": "add", "group": "Identity", "name": "Other", "kind": "text", "guid": FIRE})), FIRE),
        (sp(json!({"path": path, "action": "add", "group": "Identity", "name": "Other", "kind": "text", "guid": "x"})), "GUID"),
        (sp(json!({"path": path, "action": "add", "group": "Identity", "name": "Other", "kind": "money"})), "money"),
        (sp(json!({"path": path, "action": "add", "group": "Identity", "name": "Other"})), "kind"),
        (sp(json!({"path": path, "action": "add", "group": "Identity", "name": "Other", "kind": "familytype"})), "`of`"),
        (sp(json!({"path": path, "action": "add", "group": "Identity", "name": "Other", "kind": "familytype", "of": "rack"})), "rack"),
        (sp(json!({"path": path, "action": "add", "group": "Identity", "name": "Other", "kind": "text", "of": "furniture"})), "`of`"),
        (sp(json!({"path": path, "action": "add", "group": "Identity", "name": "1 Bad-Name", "kind": "text"})), "1 Bad-Name"),
        (
            sp(json!({"path": path, "action": "add", "group": "Identity", "name": "Other", "kind": "text", "description": "a\nb"})),
            "description",
        ),
        (sp(json!({"path": path, "action": "add", "group": "Identity", "name": "Other", "kind": "text", "visible": "no"})), "visible"),
        (sp(json!({"path": path, "action": "load", "family": "Cabinet"})), "`name` or `guid`"),
        (sp(json!({"path": path, "action": "load", "family": "Cabinet", "name": "Nothing"})), "Nothing"),
        (sp(json!({"path": path, "action": "load", "family": "Cabinet", "guid": COST})), COST),
        (sp(json!({"path": path, "action": "load", "name": "Fire Rating"})), "`family`"),
        (sp(json!({"path": path, "action": "load", "family": "Nowhere", "name": "Fire Rating"})), "Nowhere"),
        (sp(json!({"path": path, "action": "load", "family": "Cabinet", "name": "Fire Rating", "under": "two\nlines"})), "under"),
        (sp(json!({"path": path, "action": "load", "family": "Cabinet", "name": "Fire Rating", "instance": "x"})), "instance"),
        (sp(json!({"path": unknown, "action": "load", "family": "Cabinet", "name": "Cost"})), "line 13"),
        (sp(json!({"path": twice, "action": "load", "family": "Cabinet", "name": "Fire Rating"})), "`guid`"),
        (sp(json!({"path": path, "action": "export", "family": "Nowhere"})), "Nowhere"),
        (sp(json!({"path": path, "action": "export", "family": "Shelf"})), "no shared parameters"),
        (sp(json!({"path": path, "action": "export"})), "no shared parameters"),
    ] {
        let e = err(&mut s, cmd, p.clone());
        assert!(e.contains(said), "{p}: {e}");
    }
    assert_eq!(std::fs::read(&path).unwrap_or_default(), before, "the file is as it was");
    assert_eq!(crate::bimc::read_to_string(&garbage).unwrap_or_default(), "not a shared parameters file", "never written over");
    // A family parameter of the name already there, and the parameter itself already there.
    run(&mut s, "family", json!({"name": "Cabinet", "parameter": {"name": "Fire Rating", "kind": "text"}}));
    let e = err(&mut s, "shared_parameters", json!({"path": path, "action": "load", "family": "Cabinet", "name": "Fire Rating"}));
    assert!(e.contains("Cabinet") && e.contains("Fire Rating") && e.contains("already"), "{e}");
    run(&mut s, "family", json!({"name": "Cabinet", "delete_parameter": ["Fire Rating"]}));
    run(&mut s, "shared_parameters", json!({"path": path, "action": "load", "family": "Cabinet", "name": "Fire Rating"}));
    let e = err(&mut s, "shared_parameters", json!({"path": path, "action": "load", "family": "Cabinet", "guid": FIRE}));
    assert!(e.contains("already"), "{e}");
    // A family type parameter whose category the file can't say takes the caller's `of`; none, refused.
    let open = d.join("open.txt");
    let text = sample_v2().replace("FAMILYTYPE\t-2000080", "FAMILYTYPE\t-1");
    std::fs::write(&open, text).unwrap_or_else(|e| panic!("{e}"));
    let e = err(&mut s, "shared_parameters", json!({"path": open, "action": "load", "family": "Shelf", "name": "Handle"}));
    assert!(e.contains("`of`") && e.contains("Handle"), "{e}");
    run(&mut s, "shared_parameters", json!({"path": open, "action": "load", "family": "Shelf", "name": "Handle", "of": "casework"}));
    let handle = s.model.families["Shelf"].parameters.iter().find(|p| p.name == "Handle").cloned().unwrap_or_default();
    assert_eq!((handle.kind, handle.of), (Kind::FamilyType, Some(Category::Casework)));
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_parameter_from_the_file_goes_into_a_family_and_is_there_when_the_project_is_read_again() {
    let d = dir("load");
    let path = d.join("shared.txt");
    let mut s = Session::new();
    families(&mut s);
    made(&mut s, &path);
    let steps = s.history().len();
    let r = run(
        &mut s,
        "shared_parameters",
        json!({"path": path, "action": "load", "family": "Cabinet", "name": "Fire Rating", "instance": true, "under": "identity-data"}),
    );
    assert_eq!(
        (r["family"].clone(), r["parameter"].clone(), r["guid"].clone()),
        (json!("Cabinet"), json!("Fire Rating"), json!(FIRE)),
        "{r}"
    );
    assert_eq!(s.history().len(), steps + 1, "one undo step");
    run(&mut s, "shared_parameters", json!({"path": path, "action": "load", "family": "Cabinet", "guid": SPAN}));
    let f = &s.model.families["Cabinet"];
    let fire = f.parameters.iter().find(|p| p.name == "Fire Rating").cloned().unwrap_or_default();
    assert_eq!(fire.shared.map(|g| g.to_string()).as_deref(), Some(FIRE));
    assert_eq!((fire.kind, fire.instance, fire.group()), (Kind::Text, true, Under::Group(Group::IdentityData)));
    let span = f.parameters.iter().find(|p| p.name == "Clear Span").cloned().unwrap_or_default();
    assert_eq!(
        (span.kind, span.instance, span.group()),
        (Kind::Length, false, Under::Group(Group::Dimensions)),
        "a type parameter under its kind's group"
    );
    // Its name and kind are the file's: the family can't change them.
    let e = err(&mut s, "family", json!({"name": "Cabinet", "parameter": {"name": "Fire Rating", "rename": "Rating"}}));
    assert!(e.contains("shared"), "{e}");
    // A component holds a value of it; the project, written and read, and the family, saved and loaded elsewhere, keep it all.
    let id = run(&mut s, "component", json!({"family": "Cabinet", "at": [0, 0]}))["id"].as_u64().unwrap_or_default();
    run(&mut s, "modify", json!({"id": id, "params": {"Fire Rating": "EI 60"}}));
    let (back, left_out) = crate::model::Model::read(&s.model.to_json().unwrap_or_default()).unwrap_or_else(|e| panic!("{e}"));
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(back.families["Cabinet"], s.model.families["Cabinet"]);
    assert_eq!(crate::element_json(&back, id)["values"]["Fire Rating"], json!("EI 60"));
    let family = d.join("cabinet.json");
    run(&mut s, "family", json!({"name": "Cabinet", "save": family}));
    let mut other = Session::new();
    run(&mut other, "family", json!({"path": family}));
    assert_eq!(other.model.families["Cabinet"].parameters, s.model.families["Cabinet"].parameters);
    // Undone, the family is as it was.
    run(&mut s, "undo", json!({}));
    run(&mut s, "undo", json!({}));
    run(&mut s, "undo", json!({}));
    run(&mut s, "undo", json!({}));
    assert!(s.model.families["Cabinet"].parameters.iter().all(|p| p.shared.is_none()));
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn one_shared_parameter_in_two_families_is_one_guid_and_another_name_for_it_is_refused() {
    let d = dir("two");
    let path = d.join("shared.txt");
    let mut s = Session::new();
    families(&mut s);
    made(&mut s, &path);
    run(&mut s, "shared_parameters", json!({"path": path, "action": "load", "family": "Cabinet", "name": "Fire Rating"}));
    run(&mut s, "shared_parameters", json!({"path": path, "action": "load", "family": "Shelf", "name": "Fire Rating", "instance": true}));
    let guid = |s: &Session, f: &str| s.model.families[f].parameters.iter().find(|p| p.name == "Fire Rating").and_then(|p| p.shared);
    assert_eq!(guid(&s, "Cabinet"), guid(&s, "Shelf"));
    // The project lists where each of the file's parameters is used.
    let r = run(&mut s, "shared_parameters", json!({"path": path}));
    assert_eq!(r["groups"][0]["parameters"][0]["families"], json!(["Cabinet", "Shelf"]), "{r}");
    // Another file says the same GUID under another name, or of another kind: refused, saying which family holds it as what.
    let renamed = d.join("renamed.txt");
    let text = crate::bimc::read_to_string(&path).unwrap_or_default().replace("\tFire Rating\tTEXT", "\tFire Resistance\tTEXT");
    std::fs::write(&renamed, text).unwrap_or_else(|e| panic!("{e}"));
    run(&mut s, "family", json!({"name": "Bench", "parameters": [{"name": "Seats", "kind": "integer", "value": 2}], "types": {"A": {}}}));
    let e = err(&mut s, "shared_parameters", json!({"path": renamed, "action": "load", "family": "Bench", "name": "Fire Resistance"}));
    assert!(e.contains(FIRE) && e.contains("Fire Resistance") && e.contains("`Fire Rating`") && e.contains("Cabinet"), "{e}");
    let retyped = d.join("retyped.txt");
    let text = crate::bimc::read_to_string(&path).unwrap_or_default().replace("\tFire Rating\tTEXT", "\tFire Rating\tINTEGER");
    std::fs::write(&retyped, text).unwrap_or_else(|e| panic!("{e}"));
    let e = err(&mut s, "shared_parameters", json!({"path": retyped, "action": "load", "family": "Bench", "name": "Fire Rating"}));
    assert!(e.contains(FIRE) && e.contains("a whole number") && e.contains("text") && e.contains("Cabinet"), "{e}");
    // And one name the project shares as another GUID.
    let regiud = d.join("reguid.txt");
    let text = crate::bimc::read_to_string(&path).unwrap_or_default().replace(FIRE, COST);
    std::fs::write(&regiud, text).unwrap_or_else(|e| panic!("{e}"));
    let e = err(&mut s, "shared_parameters", json!({"path": regiud, "action": "load", "family": "Bench", "name": "Fire Rating"}));
    assert!(e.contains(FIRE) && e.contains(COST) && e.contains("Cabinet"), "{e}");
    assert!(s.model.families["Bench"].parameters.iter().all(|p| p.shared.is_none()));
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_familys_shared_parameters_go_out_to_a_file() {
    let d = dir("export");
    let mut s = Session::new();
    families(&mut s);
    run(
        &mut s,
        "family",
        json!({"name": "Cabinet", "parameter": {"name": "Fire Rating", "kind": "text", "shared": FIRE, "instance": true}}),
    );
    run(&mut s, "family", json!({"name": "Cabinet", "parameter": {"name": "Clear Span", "kind": "length", "shared": SPAN}}));
    run(
        &mut s,
        "family",
        json!({"name": "Cabinet", "parameter": {"name": "Handle", "kind": "familytype", "of": "door", "shared": HANDLE}}),
    );
    run(&mut s, "family", json!({"name": "Shelf", "parameter": {"name": "Fire Rating", "kind": "text", "shared": FIRE}}));
    run(&mut s, "family", json!({"name": "Shelf", "parameter": {"name": "Load Class", "kind": "integer", "shared": LOAD}}));
    // Into a new file, under a group named for the family; only the shared ones.
    let path = d.join("out.txt");
    let r = run(&mut s, "shared_parameters", json!({"path": path, "action": "export", "family": "Cabinet"}));
    assert_eq!(r["added"], json!(["Fire Rating", "Clear Span", "Handle"]), "{r}");
    assert_eq!(r["group"], json!("Cabinet"));
    let listed = run(&mut s, "shared_parameters", json!({"path": path}));
    let ps = &listed["groups"][0]["parameters"];
    assert_eq!(ps.as_array().map(Vec::len), Some(3), "{listed}");
    assert_eq!((ps[0]["guid"].clone(), ps[0]["kind"].clone()), (json!(FIRE), json!("text")));
    assert_eq!((ps[2]["datatype"].clone(), ps[2]["of"].clone()), (json!("FAMILYTYPE"), json!("door")));
    // What the file has already is kept as the file says it (its description too); every family's, into a group given.
    let text = crate::bimc::read_to_string(&path)
        .unwrap_or_default()
        .replace("\tFire Rating\tTEXT\t\t1\t1\t\t", "\tFire Rating\tTEXT\t\t1\t1\tIn minutes\t");
    std::fs::write(&path, text).unwrap_or_else(|e| panic!("{e}"));
    let r = run(&mut s, "shared_parameters", json!({"path": path, "action": "export", "group": "Everything"}));
    assert_eq!((r["added"].clone(), r["kept"].clone()), (json!(["Load Class"]), json!(["Clear Span", "Fire Rating", "Handle"])), "{r}");
    let listed = run(&mut s, "shared_parameters", json!({"path": path}));
    assert_eq!(listed["groups"][0]["parameters"][0]["description"], json!("In minutes"));
    assert_eq!(listed["groups"][1]["name"], json!("Everything"));
    assert_eq!(listed["groups"][1]["parameters"][0]["name"], json!("Load Class"));
    // Again, nothing new.
    let r = run(&mut s, "shared_parameters", json!({"path": path, "action": "export"}));
    assert_eq!(r["added"], json!([]), "{r}");
    // Loaded into a project of its own from the file, it is the same parameter.
    let mut other = Session::new();
    run(&mut other, "family", json!({"name": "Writing Desk", "parameters": [{"name": "Width", "value": 1200}], "types": {"A": {}}}));
    run(&mut other, "shared_parameters", json!({"path": path, "action": "load", "family": "Writing Desk", "name": "Load Class"}));
    let p = other.model.families["Writing Desk"].parameters.iter().find(|p| p.name == "Load Class").cloned().unwrap_or_default();
    assert_eq!((p.kind, p.shared.map(|g| g.to_string())), (Kind::Integer, Some(LOAD.to_string())));
    // A file that holds one of the GUIDs as something else is refused, naming its line, and isn't written.
    let clash = d.join("clash.txt");
    let text = crate::bimc::read_to_string(&path).unwrap_or_default().replace("\tLoad Class\tINTEGER", "\tLoad Class\tTEXT");
    std::fs::write(&clash, &text).unwrap_or_else(|e| panic!("{e}"));
    let e = err(&mut s, "shared_parameters", json!({"path": clash, "action": "export", "family": "Shelf"}));
    assert!(e.contains("Load Class") && e.contains("line 11"), "{e}");
    let named = d.join("named.txt");
    let text = crate::bimc::read_to_string(&path).unwrap_or_default().replace(LOAD, COST);
    std::fs::write(&named, &text).unwrap_or_else(|e| panic!("{e}"));
    let e = err(&mut s, "shared_parameters", json!({"path": named, "action": "export", "family": "Shelf"}));
    assert!(e.contains("Load Class") && e.contains(COST), "{e}");
    assert_eq!(crate::bimc::read_to_string(&named).unwrap_or_default(), text);
    // A family type parameter of a category whose number BIMCraft doesn't know can't be written.
    run(&mut s, "family", json!({"name": "Shelf", "parameter": {"name": "Rack Type", "kind": "familytype", "of": "rack", "shared": COST}}));
    let e = err(&mut s, "shared_parameters", json!({"path": d.join("rack.txt"), "action": "export", "family": "Shelf"}));
    assert!(e.contains("Rack Type") && e.contains("rack"), "{e}");
    let _ = std::fs::remove_dir_all(d);
}

/// What the hostile sweep gives `shared_parameters` besides its every-command values: each action with keys it takes, in a
/// folder of the sweep's own, some files real and some hostile.
pub(crate) fn sweep_payloads() -> Vec<(&'static str, Value)> {
    let d = std::env::temp_dir().join(format!("bimcraft-shared-params-sweep-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&d);
    let good = d.join("good.txt");
    let _ = std::fs::write(&good, sample_v2());
    let utf16 = d.join("utf16.txt");
    let _ = std::fs::write(&utf16, sample_v1_utf16());
    let junk = d.join("junk.txt");
    let _ = std::fs::write(&junk, b"\xff\xfe\x00\xd8\tPARAM\t\x00\x00");
    let deep = d.join("deep.txt");
    let _ = std::fs::write(&deep, format!("*META\tVERSION\tMINVERSION\nMETA\t2\t1\n{}", "*PARAM\t".repeat(10_000)));
    let fresh = d.join("fresh.txt");
    let _ = std::fs::remove_file(&fresh);
    let sp = |p: Value| ("shared_parameters", p);
    vec![
        sp(json!({"path": good})),
        sp(json!({"path": utf16})),
        sp(json!({"path": junk})),
        sp(json!({"path": deep})),
        sp(json!({"path": d})),
        sp(json!({"path": good, "action": 5})),
        sp(json!({"path": fresh, "action": "create"})),
        sp(json!({"path": fresh, "action": "create", "replace": "yes"})),
        sp(json!({"path": good, "action": "add_group", "group": "x".repeat(10_000)})),
        sp(json!({"path": good, "action": "add_group", "group": "\u{0}"})),
        sp(
            json!({"path": good, "action": "add", "group": "General", "name": "A", "kind": "text", "guid": "00000000-0000-0000-0000-000000000005"}),
        ),
        sp(json!({"path": good, "action": "add", "group": "General", "name": "A", "kind": ["text"], "of": 5, "visible": 1e300})),
        sp(json!({"path": good, "action": "add", "group": [], "name": {}, "kind": "familytype", "of": "nowhere"})),
        sp(json!({"path": good, "action": "add", "group": "General", "name": "A", "kind": "text", "description": "d".repeat(100_000)})),
        sp(json!({"path": good, "action": "load", "family": "Outlet", "name": "Fire Rating", "instance": true, "under": "data"})),
        sp(json!({"path": good, "action": "load", "family": "Outlet", "name": "Cost"})),
        sp(json!({"path": good, "action": "load", "family": "Outlet", "guid": SPAN, "under": 5, "of": "furniture"})),
        sp(json!({"path": good, "action": "load", "family": "Outlet", "name": "Handle", "of": []})),
        sp(json!({"path": good, "action": "load", "family": "Outlet", "guid": 100002})),
        sp(json!({"path": good, "action": "load", "family": "Outlet", "guid": "{6F1C2A3B-4D5E-4F60-8A71-92B3C4D5E6F7}"})),
        sp(json!({"path": utf16, "action": "load", "family": "Outlet", "name": "Maker"})),
        sp(json!({"path": junk, "action": "load", "family": "Outlet", "name": "Maker"})),
        sp(json!({"path": d.join("out.txt"), "action": "export", "family": "Outlet", "group": "G"})),
        sp(json!({"path": d.join("out.txt"), "action": "export", "group": "\t"})),
        sp(json!({"path": junk, "action": "export"})),
    ]
}
