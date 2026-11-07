//! Classification: tables loaded from the user's files, a system's parameters, codes written by hand and by an agent's
//! answer, driven through commands as a front end drives them.

use serde_json::{Value, json};

use crate::classification::{self, Table, normal, table_from_rows};
use crate::engine::Session;
use crate::model::{Element, Scope};
use crate::parameters;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn file(name: &str, text: &str) -> String {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path.to_string_lossy().into_owned()
}

/// A few rows of a Uniclass-like Systems table, as the publisher's sheet lays them out (original titles, for the test).
const SS: &str = "Code,Group,Sub group,Section,Object,Title\n\
    Ss_25,25,,,,Wall and barrier systems\n\
    Ss_25_10,25,10,,,Framed wall systems\n\
    Ss_25_10_30,25,10,30,,Masonry wall systems\n\
    Ss_25_10_30_35,25,10,30,35,Brick and block wall systems\n\
    Ss_25_30,25,30,,,Door and window systems\n\
    Ss_25_30_20,25,30,20,,Door systems\n\
    Ss_30,30,,,,Roof and floor systems\n";

const SL: &str = "Code,Title\nSL_45,Office spaces\nSL_45_10,Workspaces\nSL_45_10_49,Open plan offices\n";

/// A project with a wall and a door in it; their ids.
fn house() -> (Session, u64, u64) {
    let mut s = Session::new();
    let r = run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]]}));
    let wall = r["id"].as_u64().or_else(|| r["ids"][0].as_u64()).unwrap();
    let door = run(&mut s, "door", json!({"at": [2000, 0]}))["id"].as_u64().unwrap();
    (s, wall, door)
}

/// `s` with Uniclass's Ss table and parameters loaded.
fn uniclass(s: &mut Session) {
    run(s, "classification_table", json!({"path": file("ss.csv", SS), "system": "uniclass", "table": "Ss", "edition": "2024-07"}));
    run(s, "classification_parameters", json!({"system": "uniclass", "tables": ["Ss"]}));
}

fn type_value(s: &Session, param: &str, id: u64) -> Option<Value> {
    let e = s.model.elements.get(id)?;
    let g = parameters::type_guid(&s.model, e)?;
    s.model.parameters.get(param)?.type_values.get(&g).cloned()
}

#[test]
fn codes_written_any_way_are_one_code_and_their_parents_come_from_their_beginnings() {
    assert_eq!(normal("Ss_25_10_30"), normal("SS 25 10 30"));
    assert_eq!(normal("23-13 35 11 00"), normal("23-13 35 11"));
    assert_eq!(normal("08 11 00"), "0811");
    let t = Table {
        system: "masterformat".into(),
        items: ["08 00 00", "08 10 00", "08 11 00", "08 11 13", "B20", "B2010", "B2010.10"]
            .iter()
            .map(|c| classification::Item { code: c.to_string(), title: c.to_string(), parent: None })
            .collect(),
        ..Table::default()
    };
    let p = t.parents();
    let parent = |i: usize| p[i].map(|q| t.items[q].code.as_str());
    assert_eq!(parent(0), None);
    assert_eq!(parent(1), Some("08 00 00"));
    assert_eq!(parent(3), Some("08 11 00"));
    assert_eq!(parent(5), Some("B20"));
    assert_eq!(parent(6), Some("B2010"));
    assert_eq!(t.find("081113").map(|i| i.code.as_str()), Some("08 11 13"));
}

