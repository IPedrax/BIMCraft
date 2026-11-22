//! Starter title blocks, and the title block a sheet takes for its paper.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::language::Language;
use crate::model::{Element, Id};
use crate::render::png;
use crate::sheet;
use crate::title_starters;

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

fn sheet_id(s: &Session, number: &str) -> Id {
    s.model.elements.iter().find_map(|(id, e)| matches!(e, Element::Sheet(sh) if sh.number == number).then_some(*id)).unwrap()
}

/// What sheet `number` draws, as the PNG of its page.
fn picture(s: &Session, number: &str) -> Vec<u8> {
    png(&sheet::draw(&s.model, sheet_id(s, number)).unwrap()).unwrap()
}

fn sheet_json(s: &mut Session, number: &str) -> Value {
    run(s, "elements", json!({"category": "sheet"}))["elements"].as_array().unwrap().iter().find(|e| e["number"] == number).unwrap().clone()
}

#[test]
fn every_starter_loads_with_its_fields_defined_and_what_it_draws_inside_its_box() {
    let mut s = Session::new();
    let listed = run(&mut s, "title_block", json!({}));
    let names: Vec<String> = listed["starters"].as_array().unwrap().iter().map(|n| n.as_str().unwrap().to_string()).collect();
    assert_eq!(names.len(), 6, "{listed}");
    for name in &names {
        let made = run(&mut s, "title_block", json!({"starter": name}));
        assert_eq!(made["name"], json!(name));
        assert!(made.get("undefined").is_none(), "{name} names a field nothing defines: {made}");
        let tb = &made["title_block"];
        let (w, h) = (tb["width"].as_f64().unwrap(), tb["height"].as_f64().unwrap());
        assert!(!tb["sizes"].as_array().unwrap().is_empty(), "{name} is for some papers");
        let inside = |p: &Value| {
            let (x, y) = (p[0].as_f64().unwrap(), p[1].as_f64().unwrap());
            (-1e-9..=w + 1e-9).contains(&x) && (-1e-9..=h + 1e-9).contains(&y)
        };
        for l in tb["lines"].as_array().unwrap() {
            assert!(l["points"].as_array().unwrap().iter().all(inside), "{name}: a line leaves its box");
        }
        for t in tb["texts"].as_array().unwrap() {
            assert!(inside(&t["at"]), "{name}: a text starts outside its box");
        }
    }
    // Each is in the project now, and a second of the same name is refused.
    let e = s.execute("title_block", &json!({"starter": names[0]})).unwrap_err();
    assert!(e.contains("already"), "{e}");
    let e = s.execute("title_block", &json!({"starter": "No such stamp"})).unwrap_err();
    assert!(e.contains("ISO 7200 A4-A3"), "the starters are named: {e}");
    let e = s.execute("title_block", &json!({"name": "X", "starter": names[0], "like": names[1]})).unwrap_err();
    assert!(e.contains("one"), "{e}");
}

#[test]
fn a_starter_is_made_in_the_projects_language_and_its_grid_fits_its_cells() {
    // The grids' rows each run as wide as the first, and no label is wider than its cell.
    for spec in super::SPECS {
        let across: Vec<f64> = spec.rows.iter().map(|r| r.1.iter().map(|c| c.w).sum::<f64>()).collect();
        assert!(across.iter().all(|w| (w - across[0]).abs() < 1e-9), "{}: the rows differ in width: {across:?}", spec.name);
        let (sx, k) = (spec.width / across[0], spec.k);
        for (_, cells) in spec.rows {
            for cell in *cells {
                let label = crate::titleblock::Text {
                    at: crate::geom::P2(0.0, 0.0),
                    text: cell.label.into(),
                    size: 1.8 * k,
                    angle: 0.0,
                    align: crate::titleblock::Align::Left,
                    color: None,
                    width_factor: 1.0,
                };
                let wide = label.layout("Arial", cell.label).width;
                assert!(wide <= cell.w * sx - 3.0 * k, "{}: `{}` is {wide} mm in a cell {} mm wide", spec.name, cell.label, cell.w * sx);
            }
        }
    }
    let english = title_starters::make("NBR 16752 A4-A3", Language::English).unwrap();
    let portuguese = title_starters::make("nbr 16752 a4-a3 ", Language::Portuguese).unwrap();
    assert_eq!(english.texts.len(), portuguese.texts.len());
    assert!(english.texts.iter().any(|t| t.text == "Project"));
    assert!(portuguese.texts.iter().all(|t| t.text != "Project"), "a Portuguese project has its labels in Portuguese");
}

