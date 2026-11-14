//! A sheet drawn with no title block (`title_block: "none"`, or `false`): inspect the actual PDF streams. An empty sheet
//! with none paints nothing; placed content survives.
use std::io::Read as _;

use bimcraft_core::engine::Session;
use bimcraft_core::model::FORMAT;
use flate2::read::ZlibDecoder;
use serde_json::{Value, json};

fn printed(session: &mut Session) -> Result<(Vec<u8>, String), String> {
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let path = dir.path().join("sheet.pdf");
    session.execute("save", &json!({"path": path}))?;
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let start = bytes.windows(7).position(|b| b == b"stream\n").ok_or("PDF has no stream")?;
    let header = String::from_utf8_lossy(&bytes[..start]);
    let length: usize = header
        .rsplit("/Length ")
        .next()
        .map(|s| s.chars().take_while(char::is_ascii_digit).collect::<String>())
        .filter(|d| !d.is_empty())
        .ok_or("PDF stream has no length")?
        .parse::<usize>()
        .map_err(|e| e.to_string())?;
    let compressed = bytes.get(start + 7..start + 7 + length).ok_or("PDF stream is truncated")?;
    let mut contents = String::new();
    ZlibDecoder::new(compressed).read_to_string(&mut contents).map_err(|e| e.to_string())?;
    Ok((bytes, contents))
}

