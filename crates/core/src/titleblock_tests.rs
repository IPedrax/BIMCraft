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

#[test]
fn a_sheets_fields_take_keys_in_any_case_through_modify_too() {
    let mut s = Session::new();
    civiltec(&mut s);
    run(&mut s, "sheet", json!({"number": "A101", "title_block": "CIVILTEC", "fields": {"desenho": "J.P."}}));
    let id = sheet_id(&s, "A101");
    assert_eq!(s.model.sheet(id).unwrap().fields["DESENHO"], "J.P.");
    run(&mut s, "modify", json!({"id": id, "fields": {"Desenho": "M.S."}}));
    assert_eq!(s.model.sheet(id).unwrap().fields["DESENHO"], "M.S.");
    run(&mut s, "modify", json!({"id": id, "fields": {"desenho": null}}));
    assert!(s.model.sheet(id).unwrap().fields.is_empty(), "a null clears it whatever its case");
}

#[test]
fn scale_is_the_views_scale_alone_or_as_indicated() {
    let mut s = Session::new();
    s.execute("new", &json!({"language": "pt"})).unwrap();
    s.script(
        r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
           title_block {"name": "T"}
           sheet {"number": "A101", "title_block": "T"}"#,
    )
    .unwrap();
    let id = sheet_id(&s, "A101");
    let levels: Vec<Id> = s.model.levels().iter().map(|(id, _)| *id).collect();
    assert_eq!(sheet::title_values(&s.model, id)["SCALE"], "", "nothing on it, no scale");
    run(&mut s, "place", json!({"sheet": "A101", "view": levels[0], "scale": 50}));
    assert_eq!(sheet::title_values(&s.model, id)["SCALE"], "1 : 50", "the scale alone, for the title block to letter as it likes");
    run(&mut s, "place", json!({"sheet": "A101", "view": levels[1], "scale": 100}));
    assert_eq!(sheet::title_values(&s.model, id)["SCALE"], "INDICADA");
}

#[test]
fn a_blocks_attributes_are_fields_of_their_tags_and_a_drawing_of_no_unit_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("selo.dxf");
    // A title block as offices keep one: a block with an attribute, placed with its value.
    std::fs::write(
        &path,
        "0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n8\n0\n2\nSELO\n70\n2\n10\n0\n20\n0\n\
         0\nLINE\n8\nSELO\n10\n0\n20\n0\n11\n100\n21\n30\n\
         0\nATTDEF\n8\nTEXTO\n10\n5\n20\n20\n40\n3\n1\n\n3\nCliente\n2\nCLIENTE\n70\n0\n\
         0\nENDBLK\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nINSERT\n8\n0\n66\n1\n2\nSELO\n10\n0\n20\n0\n\
         0\nATTRIB\n8\nTEXTO\n10\n5\n20\n20\n40\n3\n1\nJoão da Silva\n2\ncliente\n70\n0\n0\nSEQEND\n0\nENDSEC\n0\nEOF\n",
    )
    .unwrap();
    let mut s = Session::new();
    let r = run(&mut s, "title_block", json!({"name": "B", "path": path}));
    let tb = &s.model.title_blocks["B"];
    assert_eq!(tb.texts[0].text, "{CLIENTE}", "{r}");
    assert_eq!((tb.fields[0].key.as_str(), tb.fields[0].default.as_str()), ("CLIENTE", "João da Silva"));
    // It names no unit: read in mm, and the reply says so.
    assert!(r["notes"][0].as_str().is_some_and(|n| n.contains("no unit") && n.contains("100.0 by 30.0")), "{r}");
    let r = run(&mut s, "title_block", json!({"name": "B", "path": path, "units": "cm"}));
    assert!(r.get("notes").is_none(), "{r}");
    assert_eq!(s.model.title_blocks["B"].width, 1000.0);
}

#[test]
fn purge_takes_out_a_title_block_no_sheet_names_when_asked() {
    let mut s = Session::new();
    civiltec(&mut s);
    run(&mut s, "title_block", json!({"name": "Spare"}));
    run(&mut s, "sheet", json!({"number": "A101", "title_block": "CIVILTEC"}));
    run(&mut s, "purge", json!({}));
    assert_eq!(s.model.title_blocks.len(), 2, "types and families by default");
    let r = run(&mut s, "purge", json!({"kinds": ["titleblock"]}));
    assert_eq!(r["removed"], json!([{"kind": "title block", "name": "Spare"}]));
    assert!(s.model.title_blocks.contains_key("CIVILTEC"));
}