#[test]
fn a_table_is_read_by_its_headings_in_the_systems_languages_or_by_its_first_two_columns() {
    let rows = |t: &str| parameters::table::read_csv(t);
    let uni = table_from_rows(&rows(SS), None);
    assert_eq!(uni.items.len(), 7);
    assert_eq!(uni.items[3].title, "Brick and block wall systems", "the title is under Title, after Group…");
    let pt = table_from_rows(&rows("Código;Descrição\n3E 20 00 00 00 00 00;Vedações\n3E 20 02 00 00 00 00;Portas\n"), None);
    assert_eq!(pt.items.len(), 2);
    assert_eq!(pt.items[1].title, "Portas");
    let bare = table_from_rows(&rows("21.12,Buitenwanden\n21.13,Binnenwanden\n"), None);
    assert_eq!(bare.items.len(), 2, "no headings: code and title are the first two");
    let all = "Code,Title\nPr_20,Structural products\nSs_25,Wall and barrier systems\nSs_30,Roof and floor systems\n";
    let ss = table_from_rows(&rows(all), Some("Ss"));
    assert_eq!(ss.items.iter().map(|i| i.code.as_str()).collect::<Vec<_>>(), ["Ss_25", "Ss_30"], "one table of a file of all");
    let dup = table_from_rows(&rows("Code,Title\nX1,a\nX1,b\n,orphan\n"), None);
    assert_eq!(dup.items.len(), 1);
    assert_eq!(dup.left_out.len(), 2, "{:?}", dup.left_out);
}

#[test]
fn a_type_is_classified_once_and_every_element_of_it_reads_the_code_and_its_title() {
    let (mut s, wall, _) = house();
    uniclass(&mut s);
    assert_eq!(s.model.parameters["Uniclass Ss Number"].scope, Scope::Type);
    let r = run(&mut s, "classify", json!({"assign": [{"target": wall.to_string(), "table": "Uniclass Ss", "code": "ss 25 10 30 35"}]}));
    assert_eq!(r["rows"][0]["set"]["code"], "Ss_25_10_30_35", "{r}");
    assert_eq!(type_value(&s, "Uniclass Ss Number", wall), Some(json!("Ss_25_10_30_35")), "written as the table writes it");
    assert_eq!(type_value(&s, "Uniclass Ss Title", wall), Some(json!("Brick and block wall systems")));
    let other = run(&mut s, "wall", json!({"points": [[0, 3000], [4000, 3000]]}));
    let other = other["id"].as_u64().or_else(|| other["ids"][0].as_u64()).unwrap();
    assert_eq!(type_value(&s, "Uniclass Ss Number", other), Some(json!("Ss_25_10_30_35")), "the type's, so every wall of it");
    run(&mut s, "undo", json!({}));
    run(&mut s, "undo", json!({}));
    assert_eq!(type_value(&s, "Uniclass Ss Number", wall), None, "one undo step");
}

#[test]
fn a_code_the_table_hasnt_is_refused_and_nothing_is_written() {
    let (mut s, wall, door) = house();
    uniclass(&mut s);
    let assign = json!({"assign": [
        {"target": door.to_string(), "table": "Uniclass Ss", "code": "Ss_25_30_20"},
        {"target": wall.to_string(), "table": "Uniclass Ss", "code": "Ss_99_99"}]});
    let e = s.execute("classify", &assign).unwrap_err();
    assert!(e.contains("Ss_99_99") && e.contains("isn't in"), "{e}");
    assert_eq!(type_value(&s, "Uniclass Ss Number", door), None, "all or none");
    let dry = run(&mut s, "classify", json!({"assign": assign["assign"], "dry_run": true}));
    assert_eq!(dry["refused"].as_array().unwrap().len(), 1, "{dry}");
    assert!(s.execute("classify", &json!({"assign": [{"target": "424242", "table": "Uniclass Ss", "code": "Ss_25"}]})).is_err());
    let unloaded = s.execute("classify", &json!({"assign": [{"target": wall.to_string(), "table": "Uniclass Pr", "code": "Pr_20"}]}));
    assert!(unloaded.unwrap_err().contains("load the system's parameters"));
}

