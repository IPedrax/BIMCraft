//! Title blocks of the project's own: made, read from DXF, named by sheets, filled in, drawn, kept in a file.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom::P2;
use crate::model::{Element, Id};
use crate::render::png;
use crate::sheet;

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

fn sheet_id(s: &Session, number: &str) -> Id {
    s.model.elements.iter().find_map(|(id, e)| matches!(e, Element::Sheet(sh) if sh.number == number).then_some(*id)).unwrap()
}

fn dark(png: &[u8], (x, y): (f32, f32)) -> bool {
    let pm = tiny_skia::Pixmap::decode_png(png).unwrap();
    let c = pm.pixel(x as u32, y as u32).unwrap();
    c.red() < 120 && c.green() < 120 && c.blue() < 120
}

/// A title block 100 by 30 mm: its outline, a solid square in its top left corner (a logo), a client field of the project's
/// and a drawn-by field of each sheet's.
fn civiltec(s: &mut Session) {
    run(
        s,
        "title_block",
        json!({
            "name": "CIVILTEC",
            "width": 100, "height": 30,
            "lines": [{"points": [[0, 0], [100, 0], [100, 30], [0, 30]], "closed": true, "pen": 8}],
            "fills": [{"rings": [[[2, 20], [10, 20], [10, 28], [2, 28]]]}],
            "texts": [
                {"at": [12, 22], "text": "Cliente: {CLIENTE}", "size": 3},
                {"at": [12, 12], "text": "Desenho: {DESENHO}"},
                {"at": [98, 3], "text": "{SHEET_INDEX}/{SHEET_COUNT}", "align": "right"}
            ],
            "fields": [
                {"key": "cliente", "label": "Cliente", "scope": "project"},
                {"key": "DESENHO", "label": "Desenhista", "default": "B.R."}
            ]
        }),
    );
}

#[test]
fn a_sheet_draws_the_title_block_it_names_in_its_corner_in_place_of_the_built_in_one() {
    let mut s = Session::new();
    civiltec(&mut s);
    run(&mut s, "sheet", json!({"number": "A101", "name": "Plantas", "title_block": "CIVILTEC"}));
    run(&mut s, "sheet", json!({"number": "A102", "name": "Cortes"}));
    let (own, built_in) = (sheet_id(&s, "A101"), sheet_id(&s, "A102"));
    // A3, ABNT's margins: the border's bottom right corner 10 mm in from the paper's on the right and at the bottom.
    let margins = s.model.standard().sheet_margins("A3");
    let corner = P2(420.0 - margins.right - 100.0, margins.bottom);
    let logo = corner + P2(6.0, 24.0);
    let d = sheet::draw(&s.model, own).unwrap();
    assert!(dark(&png(&d).unwrap(), d.view.px(logo)), "the logo, filled through");
    let d = sheet::draw(&s.model, built_in).unwrap();
    assert!(!dark(&png(&d).unwrap(), d.view.px(logo)), "the built-in title box has nothing there");
    // A view placed on it goes above it.
    let at = sheet::spot(&s.model, own, own, 100.0);
    assert!((at.1 - (margins.bottom + 30.0 + 297.0 - margins.top) / 2.0).abs() < 1e-9, "{at:?}");
}

