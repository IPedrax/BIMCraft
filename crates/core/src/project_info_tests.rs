//! The project's information (Revit's Project Information): its built-in fields and the project parameters for it, driven
//! through commands as a front end drives them.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::Element;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn file(name: &str) -> String {
    let dir = tempfile::tempdir().unwrap().keep();
    dir.join(name).to_string_lossy().into_owned()
}

/// The project's information element, if it has one: its id.
fn info_id(s: &Session) -> Option<u64> {
    s.model.elements.iter().find(|(_, e)| matches!(e, Element::ProjectInfo(_))).map(|(id, _)| *id)
}

#[test]
fn the_projects_information_is_set_read_saved_and_one_undo_step() {
    let mut s = Session::new();
    // Read before anything is set: empty fields, and nothing made.
    let r = run(&mut s, "project_info", json!({}));
    assert_eq!((r["number"].clone(), r["client"].clone()), (json!(""), json!("")), "{r}");
    assert!(info_id(&s).is_none() && s.history().is_empty(), "a read makes nothing and no undo step");

    let r = run(
        &mut s,
        "project_info",
        json!({"name": "Casa Lima", "number": "2026-014", "client": "Ana Lima", "address": "Rua das Flores 12\nRecife PE",
               "status": "Permit", "issue_date": "2026-11-09", "author": "D. Silva", "organization": "Studio Norte",
               "organization_description": "Architecture", "building": "House A"}),
    );
    assert_eq!(r["client"], "Ana Lima");
    assert_eq!(s.model.name, "Casa Lima", "the project's name is its name");
    let id = info_id(&s).expect("an element of its own, as Revit's Project Information is");
    assert!((100_000..=999_999).contains(&id) && r["id"] == json!(id), "an id as any element's: {r}");
    let guid = s.model.guid(id);

    // One undo step takes it all back.
    run(&mut s, "undo", json!({}));
    assert!(info_id(&s).is_none() && s.model.name != "Casa Lima");
    run(&mut s, "redo", json!({}));

    // A field is changed alone; "" empties it.
    run(&mut s, "project_info", json!({"status": "Construction", "author": ""}));
    let r = run(&mut s, "project_info", json!({}));
    assert_eq!((r["status"].as_str(), r["author"].as_str(), r["number"].as_str()), (Some("Construction"), Some(""), Some("2026-014")));

    // Through a file, with its GUID.
    let path = file("info.bimcraft");
    run(&mut s, "save", json!({"path": path}));
    let mut again = Session::new();
    run(&mut again, "open", json!({"path": path}));
    let r = run(&mut again, "project_info", json!({}));
    assert_eq!((r["address"].as_str(), r["building"].as_str()), (Some("Rua das Flores 12\nRecife PE"), Some("House A")));
    assert_eq!(again.model.guid(info_id(&again).unwrap()), guid);
}

#[test]
fn the_projects_information_is_checked_and_never_deleted_or_doubled() {
    let mut s = Session::new();
    assert!(s.execute("project_info", &json!({"clients": "x"})).is_err(), "a misspelled field is an error");
    assert!(s.execute("project_info", &json!({"number": 12})).is_err(), "fields are text");
    let e = s.execute("project_info", &json!({"client": "x".repeat(300)})).unwrap_err();
    assert!(e.contains("client") && e.contains("256"), "{e}");
    let e = s.execute("project_info", &json!({"number": "A\nB"})).unwrap_err();
    assert!(e.contains("one line"), "only the address has lines: {e}");
    assert!(s.execute("project_info", &json!({"name": "  "})).is_err(), "a project has a name");
    run(&mut s, "project_info", json!({"number": "7"}));
    let id = info_id(&s).unwrap();
    let e = s.execute("delete", &json!({"ids": [id]})).unwrap_err();
    assert!(e.contains("project_info"), "says how to empty it instead: {e}");
    assert!(s.execute("copy", &json!({"ids": [id], "by": [1000, 0]})).is_err(), "a project has one");
    assert_eq!(s.model.elements.values().filter(|e| matches!(e, Element::ProjectInfo(_))).count(), 1);
}