#[test]
fn a_space_is_classified_room_by_room() {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0], [4000, 3000], [0, 3000], [0, 0]]}));
    let room = run(&mut s, "room", json!({"point": [2000, 1500], "name": "Office"}))["id"].as_u64().unwrap();
    run(&mut s, "classification_table", json!({"path": file("sl.csv", SL), "system": "uniclass", "table": "SL"}));
    run(&mut s, "classification_parameters", json!({"system": "uniclass", "tables": ["SL"]}));
    assert_eq!(s.model.parameters["Uniclass SL Number"].scope, Scope::Instance);
    run(&mut s, "classify", json!({"assign": [{"target": room.to_string(), "table": "Uniclass SL", "code": "SL_45_10_49"}]}));
    let params = s.model.elements.get(room).and_then(Element::params).unwrap();
    assert_eq!(params["Uniclass SL Number"], "SL_45_10_49");
    assert_eq!(params["Uniclass SL Title"], "Open plan offices");
}

#[test]
fn a_systems_parameters_have_the_same_guids_in_every_project() {
    let mut a = Session::new();
    let mut b = Session::new();
    for s in [&mut a, &mut b] {
        run(s, "classification_parameters", json!({"system": "omniclass", "tables": ["23"]}));
        s.model_mut().sync_library();
    }
    let g = |s: &Session| s.model.library.guid(&crate::library::Kind::Parameter, "OmniClass 23 Number");
    assert!(g(&a).is_some());
    assert_eq!(g(&a), g(&b));
}

#[test]
fn cobie_types_and_components_are_parameters_of_their_own_scope_and_its_category_takes_another_systems_codes() {
    let (mut s, _, door) = house();
    let pr = "Code,Title\nPr_30,Opening products\nPr_30_59,Doors and windows\nPr_30_59_24,Doorsets\n";
    run(&mut s, "classification_table", json!({"path": file("pr.csv", pr), "system": "uniclass", "table": "Pr"}));
    let r = run(&mut s, "classification_parameters", json!({"system": "cobie", "cobie": {"Type": "Uniclass Pr"}}));
    assert!(r["notes"].to_string().contains("Space Category"), "the space's category has no table yet: {r}");
    let p = &s.model.parameters;
    assert_eq!(p["COBie Type Manufacturer"].scope, Scope::Type);
    assert_eq!(p["COBie Component SerialNumber"].scope, Scope::Instance, "each one's own");
    assert_eq!(p["COBie Component InstallationDate"].scope, Scope::Instance);
    assert!(p["COBie Component SerialNumber"].categories.contains(&"door".to_string()));
    assert!(!p["COBie Type Manufacturer"].categories.contains(&"wall".to_string()), "walls aren't assets");
    run(&mut s, "classify", json!({"assign": [{"target": door.to_string(), "parameter": "COBie Type Category", "code": "Pr_30_59_24"}]}));
    assert_eq!(type_value(&s, "COBie Type Category", door), Some(json!("Pr_30_59_24: Doorsets")), "code: title");
    run(&mut s, "modify", json!({"id": door, "params": {"COBie Component SerialNumber": "SN-0042"}}));
    let e = s.execute("classification_parameters", &json!({"system": "cobie", "cobie": {"Floor": "Uniclass Pr"}}));
    assert!(e.is_err());
}