#[test]
fn its_fields_are_filled_in_from_the_project_the_sheet_and_the_automatic_ones() {
    let mut s = Session::new();
    s.execute("new", &json!({"name": "Casa Güths", "language": "pt"})).unwrap();
    civiltec(&mut s);
    run(&mut s, "sheet", json!({"number": "A102", "name": "Cortes", "title_block": "CIVILTEC", "fields": {"desenho": "M.S."}}));
    run(&mut s, "sheet", json!({"number": "A101", "name": "Plantas", "title_block": "CIVILTEC"}));
    run(&mut s, "title_values", json!({"values": {"CLIENTE": "Henrique Güths"}}));
    let a101 = sheet::title_values(&s.model, sheet_id(&s, "A101"));
    let a102 = sheet::title_values(&s.model, sheet_id(&s, "A102"));
    assert_eq!(a101["CLIENTE"], "Henrique Güths");
    assert_eq!(a102["CLIENTE"], "Henrique Güths", "a project field is the same on every sheet");
    assert_eq!((a101["DESENHO"].as_str(), a102["DESENHO"].as_str()), ("B.R.", "M.S."), "a sheet field is each sheet's, else its default");
    assert_eq!((a101["SHEET_INDEX"].as_str(), a102["SHEET_INDEX"].as_str(), a101["SHEET_COUNT"].as_str()), ("01", "02", "02"));
    assert_eq!((a101["PROJECT"].as_str(), a101["SHEET_NAME"].as_str(), a101["SHEET_NUMBER"].as_str()), ("Casa Güths", "Plantas", "A101"));
    assert_eq!(a101["DATE"].len(), 10);
    assert_eq!(a101["DATE"].chars().nth(2), Some('/'), "dd/mm/aaaa in Portuguese: {}", a101["DATE"]);
    // An automatic field given a value says that; the project's for every sheet, a sheet's over it.
    run(&mut s, "title_values", json!({"values": {"DATE": "10/11/2026"}}));
    let a102_id = sheet_id(&s, "A102");
    run(&mut s, "modify", json!({"id": a102_id, "fields": {"DATE": "12/11/2026", "DESENHO": null}}));
    let a101 = sheet::title_values(&s.model, sheet_id(&s, "A101"));
    let a102 = sheet::title_values(&s.model, sheet_id(&s, "A102"));
    assert_eq!((a101["DATE"].as_str(), a102["DATE"].as_str()), ("10/11/2026", "12/11/2026"));
    assert_eq!(a102["DESENHO"], "B.R.", "a value taken away gives the default again");
    // What `elements` says of a sheet: its fields filled in, and which are its own to give.
    let e = run(&mut s, "elements", json!({"category": "sheet"}));
    let e = e["elements"].as_array().unwrap().iter().find(|e| e["id"] == json!(a102_id)).unwrap().clone();
    assert_eq!(e["title_values"]["CLIENTE"], "Henrique Güths", "{e}");
    assert_eq!(e["sheet_fields"], json!(["DESENHO"]));
    // A null clears a project value.
    run(&mut s, "title_values", json!({"values": {"CLIENTE": null}}));
    assert_eq!(sheet::title_values(&s.model, sheet_id(&s, "A101"))["CLIENTE"], "");
}

#[test]
fn the_revision_is_the_latest_of_the_clouds_on_the_sheet_and_its_views() {
    let mut s = Session::new();
    s.script(
        r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
           title_block {"name": "T"}
           sheet {"number": "A101", "title_block": "T"}
           place {"sheet": "A101", "view": "Level 1"}"#,
    )
    .unwrap();
    let id = sheet_id(&s, "A101");
    assert_eq!(sheet::title_values(&s.model, id)["REVISION"], "");
    run(
        &mut s,
        "detail_line",
        json!({"points": [[0, 0], [1000, 0], [1000, 1000]], "closed": true, "cloud": 200, "revision": "B", "view": "Level 1"}),
    );
    run(
        &mut s,
        "detail_line",
        json!({"points": [[10, 10], [30, 10], [30, 30]], "closed": true, "cloud": 5, "revision": "C", "view": "A101"}),
    );
    run(
        &mut s,
        "detail_line",
        json!({"points": [[0, 0], [1000, 0], [1000, 1000]], "closed": true, "cloud": 200, "revision": "D", "view": "Level 2"}),
    );
    assert_eq!(sheet::title_values(&s.model, id)["REVISION"], "C", "Level 2 isn't on the sheet");
}