#[test]
fn a_drawings_colours_and_texts_that_hang_or_centre_on_their_point_come_in_as_drawn() {
    // A label centred on its point (73 = 2, Middle) and one hanging from it (Top), a grey hatch, a red line, and an MTEXT
    // whose true colour code (`\c`) is formatting, not words.
    let dxf = "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n\
               0\nLINE\n8\nSELO\n62\n1\n10\n0\n20\n0\n11\n100\n21\n40\n\
               0\nTEXT\n8\nT\n10\n10\n20\n20\n40\n2\n1\nMeio\n72\n0\n73\n2\n11\n10\n21\n20\n\
               0\nTEXT\n8\nT\n10\n10\n20\n30\n40\n2\n1\nTopo\n72\n0\n73\n3\n11\n10\n21\n30\n\
               0\nMTEXT\n8\nT\n10\n50\n20\n10\n40\n2\n71\n1\n1\n{\\C253;\\c10066329;CIVIL}\n101\nEmbedded Object\n70\n1\n10\n1.0\n20\n0.0\n11\n7.2\n21\n8.0\n\
               0\nHATCH\n8\nT\n62\n251\n420\n4481116\n10\n0\n20\n0\n30\n0\n2\nSOLID\n70\n1\n71\n0\n91\n1\n92\n1\n93\n3\n\
               72\n1\n10\n60\n20\n30\n11\n70\n21\n30\n72\n1\n10\n70\n20\n30\n11\n70\n21\n35\n72\n1\n10\n70\n20\n35\n11\n60\n21\n30\n\
               97\n0\n75\n0\n76\n1\n98\n0\n0\nENDSEC\n0\nEOF\n";
    let tb = crate::titleblock::read_dxf(dxf.as_bytes(), None).unwrap().title_block;
    let text = |s: &str| tb.texts.iter().find(|t| t.text == s).unwrap_or_else(|| panic!("{s} in {:?}", tb.texts));
    assert!((text("Meio").at.1 - 19.0).abs() < 1e-9, "its baseline half its height under the point: {:?}", text("Meio").at);
    assert!((text("Topo").at.1 - 28.0).abs() < 1e-9, "its baseline its height under the point: {:?}", text("Topo").at);
    assert!(tb.texts.iter().any(|t| t.text == "CIVIL" && t.angle == 0.0), "level, not turned by its embedded object: {:?}", tb.texts);
    let civil = text("CIVIL");
    assert_eq!(civil.color, Some([0x99, 0x99, 0x99]), "the colour its formatting gives it, `\\c` over `\\C`");
    assert_eq!(tb.lines[0].color, Some([255, 0, 0]));
    assert_eq!(tb.fills[0].color, Some([0x44, 0x60, 0x5c]), "the hatch's true colour");
}

#[test]
fn its_font_and_a_texts_width_factor_are_given_as_the_rest_and_checked() {
    let mut s = Session::new();
    let r = run(
        &mut s,
        "title_block",
        json!({"name": "Selo", "font": "DejaVu Sans", "texts": [{"at": [4, 4], "text": "OBRA", "width_factor": 0.8, "color": [0, 128, 128]}]}),
    );
    assert_eq!(r["title_block"]["font"], "DejaVu Sans");
    let tb = &s.model.title_blocks["Selo"];
    assert_eq!(tb.font, "DejaVu Sans");
    assert_eq!(tb.texts[0].width_factor, 0.8);
    assert_eq!(tb.texts[0].color, Some([0, 128, 128]));
    // A font left out is kept; a width factor past its range is refused.
    run(&mut s, "title_block", json!({"name": "Selo", "width": 120}));
    assert_eq!(s.model.title_blocks["Selo"].font, "DejaVu Sans");
    let e = s.execute("title_block", &json!({"name": "Selo", "texts": [{"at": [4, 4], "text": "OBRA", "width_factor": 40}]}));
    assert!(e.is_err_and(|e| e.to_string().contains("width factor")));
    let e = s.execute("title_block", &json!({"name": "Selo", "font": ""}));
    assert!(e.is_err_and(|e| e.to_string().contains("font")));
}

#[test]
fn a_fill_with_islands_is_drawn_as_lettering_is_so_every_canvas_leaves_them_empty() {
    let mut s = Session::new();
    let band = json!([[[0, 0], [60, 0], [60, 10], [0, 10]], [[2, 2], [58, 2], [58, 8], [2, 8]]]);
    run(
        &mut s,
        "title_block",
        json!({"name": "Selo", "width": 60, "height": 20, "fills": [{"rings": band}, {"rings": [[[0, 12], [5, 12], [5, 17]]]}]}),
    );
    let tb = s.model.title_blocks["Selo"].clone();
    let d = crate::titleblock::preview(&s.model, &tb, &Default::default());
    let parts: Vec<crate::render::Part> =
        d.prims.iter().zip(&d.parts).filter_map(|(p, part)| matches!(p, crate::render::Prim::Fill { .. }).then_some(*part)).collect();
    // Its fills come first, then its lettering (a new one starts as the built-in box, with its texts).
    assert!(parts.len() >= 2, "{parts:?}");
    assert_eq!(parts[0], crate::render::Part::Text, "a band and its island fill by the non-zero rule");
    assert_ne!(parts[1], crate::render::Part::Text, "a plain fill stays as it was");
}