#[test]
fn the_brief_asks_once_per_type_and_gives_candidates_of_its_category() {
    let (mut s, wall, door) = house();
    run(&mut s, "wall", json!({"points": [[0, 3000], [4000, 3000]]}));
    uniclass(&mut s);
    let r = run(&mut s, "classification_brief", json!({"files": true}));
    let rows = r["brief"]["rows"].as_array().unwrap();
    let walls: Vec<&Value> = rows.iter().filter(|x| x["category"] == "wall").collect();
    assert_eq!(walls.len(), 1, "two walls of one type are one row: {rows:?}");
    assert_eq!(walls[0]["count"], 2);
    let doors: Vec<&Value> = rows.iter().filter(|x| x["category"] == "door").collect();
    assert_eq!(doors.len(), 1);
    assert!(doors[0]["hosts"][0].as_str().unwrap().starts_with("wall"), "{}", doors[0]);
    let cand = r["brief"]["candidates"]["Uniclass Ss"]["door"].to_string();
    assert!(cand.contains("Ss_25_30_20"), "a door's candidates have the door systems: {cand}");
    let wcand = r["brief"]["candidates"]["Uniclass Ss"]["wall"].to_string();
    assert!(wcand.contains("Ss_25"), "{wcand}");
    assert!(r["instructions"].as_str().unwrap().contains("Uniclass-Ss.csv"));
    assert_eq!(r["schema"]["required"][0], "rows");
    run(&mut s, "classify", json!({"assign": [{"target": wall.to_string(), "table": "Uniclass Ss", "code": "Ss_25_10_30_35"}]}));
    let after = run(&mut s, "classification_brief", json!({}));
    assert!(after["brief"]["rows"].as_array().unwrap().iter().all(|x| x["category"] != "wall"), "only what has no code yet");
    let all = run(&mut s, "classification_brief", json!({"rows": "all"}));
    assert_eq!(all["brief"]["rows"].as_array().unwrap().len(), 2);
    let just = run(&mut s, "classification_brief", json!({"rows": "all", "elements": [door.to_string()]}));
    assert_eq!(just["brief"]["rows"].as_array().unwrap().len(), 1);
}

#[test]
fn an_agents_answer_is_checked_and_only_what_holds_is_applied() {
    let (mut s, wall, door) = house();
    uniclass(&mut s);
    let r = run(&mut s, "classification_brief", json!({}));
    let key = |cat: &str| {
        r["brief"]["rows"].as_array().unwrap().iter().find(|x| x["category"] == cat).unwrap()["key"].as_str().unwrap().to_string()
    };
    let text = format!(
        "Here you are:\n```json\n{}\n```\n",
        json!({"rows": [
            {"key": key("wall"), "table": "Uniclass Ss", "code": "Ss_25_10_30_35", "confidence": 0.9, "reason": "brick wall"},
            {"key": key("door"), "table": "Uniclass Ss", "code": "Ss_25_30_77", "confidence": 2.0, "reason": "made up"},
            {"key": "00000000-0000-4000-8000-000000000001", "table": "Uniclass Ss", "code": "Ss_25", "confidence": 0.5, "reason": "?"},
            {"key": key("wall"), "table": "Uniclass Pr", "code": "Pr_20", "confidence": 0.5, "reason": "not loaded"}]})
    );
    let a = run(&mut s, "classification_answer", json!({"answer": text}));
    let ok = a["proposals"].as_array().unwrap();
    assert_eq!(ok.len(), 1, "{a}");
    assert_eq!(ok[0]["title"], "Brick and block wall systems");
    assert_eq!(a["refused"].as_array().unwrap().len(), 3, "{a}");
    run(&mut s, "classify", json!({"assign": [{"target": ok[0]["key"], "table": ok[0]["table"], "code": ok[0]["code"]}]}));
    assert_eq!(type_value(&s, "Uniclass Ss Number", wall), Some(json!("Ss_25_10_30_35")));
    assert_eq!(type_value(&s, "Uniclass Ss Number", door), None);
    let check = run(&mut s, "classification_check", json!({}));
    assert_eq!(check["bindings"][0]["classified"], 1, "{check}");
    assert_eq!(check["bindings"][0]["unclassified_count"], 1);
}