#[test]
fn a_title_block_is_named_by_its_sheets_through_renames_undo_and_a_file() {
    let mut s = Session::new();
    civiltec(&mut s);
    run(&mut s, "sheet", json!({"number": "A101", "title_block": "CIVILTEC", "fields": {"DESENHO": "B.R."}}));
    let id = sheet_id(&s, "A101");
    // A sheet can't name one the project hasn't, nor can one a sheet names go.
    let e = s.execute("sheet", &json!({"number": "A102", "title_block": "Nope"})).unwrap_err();
    assert!(e.contains("no title block `Nope`"), "{e}");
    let e = s.execute("title_block", &json!({"name": "CIVILTEC", "delete": true})).unwrap_err();
    assert!(e.contains("sheet A101"), "{e}");
    run(&mut s, "title_block", json!({"name": "CIVILTEC", "rename": "CIVILTEC A3"}));
    assert_eq!(s.model.sheet(id).unwrap().title_block, "CIVILTEC A3");
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(s.model.sheet(id).unwrap().title_block, "CIVILTEC");
    run(&mut s, "title_values", json!({"values": {"CLIENTE": "ACME"}}));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("p.bimcraft");
    run(&mut s, "save", json!({"path": path}));
    // The file names the title block by its GUID.
    let text = std::fs::read_to_string(&path).unwrap();
    let file: Value = serde_json::from_str(&text).unwrap();
    let named = file["elements"].as_object().unwrap().values().find(|e| e["category"] == "sheet").unwrap()["title_block"].clone();
    assert_ne!(named, json!("CIVILTEC"), "{named}");
    let mut t = Session::new();
    run(&mut t, "open", json!({"path": path}));
    assert_eq!(t.model.title_blocks, s.model.title_blocks);
    assert_eq!(t.model.title_values["CLIENTE"], "ACME");
    let sh = t.model.sheet(sheet_id(&t, "A101")).unwrap();
    assert_eq!((sh.title_block.as_str(), sh.fields["DESENHO"].as_str()), ("CIVILTEC", "B.R."));
    // Back to the built-in, and then it may go.
    let a101 = sheet_id(&t, "A101");
    run(&mut t, "modify", json!({"id": a101, "title_block": ""}));
    run(&mut t, "title_block", json!({"name": "CIVILTEC", "delete": true}));
    assert!(t.model.title_blocks.is_empty());
}

#[test]
fn a_title_block_is_checked_and_a_bad_one_changes_nothing() {
    let mut s = Session::new();
    for (p, why) in [
        (json!({"name": "T", "width": 0}), "width"),
        (json!({"name": "T", "fields": [{"key": "SHEET_NUMBER"}]}), "automatic"),
        (json!({"name": "T", "fields": [{"key": "1A"}]}), "isn't a key"),
        (json!({"name": "T", "texts": [{"at": [0, 0], "text": "x", "size": 0}]}), "tall"),
        (json!({"name": "T", "lines": [{"points": [[0, 0]]}]}), "points"),
        (json!({"name": "T", "lines": [{"points": [[0, 0], [1, 1]], "pen": 40}]}), "pen"),
        (json!({"name": "T", "texts": [{"at": [0, 0], "text": "x", "colour": 1}]}), "unknown field"),
        (json!({"name": "T", "units": "mm"}), "path"),
        (json!({"name": "T", "shape": []}), "isn't one of its parameters"),
    ] {
        let e = s.execute("title_block", &p).unwrap_err();
        assert!(e.contains(why), "{p}: {e}");
    }
    assert!(s.model.title_blocks.is_empty());
    let e = s.execute("title_values", &json!({"values": {"A B": "x"}})).unwrap_err();
    assert!(e.contains("key"), "{e}");
    // One that names a field it doesn't define says so, and is kept: the field shows as written until it's defined.
    let r = run(&mut s, "title_block", json!({"name": "T", "texts": [{"at": [0, 0], "text": "{ART}"}]}));
    assert_eq!(r["undefined"], json!(["ART"]));
    // Without a name, they're listed, with the automatic fields.
    let r = run(&mut s, "title_block", json!({}));
    assert!(r["title_blocks"]["T"].is_object() && r["auto"]["SHEET_COUNT"].is_string(), "{r}");
}