/// The stream's operators and operands one space apart, whatever lines the writer breaks them into.
fn ops(stream: &str) -> String {
    format!(" {} ", stream.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// Whether the stream paints anything: a stroke, a fill, text, an image or a shading.
fn paints(stream: &str) -> bool {
    stream.split_whitespace().any(|t| matches!(t, "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" | "Tj" | "TJ" | "Do" | "sh"))
}

/// A sheet S101 given `title_block` (left out when none), and with `decorated` a view, a note and a detail line on it.
fn fixture(title_block: Option<Value>, decorated: bool) -> Result<(Session, u64), String> {
    let mut session = Session::new();
    let mut params = json!({"number": "S101", "name": "Example sheet"});
    if let Some(choice) = title_block {
        params["title_block"] = choice;
    }
    let id = session.execute("sheet", &params)?["id"].as_u64().ok_or("sheet has no id")?;
    if decorated {
        session.execute("wall", &json!({"points": [[0,0],[4000,0]]}))?;
        session.execute("place", &json!({"sheet": "S101", "view": "Level 1", "at": [150,160], "scale": 100}))?;
        session.execute("text", &json!({"view": "S101", "text": "Example note", "at": [20,40]}))?;
        session.execute("detail_line", &json!({"view": "S101", "start": [20,20], "end": [80,20]}))?;
    }
    Ok((session, id))
}

/// What the one sheet's `title_block` is, as `elements` lists it.
fn listed(session: &mut Session) -> Result<Value, String> {
    Ok(session.execute("elements", &json!({"category": "sheet"}))?["elements"][0]["title_block"].clone())
}

#[test]
fn no_title_block_means_no_stock_pdf_painting_and_the_built_in_one_remains_the_default() {
    let (mut off, _) = fixture(Some(json!("none")), false).unwrap();
    let (none_pdf, empty) = printed(&mut off).unwrap();
    // The page's own setup (its clip), and no paths painted, fills, text or images at all.
    assert!(!paints(&empty), "unexpected content: {empty}");
    // `false` is `none`, in any case.
    for same in [json!(false), json!("None"), json!(" NONE ")] {
        assert_eq!(printed(&mut fixture(Some(same.clone()), false).unwrap().0).unwrap().0, none_pdf, "{same}");
    }
    // An NBR A3 has NBR 17067's folding marks, drawn with the built-in title box and with a project's own; none has none.
    let (mut nbr, _) = fixture(Some(json!("none")), false).unwrap();
    nbr.execute("standard", &json!({"set": "nbr", "adapt_all": true})).unwrap();
    assert!(!paints(&printed(&mut nbr).unwrap().1), "no folding marks either");
    nbr.execute("modify", &json!({"id": nbr.model.sheets()[0], "title_block": ""})).unwrap();
    assert!(paints(&printed(&mut nbr).unwrap().1));
    for decorated in [false, true] {
        let (mut default, _) = fixture(None, decorated).unwrap();
        let (default_pdf, default_stream) = printed(&mut default).unwrap();
        // The built-in one, by `""` or `true`, is what a sheet has unless it says.
        for on in [json!(""), json!(true)] {
            assert_eq!(printed(&mut fixture(Some(on), decorated).unwrap().0).unwrap().0, default_pdf, "leaving it out keeps its look");
        }
        assert!(ops(&default_stream).contains(" h S "), "the default still prints its closed frame");
    }
}

#[test]
fn no_title_block_keeps_viewports_and_sheet_annotations_and_round_trips() {
    let (mut session, id) = fixture(None, true).unwrap();
    let (on_pdf, on) = printed(&mut session).unwrap();
    let views = session.model.on_sheet(id);
    session.execute("modify", &json!({"id": id, "title_block": "none"})).unwrap();
    let (off_pdf, off) = printed(&mut session).unwrap();
    let (_, empty) = printed(&mut fixture(Some(json!("none")), false).unwrap().0).unwrap();
    // What follows the page's own setup, an empty sheet's stream short of its closing Q: the view, the note and the line.
    let setup = empty.trim_end().strip_suffix('Q').unwrap_or(&empty);
    let retained = off.strip_prefix(setup).unwrap();
    assert!(paints(retained) && ops(retained).contains(" S "), "the note, line and view must print");
    assert!(on.ends_with(retained), "all viewport and annotation operators must be unchanged");
    assert_ne!(off_pdf, on_pdf);
    assert_eq!(session.model.on_sheet(id), views);
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("sheet.bimcraft");
    session.execute("save", &json!({"path": file})).unwrap();
    let stored: Value = serde_json::from_str(&bimcraft_core::bimc::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(stored["format"], FORMAT);
    assert_eq!(stored["elements"][id.to_string()]["title_block"], "none");
    let mut reopened = Session::new();
    let opened = reopened.execute("open", &json!({"path": file})).unwrap();
    assert!(opened.get("left_out").is_none(), "{opened}");
    assert_eq!(printed(&mut reopened).unwrap().0, off_pdf);
    // `modify` takes the built-in one back as `""` or `true`, and none as `false` too.
    reopened.execute("modify", &json!({"id": id, "title_block": ""})).unwrap();
    assert_eq!(printed(&mut reopened).unwrap().0, on_pdf);
    reopened.execute("modify", &json!({"id": id, "title_block": false})).unwrap();
    assert_eq!(printed(&mut reopened).unwrap().0, off_pdf);
    reopened.execute("modify", &json!({"id": id, "title_block": true})).unwrap();
    assert_eq!(printed(&mut reopened).unwrap().0, on_pdf);
}

#[test]
fn old_sheets_default_to_a_title_block_and_bad_options_roll_back() {
    let (mut session, sheet) = fixture(None, true).unwrap();
    let before = printed(&mut session).unwrap().0;
    let mut old: Value = serde_json::from_str(&session.model.to_json().unwrap()).unwrap();
    old["format"] = json!(17);
    for element in old["elements"].as_object_mut().unwrap().values_mut() {
        element.as_object_mut().unwrap().remove("title_block");
    }
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("old.bimcraft");
    std::fs::write(&file, old.to_string()).unwrap();
    let mut reopened = Session::new();
    reopened.execute("open", &json!({"path": file})).unwrap();
    assert_eq!(printed(&mut reopened).unwrap().0, before);
    for bad in [json!("false"), json!(0), json!([]), json!({})] {
        let model = session.model.to_json().unwrap();
        assert!(session.execute("sheet", &json!({"title_block": bad})).is_err(), "{bad}");
        assert!(session.execute("modify", &json!({"id": sheet, "title_block": bad})).is_err(), "{bad}");
        assert_eq!(session.model.to_json().unwrap(), model);
    }
    // A title block of the project's can't take the name that means none.
    for name in ["none", "None"] {
        let e = session.execute("title_block", &json!({"name": name})).unwrap_err();
        assert!(e.contains("none"), "{e}");
    }
    session.execute("title_block", &json!({"name": "Office"})).unwrap();
    assert!(session.execute("title_block", &json!({"name": "Office", "rename": "NONE"})).is_err());
    assert!(session.model.title_blocks.contains_key("Office"));
}

#[test]
fn a_sheets_title_block_is_reported_even_when_the_file_omits_the_default() {
    for (option, reported) in [
        (None, ""),
        (Some(json!(true)), ""),
        (Some(json!("")), ""),
        (Some(json!(false)), "none"),
        (Some(json!("none")), "none"),
        (Some(json!("NONE")), "none"),
    ] {
        let (mut session, _) = fixture(option.clone(), false).unwrap();
        assert_eq!(listed(&mut session).unwrap(), reported, "{option:?}");
    }
}

#[test]
fn a_sheets_title_block_creation_and_edits_undo_and_redo_in_the_printed_output() {
    let (mut session, id) = fixture(Some(json!("none")), false).unwrap();
    let off = printed(&mut session).unwrap().0;
    session.execute("undo", &json!({})).unwrap();
    assert!(session.model.sheet(id).is_none());
    session.execute("redo", &json!({})).unwrap();
    assert_eq!(session.model.sheet(id).unwrap().title_block, "none");
    assert_eq!(printed(&mut session).unwrap().0, off);
    session.execute("modify", &json!({"id": id, "title_block": ""})).unwrap();
    let on = printed(&mut session).unwrap().0;
    assert_ne!(on, off);
    session.execute("undo", &json!({})).unwrap();
    assert_eq!(printed(&mut session).unwrap().0, off);
    session.execute("redo", &json!({})).unwrap();
    assert_eq!(printed(&mut session).unwrap().0, on);
    session.execute("undo", &json!({})).unwrap();
    assert_eq!(printed(&mut session).unwrap().0, off);
}

#[test]
fn none_is_no_title_block_of_the_projects_drawn_named_renamed_or_purged() {
    let mut session = Session::new();
    session.execute("title_block", &json!({"name": "Office", "width": 100, "height": 30})).unwrap();
    let a = session.execute("sheet", &json!({"number": "A1", "title_block": "Office"})).unwrap()["id"].as_u64().unwrap();
    let b = session.execute("sheet", &json!({"number": "A2", "title_block": "none"})).unwrap()["id"].as_u64().unwrap();
    // Renaming Office follows the sheet that names it; the sheet with none keeps none.
    session.execute("title_block", &json!({"name": "Office", "rename": "Studio"})).unwrap();
    assert_eq!(session.model.sheet(a).unwrap().title_block, "Studio");
    assert_eq!(session.model.sheet(b).unwrap().title_block, "none");
    // Purge: a sheet with none names no title block, so Studio is used by A1 alone; with A1 on none too, it goes.
    let dry = session.execute("purge", &json!({"kinds": ["titleblock"], "dry_run": true})).unwrap();
    assert!(!dry.to_string().contains("Studio"), "{dry}");
    session.execute("modify", &json!({"id": a, "title_block": false})).unwrap();
    session.execute("purge", &json!({"kinds": ["titleblock"]})).unwrap();
    assert!(session.model.title_blocks.is_empty());
    assert_eq!(session.model.sheet(a).unwrap().title_block, "none");
    // Its fields have nothing to fill in, and it isn't listed as having any of its own.
    let sheets = session.execute("elements", &json!({"category": "sheet"})).unwrap();
    assert!(sheets["elements"][0].get("sheet_fields").is_none(), "{sheets}");
}