#[test]
fn a_sheet_that_names_no_title_block_takes_the_one_listing_its_paper() {
    let mut s = Session::new();
    run(&mut s, "sheet", json!({"number": "A101", "size": "A3"}));
    run(&mut s, "sheet", json!({"number": "A102", "size": "A1"}));
    let (a3, a1) = (picture(&s, "A101"), picture(&s, "A102"));
    run(&mut s, "title_block", json!({"starter": "ISO 7200 A4-A3"}));
    assert_ne!(picture(&s, "A101"), a3, "the A3 sheet is drawn with the starter");
    assert_eq!(picture(&s, "A102"), a1, "an A1 sheet, which no title block lists, is not");
    assert_eq!(sheet_json(&mut s, "A101")["title_block_by_size"], "ISO 7200 A4-A3");
    assert!(sheet_json(&mut s, "A102").get("title_block_by_size").is_none());
    // A sheet that names one has that one, and one with none has none.
    run(&mut s, "title_block", json!({"name": "OWN", "like": "ISO 7200 A4-A3", "sizes": null}));
    run(&mut s, "sheet", json!({"number": "A103", "size": "A3", "title_block": "OWN"}));
    run(&mut s, "sheet", json!({"number": "A104", "size": "A3", "title_block": "none"}));
    assert!(sheet_json(&mut s, "A103").get("title_block_by_size").is_none());
    assert!(sheet_json(&mut s, "A104").get("title_block_by_size").is_none());
    assert_ne!(picture(&s, "A104"), picture(&s, "A101"), "the sheet with none draws no stamp");
    // The sheet's own fields are filled in under the one it is drawn with.
    let own = sheet_json(&mut s, "A101");
    assert_eq!(own["sheet_fields"], json!(["APPROVED"]), "{own}");
}

#[test]
fn sizes_are_papers_kept_once_as_the_sheets_name_them_and_the_first_by_name_wins() {
    let mut s = Session::new();
    let made = run(&mut s, "title_block", json!({"name": "B", "sizes": ["a3", "A3", "iso a4"]}));
    assert_eq!(made["title_block"]["sizes"], json!(["A3", "A4"]));
    assert!(made.get("notes").is_none(), "{made}");
    let made = run(&mut s, "title_block", json!({"name": "A", "sizes": ["A3"]}));
    assert!(made["notes"].to_string().contains("`B`"), "the overlap is said: {made}");
    run(&mut s, "sheet", json!({"number": "A101", "size": "A3"}));
    assert_eq!(sheet_json(&mut s, "A101")["title_block_by_size"], "A");
    run(&mut s, "title_block", json!({"name": "A", "sizes": []}));
    assert_eq!(sheet_json(&mut s, "A101")["title_block_by_size"], "B");
    let e = s.execute("title_block", &json!({"name": "C", "sizes": ["Z9"]})).unwrap_err();
    assert!(e.contains("isn't a paper size"), "{e}");
    let e = s.execute("title_block", &json!({"name": "C", "sizes": "A3"})).unwrap_err();
    assert!(e.contains("sizes") || e.contains("sequence"), "{e}");
}

#[test]
fn the_papers_a_title_block_is_for_are_kept_in_the_file() {
    let mut s = Session::new();
    run(&mut s, "title_block", json!({"starter": "ASME Y14.1 A-B"}));
    run(&mut s, "sheet", json!({"number": "A101", "size": "ANSI B"}));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("stamps.bimc");
    run(&mut s, "save", json!({"path": path}));
    let mut t = Session::new();
    let opened = run(&mut t, "open", json!({"path": path}));
    assert!(opened.get("left_out").is_none(), "{opened}");
    assert_eq!(t.model.title_blocks, s.model.title_blocks);
    assert_eq!(sheet_json(&mut t, "A101")["title_block_by_size"], "ASME Y14.1 A-B");
}