#[test]
fn project_parameters_are_for_the_project_too() {
    let mut s = Session::new();
    run(&mut s, "parameter", json!({"name": "Permit Number", "categories": ["project"]}));
    run(&mut s, "parameter", json!({"name": "Floors Above Ground", "kind": "integer", "categories": ["project"]}));
    let r = run(&mut s, "project_info", json!({"params": {"Permit Number": "PR-88", "Floors Above Ground": 2}}));
    assert_eq!(r["params"]["Permit Number"], "PR-88", "{r}");
    let e = s.execute("project_info", &json!({"params": {"Floors Above Ground": 2.5}})).unwrap_err();
    assert!(e.contains("whole number"), "{e}");
    let e = s.execute("project_info", &json!({"params": {"Fire Rating": "EI60"}})).unwrap_err();
    assert!(e.contains("Fire Rating") && e.contains("parameter"), "{e}");
    // `modify` sets them as on any element.
    let id = info_id(&s).unwrap();
    run(&mut s, "modify", json!({"ids": [id], "params": {"Permit Number": "PR-89"}}));
    // A rename follows them; taken off the project, they go.
    run(&mut s, "parameter", json!({"name": "Permit Number", "rename": "Permit"}));
    assert_eq!(run(&mut s, "project_info", json!({}))["params"]["Permit"], "PR-89");
    let e = s.execute("parameter", &json!({"name": "Permit", "kind": "number"})).unwrap_err();
    assert!(e.contains("values"), "a kind with values isn't changed: {e}");
    assert!(s.execute("parameter", &json!({"name": "Permit", "scope": "type"})).is_err(), "the project has no type");
    run(&mut s, "parameter", json!({"name": "Permit", "categories": ["wall"]}));
    assert!(run(&mut s, "project_info", json!({}))["params"].get("Permit").is_none());
    // Revit's name for it is ours.
    assert_eq!(crate::parameters::match_category("OST_ProjectInformation"), Some(("project", false)));
    assert_eq!(crate::parameters::match_category("Project Information"), Some(("project", false)));
}

#[test]
fn the_projects_information_goes_to_ifc_and_comes_back() {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]]}));
    run(&mut s, "parameter", json!({"name": "Permit Number", "categories": ["project"]}));
    run(
        &mut s,
        "project_info",
        json!({"number": "2026-014", "client": "Ana Lima", "address": "Rua das Flores 12\nRecife PE", "status": "Permit",
               "building": "House A", "params": {"Permit Number": "PR-88"}}),
    );
    let ifc = file("info.ifc");
    run(&mut s, "save", json!({"path": ifc}));
    let text = std::fs::read_to_string(&ifc).unwrap();
    let line = |start: &str| text.lines().find(|l| l.contains(start)).unwrap_or_else(|| panic!("no {start}")).to_string();
    let id = |l: &str| l.split('=').next().unwrap().to_string();
    // IFC's own attributes: the project's phase is its status, the building's name and address.
    assert!(line("=IFCPROJECT(").contains("'Permit'"), "{}", line("=IFCPROJECT("));
    let address = line("=IFCPOSTALADDRESS(");
    assert!(address.contains("('Rua das Flores 12','Recife PE')"), "{address}");
    let building = line("=IFCBUILDING(");
    assert!(building.contains("'House A'") && building.ends_with(&format!("{});", id(&address))), "{building}");
    // The rest, and the project parameters, in property sets on the project.
    let project = id(&line("=IFCPROJECT("));
    let rels: Vec<&str> = text.lines().filter(|l| l.contains("IFCRELDEFINESBYPROPERTIES") && l.contains(&format!("({project})"))).collect();
    assert_eq!(rels.len(), 2, "its information and its parameters: {rels:?}");
    for needle in ["'Client Name',$,IFCTEXT('Ana Lima')", "'Project Number',$,IFCTEXT('2026-014')", "'Permit Number',$,IFCTEXT('PR-88')"] {
        assert!(text.contains(needle), "{needle}");
    }

    let mut back = Session::new();
    run(&mut back, "open", json!({"path": ifc}));
    let r = run(&mut back, "project_info", json!({}));
    assert_eq!(
        (r["number"].as_str(), r["client"].as_str(), r["address"].as_str(), r["status"].as_str(), r["building"].as_str()),
        (Some("2026-014"), Some("Ana Lima"), Some("Rua das Flores 12\nRecife PE"), Some("Permit"), Some("House A"))
    );
    assert_eq!(r["params"]["Permit Number"], "PR-88", "{r}");
    assert_eq!(back.model.parameters["Permit Number"].categories, ["project"]);
}