#[test]
fn tables_and_what_they_fill_are_kept_in_the_file_and_a_new_edition_says_what_it_changes() {
    let (mut s, wall, _) = house();
    uniclass(&mut s);
    run(&mut s, "classify", json!({"assign": [{"target": wall.to_string(), "table": "Uniclass Ss", "code": "Ss_25_10_30_35"}]}));
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.join("p.bimcraft").to_string_lossy().into_owned();
    run(&mut s, "save", json!({"path": path}));
    let mut t = Session::new();
    run(&mut t, "open", json!({"path": path}));
    assert_eq!(t.model.classification, s.model.classification);
    assert_eq!(type_value(&t, "Uniclass Ss Number", wall), Some(json!("Ss_25_10_30_35")));
    let newer = SS.replace("Brick and block wall systems", "Brick and block walls").replace("Ss_30,30,,,,Roof and floor systems\n", "");
    let d =
        run(&mut t, "classification_table", json!({"path": file("ss2.csv", &newer), "system": "uniclass", "table": "Ss", "dry_run": true}));
    assert_eq!((d["changes"]["retitled"].as_u64(), d["changes"]["removed"].as_u64()), (Some(1), Some(1)), "{d}");
    run(&mut t, "parameter", json!({"name": "Uniclass Ss Number", "rename": "Systems Code"}));
    assert_eq!(t.model.classification.bindings[0].parameter, "Systems Code", "a rename follows the binding");
}

#[test]
fn every_system_says_what_it_fills_and_none_ships_a_table() {
    let mut s = Session::new();
    let r = run(&mut s, "classification_systems", json!({}));
    let ids: Vec<&str> = r["systems"].as_array().unwrap().iter().filter_map(|x| x["id"].as_str()).collect();
    for id in ["uniclass", "omniclass", "uniformat", "masterformat", "nbr15965", "coclass", "cci", "nlsfb", "cobie"] {
        assert!(ids.contains(&id), "{id}");
    }
    for sys in classification::SYSTEMS {
        for t in sys.tables {
            assert!(!t.why.is_empty() && !t.example.is_empty(), "{} {}", sys.id, t.key);
            let made =
                classification::plan(&classification::Ask { system: sys.id.into(), tables: vec![t.key.into()], ..Default::default() });
            let made = made.unwrap_or_else(|e| panic!("{e}"));
            assert!(
                made.definitions
                    .iter()
                    .all(|d| d.categories.iter().all(|c| d.scope != Some(Scope::Type) || !parameters::TYPELESS.contains(&c.as_str())))
            );
        }
    }
    assert!(r["loaded"].as_array().unwrap().is_empty());
}

#[test]
fn a_hostile_table_file_is_refused_or_cut_down() {
    let mut s = Session::new();
    let e = s.execute("classification_table", &json!({"path": file("e.csv", "\n\n"), "system": "uniclass", "table": "Ss"}));
    assert!(e.is_err());
    let long = format!("Code,Title\n{},x\nOK,fine\n", "A".repeat(500));
    let r = run(&mut s, "classification_table", json!({"path": file("l.csv", &long), "system": "custom", "name": "Mine"}));
    assert_eq!(r["codes"], 1);
    assert!(s.execute("classification_table", &json!({"path": file("m.csv", "a,b\n"), "system": "nope"})).is_err());
    assert!(s.execute("classification_table", &json!({"path": file("c.csv", "a,b\n"), "system": "cobie"})).is_err());
    assert!(s.execute("classification_answer", &json!({"answer": "no json here"})).is_err());
    let r = run(&mut s, "classification_answer", json!({"answer": {"rows": [{"key": 5}, "x", null]}}));
    assert_eq!(r["refused"].as_array().unwrap().len(), 3);
}

#[test]
fn a_brief_is_cut_into_batches_each_with_its_categories_candidates_and_handed_over_with_its_tables() {
    let (mut s, _, _) = house();
    uniclass(&mut s);
    let b = classification::brief(&s.model, &serde_json::from_value(json!({})).unwrap()).unwrap();
    let parts = b.chunks(1);
    assert_eq!(parts.len(), 2);
    for p in &parts {
        let cat = &p.rows[0].category;
        assert_eq!(p.candidates["Uniclass Ss"].keys().collect::<Vec<_>>(), [cat], "only its category's candidates");
    }
    let h = classification::handover(&s.model, &parts[0], true);
    let names: Vec<&str> = h.files.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["brief.json", "Uniclass-Ss.csv"]);
    assert!(h.files[1].1.starts_with("Code,Title,Parent\n"));
    assert!(h.files[1].1.contains("Ss_25_10_30_35,Brick and block wall systems,Ss_25_10_30\n"));
    assert!(classification::handover(&s.model, &parts[0], false).inline.contains("candidates"));
}