#[test]
fn a_title_block_is_read_from_a_dxf_and_keeps_its_fields_when_read_again() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("selo.dxf");
    let dxf = |client: &str| {
        format!(
            "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n\
             0\nLWPOLYLINE\n8\nCT_SELO\n90\n4\n70\n1\n10\n1000\n20\n500\n10\n1178\n20\n500\n10\n1178\n20\n560\n10\n1000\n20\n560\n\
             0\nTEXT\n8\nCT_TEXTO\n10\n1005\n20\n550\n40\n3\n1\n{client}\n\
             0\nTEXT\n8\nCT_TEXTO\n10\n1005\n20\n540\n40\n2.5\n1\nART: {{ART}} — {{SHEET_NUMBER}}\n\
             0\nENDSEC\n0\nEOF\n"
        )
    };
    std::fs::write(&path, dxf("Cliente: {CLIENTE}")).unwrap();
    let mut s = Session::new();
    let r = run(&mut s, "title_block", json!({"name": "CIVILTEC", "path": path}));
    let tb = &s.model.title_blocks["CIVILTEC"];
    assert_eq!((tb.width, tb.height), (178.0, 60.0), "{r}");
    assert_eq!(tb.lines.len(), 1);
    assert_eq!(tb.texts[1].at, P2(5.0, 40.0));
    let keys: Vec<&str> = tb.fields.iter().map(|f| f.key.as_str()).collect();
    assert_eq!(keys, ["ART", "CLIENTE"]);
    // The client made the project's, labelled; read again from a drawing that names it no more, it's gone, and ART keeps its label.
    run(
        &mut s,
        "title_block",
        json!({"name": "CIVILTEC", "fields": [{"key": "ART", "label": "ART nº"}, {"key": "CLIENTE", "scope": "project"}]}),
    );
    std::fs::write(&path, dxf("Obra: {OBRA}")).unwrap();
    run(&mut s, "title_block", json!({"name": "CIVILTEC", "path": path}));
    let tb = &s.model.title_blocks["CIVILTEC"];
    let fields: Vec<(&str, &str)> = tb.fields.iter().map(|f| (f.key.as_str(), f.label.as_str())).collect();
    assert_eq!(fields, [("ART", "ART nº"), ("OBRA", "")]);
    // A file that isn't a DXF, or draws nothing, is refused.
    std::fs::write(&path, "0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n").unwrap();
    let e = s.execute("title_block", &json!({"name": "Empty", "path": path})).unwrap_err();
    assert!(e.contains("draws nothing"), "{e}");
    let e = s.execute("title_block", &json!({"name": "Empty", "path": dir.path().join("selo.pdf")})).unwrap_err();
    assert!(e.contains("isn't a CAD drawing"), "{e}");
}

#[test]
fn a_new_title_block_starts_as_the_built_in_one_and_another_like_it() {
    let mut s = Session::new();
    run(&mut s, "title_block", json!({"name": "A"}));
    assert_eq!(s.model.title_blocks["A"], crate::titleblock::TitleBlock::default());
    run(&mut s, "title_block", json!({"name": "A", "height": 55}));
    run(&mut s, "title_block", json!({"name": "B", "like": "A"}));
    assert_eq!(s.model.title_blocks["B"].height, 55.0);
    // The built-in one drawn as a title block shows what the built-in title box does.
    run(&mut s, "sheet", json!({"number": "A101", "name": "Plans", "title_block": "B"}));
    let v = sheet::title_values(&s.model, sheet_id(&s, "A101"));
    assert_eq!(crate::titleblock::fill("{SHEET_NUMBER} {SHEET_NAME}", &v), "A101 Plans");
}