#[test]
fn another_programs_ifc_gives_its_projects_phase_building_and_address() {
    let ifc = "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION((''),'2;1');\nFILE_NAME('','',(''),(''),'','','');\nFILE_SCHEMA(('IFC4'));\nENDSEC;\nDATA;\n\
#1=IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.);\n#2=IFCUNITASSIGNMENT((#1));\n\
#3=IFCPROJECT('0YvctVUKr0kugbFTf53O9L',$,'Office',$,$,$,'Tender',$,#2);\n\
#4=IFCPOSTALADDRESS($,$,$,$,('Main Street 1'),$,'Lisbon',$,'1000-001','Portugal');\n\
#5=IFCBUILDING('1YvctVUKr0kugbFTf53O9L',$,'Block B',$,$,$,$,$,.ELEMENT.,$,$,#4);\n\
#6=IFCRELAGGREGATES('2YvctVUKr0kugbFTf53O9L',$,$,$,#3,(#5));\n\
ENDSEC;\nEND-ISO-10303-21;\n";
    let path = file("other.ifc");
    std::fs::write(&path, ifc).unwrap();
    let mut s = Session::new();
    run(&mut s, "open", json!({"path": path}));
    let r = run(&mut s, "project_info", json!({}));
    assert_eq!(
        (r["status"].as_str(), r["building"].as_str(), r["address"].as_str()),
        (Some("Tender"), Some("Block B"), Some("Main Street 1\n1000-001 Lisbon\nPortugal")),
        "{r}"
    );
}

#[test]
fn a_sheets_title_block_shows_the_projects_number_client_status_and_date() {
    use crate::geom::P2;
    let mut s = Session::new();
    run(&mut s, "sheet", json!({"name": "Plans"}));
    let sheet = s.model.elements.iter().find(|(_, e)| matches!(e, Element::Sheet(_))).map(|(id, _)| *id).unwrap();
    // Dark pixels in the title box's top band, right of its middle: where the information goes.
    let inked = |s: &Session| {
        let d = crate::sheet::draw(&s.model, sheet).unwrap();
        let png = crate::render::png(&d).unwrap();
        let pm = tiny_skia::Pixmap::decode_png(&png).unwrap();
        let Some(Element::Sheet(sh)) = s.model.elements.get(sheet) else { panic!("a sheet") };
        let paper = crate::model::paper(&sh.size).unwrap();
        let margins = s.model.standard().sheet_margins(&sh.size);
        let (x1, y0) = (paper.0 - margins.right, margins.bottom);
        let (a, b) = (d.view.px(P2(x1 - 70.0, y0 + 39.0)), d.view.px(P2(x1 - 2.0, y0 + 27.0)));
        let mut n = 0;
        for y in a.1 as u32..b.1 as u32 {
            for x in a.0 as u32..b.0 as u32 {
                let c = pm.pixel(x, y).unwrap();
                n += usize::from(c.red() < 120 && c.green() < 120 && c.blue() < 120);
            }
        }
        n
    };
    assert_eq!(inked(&s), 0, "nothing there without the information");
    run(&mut s, "project_info", json!({"number": "2026-014", "client": "Ana Lima", "status": "Permit", "issue_date": "2026-11-09"}));
    assert!(inked(&s) > 50, "the client, and the number, status and date, lettered there");
}