/// The entity `#n` of a STEP file: its line.
fn line<'a>(f: &'a str, n: &str) -> &'a str {
    f.lines().find(|l| l.starts_with(&format!("{n}="))).unwrap_or_else(|| panic!("no {n}"))
}

#[test]
fn ifc_says_each_code_as_a_classification_reference_of_its_system_related_to_the_elements_that_hold_it() {
    let (mut s, wall, door) = house();
    uniclass(&mut s);
    run(
        &mut s,
        "classify",
        json!({"assign": [
        {"target": wall.to_string(), "table": "Uniclass Ss", "code": "Ss_25_10_30_35"},
        {"target": door.to_string(), "table": "Uniclass Ss", "code": "Ss_25_30_20"}]}),
    );
    let f = crate::ifc::export(&s.model);
    let systems: Vec<&str> = f.lines().filter(|l| l.contains("=IFCCLASSIFICATION(")).collect();
    assert_eq!(systems.len(), 1, "one Uniclass 2015 for its tables");
    assert!(systems[0].contains("'NBS','2024-07',$,'Uniclass 2015'"), "{}", systems[0]);
    let sys = systems[0].split('=').next().unwrap();
    let refs: Vec<&str> = f.lines().filter(|l| l.contains("=IFCCLASSIFICATIONREFERENCE(")).collect();
    assert_eq!(refs.len(), 2);
    let brick = refs.iter().find(|l| l.contains("'Ss_25_10_30_35','Brick and block wall systems'")).unwrap();
    assert!(brick.ends_with(&format!(",{sys},$,$);")), "{brick}");
    let rel = f
        .lines()
        .find(|l| l.contains("=IFCRELASSOCIATESCLASSIFICATION(") && l.ends_with(&format!(",{});", brick.split('=').next().unwrap())))
        .unwrap();
    // The wall is the element its GlobalId names.
    let wall_gid = crate::ifc::compress(s.model.elements.guid(wall).unwrap().as_u128());
    let related = rel.split("$,$,$,(").nth(1).unwrap().split(')').next().unwrap();
    assert!(related.split(',').any(|r| line(&f, r).contains(&wall_gid)), "{rel}");

    // Read back, the code is in the system's own parameters again.
    let back = crate::import::ifc(f.as_bytes(), "t").unwrap();
    let w2 = back.model.elements.iter().find(|(_, e)| matches!(e, Element::Wall(_))).map(|(_, e)| e).unwrap();
    assert_eq!(w2.params().unwrap()["Uniclass Ss Number"], "Ss_25_10_30_35");
    assert_eq!(w2.params().unwrap()["Uniclass Ss Title"], "Brick and block wall systems");
}

#[test]
fn another_programs_classification_comes_in_as_its_systems_parameters_or_named_for_it() {
    assert_eq!(classification::imported_parameters("OmniClass", "23-17 11 00").0, "OmniClass 23 Number");
    assert_eq!(classification::imported_parameters("Uniclass 2015", "Pr_30_59").0, "Uniclass Pr Number");
    assert_eq!(classification::imported_parameters("Uniformat II", "B2010").0, "UniFormat Number");
    assert_eq!(classification::imported_parameters("NBR 15965", "3E 20 02").1, "NBR 15965 3E Title");
    assert_eq!(classification::imported_parameters("Our Office Codes", "X1").0, "Our Office Codes Number");
}
